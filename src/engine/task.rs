//! One download from start to finish: probe, plan, open the file, run connections until the
//! plan is complete, and rename. The scheduler here is what turns the planner's arithmetic
//! into connections: it grows their number as the server proves it can take more, retries a
//! segment that fails, and writes the plan beside the file as it goes. See spec/engine.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::engine::control::{self, Control};
use crate::engine::error::{Error, Result};
use crate::engine::limiter::Limiter;
use crate::engine::probe::{Probe, probe};
use crate::engine::segments::{Plan, Span};
use crate::engine::settings::Settings;
use crate::engine::worker::{Job, Outcome, fetch};
use crate::engine::writer::Writer;

/// What to download and where. The name is the server's unless given; the range is the whole
/// file unless given.
#[derive(Clone, Debug)]
pub struct Request {
	pub url: Url,
	pub directory: PathBuf,
	pub file_name: Option<String>,
	/// Only this part of the file, `start..end` with `end` None meaning to the file's end.
	pub range: Option<(u64, Option<u64>)>,
	/// Other addresses of the same file. Connections are spread across them, and a connection
	/// that fails moves to the next; the first address is the one probed and the one whose
	/// validator is trusted, so a mirror is checked by size alone.
	pub mirrors: Vec<Url>,
	pub settings: Settings,
}

impl Request {
	pub fn new(url: Url, directory: impl Into<PathBuf>) -> Request {
		Request {
			url,
			directory: directory.into(),
			file_name: None,
			range: None,
			mirrors: Vec::new(),
			settings: Settings::default(),
		}
	}
}

/// The numbers the window shows, kept current by the connections themselves.
#[derive(Debug, Default)]
pub struct Progress {
	pub done: AtomicU64,
	pub total: AtomicU64,
	pub connections: AtomicU64,
	/// Bytes per second over the last moment, smoothed.
	pub speed: AtomicU64,
}

/// What a finished download hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
	pub path: PathBuf,
	pub size: u64,
	pub probe: Probe,
}

/// The reason connections were stopped, so the caller can tell a pause from a failure.
pub struct Handle {
	pub cancel: CancellationToken,
	pub progress: Arc<Progress>,
	pub limit: Limiter,
	/// What the probe learnt, the moment it learnt it, so a snapshot can name the file and its
	/// size while the download runs rather than only once it is done.
	pub probed: Mutex<Option<Probe>>,
	/// The plan the connections are working through, shared with them rather than copied: the
	/// same `Arc` the scheduler cuts segments out of. A snapshot reads it to say how the file is
	/// being divided and how far each part has come, which is the whole of what a download's
	/// window shows about its workers. Nothing until the plan is made, and left where it was when
	/// the run ends, so a paused download still says how it was cut. See spec/engine.md.
	pub plan: Mutex<Option<Arc<Mutex<Plan>>>>,
	/// The most connections this download may hold open, as it stands now. The scheduler reads it
	/// every time it looks for room rather than keeping the number it started with, so the count
	/// can be changed while the download runs -- which is what a download's own window offers.
	///
	/// **Raising it is felt at once and lowering it is felt as connections finish.** The loop
	/// that reads this only ever starts connections; it has no way to take a byte back from one
	/// that is already reading, and cutting a connection off mid-segment would throw away what it
	/// had. So a lower number is a ceiling the download drifts down to. See spec/engine.md.
	pub ceiling: AtomicU64,
	/// Whether the download grows its own connection count and steals halves of segments, or was
	/// cut into a fixed number at the start. Read beside `ceiling` and changed with it.
	pub auto: AtomicBool,
	/// How many connections the server has shown it will take: written before the run with what
	/// an earlier download from the same host learnt, zero for nothing known, and kept current by
	/// the scheduler as it learns more. See spec/engine.md, "The server decides how many
	/// connections it takes".
	pub learned: AtomicU64,
	/// Whether the server turned a connection away during the run, which is what makes `learned`
	/// a limit rather than only as far as the run happened to go.
	pub crowded: AtomicBool,
}

