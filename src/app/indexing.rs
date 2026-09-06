//! The archives among the rows, read in the background for what they hold, so the categories
//! can judge a zip by its contents and not only its name. Each file is read once and kept in
//! the store by its stamp; a changed file is read again, a gone one is forgotten. See
//! src/index.rs and spec/state.md.

use std::sync::mpsc;
use std::time::Instant;

use crate::app::Rdm;
use crate::download::Download;
use crate::index::{self, Indexed};

/// A run of the indexer: when it started, where each file's result arrives, and how many are
/// still to come.
pub(crate) struct Indexing {
	pub since: Instant,
	receiver: mpsc::Receiver<(String, Indexed)>,
	pub pending: usize,
}

impl Rdm {
	/// Every archive among the rows that the index does not know as it now is, handed to one
	/// background task that reads them in turn. Nothing while a run is under way: the next
	/// call after it ends picks up whatever it missed.
	pub(crate) fn queue_indexing(&mut self) {
		if self.indexing.is_some() {
			return;
		}
		// An archive that is gone from disk takes its index with it.
		let gone: Vec<String> =
			self.archives.keys().filter(|path| !std::path::Path::new(path).exists()).cloned().collect();
		for path in gone {
			self.archives.remove(&path);
			if let Some(store) = &self.store
				&& let Err(error) = store.forget_archive(&path)
			{
				eprintln!("could not forget the index of {path}: {error:#}");
			}
		}
		let wanted: Vec<(String, (i64, u64))> = self
			.rows()
			.filter_map(|d| {
				let path = d.path.as_deref()?;
				index::kind_of(&d.name)?;
				let stamp = index::stamp(std::path::Path::new(path))?;
				let known = self.archives.get(path).is_some_and(|i| (i.modified, i.size) == stamp);
				(!known).then(|| (path.to_owned(), stamp))
			})
			.collect();
		if wanted.is_empty() {
			return;
		}
		let (sender, receiver) = mpsc::channel();
		let pending = wanted.len();
		let _ = self.engine.run(async move {
			let _ = tokio::task::spawn_blocking(move || {
				for (path, (modified, size)) in wanted {
					let indexed = match index::list(std::path::Path::new(&path)) {
						Ok(entries) => Indexed { modified, size, entries, error: None },
						Err(error) => {
							Indexed { modified, size, entries: Vec::new(), error: Some(format!("{error:#}")) }
						}
					};
					if sender.send((path, indexed)).is_err() {
						break;
					}
				}
			})
			.await;
		});
		self.indexing = Some(Indexing { since: Instant::now(), receiver, pending });
	}

	/// The results so far, into the map and the store; true when any arrived, since a row's
	/// category may have changed with it.
	pub(crate) fn poll_indexing(&mut self) -> bool {
		let Some(run) = &mut self.indexing else { return false };
		let mut arrived = Vec::new();
		loop {
			match run.receiver.try_recv() {
				Ok(result) => arrived.push(result),
				Err(mpsc::TryRecvError::Empty) => break,
				Err(mpsc::TryRecvError::Disconnected) => {
					run.pending = 0;
					break;
				}
			}
		}
		run.pending = run.pending.saturating_sub(arrived.len());
		if run.pending == 0 {
			self.indexing = None;
		}
		let changed = !arrived.is_empty();
		for (path, indexed) in arrived {
			if let Some(store) = &self.store
				&& let Err(error) = store.save_archive(&path, &indexed)
			{
				eprintln!("could not keep the index of {path}: {error:#}");
			}
			self.archives.insert(path, indexed);
		}
		changed
	}

	/// The names at the top of the archive a row is, for the categories; empty for a row that
	/// is not one, or one not read yet, or one that could not be.
	pub(crate) fn contents_of(&self, download: &Download) -> Vec<String> {
		download
			.path
			.as_deref()
			.and_then(|p| self.archives.get(p))
			.map(|i| index::top_level(&i.entries))
			.unwrap_or_default()
	}

	/// The index as the store had it, at launch.
	pub(crate) fn load_archives(&mut self) {
		self.archives = self.store.as_ref().and_then(|s| s.archives().ok()).unwrap_or_default();
	}
}
