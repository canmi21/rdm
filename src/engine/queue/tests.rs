use super::*;
use crate::engine::settings::Connections;
use crate::engine::testing::body;
use crate::engine::testing::{Options, TestServer, Turned};
use crate::testing::scratch;

/// The next event `want` accepts, within twenty seconds; `what` names it in the failure,
/// since a wait that runs out says nothing else about which step stalled.
fn wait_for(
	receiver: &mpsc::Receiver<Event>,
	what: &str,
	mut want: impl FnMut(&Event) -> bool,
) -> Event {
	let deadline = std::time::Instant::now() + Duration::from_secs(20);
	loop {
		let left = deadline.saturating_duration_since(std::time::Instant::now());
		let event = receiver.recv_timeout(left).unwrap_or_else(|e| panic!("waiting for {what}: {e}"));
		if want(&event) {
			return event;
		}
	}
}

#[test]
fn a_host_that_turned_connections_away_is_asked_for_that_many_next_time() {
	let server = TestServer::start(
		body(400_000),
		Options {
			delay_per_chunk: Duration::from_millis(2),
			crowded: Some((2, Turned::Status(503))),
			..Options::default()
		},
	);
	let dir = scratch("hosts");
	let (engine, events) = Engine::new(EngineSettings::default()).unwrap();
	let request = |path: &str| {
		let mut request = Request::new(server.url(path), &dir);
		request.settings.connections = Connections { min: 1, max: 8, auto: true };
		request.settings.min_segment = 1000;
		request.settings.retry_wait = Duration::from_millis(10);
		request
	};
	let first = engine.add(request("/one.bin"), None);
	wait_for(&events, "first completed", |e| matches!(e, Event::Completed(id, _) if *id == first));
	assert_eq!(
		engine.learned_hosts().get("127.0.0.1"),
		Some(&2),
		"turned away past two: {:?}",
		engine.learned_hosts()
	);
	// The second starts at the limit instead of finding it again: never more open at once than
	// the host was learnt to take, where the first went one past it to find out.
	let second = engine.add(request("/two.bin"), None);
	let mut most = 0;
	wait_for(&events, "second completed", |e| {
		if let Event::Progress(snapshot) = e
			&& snapshot.id == second
		{
			most = most.max(snapshot.connections);
		}
		matches!(e, Event::Completed(id, _) if *id == second)
	});
	assert!(most <= 2, "{most} open at once against a host learnt to take two");
	// Handed back at start, what was learnt before is what the engine holds.
	engine.learn_hosts(HashMap::from([("example.com".to_owned(), 3)]));
	assert_eq!(engine.learned_hosts().get("example.com"), Some(&3));
}

#[test]
fn downloads_queue_up_to_the_active_limit_and_report_their_end() {
	let data = body(60_000);
	let server = TestServer::start(
		data.clone(),
		Options { delay_per_chunk: Duration::from_millis(3), ..Options::default() },
	);
	let dir = scratch("queue");
	let (engine, events) =
		Engine::new(EngineSettings { max_active: 1, ..EngineSettings::default() }).unwrap();
	let mut first = Request::new(server.url("/a.bin"), &dir);
	first.settings.connections = Connections { min: 1, max: 1, auto: false };
	let mut second = first.clone();
	second.url = server.url("/b.bin");
	let a = engine.add(first, None);
	let b = engine.add(second, None);
	assert_eq!(engine.snapshot(a).unwrap().status, Status::Running);
	assert_eq!(engine.snapshot(b).unwrap().status, Status::Queued, "one at a time");
	let done = wait_for(&events, "completed", |e| matches!(e, Event::Completed(id, _) if *id == a));
	let Event::Completed(_, finished) = done else { unreachable!() };
	assert_eq!(std::fs::read(&finished.path).unwrap(), data);
	wait_for(&events, "started", |e| matches!(e, Event::Started(id) if *id == b));
	wait_for(&events, "completed", |e| matches!(e, Event::Completed(id, _) if *id == b));
	let snapshots = engine.snapshots();
	assert!(snapshots.iter().all(|s| matches!(s.status, Status::Completed(_))));
	assert_eq!(snapshots[0].file_name.as_deref(), Some("a.bin"));
	assert_eq!(snapshots[0].done, 60_000);
}

