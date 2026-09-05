//! Eyes on the download folder. The operating system says when something in it is created,
//! written or removed; the window is told once things have gone quiet, so a copy of a hundred
//! files is one rescan and not a hundred. Reads and other events that change nothing are
//! dropped before they are counted. See spec/state.md.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher as _};

/// How long the folder has to be quiet before a change counts. Long enough to fold a burst of
/// events from one operation into one, short enough that a file dropped in shows up at once.
pub const QUIET: Duration = Duration::from_millis(210);

/// A folder under watch. Dropping it stops the watch; the signal channel then closes.
pub struct Watcher {
	_watcher: notify::RecommendedWatcher,
	signals: mpsc::Receiver<Vec<PathBuf>>,
}

impl Watcher {
	/// Watches `directory`, not its subfolders. `try_signal` answers once per quiet spell that
	/// followed a change, with the paths that changed.
	pub fn new(directory: &Path) -> notify::Result<Watcher> {
		let (events, incoming) = mpsc::channel();
		let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
			if let Ok(event) = event
				&& matters(&event.kind)
			{
				let _ = events.send(event.paths);
			}
		})?;
		watcher.watch(directory, RecursiveMode::NonRecursive)?;
		let (signals, receiver) = mpsc::channel();
		thread::spawn(move || debounce(incoming, signals));
		Ok(Watcher { _watcher: watcher, signals: receiver })
	}

	/// The paths touched by every burst of changes that has ended since the last call, each
	/// once; None when none has. The paths are what the platform named, so the caller looks at
	/// those files alone rather than at the folder.
	pub fn try_signal(&self) -> Option<Vec<PathBuf>> {
		let mut paths = BTreeSet::new();
		while let Ok(burst) = self.signals.try_recv() {
			paths.extend(burst);
		}
		(!paths.is_empty()).then(|| paths.into_iter().collect())
	}
}

/// Creation, a write to the contents or a rename, removal. Not an access, not a change to
/// metadata alone, not the catch-all the platform sends for things it cannot name.
fn matters(kind: &EventKind) -> bool {
	use notify::event::ModifyKind;
	match kind {
		EventKind::Create(_) | EventKind::Remove(_) => true,
		EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Name(_) | ModifyKind::Any) => true,
		EventKind::Modify(ModifyKind::Metadata(_) | ModifyKind::Other) => false,
		EventKind::Access(_) | EventKind::Any | EventKind::Other => false,
	}
}

/// Each event starts, or restarts, the quiet timer, and its paths join the burst's; the burst
/// goes out when the timer runs down. Ends when the event channel closes, which is when the
/// watcher is dropped.
fn debounce(incoming: mpsc::Receiver<Vec<PathBuf>>, signals: mpsc::Sender<Vec<PathBuf>>) {
	while let Ok(first) = incoming.recv() {
		let mut burst: BTreeSet<PathBuf> = first.into_iter().collect();
		loop {
			match incoming.recv_timeout(QUIET) {
				Ok(more) => burst.extend(more),
				Err(mpsc::RecvTimeoutError::Timeout) => {
					let _ = signals.send(burst.into_iter().collect());
					break;
				}
				Err(mpsc::RecvTimeoutError::Disconnected) => return,
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn wait_for_signal(watcher: &Watcher, within: Duration) -> Option<Vec<PathBuf>> {
		let deadline = std::time::Instant::now() + within;
		while std::time::Instant::now() < deadline {
			if let Some(paths) = watcher.try_signal() {
				return Some(paths);
			}
			thread::sleep(Duration::from_millis(10));
		}
		None
	}

	#[test]
	fn a_burst_of_changes_is_one_signal_and_quiet_is_none() {
		let dir = crate::testing::scratch("watch");
		let watcher = Watcher::new(&dir).unwrap();
		// The platform needs a moment to start delivering, and may then deliver the directory's own
		// making: FSEvents reports with latency, so what `scratch` just did can arrive after the stream
		// starts. That is not a change of ours; drop it rather than assert the start was quiet.
		thread::sleep(Duration::from_millis(300));
		while watcher.try_signal().is_some() {
			thread::sleep(QUIET * 2);
		}
		for i in 0..20 {
			std::fs::write(dir.join(format!("file-{i}.rdm")), b"x").unwrap();
			thread::sleep(Duration::from_millis(5));
		}
		let paths =
			wait_for_signal(&watcher, Duration::from_secs(5)).expect("the burst ended and was reported");
		assert!(paths.len() >= 20 && paths.iter().any(|p| p.ends_with("file-7.rdm")), "{paths:?}");
		thread::sleep(QUIET * 2);
		assert!(watcher.try_signal().is_none(), "one burst, one signal");
		std::fs::remove_file(dir.join("file-0.rdm")).unwrap();
		let paths = wait_for_signal(&watcher, Duration::from_secs(5)).expect("a removal counts");
		assert!(paths.iter().any(|p| p.ends_with("file-0.rdm")), "{paths:?}");
	}

	#[test]
	fn only_changes_count() {
		use notify::event::{AccessKind, CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind};
		assert!(matters(&EventKind::Create(CreateKind::File)));
		assert!(matters(&EventKind::Remove(RemoveKind::File)));
		assert!(matters(&EventKind::Modify(ModifyKind::Data(DataChange::Content))));
		assert!(!matters(&EventKind::Access(AccessKind::Read)));
		assert!(!matters(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime))));
		assert!(!matters(&EventKind::Any));
	}
}
