//! The engine as the window sees it: a queue of downloads, a few at a time, each pausable,
//! resumable and removable, reporting through events and answering for its state on request.
//! It owns the tokio runtime everything below runs on, and nothing it hands out is a future,
//! so the caller's executor -- gpui's, or a test's -- is never involved. See spec/engine.md.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use crate::engine::error::{Error, Result};
use crate::engine::inspect::{self, Inspection};
use crate::engine::limiter::Limiter;
use crate::engine::segments::Segment;
use crate::engine::settings::Connections;
use crate::engine::task::{self, Finished, Handle, Progress, Request};
use crate::engine::verify::{self, Checksum};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

/// What the engine as a whole is told: how many downloads run at once, and a limit on their sum.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EngineSettings {
	pub max_active: usize,
	pub speed_limit: Option<u64>,
	/// How often a Progress event is sent for each running download.
	pub progress_every: Duration,
}

impl Default for EngineSettings {
	fn default() -> Self {
		EngineSettings { max_active: 3, speed_limit: None, progress_every: Duration::from_millis(500) }
	}
}

/// Where a download stands. `Failed` keeps the message, since the error itself is not Clone;
/// `Completed` boxes its result so the other variants stay small.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
	Queued,
	Running,
	Paused,
	Completed(Box<Finished>),
	Failed(String),
}

/// A download's state at one moment, for the window to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
	pub id: TaskId,
	pub url: String,
	pub file_name: Option<String>,
	pub status: Status,
	pub done: u64,
	/// Zero until the server said, or for a file of unknown length.
	pub total: u64,
	pub speed: u64,
	pub connections: u64,
	/// What `verify::kind` read from the finished file, if anything.
	pub kind: Option<&'static str>,
	/// How the file is being divided and how far each part has come, in the order the planner
	/// made them -- which is not the order they lie in the file, a stolen half being made after
	/// the segments on either side of it. Empty before the plan is made and for a download that
	/// was never split. A window that draws these sorts by position first, as everything that
	/// judges a plan does. See spec/engine.md.
	pub segments: Vec<Segment>,
	/// Whether the server serves ranges -- so the download can be resumed and split -- once the
	/// probe has said; None before.
	pub ranges: Option<bool>,
}

impl Snapshot {
	/// How long the rest will take at the current speed; None while nothing is known.
	pub fn remaining(&self) -> Option<Duration> {
		if self.speed == 0 || self.total == 0 || self.done >= self.total {
			return None;
		}
		Some(Duration::from_secs((self.total - self.done) / self.speed))
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
	Started(TaskId),
	Progress(Snapshot),
	Completed(TaskId, Finished),
	Failed(TaskId, String),
	Paused(TaskId),
	Removed(TaskId),
}

struct Entry {
	request: Request,
	checksum: Option<Checksum>,
	handle: Arc<Handle>,
	status: Status,
	kind: Option<&'static str>,
	running: Option<tokio::task::JoinHandle<()>>,
}

struct Inner {
	next: u64,
	entries: HashMap<TaskId, Entry>,
	settings: EngineSettings,
	global: Limiter,
	events: mpsc::Sender<Event>,
	/// How many connections each host has shown it will take, learnt by downloads from it: the
	/// next download from a host starts there rather than finding out again. A host not here has
	/// shown no limit. See spec/engine.md, "The server decides how many connections it takes".
	hosts: HashMap<String, u16>,
}

/// The host a download's connections go to, as `hosts` keys it.
fn host_of(request: &Request) -> String {
	request.url.host_str().unwrap_or_default().to_ascii_lowercase()
}

/// The engine. Cheap to clone; every clone is the same engine.
#[derive(Clone)]
pub struct Engine {
	runtime: Arc<Runtime>,
	inner: Arc<Mutex<Inner>>,
}

impl Engine {
	/// Starts the runtime. The receiver gets every event; the window reads it at its own pace,
	/// since the sender never blocks.
	pub fn new(settings: EngineSettings) -> Result<(Engine, mpsc::Receiver<Event>)> {
		let runtime = tokio::runtime::Builder::new_multi_thread()
			.enable_all()
			.thread_name("rdm-engine")
			.build()
			.map_err(|source| Error::Disk { path: PathBuf::new(), source })?;
		let (events, receiver) = mpsc::channel();
		let inner = Inner {
			next: 1,
			entries: HashMap::new(),
			global: Limiter::new(settings.speed_limit),
			settings,
			events,
			hosts: HashMap::new(),
		};
		Ok((Engine { runtime: Arc::new(runtime), inner: Arc::new(Mutex::new(inner)) }, receiver))
	}

