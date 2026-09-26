use super::*;
use crate::engine::settings::Connections;
use crate::engine::testing::{Options, Stall, TestServer, Turned, body};
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
async fn the_probes_connection_is_the_first_one_the_download_uses() {
	// Two requests, the probe's and the body's, on one socket: a second socket would be one the
	// server counted twice while it noticed the first close.
	let data = body(30_000);
	let server = TestServer::start(data.clone(), Options::default());
	let dir = scratch("reuse");
	let req = request(&server, &dir, "/reuse.bin", Connections::fixed(1));
	let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
	assert_eq!(std::fs::read(&done.path).unwrap(), data);
	assert_eq!(server.requests().len(), 2);
	assert_eq!(server.accepted(), 1, "the body was asked for on the probe's connection");
}

#[tokio::test]
async fn a_connection_that_stops_sending_is_reopened_from_where_it_stood() {
	// The second half's connection sends 8 KiB and then nothing. Left to the idle timeout it
	// would hold the download for a minute, the whole of it done but that.
	let data = body(200_000);
	let stall = Stall { from: 100_000, after: 8192, pause: Duration::from_secs(30), times: 1 };
	let server =
		TestServer::start(data.clone(), Options { stall: Some(stall), ..Options::default() });
	let dir = scratch("stopped");
	let mut req = request(&server, &dir, "/stop.bin", Connections::fixed(2));
	req.settings.stall_timeout = Duration::from_millis(600);
	let started = Instant::now();
	let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
	assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
	assert_eq!(std::fs::read(&done.path).unwrap(), data);
	let starts: Vec<u64> = server.requests().iter().filter_map(|r| r.range.map(|(s, _)| s)).collect();
	assert!(starts.iter().any(|&s| s > 100_000), "reopened past what had landed: {starts:?}");
}

#[tokio::test]
async fn a_connection_crawling_far_behind_the_others_is_reopened() {
	// Four quarters, three at up to a megabyte a second and the last at ten kilobytes once its
	// first 8 KiB are in: it would take a minute and a half where the others take a second or
	// three. Far enough apart that a runner's coarse sleeps -- a 4 ms one on a macOS runner is
	// nearer 15 -- cannot bring the two within the sixteen times a crawl is judged by, which a
	// crawler at forty kilobytes did there. The watch waits longer than the crawler's pauses,
	// so the crawl is what it sees rather than a stop.
	let data = body(4_000_000);
	let stall = Stall { from: 3_000_000, after: 8192, pause: Duration::from_millis(400), times: 1 };
	let server = TestServer::start(
		data.clone(),
		Options { stall: Some(stall), delay_per_chunk: Duration::from_millis(4), ..Options::default() },
	);
	let dir = scratch("crawl");
	let mut req = request(&server, &dir, "/crawl.bin", Connections::fixed(4));
	req.settings.stall_timeout = Duration::from_millis(1500);
	let started = Instant::now();
	let done = run(req, &Handle::new(), Limiter::unlimited()).await.unwrap();
	assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
	assert_eq!(std::fs::read(&done.path).unwrap(), data);
	let last: Vec<u64> = server
		.requests()
		.iter()
		.filter_map(|r| r.range.map(|(s, _)| s))
		.filter(|&s| s >= 3_000_000)
		.collect();
	assert!(last.len() >= 2 && last[1] > 3_000_000, "the last quarter reopened mid-way: {last:?}");
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
	let h = Arc::new(Handle::new());
	let watched = h.clone();
	// Cancel once both halves have something on disk, not at a moment on the clock: on a busy
	// runner the probe and the connections alone took longer than any moment chosen, and the
	// cancel landed before the first byte; and cancelled when only one half had bytes, the
	// other rightly began again from its start.
	tokio::spawn(async move {
		let deadline = Instant::now() + Duration::from_secs(20);
		let both = || {
			let plan = watched.plan.lock().unwrap().clone();
			plan.is_some_and(|plan| {
				let plan = plan.lock().unwrap();
				plan.segments.len() == 2 && plan.segments.iter().all(|s| s.done > 0)
			})
		};
		while !both() && Instant::now() < deadline {
			tokio::time::sleep(Duration::from_millis(2)).await;
		}
		watched.cancel.cancel();
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
	let server =
		TestServer::start(data.clone(), Options { ranges: false, length: false, ..Options::default() });
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
	assert!(mirror.requests().iter().all(|r| r.if_range.is_none()), "a mirror is not asked If-Range");
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
	let done =
		run(req, &Handle::new(), Limiter::unlimited()).await.expect("finished despite the server");
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
	let server = TestServer::start(vec![0; 10], Options { status: Some(404), ..Options::default() });
	let dir = scratch("refused");
	let req = request(&server, &dir, "/gone", Connections::default());
	assert!(matches!(
		run(req, &Handle::new(), Limiter::unlimited()).await,
		Err(Error::Refused { status: 404 })
	));
	assert_eq!(server.requests().len(), 1);
}