impl Handle {
	pub fn new() -> Handle {
		Handle {
			cancel: CancellationToken::new(),
			progress: Arc::new(Progress::default()),
			limit: Limiter::unlimited(),
			probed: Mutex::new(None),
			plan: Mutex::new(None),
			// Nothing until the settings are read; the scheduler writes both before it starts.
			ceiling: AtomicU64::new(0),
			learned: AtomicU64::new(0),
			crowded: AtomicBool::new(false),
			auto: AtomicBool::new(true),
		}
	}
}

impl Default for Handle {
	fn default() -> Self {
		Handle::new()
	}
}

/// Runs a download to its end, or until `handle.cancel` is cancelled, in which case the plan is
/// left beside the partial file for a later run to continue. `global` is the engine's limiter,
/// shared with every other download.
pub async fn run(request: Request, handle: &Handle, global: Limiter) -> Result<Finished> {
	let settings =
		Settings { connections: request.settings.connections.clamped(), ..request.settings.clone() };
	// The probe's client goes as soon as it has answered, and its connection with it: kept, the
	// connection would sit idle in the pool for the whole download, holding one of the places a
	// server limiting connections per client counts. See spec/engine.md, "The server decides how
	// many connections it takes".
	let probed = {
		let single = crate::engine::client::build(&settings, false)?;
		probe(&single, request.url.clone()).await?
	};
	*handle.probed.lock().unwrap() = Some(probed.clone());
	if let (Some(size), Some(limit)) = (probed.size, settings.max_size)
		&& size > limit
	{
		return Err(Error::TooLarge { size, limit });
	}
	let file_name = request.file_name.clone().unwrap_or_else(|| probed.file_name.clone());
	let target = request.directory.join(&file_name);

	// The span: the user's range clipped to the file, or the whole file, or -- when the server
	// would not say how long the file is -- open-ended, to be closed when the body ends.
	let span = match (request.range, probed.size) {
		(Some((start, end)), Some(size)) => {
			let end = end.map_or(size, |e| e.min(size));
			if start >= size || start >= end {
				return Err(Error::OutOfRange);
			}
			if !probed.ranges && start > 0 {
				return Err(Error::NoRanges);
			}
			Span::new(start, end)
		}
		(Some((start, end)), None) => {
			if !probed.ranges {
				return Err(Error::NoRanges);
			}
			Span::new(start, end.unwrap_or(u64::MAX))
		}
		(None, Some(size)) => Span::new(0, size),
		(None, None) => Span::new(0, u64::MAX),
	};
	let open_ended = span.end == u64::MAX;
	let validator = probed.validator().map(str::to_owned);

	// A plan left by an earlier run continues if it is for the same file: same address, same
	// span, and the same validator when the server gives one. Otherwise it is discarded and
	// the download starts over, since bytes from a different file are worth nothing.
	let saved = control::load(&target)?.filter(|c| {
		c.url == probed.url.as_str()
			&& c.plan.span == span
			&& (c.validator.is_none() || c.validator == validator)
			&& probed.ranges
	});
	let plan = match saved {
		Some(control) => control.plan,
		None if probed.ranges && !settings.connections.auto && !open_ended => {
			Plan::split(span, settings.connections.max, settings.min_segment)
		}
		None => Plan::whole(span),
	};
	let plan = Arc::new(Mutex::new(plan));
	// The same plan the connections cut and fill, handed to the handle so a snapshot can read it
	// without the scheduler having to report anything.
	*handle.plan.lock().unwrap() = Some(plan.clone());
	let writer =
		Writer::open(&target, (!open_ended).then(|| span.len()), settings.preallocate && !open_ended)?;
	let controls = Control::new(
		probed.url.as_str(),
		probed.size,
		validator.as_deref(),
		plan.lock().unwrap().clone(),
	);
	control::save(&target, &controls)?;

	handle.progress.total.store(span.len_or_zero(), Ordering::Relaxed);
	handle.progress.done.store(plan.lock().unwrap().done(), Ordering::Relaxed);
	handle.limit.set_rate(settings.speed_limit);

	let mut sources = vec![probed.url.clone()];
	sources.extend(request.mirrors.iter().cloned());
	let result =
		schedule(&settings, &probed, &sources, validator, plan.clone(), writer.clone(), handle, global)
			.await;
	match result {
		Ok(()) => {
			let size = plan.lock().unwrap().span.len();
			let path = writer.finish(Some(size)).await?;
			control::remove(&target);
			Ok(Finished { path, size, probe: probed })
		}
		Err(e) => {
			// Whatever happened, the plan is written so a later run knows where things stand;
			// a cancelled download is a paused one until somebody removes its files.
			let _ = control::save(
				&target,
				&Control::new(
					probed.url.as_str(),
					probed.size,
					controls.validator.as_deref(),
					plan.lock().unwrap().clone(),
				),
			);
			Err(e)
		}
	}
}

