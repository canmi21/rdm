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
use crate::engine::limiter::Limiter;
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
		};
		Ok((Engine { runtime: Arc::new(runtime), inner: Arc::new(Mutex::new(inner)) }, receiver))
	}

	/// Queues a download; it starts when fewer than `max_active` are running.
	pub fn add(&self, request: Request, checksum: Option<Checksum>) -> TaskId {
		let id = {
			let mut inner = self.inner.lock().unwrap();
			let id = TaskId(inner.next);
			inner.next += 1;
			inner.entries.insert(
				id,
				Entry {
					request,
					checksum,
					handle: Arc::new(fresh_handle()),
					status: Status::Queued,
					kind: None,
					running: None,
				},
			);
			id
		};
		self.pump();
		id
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
				entry.handle = Arc::new(fresh_handle());
			}
		}
		self.pump();
	}

	/// Forgets the download. With `delete`, its partial file and plan go too; a completed
	/// file is never deleted here, since it is the user's now.
	pub fn remove(&self, id: TaskId, delete: bool) {
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
			let name = entry.request.file_name.clone();
			let directory = entry.request.directory.clone();
			// The files go once the download has actually stopped: it writes its plan on the way
			// out, and a plan written after the discard would be a ghost. The name may be the
			// server's, which only the probe learnt; a partial file without a plan is a stray this
			// cannot find.
			let running = entry.running;
			self.runtime.spawn(async move {
				if let Some(running) = running {
					let _ = running.await;
				}
				if let Some(name) = name {
					task::discard(&directory, &name);
				}
			});
		}
		self.pump();
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
			let entry = inner.entries.get_mut(&id).expect("just listed");
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

fn fresh_handle() -> Handle {
	Handle {
		cancel: CancellationToken::new(),
		progress: Arc::new(Progress::default()),
		limit: Limiter::unlimited(),
	}
}

fn snapshot_of(id: TaskId, entry: &Entry) -> Snapshot {
	let p = &entry.handle.progress;
	Snapshot {
		id,
		url: entry.request.url.to_string(),
		file_name: match &entry.status {
			Status::Completed(f) => f.path.file_name().map(|n| n.to_string_lossy().into_owned()),
			_ => entry.request.file_name.clone(),
		},
		status: entry.status.clone(),
		done: p.done.load(Ordering::Relaxed),
		total: p.total.load(Ordering::Relaxed),
		speed: p.speed.load(Ordering::Relaxed),
		connections: p.connections.load(Ordering::Relaxed),
		kind: entry.kind,
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::settings::Connections;
	use crate::engine::testing::{Options, TestServer};

	fn scratch(name: &str) -> PathBuf {
		let dir = std::env::temp_dir().join(format!("rdm-engine-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		dir
	}

	fn body(len: usize) -> Vec<u8> {
		(0..len).map(|i| (i % 253) as u8).collect()
	}

	fn wait_for(receiver: &mpsc::Receiver<Event>, mut want: impl FnMut(&Event) -> bool) -> Event {
		let deadline = std::time::Instant::now() + Duration::from_secs(20);
		loop {
			let left = deadline.saturating_duration_since(std::time::Instant::now());
			let event = receiver.recv_timeout(left).expect("an event before the deadline");
			if want(&event) {
				return event;
			}
		}
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
		let done = wait_for(&events, |e| matches!(e, Event::Completed(id, _) if *id == a));
		let Event::Completed(_, finished) = done else { unreachable!() };
		assert_eq!(std::fs::read(&finished.path).unwrap(), data);
		wait_for(&events, |e| matches!(e, Event::Started(id) if *id == b));
		wait_for(&events, |e| matches!(e, Event::Completed(id, _) if *id == b));
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
		wait_for(&events, |e| matches!(e, Event::Progress(s) if s.id == id && s.done > 0));
		engine.pause(id);
		wait_for(&events, |e| matches!(e, Event::Paused(i) if *i == id));
		let paused = engine.snapshot(id).unwrap();
		assert_eq!(paused.status, Status::Paused);
		assert!(paused.done > 0 && paused.done < 400_000);
		assert!(crate::engine::control::control_path(&dir.join("p.bin")).exists());
		engine.resume(id);
		wait_for(&events, |e| matches!(e, Event::Completed(i, _) if *i == id));
		assert_eq!(std::fs::read(dir.join("p.bin")).unwrap(), data);
		assert!(!crate::engine::control::control_path(&dir.join("p.bin")).exists());
	}

	#[test]
	fn removing_with_delete_takes_the_partial_file_and_a_checksum_guards_the_result() {
		let data = body(200_000);
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
		let mut request = Request::new(server.url("/r.bin"), &dir);
		request.file_name = Some("r.bin".into());
		let id = engine.add(request.clone(), None);
		wait_for(&events, |e| matches!(e, Event::Progress(s) if s.id == id && s.done > 0));
		engine.remove(id, true);
		wait_for(&events, |e| matches!(e, Event::Removed(i) if *i == id));
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
		let failed = wait_for(&events, |e| matches!(e, Event::Failed(i, _) if *i == id));
		let Event::Failed(_, message) = failed else { unreachable!() };
		assert!(message.contains("checksum"), "{message}");
		assert!(!dir.join("r.bin").exists(), "a file that fails its checksum is not kept");
		let right = Checksum::Sha256(sha256_hex(&data));
		let id = engine.add(request, Some(right));
		wait_for(&events, |e| matches!(e, Event::Completed(i, _) if *i == id));
		assert_eq!(std::fs::read(dir.join("r.bin")).unwrap(), data);
	}

	fn sha256_hex(data: &[u8]) -> String {
		use sha2::Digest;
		sha2::Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
	}
}
