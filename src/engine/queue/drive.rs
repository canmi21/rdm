//! The queue moving: which downloads start when a place frees, and one download run from start to
//! its end, with what it reports along the way.

use super::*;

impl Engine {
	/// Starts queued downloads while there is room. Called after anything that could make room.
	pub(super) fn pump(&self) {
		let mut inner = self.inner.lock().unwrap();
		let running = inner.entries.values().filter(|e| e.status == Status::Running).count();
		let room = inner.settings.max_active.saturating_sub(running);
		let mut queued: Vec<(i64, TaskId)> = inner
			.entries
			.iter()
			.filter(|(_, e)| e.status == Status::Queued)
			.map(|(id, e)| (e.ticket, *id))
			.collect();
		queued.sort();
		let queued: Vec<TaskId> = queued.into_iter().map(|(_, id)| id).collect();
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
			entry.started = Some(std::time::Instant::now());
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
	pub(super) async fn drive(
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
				// Stopped to give its place away: back in the queue where it was sent, with a fresh
				// handle as a resume gives it, rather than paused.
				Err(Error::Cancelled) if entry.requeue.is_some() => {
					entry.ticket = entry.requeue.take().unwrap_or_default();
					entry.status = Status::Queued;
					entry.handle = Arc::new(Handle::new());
					Event::Queued(id)
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

pub(super) fn snapshot_of(id: TaskId, entry: &Entry) -> Snapshot {
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