trait LenOrZero {
	fn len_or_zero(self) -> u64;
}

/// How long a limit has to hold, unchallenged and in full use, before one more connection is asked
/// for.
const PROBE_AFTER: Duration = Duration::from_secs(10);

/// How long a connection of ours may go on being counted by a server after we closed it: the
/// round trip for it to notice, with room to spare.
const LINGER: Duration = Duration::from_millis(500);

/// Whether a failure is the server turning a connection away rather than the network failing:
/// too busy, a refusal, or a connection closed before it delivered a byte. A refusal and a close
/// only count while another connection is running, which the caller checks; on their own they
/// are the ordinary failures they look like. See spec/engine.md, "The server decides how many
/// connections it takes".
fn crowding(error: &Error, answered: bool) -> bool {
	match error {
		Error::Busy { .. } => true,
		Error::Refused { status: 403 } => true,
		Error::Http(_) => !answered,
		_ => false,
	}
}

/// The wait a server asked for with its refusal, held to a minute.
fn asked_wait(error: &Error) -> Option<Duration> {
	match error {
		Error::Busy { retry_after: Some(wait), .. } => Some((*wait).min(Duration::from_secs(60))),
		_ => None,
	}
}

impl LenOrZero for Span {
	fn len_or_zero(self) -> u64 {
		if self.end == u64::MAX { 0 } else { self.len() }
	}
}