/// The queue's story until every one of `ids` has completed: who started, who went back to
/// wait, who finished, in order, by the names given.
fn story(receiver: &mpsc::Receiver<Event>, ids: &[(TaskId, &str)]) -> Vec<String> {
	let name = |id: &TaskId| ids.iter().find(|(i, _)| i == id).map(|(_, n)| *n).unwrap_or("?");
	let mut told = Vec::new();
	let mut done = 0;
	while done < ids.len() {
		let event = wait_for(receiver, "the queue to move", |e| {
			matches!(e, Event::Started(_) | Event::Queued(_) | Event::Completed(..) | Event::Failed(..))
		});
		match &event {
			Event::Started(id) => told.push(format!("start {}", name(id))),
			Event::Queued(id) => told.push(format!("wait {}", name(id))),
			Event::Completed(id, _) => {
				told.push(format!("done {}", name(id)));
				done += 1;
			}
			Event::Failed(id, message) => panic!("{} failed: {message}", name(id)),
			_ => {}
		}
	}
	told
}

/// One place, and downloads slow enough to be caught running.
fn one_at_a_time(label: &str) -> (TestServer, PathBuf, Engine, mpsc::Receiver<Event>, Request) {
	let server = TestServer::start(
		body(400_000),
		Options { delay_per_chunk: Duration::from_millis(4), ..Options::default() },
	);
	let dir = scratch(label);
	let (engine, events) = Engine::new(EngineSettings {
		max_active: 1,
		progress_every: Duration::from_millis(20),
		..EngineSettings::default()
	})
	.unwrap();
	let mut request = Request::new(server.url("/a.bin"), &dir);
	request.settings.connections = Connections::fixed(1);
	(server, dir, engine, events, request)
}

fn named(request: &Request, server: &TestServer, name: &str) -> Request {
	let mut request = request.clone();
	request.url = server.url(&format!("/{name}.bin"));
	request
}

#[test]
fn a_running_download_gives_its_place_to_the_next_and_waits_again() {
	let (server, dir, engine, events, request) = one_at_a_time("yield");
	let a = engine.add(named(&request, &server, "a"), None);
	let b = engine.add(named(&request, &server, "b"), None);
	wait_for(&events, "a moving", |e| matches!(e, Event::Progress(s) if s.id == a && s.done > 0));
	engine.yield_place(a);
	let told = story(&events, &[(a, "a"), (b, "b")]);
	assert_eq!(told, ["wait a", "start b", "done b", "start a", "done a"], "{told:?}");
	assert_eq!(
		std::fs::read(dir.join("a.bin")).unwrap(),
		server.body(),
		"a went on from where it was"
	);
	// With nothing waiting, giving the place away would only start it again, so nothing happens.
	let c = engine.add(named(&request, &server, "c"), None);
	wait_for(&events, "c moving", |e| matches!(e, Event::Progress(s) if s.id == c && s.done > 0));
	engine.yield_place(c);
	assert_eq!(story(&events, &[(c, "c")]), ["done c"]);
}

#[test]
fn a_download_started_now_takes_a_place_and_the_one_it_displaced_goes_next() {
	let (server, _dir, engine, events, request) = one_at_a_time("now");
	let a = engine.add(named(&request, &server, "a"), None);
	let b = engine.add(named(&request, &server, "b"), None);
	let c = engine.add(named(&request, &server, "c"), None);
	wait_for(&events, "a moving", |e| matches!(e, Event::Progress(s) if s.id == a && s.done > 0));
	engine.start_now(c);
	let told = story(&events, &[(a, "a"), (b, "b"), (c, "c")]);
	assert_eq!(
		told,
		["wait a", "start c", "done c", "start a", "done a", "start b", "done b"],
		"c first, then a, which it displaced, before b, which had been waiting: {told:?}"
	);
}

#[test]
fn the_one_that_gives_way_is_the_newest_the_furthest_from_done_or_the_slowest() {
	let now = std::time::Instant::now();
	let running = [
		Candidate { id: TaskId(1), started: now, left: 10.0, speed: 500 },
		Candidate { id: TaskId(2), started: now + Duration::from_secs(5), left: 2.0, speed: 900 },
		Candidate {
			id: TaskId(3),
			started: now + Duration::from_secs(1),
			left: f64::INFINITY,
			speed: 100,
		},
	];
	assert_eq!(give_way(Bump::Newest, &running), Some(TaskId(2)));
	assert_eq!(give_way(Bump::MostLeft, &running), Some(TaskId(3)), "no pace yet is furthest of all");
	assert_eq!(give_way(Bump::Slowest, &running), Some(TaskId(3)));
	assert_eq!(give_way(Bump::Newest, &[]), None);
}