	/// Queues a download; it starts when fewer than `max_active` are running.
	pub fn add(&self, request: Request, checksum: Option<Checksum>) -> TaskId {
		let id = TaskId(self.inner.lock().unwrap().next);
		self.add_with_id(id, request, checksum);
		id
	}

	/// Queues a download under an id the caller chose -- the row's id in its own store, so the
	/// two never need mapping. Ids handed out afterwards continue above it.
	pub fn add_with_id(&self, id: TaskId, request: Request, checksum: Option<Checksum>) {
		{
			let mut inner = self.inner.lock().unwrap();
			inner.next = inner.next.max(id.0 + 1);
			inner.entries.insert(
				id,
				Entry {
					request,
					checksum,
					handle: Arc::new(Handle::new()),
					status: Status::Queued,
					kind: None,
					running: None,
				},
			);
		}
		self.pump();
	}

	pub fn contains(&self, id: TaskId) -> bool {
		self.inner.lock().unwrap().entries.contains_key(&id)
	}

	/// Stops the connections and keeps the plan; `resume` picks it up.
	pub fn pause(&self, id: TaskId) {
		let mut inner = self.inner.lock().unwrap();
		let Some(entry) = inner.entries.get_mut(&id) else { return };
		match entry.status {
			Status::Running => entry.handle.cancel.cancel(),
			Status::Queued => {
				entry.status = Status::Paused;
				let _ = inner.events.send(Event::Paused(id));
			}
			_ => {}
		}
	}

	/// Queues a paused or failed download again; a completed one is left alone.
	pub fn resume(&self, id: TaskId) {
		{
			let mut inner = self.inner.lock().unwrap();
			let Some(entry) = inner.entries.get_mut(&id) else { return };
			if matches!(entry.status, Status::Paused | Status::Failed(_)) {
				entry.status = Status::Queued;
				entry.handle = Arc::new(Handle::new());
			}
		}
		self.pump();
	}

	/// Forgets the download and leaves its files where they are: a partial file with its plan
	/// beside it, which a later download of the same address continues, or a finished file.
	pub fn forget(&self, id: TaskId) {
		self.remove(id, false);
	}

	/// Forgets the download and throws away what it left unfinished, the partial file and the
	/// plan. A finished file is never deleted here, since it is the user's now.
	pub fn discard(&self, id: TaskId) {
		self.remove(id, true);
	}

	fn remove(&self, id: TaskId, delete: bool) {
		let removed = {
			let mut inner = self.inner.lock().unwrap();
			let entry = inner.entries.remove(&id);
			if let Some(entry) = &entry {
				entry.handle.cancel.cancel();
				let _ = inner.events.send(Event::Removed(id));
			}
			entry
		};
		if let (Some(entry), true) = (removed, delete)
			&& !matches!(entry.status, Status::Completed(_))
		{
			let directory = entry.request.directory.clone();
			let handle = entry.handle.clone();
			let asked = entry.request.file_name.clone();
			// The files go once the download has actually stopped: it writes its plan on the way
			// out, and a plan written after the discard would be a ghost. The name is the one the
			// download wrote under -- the caller's, else the server's, which only the probe learnt
			// and is read once the probe is over.
			let running = entry.running;
			self.runtime.spawn(async move {
				if let Some(running) = running {
					let _ = running.await;
				}
				let name =
					asked.or_else(|| handle.probed.lock().unwrap().as_ref().map(|p| p.file_name.clone()));
				if let Some(name) = name {
					task::discard(&directory, &name);
				}
			});
		}
		self.pump();
	}

	/// Looks at an address without downloading it: what the server says it is, and, when it is
	/// a web page, the files that page links to. The answer arrives on the returned channel,
	/// which the caller polls the way it polls events; an address that could not be reached
	/// arrives as the error's summary and its whole text.
	pub fn inspect(
		&self,
		url: reqwest::Url,
	) -> mpsc::Receiver<std::result::Result<Inspection, crate::engine::Failure>> {
		let (sender, receiver) = mpsc::channel();
		let settings = crate::engine::Settings::default();
		self.runtime.spawn(async move {
			let result = match crate::engine::client::build(&settings, false) {
				Ok(client) => inspect::inspect(&client, url).await.map_err(|e| e.failure()),
				Err(e) => Err(e.failure()),
			};
			let _ = sender.send(result);
		});
		receiver
	}

