//! The downloads as the window drives them: added, paused, resumed, removed, brought in from
//! the folder, and kept in step with the engine's events and the store.

use gpui::Context;

use crate::app::Rdm;
use crate::download::{Download, Status};
use crate::engine::{self, Event, TaskId};

/// The engine's shape for what a row asked: its own judgement, or exactly this many.
pub(crate) fn connections_for(asked: Option<u16>) -> engine::Connections {
	match asked {
		None => engine::Connections::auto(),
		Some(count) => engine::Connections::fixed(count),
	}
}

impl Rdm {
	/// Downloads the folder holds that the list does not: a plan and a partial file left by a
	/// run whose rows were lost, or copied in from elsewhere. Each that can be continued comes
	/// in as a paused row, to be resumed by hand; what cannot be read is left where it is. Run
	/// at launch and whenever the folder changes; true when a row was added.
	pub(crate) fn import_strays(&mut self) -> bool {
		let Some(directory) = self.paths.as_ref().map(|p| p.downloads.clone()) else { return false };
		let found = engine::control::find(&directory);
		self.import_found(found)
	}

	/// The same, for the files the watcher named: each that is one of a download's two files is
	/// looked at on its own, so a change to one file costs one look and not a read of the folder.
	pub(crate) fn import_paths(&mut self, paths: &[std::path::PathBuf]) -> bool {
		let mut targets: Vec<std::path::PathBuf> =
			paths.iter().filter_map(|p| engine::control::target_of(p)).collect();
		targets.sort();
		targets.dedup();
		let found = targets.iter().filter_map(|t| engine::control::find_one(t)).collect();
		self.import_found(found)
	}

	fn import_found(&mut self, found: Vec<engine::Found>) -> bool {
		let mut added = false;
		for found in found {
			let name = found.target.file_name().map(|n| n.to_string_lossy().into_owned());
			let Some(name) = name else { continue };
			if self.downloads.iter().any(|d| d.name == name || d.url == found.control.url) {
				continue;
			}
			let id = self
				.store
				.as_ref()
				.and_then(|s| s.next_id().ok())
				.unwrap_or(0)
				.max(self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1);
			self.downloads.push(Download {
				id,
				name,
				url: found.control.url.clone(),
				size: found.control.size.unwrap_or(0),
				received: found.control.plan.done(),
				speed: 0,
				status: Status::Paused,
				added: found.modified.map_or_else(chrono::Local::now, chrono::DateTime::from),
				source: None,
				path: None,
				error: None,
				connections: None,
			});
			self.persist(id);
			added = true;
		}
		added
	}

	/// Every event the engine has sent since the last look, applied to the rows.
	pub(super) fn pump_events(&mut self, cx: &mut Context<Self>) {
		let mut changed = false;
		while let Ok(event) = self.events.try_recv() {
			self.apply(event);
			changed = true;
		}
		// The folder changed and has been quiet since: look at the files that changed, and only
		// those, for a plan that arrived.
		if let Some(paths) = self.watcher.as_ref().and_then(|w| w.try_signal())
			&& self.import_paths(&paths)
		{
			changed = true;
		}
		if changed {
			cx.notify();
		}
		self.poll_add(cx);
	}

	/// The row as it now is, written to the store.
	fn persist(&self, id: u64) {
		if let (Some(store), Some(download)) = (&self.store, self.download(id))
			&& let Err(error) = store.save(download)
		{
			eprintln!("could not keep download {id}: {error:#}");
		}
	}

	fn apply(&mut self, event: Event) {
		let touched = match &event {
			Event::Started(id)
			| Event::Completed(id, _)
			| Event::Failed(id, _)
			| Event::Paused(id)
			| Event::Removed(id) => id.0,
			Event::Progress(s) => s.id.0,
		};
		self.apply_event(event);
		self.persist(touched);
	}

