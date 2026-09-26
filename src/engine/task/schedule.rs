//! The connections of one download: opened as the server allows, each given the largest span left,
//! watched for a stall or a crawl, and closed or reopened by what they report.

use super::*;

/// How long a limit has to hold, unchallenged and in full use, before one more connection is asked
/// for.
pub(super) const PROBE_AFTER: Duration = Duration::from_secs(10);

/// How long a connection of ours may go on being counted by a server after we closed it: the
/// round trip for it to notice, with room to spare.
pub(super) const LINGER: Duration = Duration::from_millis(500);

/// How many times slower than the others a connection has to be, for `stall_timeout` on end,
/// before it is taken for stuck on a bad path rather than merely slower.
pub(super) const CRAWL: f64 = 16.0;

/// One running connection as the stall watch sees it: its own stop, when it started and from how
/// far into its segment, and when its segment last moved.
pub(super) struct Watch {
	stop: CancellationToken,
	started: Instant,
	from: u64,
	seen: u64,
	moved: Instant,
	slow_since: Option<Instant>,
}

pub(super) fn median(mut values: Vec<f64>) -> f64 {
	values.sort_by(f64::total_cmp);
	values[values.len() / 2]
}

/// Whether a failure is the server turning a connection away rather than the network failing:
/// too busy, a refusal, or a connection closed before it delivered a byte. A refusal and a close
/// only count while another connection is running, which the caller checks; on their own they
/// are the ordinary failures they look like. See spec/engine.md, "The server decides how many
/// connections it takes".
pub(super) fn crowding(error: &Error, answered: bool) -> bool {
	match error {
		Error::Busy { .. } => true,
		Error::Refused { status: 403 } => true,
		Error::Http(_) => !answered,
		_ => false,
	}
}

/// The wait a server asked for with its refusal, held to a minute.
pub(super) fn asked_wait(error: &Error) -> Option<Duration> {
	match error {
		Error::Busy { retry_after: Some(wait), .. } => Some((*wait).min(Duration::from_secs(60))),
		_ => None,
	}
}

