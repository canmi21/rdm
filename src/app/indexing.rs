//! The archives among the rows, read in the background for what they hold, so the categories
//! can judge a zip by its contents and not only its name. Each file is read once and kept in
//! the store by its stamp; a changed file is read again, a gone one is forgotten. See
//! src/index/ and spec/state.md.

use std::sync::mpsc;
use std::time::Instant;

use crate::app::Rdm;
use crate::download::Download;
use crate::index::{self, Indexed};

/// How many seconds a partial archive's reading stands before it is read again.
const AGAIN_AFTER: i64 = 20;

/// For a partial file, the size it will have and the byte ranges that have arrived.
type Partial = Option<(u64, Vec<(u64, u64)>)>;

/// One file for the indexer to read.
struct Wanted {
	path: String,
	kind: index::Kind,
	stamp: (i64, u64),
	partial: Partial,
}

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
		let now = chrono::Local::now().timestamp();
		let wanted: Vec<Wanted> = self
			.rows()
			.filter_map(|d| {
				let kind = index::kind_of(&d.name)?;
				let (path, partial) = self.archive_file(d)?;
				let stamp = index::stamp(std::path::Path::new(&path))?;
				let known = self.archives.get(&path);
				// A partial file changes with every write, so it is read again only once its last
				// reading has had a while to go stale.
				let fresh = match &partial {
					None => known.is_some_and(|i| (i.modified, i.size) == stamp),
					Some(_) => known.is_some_and(|i| now - i.modified < AGAIN_AFTER),
				};
				// A partial reading is stamped with when it was read, which is what it goes stale by.
				let stamp = if partial.is_some() { (now, stamp.1) } else { stamp };
				(!fresh).then_some(Wanted { path, kind, stamp, partial })
			})
			.collect();
		if wanted.is_empty() {
			return;
		}
		let (sender, receiver) = mpsc::channel();
		let pending = wanted.len();
		let _ = self.engine.run(async move {
			let _ = tokio::task::spawn_blocking(move || {
				for Wanted { path, kind, stamp: (modified, size), partial } in wanted {
					let listed = std::fs::File::open(&path).map_err(anyhow::Error::from).and_then(|file| {
						let file = match partial {
							None => index::Available::whole(file)?,
							Some((len, done)) => index::Available::partial(file, len, done),
						};
						index::list_available(file, kind)
					});
					let indexed = match listed {
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
		self.archive_of(download).map(|i| index::top_level(&i.entries)).unwrap_or_default()
	}

	/// What the index read of the archive a row is, whole or as far as it has arrived.
	pub(crate) fn archive_of(&self, download: &Download) -> Option<&Indexed> {
		self.archives.get(&self.archive_file(download)?.0)
	}

	/// The file an archive row is read from: the finished file, or while it downloads the partial
	/// file beside its plan, with the size it will have and the ranges that have arrived.
	fn archive_file(&self, download: &Download) -> Option<(String, Partial)> {
		if let Some(path) = &download.path {
			return Some((path.clone(), None));
		}
		let folder = match &download.directory {
			Some(directory) => std::path::PathBuf::from(directory),
			None => self.paths.as_ref()?.downloads.clone(),
		};
		let target = folder.join(&download.name);
		let part = crate::engine::control::part_path(&target);
		if !part.exists() {
			return None;
		}
		let control = crate::engine::control::load(&target).ok().flatten()?;
		let segments = &control.plan.segments;
		let len = control.size.or_else(|| segments.iter().map(|s| s.span.end).max())?;
		let done = segments.iter().map(|s| (s.span.start, s.position())).collect();
		Some((part.to_string_lossy().into_owned(), Some((len, done))))
	}

	/// The index as the store had it, at launch.
	pub(crate) fn load_archives(&mut self) {
		self.archives = self.store.as_ref().and_then(|s| s.archives().ok()).unwrap_or_default();
	}
}