#[test]
fn a_restart_downloads_a_finished_file_again_from_nothing() {
	let (server, dir, engine, events, request) = one_at_a_time("restart");
	let a = engine.add(named(&request, &server, "a"), None);
	wait_for(&events, "a done", |e| matches!(e, Event::Completed(id, _) if *id == a));
	let asked = server.requests().len();
	// The finished file is the caller's to move; here it is simply deleted.
	std::fs::remove_file(dir.join("a.bin")).unwrap();
	engine.restart(a);
	wait_for(&events, "a done again", |e| matches!(e, Event::Completed(id, _) if *id == a));
	assert_eq!(std::fs::read(dir.join("a.bin")).unwrap(), server.body());
	let again: Vec<_> = server.requests().into_iter().skip(asked).collect();
	assert!(
		again.iter().any(|r| r.range.is_some_and(|(start, _)| start == 0)),
		"from the first byte: {again:?}"
	);
}

#[test]
fn a_download_pauses_keeps_its_plan_and_resumes() {
	let data = body(400_000);
	let server = TestServer::start(
		data.clone(),
		Options { delay_per_chunk: Duration::from_millis(10), ..Options::default() },
	);
	let dir = scratch("pause");
	let (engine, events) = Engine::new(EngineSettings {
		progress_every: Duration::from_millis(20),
		..EngineSettings::default()
	})
	.unwrap();
	let mut request = Request::new(server.url("/p.bin"), &dir);
	request.file_name = Some("p.bin".into());
	request.settings.connections = Connections { min: 2, max: 2, auto: false };
	request.settings.min_segment = 1000;
	let id = engine.add(request, None);
	wait_for(
		&events,
		"first progress",
		|e| matches!(e, Event::Progress(s) if s.id == id && s.done > 0),
	);
	engine.pause(id);
	wait_for(&events, "paused", |e| matches!(e, Event::Paused(i) if *i == id));
	let paused = engine.snapshot(id).unwrap();
	assert_eq!(paused.status, Status::Paused);
	assert!(paused.done > 0 && paused.done < 400_000);
	assert!(crate::engine::control::control_path(&dir.join("p.bin")).exists());
	engine.resume(id);
	wait_for(&events, "completed", |e| matches!(e, Event::Completed(i, _) if *i == id));
	assert_eq!(std::fs::read(dir.join("p.bin")).unwrap(), data);
	assert!(!crate::engine::control::control_path(&dir.join("p.bin")).exists());
}

#[test]
fn discarding_takes_the_partial_file_the_server_named_and_a_checksum_guards_the_result() {
	// Long enough that the removal lands mid-download however late the first progress event
	// reaches a test thread starved by the others: a download that had finished would keep
	// its file, and the assertion below would blame the checksum.
	let data = body(400_000);
	let server = TestServer::start(
		data.clone(),
		Options { delay_per_chunk: Duration::from_millis(10), ..Options::default() },
	);
	let dir = scratch("remove");
	let (engine, events) = Engine::new(EngineSettings {
		progress_every: Duration::from_millis(20),
		..EngineSettings::default()
	})
	.unwrap();
	// No name given: the file is named for the address by the probe, and discarding finds it
	// all the same.
	let request = Request::new(server.url("/r.bin"), &dir);
	let id = engine.add(request.clone(), None);
	wait_for(
		&events,
		"first progress",
		|e| matches!(e, Event::Progress(s) if s.id == id && s.done > 0),
	);
	engine.discard(id);
	wait_for(&events, "removed", |e| matches!(e, Event::Removed(i) if *i == id));
	assert!(engine.snapshot(id).is_none());
	// The files go once the download has stopped, a moment after the event.
	let gone = (0..100).any(|_| {
		std::thread::sleep(Duration::from_millis(20));
		!crate::engine::control::part_path(&dir.join("r.bin")).exists()
			&& !crate::engine::control::control_path(&dir.join("r.bin")).exists()
	});
	assert!(gone, "the partial file and the plan are gone");

	let wrong = Checksum::Sha256("0".repeat(64));
	let id = engine.add(request.clone(), Some(wrong));
	let failed = wait_for(&events, "failed", |e| matches!(e, Event::Failed(i, _) if *i == id));
	let Event::Failed(_, message) = failed else { unreachable!() };
	assert!(message.contains("checksum"), "{message}");
	assert!(!dir.join("r.bin").exists(), "a file that fails its checksum is not kept");
	let right = Checksum::Sha256(sha256_hex(&data));
	let id = engine.add(request, Some(right));
	wait_for(&events, "completed", |e| matches!(e, Event::Completed(i, _) if *i == id));
	assert_eq!(std::fs::read(dir.join("r.bin")).unwrap(), data);
}

fn sha256_hex(data: &[u8]) -> String {
	use sha2::Digest;
	sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
}