/// Connections come and go here until the plan is complete. One at a time on a server without
/// ranges; otherwise up to `handle.ceiling`, which the window may move while this runs, each new
/// one allowed once the last has proved itself by delivering a byte, and each taking an idle
/// segment or cutting the largest remainder in two.
#[allow(clippy::too_many_arguments)]
pub(super) async fn schedule(
	settings: &Settings,
	probed: &Probe,
	first: reqwest::Client,
	sources: &[Url],
	validator: Option<String>,
	plan: Arc<Mutex<Plan>>,
	writer: Writer,
	handle: &Handle,
	global: Limiter,
) -> Result<()> {
	let connections = settings.connections;
	// The ceiling the loop reads, and the one the window writes. A server that will not serve
	// ranges is one connection whatever anybody asks, so it is pinned here rather than left to be
	// raised into a promise the server would not keep.
	let ranges = probed.ranges;
	handle.ceiling.store(if ranges { connections.max as u64 } else { 1 }, Ordering::Relaxed);
	handle.auto.store(connections.auto && ranges, Ordering::Relaxed);
	let ceiling = || handle.ceiling.load(Ordering::Relaxed).max(1) as usize;
	// How many connections are allowed right now: starts at `min` and grows by one each time a
	// connection delivers its first byte, up to the ceiling. Without auto, all of it at once.
	let allowed = Arc::new(AtomicU64::new(if connections.auto && ranges {
		connections.min as u64
	} else {
		ceiling() as u64
	}));
	// Rung by a connection's first byte, so the next one is started then and not at the next
	// tick; a file that takes less than a tick would otherwise never see a second connection.
	let grew = Arc::new(Notify::new());
	let received = Arc::new(AtomicU64::new(0));
	// What the server will take, learnt as it goes: nothing known, or what an earlier download
	// from the host learnt. Halved when a connection is turned away while others run, raised by
	// one when it has held a while unchallenged. See spec/engine.md, "The server decides how
	// many connections it takes".
	let mut cap = match handle.learned.load(Ordering::Relaxed) as usize {
		0 => usize::MAX,
		learned => learned,
	};
	let mut settled_at = Instant::now();
	// The most connections the server has served at once, every one of them answering: a refusal
	// past this is its limit found; one within it is more likely a connection of ours it has not
	// finished closing, and is waited out -- three of those in a row, with nothing answering in
	// between, and the limit is taken as lowered.
	let mut served = 0usize;
	let mut strikes = 0u32;
	// When a connection of ours that was being served last closed. A server goes on counting one
	// until it has noticed, so a refusal just after is more likely that than a limit, and is
	// waited out the same way. The probe's is not one: it goes on as the first connection.
	let mut closed_at: Option<Instant> = None;
	// When a connection of ours was last turned away. Not a reason to doubt the refusals that
	// come after it, which were mostly asked for at the same moment; only one opened within a
	// LINGER after it, which the server may have counted beside the one it was still closing.
	let mut refused_at: Option<Instant> = None;
	let mut answering: HashMap<usize, Arc<AtomicBool>> = HashMap::new();
	// No new connection before this, after one was turned away: the wait the server asked for,
	// or the retry wait.
	let mut hold: Option<Instant> = None;
	// Each segment's pace in bytes a second, smoothed, for cutting the one that will finish last.
	let mut pace: Vec<f64> = Vec::new();
	let mut landed: Vec<u64> = Vec::new();
	// Each running connection's own stop, and what the stall watch knows of it; and the pace the
	// fastest of them kept, for judging the last one left, which has nobody else to be judged
	// against. See spec/engine.md, "A stuck connection is reopened".
	let mut watch: HashMap<usize, Watch> = HashMap::new();
	let mut peers_pace = 0f64;
	// The probe's client, for the first connection to go on with the connection it left.
	let mut reuse = Some(first);
	let mut workers: JoinSet<(usize, bool, Result<Outcome>)> = JoinSet::new();
	let mut active: Vec<usize> = Vec::new();
	let mut attempts: Vec<u32> = vec![0; plan.lock().unwrap().segments.len()];
	let mut ticker = tokio::time::interval(Duration::from_millis(500));
	let mut last_tick = (Instant::now(), received.load(Ordering::Relaxed));
	let target_control = |plan: &Plan| {
		Control::new(probed.url.as_str(), probed.size, validator.as_deref(), plan.clone())
	};
	let target = writer.part_path().with_extension("");

	loop {
		// Fill the allowed connections.
		loop {
			if plan.lock().unwrap().is_complete() {
				break;
			}
			// Both are read every time round: the ceiling because the window may have moved it,
			// and `allowed` because a connection may have earned the next one. The clamp is
			// written back so that a ceiling lowered and raised again grows one connection at a
			// time as it did the first time, rather than opening every one it had earned at once.
			let allowed_now = (allowed.load(Ordering::Relaxed) as usize).min(ceiling()).min(cap);
			allowed.store(allowed_now as u64, Ordering::Relaxed);
			if active.len() >= allowed_now || hold.is_some_and(|until| Instant::now() < until) {
				break;
			}
			let index = {
				let mut plan = plan.lock().unwrap();
				match plan.idle(&active) {
					Some(i) => Some(i),
					None if ranges && handle.auto.load(Ordering::Relaxed) => {
						// The cut goes where the download will finish last: a segment's bytes left
						// at its own pace, or at the typical pace while it has none yet.
						let known: Vec<f64> = pace.iter().copied().filter(|r| *r > 0.0).collect();
						let typical =
							if known.is_empty() { 1.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
						let pace = &pace;
						plan.steal_latest(settings.min_segment, |i, segment| {
							let rate = pace.get(i).copied().filter(|r| *r > 0.0).unwrap_or(typical);
							segment.remaining() as f64 / rate
						})
					}
					None => None,
				}
			};
			let Some(index) = index else { break };
			if index >= attempts.len() {
				attempts.resize(index + 1, 0);
			}
			active.push(index);
			let split = ceiling() > 1;
			let client = match reuse.take() {
				Some(client) => client,
				None => crate::engine::client::build(settings, split)?,
			};
			let allowed = allowed.clone();
			let grew = grew.clone();
			// Whether this connection delivered a byte: one turned away before it did is the
			// server saying no, one that failed after is the network.
			let first = Arc::new(AtomicBool::new(false));
			let answered = first.clone();
			answering.insert(index, first.clone());
			let (base, from) = {
				let plan = plan.lock().unwrap();
				(plan.span.start, plan.segments[index].done)
			};
			let stop = handle.cancel.child_token();
			let now = Instant::now();
			watch.insert(
				index,
				Watch { stop: stop.clone(), started: now, from, seen: from, moved: now, slow_since: None },
			);
			let received = received.clone();
			let done = handle.progress.clone();
			// Spread across the sources by segment, and on to the next source with each retry.
			let source = &sources[(index + attempts[index] as usize) % sources.len()];
			let primary = source == &probed.url;
			let job = Job {
				client,
				url: source.clone(),
				index,
				plan: plan.clone(),
				validator: validator.clone().filter(|_| primary),
				size: probed.size,
				ranges: probed.ranges,
				base,
				writer: writer.clone(),
				limits: vec![handle.limit.clone(), global.clone()],
				idle_timeout: settings.idle_timeout,
				cancel: stop,
				// Counted as the bytes land, so a progress report between the ticks below is
				// current: a download shorter than a tick once reported nothing but its end, and
				// on Linux, whose sleeps are punctual, a half-second test file was exactly that.
				progress: Arc::new(move |n| {
					if n == 0 {
						answered.store(true, Ordering::Relaxed);
						// Two more for each that answers: the count doubles each round rather than
						// climbing by one, and a server that takes fewer says so.
						allowed.fetch_add(2, Ordering::Relaxed);
						grew.notify_one();
					} else {
						received.fetch_add(n as u64, Ordering::Relaxed);
						done.done.fetch_add(n as u64, Ordering::Relaxed);
					}
				}),
			};
			handle.progress.connections.store(active.len() as u64, Ordering::Relaxed);
			workers.spawn(async move {
				let outcome = fetch(job).await;
				(index, first.load(Ordering::Relaxed), outcome)
			});
		}
		if active.is_empty() {
			// Everything turned away and the wait not over: wait it out, then start again.
			if let Some(until) = hold.filter(|until| Instant::now() < *until) {
				tokio::select! {
					_ = tokio::time::sleep_until(until) => {}
					_ = handle.cancel.cancelled() => return Err(Error::Cancelled),
				}
				continue;
			}
			let plan = plan.lock().unwrap();
			if plan.is_complete() {
				return Ok(());
			}
			// Nothing running and nothing to start: every open segment is waiting on a retry
			// timer, which is handled below by re-queueing; reaching here means a segment could
			// not be started, which cannot happen while the plan is open.
			unreachable!("open plan with no connection to run");
		}
		tokio::select! {
			Some(finished) = workers.join_next() => {
				let (index, answered, outcome) = finished.map_err(|e| Error::Disk { path: target.clone(), source: std::io::Error::other(e) })?;
				served = served.max(answering.values().filter(|a| a.load(Ordering::Relaxed)).count());
				answering.remove(&index);
				active.retain(|&i| i != index);
				let watched = watch.remove(&index);
				let opened_after_refusal = refused_at
					.zip(watched.as_ref().map(|w| w.started))
					.is_some_and(|(refused, opened)| opened >= refused && opened.duration_since(refused) < LINGER);
				let turned_away = matches!(&outcome, Err(e) if crowding(e, answered));
				if answered {
					strikes = 0;
				}
				handle.progress.connections.store(active.len() as u64, Ordering::Relaxed);
				match outcome {
					Ok(Outcome::Complete) => {}
					Ok(Outcome::EndOfFile(size)) => {
						handle.progress.total.store(size, Ordering::Relaxed);
					}
					// Dropped by the stall watch, not by the user: the segment goes back and is reopened
					// from where it stands. One that got nowhere at all spends a try, so a server that
					// never sends a byte still fails the download in the end.
					Err(Error::Cancelled) if !handle.cancel.is_cancelled() => {
						let done_now = plan.lock().unwrap().segments[index].done;
						if watched.is_some_and(|w| done_now == w.from) {
							if attempts[index] >= settings.retries {
								handle.cancel.cancel();
								while workers.join_next().await.is_some() {}
								let seconds = settings.stall_timeout.as_secs();
								return Err(Error::GaveUp { tries: attempts[index] + 1, last: Box::new(Error::Stalled { seconds }) });
							}
							attempts[index] += 1;
						}
						if let Some(pace) = pace.get_mut(index) {
							*pace = 0.0;
						}
					}
					Err(Error::Cancelled) => {
						handle.cancel.cancel();
						while workers.join_next().await.is_some() {}
						return Err(Error::Cancelled);
					}
					// Turned away while others run: the server's limit, not this connection's fault.
					// As many as it is serving now, which is the one number it has shown it takes;
					// the segment back for whoever comes free; no new connection until the wait is
					// over; and no try spent on it.
					Err(e) if crowding(&e, answered) && !active.is_empty() => {
						strikes += 1;
						let lingering = closed_at.is_some_and(|at| at.elapsed() < LINGER) || opened_after_refusal;
						if (active.len() + 1 > served && !lingering) || strikes >= 3 {
							cap = active.len().min(cap);
							strikes = 0;
							handle.learned.store(cap as u64, Ordering::Relaxed);
							handle.crowded.store(true, Ordering::Relaxed);
							settled_at = Instant::now();
						}
						// Whatever it was, no more are opened than are running until the wait is over.
						allowed.store((allowed.load(Ordering::Relaxed) as usize).min(cap).min(active.len()) as u64, Ordering::Relaxed);
						hold = Some(Instant::now() + asked_wait(&e).unwrap_or(settings.retry_wait));
					}
					// Alone and turned away, or failed as the network fails: tried again from where
					// it stands, after a wait that doubles, or the one the server asked for if longer.
					Err(e) if (e.is_transient() || crowding(&e, answered)) && attempts[index] < settings.retries => {
						// Alone and turned away is the limit at one -- unless one of ours closed just
						// now, which the server may still be counting.
						if crowding(&e, answered) && !closed_at.is_some_and(|at| at.elapsed() < LINGER) && !opened_after_refusal {
							cap = 1;
							handle.learned.store(1, Ordering::Relaxed);
							handle.crowded.store(true, Ordering::Relaxed);
							settled_at = Instant::now();
						}
						attempts[index] += 1;
						let backoff = settings.retry_wait * 2u32.pow(attempts[index] - 1);
						let wait = asked_wait(&e).map_or(backoff, |asked| asked.max(backoff));
						tokio::select! {
							_ = tokio::time::sleep(wait) => {}
							_ = handle.cancel.cancelled() => return Err(Error::Cancelled),
						}
					}
					Err(e) => {
						handle.cancel.cancel();
						while workers.join_next().await.is_some() {}
						return Err(match e {
							e if attempts[index] >= settings.retries && (e.is_transient() || crowding(&e, answered)) => Error::GaveUp { tries: attempts[index] + 1, last: Box::new(e) },
							e => e,
						});
					}
				}
				// Only a connection that was being served lingers on the server once closed; one
				// turned away was never counted there as served.
				if answered {
					closed_at = Some(Instant::now());
				}
				if turned_away {
					refused_at = Some(Instant::now());
				}
				let snapshot = plan.lock().unwrap().clone();
				handle.progress.done.store(snapshot.done(), Ordering::Relaxed);
				control::save(&target, &target_control(&snapshot))?;
			}
			_ = ticker.tick() => {
				let now = Instant::now();
				let total = received.load(Ordering::Relaxed);
				let elapsed = now.duration_since(last_tick.0).as_secs_f64();
				if elapsed > 0.0 {
					let instant = ((total - last_tick.1) as f64 / elapsed) as u64;
					let previous = handle.progress.speed.load(Ordering::Relaxed);
					// Smoothed, so the readout does not jitter with every chunk.
					let smoothed = if previous == 0 { instant } else { (previous * 3 + instant) / 4 };
					handle.progress.speed.store(smoothed, Ordering::Relaxed);
				}
				last_tick = (now, total);
				let snapshot = plan.lock().unwrap().clone();
				if elapsed > 0.0 {
					pace.resize(snapshot.segments.len(), 0.0);
					landed.resize(snapshot.segments.len(), 0);
					for (i, segment) in snapshot.segments.iter().enumerate() {
						let instant = segment.done.saturating_sub(landed[i]) as f64 / elapsed;
						pace[i] = if pace[i] == 0.0 { instant } else { pace[i] * 0.75 + instant * 0.25 };
						landed[i] = segment.done;
					}
				}
				served = served.max(answering.values().filter(|a| a.load(Ordering::Relaxed)).count());
				// A server that will not serve ranges cannot be asked to go on from the middle, and
				// under a speed limit a slow connection is the limit working.
				let limited = handle.limit.rate().is_some() || global.rate().is_some();
				if ranges && !limited {
					let answered = |i: &usize| answering.get(i).is_some_and(|a| a.load(Ordering::Relaxed));
					let moving: Vec<(usize, f64)> = active
						.iter()
						.filter(|i| answered(i))
						.filter_map(|&i| pace.get(i).copied().filter(|p| *p > 0.0).map(|p| (i, p)))
						.collect();
					if moving.len() >= 2 {
						peers_pace = moving.iter().map(|(_, p)| *p).fold(0.0, f64::max);
					}
					let alone = active.len() == 1;
					for (&index, w) in watch.iter_mut() {
						let Some(segment) = snapshot.segments.get(index) else { continue };
						if segment.done != w.seen {
							w.seen = segment.done;
							w.moved = now;
						}
						if now.duration_since(w.started) < settings.stall_timeout {
							continue;
						}
						let own = pace.get(index).copied().unwrap_or(0.0);
						let others: Vec<f64> = moving.iter().filter(|(i, _)| *i != index).map(|(_, p)| *p).collect();
						let reference = if others.is_empty() { peers_pace } else { median(others) };
						let crawling = reference > 0.0
							&& own * CRAWL < reference
							&& segment.remaining() as f64 / own.max(1.0) > settings.stall_timeout.as_secs_f64();
						w.slow_since = if crawling { w.slow_since.or(Some(now)) } else { None };
						let stopped = now.duration_since(w.moved) >= settings.stall_timeout && (answered(&index) || alone);
						let crawled = w.slow_since.is_some_and(|since| now.duration_since(since) >= settings.stall_timeout);
						if stopped || crawled {
							w.stop.cancel();
						}
					}
				}
				// Held a while at the limit and never turned away since: one more is asked for, so
				// a limit learnt on a bad moment does not hold the download down for good.
				if cap < ceiling() && active.len() >= cap && now.duration_since(settled_at) >= PROBE_AFTER {
					cap += 1;
					allowed.fetch_add(1, Ordering::Relaxed);
					handle.learned.store(cap as u64, Ordering::Relaxed);
					settled_at = now;
				}
				handle.progress.done.store(snapshot.done(), Ordering::Relaxed);
				control::save(&target, &target_control(&snapshot))?;
			}
			_ = grew.notified() => {}
			_ = handle.cancel.cancelled() => {
				while workers.join_next().await.is_some() {}
				return Err(Error::Cancelled);
			}
		}
	}
}
