//! One download from start to finish: probe, plan, open the file, run connections until the
//! plan is complete, and rename. The scheduler here is what turns the planner's arithmetic
//! into connections: it grows their number as the server proves it can take more, retries a
//! segment that fails, and writes the plan beside the file as it goes. See spec/engine.md.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
}

impl Handle {
	pub fn new() -> Handle {
		Handle {
			cancel: CancellationToken::new(),
			progress: Arc::new(Progress::default()),
			limit: Limiter::unlimited(),
			probed: Mutex::new(None),
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
	let single = crate::engine::client::build(&settings, false)?;
	let probed = probe(&single, request.url.clone()).await?;
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

impl LenOrZero for Span {
	fn len_or_zero(self) -> u64 {
		if self.end == u64::MAX { 0 } else { self.len() }
	}
}

/// Connections come and go here until the plan is complete. One at a time on a server without
/// ranges; otherwise up to `max`, each new one allowed once the last has proved itself by
/// delivering a byte, and each taking an idle segment or cutting the largest remainder in two.
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
	let max = if probed.ranges { connections.max as usize } else { 1 };
	// How many connections are allowed right now: starts at `min` and grows by one each time a
	// connection delivers its first byte, up to `max`. Without auto, all of `max` at once.
	let allowed =
		Arc::new(AtomicU64::new(if connections.auto { connections.min as u64 } else { max as u64 }));
	// Rung by a connection's first byte, so the next one is started then and not at the next
	// tick; a file that takes less than a tick would otherwise never see a second connection.
	let grew = Arc::new(Notify::new());
	let received = Arc::new(AtomicU64::new(0));
	let mut workers: JoinSet<(usize, Result<Outcome>)> = JoinSet::new();
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
			let allowed_now = (allowed.load(Ordering::Relaxed) as usize).min(max);
			if active.len() >= allowed_now {
				break;
			}
			let index = {
				let mut plan = plan.lock().unwrap();
				match plan.idle(&active) {
					Some(i) => Some(i),
					None if probed.ranges && connections.auto => plan.steal(settings.min_segment),
					None => None,
				}
			};
			let Some(index) = index else { break };
			if index >= attempts.len() {
				attempts.resize(index + 1, 0);
			}
			active.push(index);
			let split = max > 1;
			let client = crate::engine::client::build(settings, split)?;
			let allowed = allowed.clone();
			let grew = grew.clone();
			let base = plan.lock().unwrap().span.start;
			let received = received.clone();
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
				progress: Arc::new(move |n| {
					if n == 0 {
						allowed.fetch_add(1, Ordering::Relaxed);
						grew.notify_one();
					} else {
						received.fetch_add(n as u64, Ordering::Relaxed);
					}
				}),
			};
			handle.progress.connections.store(active.len() as u64, Ordering::Relaxed);
			workers.spawn(async move { (index, fetch(job).await) });
		}
		if active.is_empty() {
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
				let (index, outcome) = finished.map_err(|e| Error::Disk { path: target.clone(), source: std::io::Error::other(e) })?;
				active.retain(|&i| i != index);
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
					Err(e) if e.is_transient() && attempts[index] < settings.retries => {
						attempts[index] += 1;
						let wait = settings.retry_wait * 2u32.pow(attempts[index] - 1);
						tokio::select! {
							_ = tokio::time::sleep(wait) => {}
							_ = handle.cancel.cancelled() => return Err(Error::Cancelled),
						}
					}
					Err(e) => {
						handle.cancel.cancel();
						while workers.join_next().await.is_some() {}
						return Err(match e {
							e if attempts[index] >= settings.retries && e.is_transient() => Error::GaveUp { tries: attempts[index] + 1, last: Box::new(e) },
							e => e,
						});
					}
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
	use crate::engine::testing::{Options, TestServer};
	use crate::testing::scratch;

	fn body(len: usize) -> Vec<u8> {
		(0..len).map(|i| (i % 251) as u8).collect()
	}

	fn handle() -> Handle {
		Handle::new()
	}

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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		let h = handle();
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), data);
		let starts: Vec<u64> =
			server.requests().iter().skip(1).filter_map(|r| r.range.map(|(s, _)| s)).collect();
		assert_eq!(starts.len(), 3, "three segments from the start: {starts:?}");

		let plain = TestServer::start(data.clone(), Options { ranges: false, ..Options::default() });
		let req = request(&plain, &dir, "/plain.bin", Connections { min: 3, max: 3, auto: false });
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		// Slow enough that the cancel at 60 ms lands mid-file even on a busy machine.
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
		let h = handle();
		let cancel = h.cancel.clone();
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(60)).await;
			cancel.cancel();
		});
		let first = run(req.clone(), &h, Limiter::unlimited()).await;
		assert!(matches!(first, Err(Error::Cancelled)));
		let target = dir.join("res.bin");
		let saved = control::load(&target).unwrap().expect("the plan stays beside the file");
		let done_before = saved.plan.done();
		assert!(done_before > 0 && done_before < data.len() as u64, "stopped part way: {done_before}");
		let requests_before = server.requests().len();
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		let h = handle();
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), fresh);
	}

	#[tokio::test]
	async fn a_range_downloads_only_that_part_and_a_ceiling_refuses_a_large_file() {
		let data = body(10_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = scratch("range");
		let mut req = request(&server, &dir, "/part.bin", Connections { min: 1, max: 2, auto: true });
		req.range = Some((2000, Some(5000)));
		let done = run(req.clone(), &handle(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&done.path).unwrap(), &data[2000..5000]);
		assert_eq!(done.size, 3000);
		req.range = Some((9000, None));
		let tail = run(req.clone(), &handle(), Limiter::unlimited()).await.unwrap();
		assert_eq!(std::fs::read(&tail.path).unwrap(), &data[9000..]);
		req.range = Some((20_000, None));
		assert!(matches!(
			run(req.clone(), &handle(), Limiter::unlimited()).await,
			Err(Error::OutOfRange)
		));
		req.range = None;
		req.settings.max_size = Some(5000);
		assert!(matches!(
			run(req, &handle(), Limiter::unlimited()).await,
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		let done = run(req, &handle(), Limiter::unlimited()).await.unwrap();
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
		assert!(matches!(run(req, &handle(), Limiter::unlimited()).await, Err(Error::Changed)));
	}

	#[tokio::test]
	async fn a_refusal_that_will_not_change_is_not_retried() {
		let server =
			TestServer::start(vec![0; 10], Options { status: Some(404), ..Options::default() });
		let dir = scratch("refused");
		let req = request(&server, &dir, "/gone", Connections::default());
		assert!(matches!(
			run(req, &handle(), Limiter::unlimited()).await,
			Err(Error::Refused { status: 404 })
		));
		assert_eq!(server.requests().len(), 1);
	}
}