/// Connections come and go here until the plan is complete. One at a time on a server without
/// ranges; otherwise up to `handle.ceiling`, which the window may move while this runs, each new
/// one allowed once the last has proved itself by delivering a byte, and each taking an idle
/// segment or cutting the largest remainder in two.
#[allow(clippy::too_many_arguments)]
async fn schedule(
	settings: &Settings,
	probed: &Probe,
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
	// waited out the same way. The probe's is not one: its client is dropped as it answers.
	let mut closed_at: Option<Instant> = None;
	let mut answering: HashMap<usize, Arc<AtomicBool>> = HashMap::new();
	// No new connection before this, after one was turned away: the wait the server asked for,
	// or the retry wait.
	let mut hold: Option<Instant> = None;
	// Each segment's pace in bytes a second, smoothed, for cutting the one that will finish last.
	let mut pace: Vec<f64> = Vec::new();
	let mut landed: Vec<u64> = Vec::new();
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
						let typical = if known.is_empty() { 1.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
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
			let client = crate::engine::client::build(settings, split)?;
			let allowed = allowed.clone();
			let grew = grew.clone();
			// Whether this connection delivered a byte: one turned away before it did is the
			// server saying no, one that failed after is the network.
			let first = Arc::new(AtomicBool::new(false));
			let answered = first.clone();
			answering.insert(index, first.clone());
			let base = plan.lock().unwrap().span.start;
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
				cancel: handle.cancel.clone(),
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
				if answered {
					strikes = 0;
				}
				handle.progress.connections.store(active.len() as u64, Ordering::Relaxed);
				match outcome {
					Ok(Outcome::Complete) => {}
					Ok(Outcome::EndOfFile(size)) => {
						handle.progress.total.store(size, Ordering::Relaxed);
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
						let lingering = closed_at.is_some_and(|at| at.elapsed() < LINGER);
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
						if crowding(&e, answered) && !closed_at.is_some_and(|at| at.elapsed() < LINGER) {
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

/// Removes what a download left behind: the partial file and the plan. For a download the
/// user does not want continued.
pub fn discard(directory: &Path, file_name: &str) {
	let target = directory.join(file_name);
	let _ = std::fs::remove_file(control::part_path(&target));
	control::remove(&target);
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::settings::Connections;
	use crate::engine::testing::{Options, TestServer, Turned, body};
	use crate::testing::scratch;

	fn request(server: &TestServer, dir: &Path, path: &str, connections: Connections) -> Request {
		let mut request = Request::new(server.url(path), dir);
		request.settings.connections = connections;
		request.settings.min_segment = 1000;
		request.settings.retry_wait = Duration::from_millis(10);
		request
	}

	#[tokio::test]
	async fn a_single_connection_downloads_the_whole_file_and_names_it() {
		let data = body(20_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = scratch("single");
		let req = request(&server, &dir, "/files/one.bin", Connections { min: 1, max: 1, auto: false });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(done.path, dir.join("one.bin"));
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		assert!(!control::control_path(&done.path).exists(), "the plan is removed when done");
		assert_eq!(server.peak_connections(), 1);
	}

	#[tokio::test]
	async fn connections_grow_to_the_limit_and_every_byte_lands_once() {
		let data = body(200_000);
		let server = TestServer::start(
			data.clone(),
			Options { delay_per_chunk: Duration::from_millis(5), ..Options::default() },
		);
		let dir = scratch("grow");
		let req = request(&server, &dir, "/big.bin", Connections { min: 1, max: 4, auto: true });
		let h = Handle::new();
		// The engine's own count of connections in flight, sampled while it runs; the server's
		// count runs high, since a connection dropped by a worker stays open on that side until
		// its writes fail.
		let progress = h.progress.clone();
		let peak = Arc::new(AtomicU64::new(0));
		let sampler = {
			let peak = peak.clone();
			tokio::spawn(async move {
				loop {
					peak.fetch_max(progress.connections.load(Ordering::Relaxed), Ordering::Relaxed);
					tokio::time::sleep(Duration::from_millis(1)).await;
				}
			})
		};
		let done = run(req, &h, Limiter::unlimited()).await.unwrap();
		sampler.abort();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		let peak = peak.load(Ordering::Relaxed);
		assert!((2..=4).contains(&peak), "grew past one and never past four: {peak}");
		let ranges: Vec<_> = server.requests().iter().filter_map(|r| r.range).collect();
		assert!(
			ranges.iter().any(|(start, _)| *start > 0),
			"later connections start mid-file: {ranges:?}"
		);
	}

	#[tokio::test]
	async fn a_fixed_count_splits_at_once_and_a_server_without_ranges_gets_one() {
		let data = body(50_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = scratch("fixed");
		let req = request(&server, &dir, "/f.bin", Connections { min: 3, max: 3, auto: false });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		let starts: Vec<u64> =
			server.requests().iter().skip(1).filter_map(|r| r.range.map(|(s, _)| s)).collect();
		assert_eq!(starts.len(), 3, "three segments from the start: {starts:?}");

		let plain = TestServer::start(data.clone(), Options { ranges: false, ..Options::default() });
		let req = request(&plain, &dir, "/plain.bin", Connections { min: 3, max: 3, auto: false });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		assert_eq!(plain.peak_connections(), 1);
	}

	#[tokio::test]
	async fn a_dropped_connection_is_retried_from_where_it_stopped() {
		let data = body(30_000);
		let server = TestServer::start(
			data.clone(),
			Options { fail_after: Some(8192), fail_times: 2, ..Options::default() },
		);
		let dir = scratch("retry");
		let req = request(&server, &dir, "/r.bin", Connections { min: 1, max: 1, auto: false });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		let starts: Vec<u64> =
			server.requests().iter().skip(1).filter_map(|r| r.range.map(|(s, _)| s)).collect();
		assert_eq!(starts.len(), 3, "first try, two retries: {starts:?}");
		assert!(
			starts[1] >= 8192 && starts[2] >= starts[1],
			"each retry continues, never restarts: {starts:?}"
		);
		assert!(
			server.requests().iter().skip(2).all(|r| r.if_range.is_none()),
			"no validator, so no If-Range"
		);
	}

	#[tokio::test]
	async fn a_cancelled_download_resumes_from_its_plan_in_a_later_run() {
		// Slow enough that the cancel, sent once bytes have landed, comes before the end.
		let data = body(400_000);
		let server = TestServer::start(
			data.clone(),
			Options {
				etag: Some("\"same\"".into()),
				delay_per_chunk: Duration::from_millis(5),
				..Options::default()
			},
		);
		let dir = scratch("resume");
		let req = request(&server, &dir, "/res.bin", Connections { min: 2, max: 2, auto: false });
		let h = Handle::new();
		let cancel = h.cancel.clone();
		let progress = h.progress.clone();
		// Cancel once something is on disk, not at a moment on the clock: on a busy runner the
		// probe and the connections alone took longer than any moment chosen, and the cancel
		// landed before the first byte. The worker marks the plan before it reports, so a
		// report of bytes means the plan has them.
		tokio::spawn(async move {
			let deadline = Instant::now() + Duration::from_secs(20);
			while progress.done.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
				tokio::time::sleep(Duration::from_millis(2)).await;
			}
			cancel.cancel();
		});
		let first = run(req.clone(), &h, Limiter::unlimited()).await;
		assert!(matches!(first, Err(Error::Cancelled)));
		let target = dir.join("res.bin");
		let saved = control::load(&target).unwrap().expect("the plan stays beside the file");
		let done_before = saved.plan.done();
		assert!(done_before > 0 && done_before < data.len() as u64, "stopped part way: {done_before}");
		let requests_before = server.requests().len();
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		let all = server.requests();
		let resumed: Vec<_> = all.iter().skip(requests_before + 1).collect();
		assert!(
			resumed.iter().all(|r| r.range.is_some_and(|(s, _)| s > 0)),
			"continued mid-file: {resumed:?}"
		);
		assert!(
			resumed.iter().all(|r| r.if_range.as_deref() == Some("\"same\"")),
			"the validator rides along"
		);
	}

	#[tokio::test]
	async fn a_file_that_changed_on_the_server_is_not_spliced() {
		let data = body(60_000);
		let server = TestServer::start(
			data.clone(),
			Options {
				etag: Some("\"v1\"".into()),
				delay_per_chunk: Duration::from_millis(5),
				..Options::default()
			},
		);
		let dir = scratch("changed");
		let req = request(&server, &dir, "/c.bin", Connections { min: 1, max: 1, auto: false });
		let h = Handle::new();
		let cancel = h.cancel.clone();
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(40)).await;
			cancel.cancel();
		});
		assert!(run(req.clone(), &h, Limiter::unlimited()).await.is_err());
		let fresh = body(60_000).into_iter().rev().collect::<Vec<u8>>();
		server.set_body(fresh.clone());
		server.set_options(|o| o.etag = Some("\"v2\"".into()));
		// The probe sees a new validator, the old plan is discarded, and the new file is fetched whole.
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), fresh);
	}

	#[tokio::test]
	async fn a_range_downloads_only_that_part_and_a_ceiling_refuses_a_large_file() {
		let data = body(10_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = scratch("range");
		let mut req = request(&server, &dir, "/part.bin", Connections { min: 1, max: 2, auto: true });
		req.range = Some((2000, Some(5000)));
		let done = run(req.clone(), &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), &data[2000..5000]);
		assert_eq!(done.size, 3000);
		req.range = Some((9000, None));
		let tail = run(req.clone(), &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&tail.path).unwrap(), &data[9000..]);
		req.range = Some((20_000, None));
		assert!(matches!(
			run(req.clone(), &Handle::new(), Limiter::unlimited()).await,
			Err(Error::OutOfRange)
		));
		req.range = None;
		req.settings.max_size = Some(5000);
		assert!(matches!(
			run(req, &Handle::new(), Limiter::unlimited()).await,
			Err(Error::TooLarge { size: 10_000, limit: 5000 })
		));
	}

	#[tokio::test]
	async fn a_body_without_a_length_is_taken_to_its_end() {
		let data = body(33_333);
		let server = TestServer::start(
			data.clone(),
			Options { ranges: false, length: false, ..Options::default() },
		);
		let dir = scratch("chunked");
		let req = request(&server, &dir, "/stream", Connections { min: 1, max: 4, auto: true });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		assert_eq!(done.size, 33_333);
	}

	#[tokio::test]
	async fn a_speed_limit_holds_the_transfer_to_the_rate() {
		let data = body(120_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = scratch("limit");
		let mut req = request(&server, &dir, "/slow.bin", Connections { min: 2, max: 2, auto: false });
		req.settings.speed_limit = Some(40_000);
		let start = Instant::now();
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		let elapsed = start.elapsed();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		// One second's worth is in the bucket already; the other 80 000 bytes at 40 000/s are
		// earned over the two seconds after.
		assert!(
			elapsed >= Duration::from_millis(1500) && elapsed < Duration::from_secs(5),
			"{elapsed:?}"
		);
	}

	#[tokio::test]
	async fn a_mirror_takes_over_when_the_first_source_keeps_failing() {
		let data = body(60_000);
		let flaky = TestServer::start(
			data.clone(),
			Options { fail_after: Some(4096), etag: Some("\"a\"".into()), ..Options::default() },
		);
		let mirror =
			TestServer::start(data.clone(), Options { etag: Some("\"b\"".into()), ..Options::default() });
		let dir = scratch("mirror");
		let mut req = request(&flaky, &dir, "/m.bin", Connections { min: 1, max: 1, auto: false });
		req.mirrors = vec![mirror.url("/m.bin")];
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		assert!(
			mirror.requests().iter().all(|r| r.if_range.is_none()),
			"a mirror is not asked If-Range"
		);
		assert!(!mirror.requests().is_empty(), "the mirror was used");

		// A mirror serving a different file is refused by its size.
		let other = TestServer::start(body(61_000), Options::default());
		let flaky =
			TestServer::start(data.clone(), Options { fail_after: Some(4096), ..Options::default() });
		let mut req = request(&flaky, &dir, "/n.bin", Connections { min: 1, max: 1, auto: false });
		req.mirrors = vec![other.url("/n.bin")];
		assert!(matches!(run(req, &Handle::new(), Limiter::unlimited()).await, Err(Error::Changed)));
	}

	/// A download against a server that turns away connections past `limit` the way `turned`
	/// says: it has to finish, every byte right, without failing for it.
	async fn crowded(name: &str, limit: usize, turned: Turned) -> TestServer {
		let data = body(400_000);
		let server = TestServer::start(
			data.clone(),
			Options {
				delay_per_chunk: Duration::from_millis(2),
				crowded: Some((limit, turned)),
				..Options::default()
			},
		);
		let dir = scratch(name);
		let req = request(&server, &dir, "/crowded.bin", Connections { min: 1, max: 8, auto: true });
		let done = run(req, &Handle::new(), Limiter::unlimited()).await.expect("finished despite the server");
		assert_eq!(std::fs::read(&done.path).unwrap(), data, "every byte, once");
		// Turned away past the limit, the download holds at what the server takes and waits a
		// refusal out, rather than opening a new connection into it again and again: before this,
		// the busy server saw two dozen requests, most of them turned away. The first round asks
		// for four at once and each answer asks for two more, so a round or so is turned away
		// before the limit is found, and a refusal more is a connection of ours still closing.
		let turned = server.turned_away();
		assert!(turned <= 8, "{name}: turned away {turned} times");
		server
	}

	#[tokio::test]
	async fn a_server_that_answers_busy_past_two_connections_is_downloaded_on_fewer() {
		crowded("busy", 2, Turned::Status(503)).await;
	}

	#[tokio::test]
	async fn a_server_that_forbids_extra_connections_is_downloaded_on_fewer() {
		crowded("forbids", 2, Turned::Status(403)).await;
	}

	#[tokio::test]
	async fn a_server_that_closes_extra_connections_is_downloaded_on_fewer() {
		crowded("closes", 2, Turned::Closed).await;
	}

	#[tokio::test]
	async fn a_server_that_takes_one_connection_is_downloaded_on_one() {
		crowded("one", 1, Turned::Status(403)).await;
	}

	#[tokio::test]
	async fn a_refusal_that_will_not_change_is_not_retried() {
		let server =
			TestServer::start(vec![0; 10], Options { status: Some(404), ..Options::default() });
		let dir = scratch("refused");
		let req = request(&server, &dir, "/gone", Connections::default());
		assert!(matches!(
			run(req, &Handle::new(), Limiter::unlimited()).await,
			Err(Error::Refused { status: 404 })
		));
		assert_eq!(server.requests().len(), 1);
	}
}
