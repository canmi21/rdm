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

mod drive;
#[cfg(test)]
mod tests;

use drive::snapshot_of;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(pub u64);

/// What the engine as a whole is told: how many downloads run at once, and a limit on their sum.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EngineSettings {
	pub max_active: usize,
	pub speed_limit: Option<u64>,
	/// How often a Progress event is sent for each running download.
	pub progress_every: Duration,
	/// Which running download goes back to the queue when a waiting one is started now and every
	/// place is taken.
	pub bump: Bump,
}

impl Default for EngineSettings {
	fn default() -> Self {
		EngineSettings {
			max_active: 3,
			speed_limit: None,
			progress_every: Duration::from_millis(500),
			bump: Bump::default(),
		}
	}
}

/// Which running download gives up its place to one started out of turn. See spec/engine.md,
/// "The queue can be reordered".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Bump {
	/// The one that started last: it has had the least time to reach its speed, so stopping it
	/// throws away the least.
	#[default]
	Newest,
	/// The one furthest from done at the pace it is going.
	MostLeft,
	/// The one going slowest.
	Slowest,
}

/// One running download as the choice of which gives way sees it.
struct Candidate {
	id: TaskId,
	started: std::time::Instant,
	left: f64,
	speed: u64,
}

/// The running download that gives way, by `bump`; None when nothing is running.
fn give_way(bump: Bump, candidates: &[Candidate]) -> Option<TaskId> {
	let pick = match bump {
		Bump::Newest => candidates.iter().max_by_key(|c| c.started),
		Bump::MostLeft => candidates.iter().max_by(|a, b| a.left.total_cmp(&b.left)),
		Bump::Slowest => candidates.iter().min_by_key(|c| c.speed),
	};
	pick.map(|c| c.id)
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
	/// Stopped to wait in the queue again rather than paused: it gave its place away.
	Queued(TaskId),
	Removed(TaskId),
}

struct Entry {
	request: Request,
	checksum: Option<Checksum>,
	handle: Arc<Handle>,
	status: Status,
	kind: Option<&'static str>,
	running: Option<tokio::task::JoinHandle<()>>,
	/// Its place in the queue: the lowest waiting is started first. New and resumed downloads go
	/// to the back; one started out of turn, and the one it displaced, to the front.
	ticket: i64,
	/// When it last started, for choosing which gives way.
	started: Option<std::time::Instant>,
	/// Set on a running download that is being stopped to wait again: where it goes in the queue
	/// once it has stopped, instead of being paused.
	requeue: Option<i64>,
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
	/// The next ticket at the back of the queue.
	back: i64,
}

impl Inner {
	fn take_back(&mut self) -> i64 {
		self.back += 1;
		self.back
	}

	/// A ticket ahead of everything waiting.
	fn front(&self) -> i64 {
		self
			.entries
			.values()
			.filter(|e| e.status == Status::Queued)
			.map(|e| e.ticket)
			.min()
			.unwrap_or(self.back)
			- 2
	}
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
			back: 0,
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
			let ticket = inner.take_back();
			inner.entries.insert(
				id,
				Entry {
					request,
					checksum,
					handle: Arc::new(Handle::new()),
					status: Status::Queued,
					kind: None,
					running: None,
					ticket,
					started: None,
					requeue: None,
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
		// A pause wins over a place given away: stopped, it stays stopped.
		entry.requeue = None;
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
			let ticket = inner.take_back();
			let Some(entry) = inner.entries.get_mut(&id) else { return };
			if matches!(entry.status, Status::Paused | Status::Failed(_)) {
				entry.status = Status::Queued;
				entry.handle = Arc::new(Handle::new());
				entry.ticket = ticket;
			}
		}
		self.pump();
	}

	/// Gives a running download's place to the next one waiting, and sends it to the back of the
	/// queue; its plan is kept, so it goes on from where it was when its turn comes. Nothing
	/// happens while nothing else is waiting, since it would only be started again.
	pub fn yield_place(&self, id: TaskId) {
		let mut inner = self.inner.lock().unwrap();
		let waiting = inner.entries.iter().any(|(other, e)| *other != id && e.status == Status::Queued);
		let ticket = inner.take_back();
		let Some(entry) = inner.entries.get_mut(&id) else { return };
		if entry.status == Status::Running && waiting {
			entry.requeue = Some(ticket);
			entry.handle.cancel.cancel();
		}
	}

	/// Starts a waiting, paused or failed download now, ahead of the queue. With every place taken,
	/// one running download gives way -- which one is `EngineSettings::bump` -- and waits at the
	/// front of the queue, next after this one, so it goes on as soon as a place comes free.
	pub fn start_now(&self, id: TaskId) {
		{
			let mut inner = self.inner.lock().unwrap();
			let front = inner.front();
			let Some(entry) = inner.entries.get_mut(&id) else { return };
			match entry.status {
				Status::Queued => {}
				Status::Paused | Status::Failed(_) => {
					entry.status = Status::Queued;
					entry.handle = Arc::new(Handle::new());
				}
				_ => return,
			}
			entry.ticket = front;
			let running: Vec<Candidate> = inner
				.entries
				.iter()
				.filter(|(_, e)| e.status == Status::Running && e.requeue.is_none())
				.map(|(other, e)| {
					let p = &e.handle.progress;
					let (done, total) = (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
					let speed = p.speed.load(Ordering::Relaxed);
					let left = if total == 0 || speed == 0 {
						f64::INFINITY
					} else {
						total.saturating_sub(done) as f64 / speed as f64
					};
					Candidate {
						id: *other,
						started: e.started.unwrap_or_else(std::time::Instant::now),
						left,
						speed,
					}
				})
				.collect();
			// Only the ones staying count: one already giving its place away frees it for this.
			if running.len() >= inner.settings.max_active
				&& let Some(victim) = give_way(inner.settings.bump, &running)
				&& let Some(entry) = inner.entries.get_mut(&victim)
			{
				entry.requeue = Some(front + 1);
				entry.handle.cancel.cancel();
			}
		}
		self.pump();
	}

	/// Starts a download over from nothing: what it left unfinished is deleted, and it waits at the
	/// back of the queue. Not while it runs, which would race its own files. A finished file is
	/// the caller's to move out of the way first; this does not touch it.
	pub fn restart(&self, id: TaskId) {
		{
			let mut inner = self.inner.lock().unwrap();
			let ticket = inner.take_back();
			let Some(entry) = inner.entries.get_mut(&id) else { return };
			if entry.status == Status::Running {
				return;
			}
			let name = entry
				.request
				.file_name
				.clone()
				.or_else(|| entry.handle.probed.lock().unwrap().as_ref().map(|p| p.file_name.clone()));
			if let Some(name) = name {
				task::discard(&entry.request.directory, &name);
			}
			entry.status = Status::Queued;
			entry.handle = Arc::new(Handle::new());
			entry.kind = None;
			entry.ticket = ticket;
		}
		self.pump();
	}

	pub fn set_bump(&self, bump: Bump) {
		self.inner.lock().unwrap().settings.bump = bump;
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
}