	fn apply_event(&mut self, event: Event) {
		fn row(downloads: &mut [Download], id: TaskId) -> Option<&mut Download> {
			downloads.iter_mut().find(|d| d.id == id.0)
		}
		match event {
			Event::Started(id) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Downloading;
					d.error = None;
				}
			}
			Event::Progress(s) => {
				if let Some(d) = row(&mut self.downloads, s.id) {
					d.received = s.done;
					if s.total > 0 {
						d.size = s.total;
					}
					d.speed = s.speed;
					if let Some(name) = s.file_name {
						d.name = name;
					}
					d.status = Status::from_engine(&s.status);
				}
			}
			Event::Completed(id, finished) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Completed;
					d.size = finished.size;
					d.received = finished.size;
					d.speed = 0;
					if let Some(name) = finished.path.file_name() {
						d.name = name.to_string_lossy().into_owned();
					}
					d.path = Some(finished.path.to_string_lossy().into_owned());
				}
			}
			Event::Failed(id, message) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Failed;
					d.speed = 0;
					d.error = Some(message);
				}
			}
			Event::Paused(id) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Paused;
					d.speed = 0;
				}
			}
			Event::Removed(_) => {}
		}
	}

	/// A new download from an address as typed; the sheet has already looked at it, so this is
	/// for the control socket. What is not an address is dropped.
	pub(crate) fn add_url(&mut self, url: &str, cx: &mut Context<Self>) {
		if let Some(parsed) = crate::ui::add_dialog::parse_address(url) {
			self.add_request(parsed, None, None, self.preferences.connections, cx);
		}
	}

	/// A new download, handed to the engine and shown at once under `name` or the address's
	/// last path segment; the probe's name replaces it as soon as it is known. `source` is the
	/// page it was found on, if any. The id is the store's next, so it is never reused while a
	/// partial file might still carry it.
	pub(crate) fn add_request(
		&mut self,
		url: reqwest::Url,
		name: Option<String>,
		source: Option<String>,
		connections: Option<u16>,
		cx: &mut Context<Self>,
	) {
		let directory = self
			.paths
			.as_ref()
			.map(|p| p.downloads.clone())
			.unwrap_or_else(|| std::path::PathBuf::from("."));
		let id = self
			.store
			.as_ref()
			.and_then(|s| s.next_id().ok())
			.unwrap_or(0)
			.max(self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1);
		let mut request = engine::Request::new(url.clone(), directory);
		request.settings = self.preferences.engine_settings();
		request.settings.connections = connections_for(connections);
		self.engine.add_with_id(TaskId(id), request, None);
		let name = name.unwrap_or_else(|| {
			url
				.path_segments()
				.and_then(|mut s| s.next_back())
				.filter(|n| !n.is_empty())
				.unwrap_or("download")
				.to_owned()
		});
		self.downloads.push(Download {
			id,
			name,
			url: url.to_string(),
			size: 0,
			received: 0,
			speed: 0,
			status: Status::Queued,
			added: chrono::Local::now(),
			source,
			path: None,
			error: None,
			connections,
		});
		self.persist(id);
		self.selected = Some(id);
		cx.notify();
	}

	pub(crate) fn pause_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.pause(id, cx);
		}
	}

	pub(crate) fn resume_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.resume(id, cx);
		}
	}

	pub(crate) fn remove_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.remove(id, cx);
		}
	}

	/// The row changes at once and the engine confirms by event; a click should not wait on a
	/// connection to close.
	pub(crate) fn pause(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Paused;
			download.speed = 0;
			self.engine.pause(TaskId(id));
			self.persist(id);
			cx.notify();
		}
	}

	pub(crate) fn resume(&mut self, id: u64, cx: &mut Context<Self>) {
		let directory = self
			.paths
			.as_ref()
			.map(|p| p.downloads.clone())
			.unwrap_or_else(|| std::path::PathBuf::from("."));
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Queued;
			download.error = None;
			// A row from an earlier run is not in the engine yet; it is queued afresh and the
			// engine picks up the plan beside its partial file.
			if self.engine.contains(TaskId(id)) {
				self.engine.resume(TaskId(id));
			} else if let Ok(url) = reqwest::Url::parse(&download.url) {
				let mut request = engine::Request::new(url, directory);
				request.file_name = Some(download.name.clone());
				request.settings = self.preferences.engine_settings();
				request.settings.connections = connections_for(download.connections);
				self.engine.add_with_id(TaskId(id), request, None);
			}
			self.persist(id);
			cx.notify();
		}
	}

	/// The row goes, and with it a partial file and its plan; a finished file stays where it
	/// landed, since it is the user's now.
	pub(crate) fn remove(&mut self, id: u64, cx: &mut Context<Self>) {
		self.downloads.retain(|d| d.id != id);
		if self.selected == Some(id) {
			self.selected = None;
		}
		self.open.remove(&id);
		self.engine.remove(TaskId(id), true);
		if let Some(store) = &self.store
			&& let Err(error) = store.remove(id)
		{
			eprintln!("could not forget download {id}: {error:#}");
		}
		cx.notify();
	}
}