	/// Runs one future on the runtime for whoever has none: the answer arrives on the returned
	/// channel, polled the way events are. The window's update check is the one caller.
	pub fn run<T: Send + 'static>(
		&self,
		future: impl std::future::Future<Output = T> + Send + 'static,
	) -> mpsc::Receiver<T> {
		let (sender, receiver) = mpsc::channel();
		self.runtime.spawn(async move {
			let _ = sender.send(future.await);
		});
		receiver
	}

	/// How many connections each host has shown it will take, for the window to keep between runs.
	pub fn learned_hosts(&self) -> HashMap<String, u16> {
		self.inner.lock().unwrap().hosts.clone()
	}

	/// What earlier runs learnt of each host, handed back at start.
	pub fn learn_hosts(&self, hosts: HashMap<String, u16>) {
		self.inner.lock().unwrap().hosts = hosts;
	}

	pub fn snapshot(&self, id: TaskId) -> Option<Snapshot> {
		let inner = self.inner.lock().unwrap();
		inner.entries.get(&id).map(|entry| snapshot_of(id, entry))
	}

	pub fn snapshots(&self) -> Vec<Snapshot> {
		let inner = self.inner.lock().unwrap();
		let mut all: Vec<Snapshot> = inner.entries.iter().map(|(id, e)| snapshot_of(*id, e)).collect();
		all.sort_by_key(|s| s.id);
		all
	}

	pub fn set_speed_limit(&self, limit: Option<u64>) {
		let mut inner = self.inner.lock().unwrap();
		inner.settings.speed_limit = limit;
		inner.global.set_rate(limit);
	}

	/// A running download's own limit, changed in place.
	pub fn set_task_speed_limit(&self, id: TaskId, limit: Option<u64>) {
		let mut inner = self.inner.lock().unwrap();
		if let Some(entry) = inner.entries.get_mut(&id) {
			entry.request.settings.speed_limit = limit;
			entry.handle.limit.set_rate(limit);
		}
	}

	/// A running download's connection count, changed in place. The scheduler reads the ceiling
	/// every time it looks for room, so a higher number opens connections at once and a lower one
	/// is a ceiling the download drifts down to as its connections finish -- there is no way to
	/// take a byte back from one that is already reading. The request is changed too, so the
	/// number survives a pause and is what a later run starts from. See spec/engine.md.
	pub fn set_task_connections(&self, id: TaskId, connections: Connections) {
		let mut inner = self.inner.lock().unwrap();
		if let Some(entry) = inner.entries.get_mut(&id) {
			entry.request.settings.connections = connections;
			// Not raised above one where the server never offered ranges: the scheduler pinned it
			// there for a reason, and a ceiling it cannot honour is a number that lies.
			if entry.handle.ceiling.load(Ordering::Relaxed) > 1 {
				entry.handle.ceiling.store(connections.max.max(1) as u64, Ordering::Relaxed);
				entry.handle.auto.store(connections.auto, Ordering::Relaxed);
			}
		}
	}

	pub fn set_max_active(&self, max: usize) {
		self.inner.lock().unwrap().settings.max_active = max.max(1);
		self.pump();
	}

	/// Starts queued downloads while there is room. Called after anything that could make room.
	fn pump(&self) {
		let mut inner = self.inner.lock().unwrap();
		let running = inner.entries.values().filter(|e| e.status == Status::Running).count();
		let room = inner.settings.max_active.saturating_sub(running);
		let mut queued: Vec<TaskId> =
			inner.entries.iter().filter(|(_, e)| e.status == Status::Queued).map(|(id, _)| *id).collect();
		queued.sort();
		for id in queued.into_iter().take(room) {
			let global = inner.global.clone();
			let every = inner.settings.progress_every;
			let events = inner.events.clone();
			let learned =
				inner.entries.get(&id).and_then(|e| inner.hosts.get(&host_of(&e.request)).copied());
			let entry = inner.entries.get_mut(&id).expect("just listed");
			entry.handle.learned.store(learned.map_or(0, u64::from), Ordering::Relaxed);
			entry.handle.crowded.store(false, Ordering::Relaxed);
			entry.status = Status::Running;
			let _ = events.send(Event::Started(id));
			let job = self.clone();
			let request = entry.request.clone();
			let checksum = entry.checksum.clone();
			let handle = entry.handle.clone();
			let running = self
				.runtime
				.spawn(async move { job.drive(id, request, checksum, handle, global, every).await });
			inner.entries.get_mut(&id).expect("just listed").running = Some(running);
		}
	}

	/// One download's life on the runtime: progress events while it runs, then its end.
	async fn drive(
		self,
		id: TaskId,
		request: Request,
		checksum: Option<Checksum>,
		handle: Arc<Handle>,
		global: Limiter,
		every: Duration,
	) {
		let reporter = {
			let engine = self.clone();
			let cancel = handle.cancel.clone();
			tokio::spawn(async move {
				let mut ticker = tokio::time::interval(every);
				loop {
					tokio::select! {
						_ = ticker.tick() => {
							let snapshot = engine.snapshot(id);
							let events = engine.inner.lock().unwrap().events.clone();
							if let Some(snapshot) = snapshot {
								let _ = events.send(Event::Progress(snapshot));
							}
						}
						_ = cancel.cancelled() => break,
					}
				}
			})
		};
		let result = task::run(request.clone(), &handle, global).await;
		let result = match result {
			Ok(finished) => match &checksum {
				Some(checksum) => match verify::verify(&finished.path, checksum).await {
					Ok(()) => Ok(finished),
					Err(e) => {
						// A file that is not what it should be is worth nothing; it goes, so a
						// retry does not find it and skip the download.
						let _ = std::fs::remove_file(&finished.path);
						Err(e)
					}
				},
				None => Ok(finished),
			},
			Err(e) => Err(e),
		};
		reporter.abort();
		let event = {
			let mut inner = self.inner.lock().unwrap();
			// What the run learnt of the host: a limit where a connection was turned away; one more
			// than it went to where none was, so the next download from it asks a little further;
			// and nothing once that reaches the most a download may ask for.
			let learned = handle.learned.load(Ordering::Relaxed);
			if learned > 0 {
				let next = if handle.crowded.load(Ordering::Relaxed) { learned } else { learned + 1 };
				let host = host_of(&request);
				if next >= u64::from(Connections::MAX) {
					inner.hosts.remove(&host);
				} else {
					inner.hosts.insert(host, next as u16);
				}
			}
			let Some(entry) = inner.entries.get_mut(&id) else { return };
			entry.running = None;
			match result {
				Ok(finished) => {
					entry.kind = verify::kind(&finished.path);
					entry.status = Status::Completed(Box::new(finished.clone()));
					Event::Completed(id, finished)
				}
				Err(Error::Cancelled) => {
					entry.status = Status::Paused;
					Event::Paused(id)
				}
				Err(e) => {
					entry.status = Status::Failed(e.to_string());
					Event::Failed(id, e.to_string())
				}
			}
		};
		let _ = self.inner.lock().unwrap().events.send(event);
		self.pump();
	}
}

fn snapshot_of(id: TaskId, entry: &Entry) -> Snapshot {
	let p = &entry.handle.progress;
	Snapshot {
		id,
		url: entry.request.url.to_string(),
		// The name as the user gave it, else as the probe learnt it, else as it landed.
		file_name: match &entry.status {
			Status::Completed(f) => f.path.file_name().map(|n| n.to_string_lossy().into_owned()),
			_ => entry
				.request
				.file_name
				.clone()
				.or_else(|| entry.handle.probed.lock().unwrap().as_ref().map(|p| p.file_name.clone())),
		},
		status: entry.status.clone(),
		done: p.done.load(Ordering::Relaxed),
		total: p.total.load(Ordering::Relaxed),
		speed: p.speed.load(Ordering::Relaxed),
		connections: p.connections.load(Ordering::Relaxed),
		kind: entry.kind,
		// A copy taken under the lock, so the window never holds the plan the connections are
		// writing through. A dozen segments is nothing to clone and the alternative is a lock the
		// engine waits on while a frame is drawn.
		segments: entry
			.handle
			.plan
			.lock()
			.unwrap()
			.as_ref()
			.map(|plan| plan.lock().unwrap().segments.clone())
			.unwrap_or_default(),
		ranges: entry.handle.probed.lock().unwrap().as_ref().map(|probe| probe.ranges),
	}
}

#[cfg(test)]
mod tests {
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
}
