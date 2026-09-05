//! Eyes on the download folder. The operating system says when something in it is created,
//! written or removed; the window is told once things have gone quiet, so a copy of a hundred
//! files is one rescan and not a hundred. Reads and other events that change nothing are
//! dropped before they are counted. See spec/state.md.

use std::path::Path;
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
	signals: mpsc::Receiver<()>,
}

impl Watcher {
	/// Watches `directory`, not its subfolders. `try_signal` answers once per quiet spell that
	/// followed a change.
	pub fn new(directory: &Path) -> notify::Result<Watcher> {
		let (events, incoming) = mpsc::channel();
		let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
			if let Ok(event) = event
				&& matters(&event.kind)
			{
				let _ = events.send(());
			}
		})?;
		watcher.watch(directory, RecursiveMode::NonRecursive)?;
		let (signals, receiver) = mpsc::channel();
		thread::spawn(move || debounce(incoming, signals));
		Ok(Watcher { _watcher: watcher, signals: receiver })
	}

	/// True once for each burst of changes that has ended.
	pub fn try_signal(&self) -> bool {
		let mut any = false;
		while self.signals.try_recv().is_ok() {
			any = true;
		}
		any
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

/// Each event starts, or restarts, the quiet timer; the signal goes out when it runs down. Ends
/// when the event channel closes, which is when the watcher is dropped.
fn debounce(incoming: mpsc::Receiver<()>, signals: mpsc::Sender<()>) {
	while incoming.recv().is_ok() {
		loop {
			match incoming.recv_timeout(QUIET) {
				Ok(()) => continue,
				Err(mpsc::RecvTimeoutError::Timeout) => {
					let _ = signals.send(());
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

	fn wait_for_signal(watcher: &Watcher, within: Duration) -> bool {
		let deadline = std::time::Instant::now() + within;
		while std::time::Instant::now() < deadline {
			if watcher.try_signal() {
				return true;
			}
			thread::sleep(Duration::from_millis(10));
		}
		false
	}

	#[test]
	fn a_burst_of_changes_is_one_signal_and_quiet_is_none() {
		let dir = std::env::temp_dir().join(format!("rdm-watch-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let watcher = Watcher::new(&dir).unwrap();
		// The platform needs a moment to start delivering.
		thread::sleep(Duration::from_millis(300));
		assert!(!watcher.try_signal(), "nothing has happened");
		for i in 0..20 {
			std::fs::write(dir.join(format!("file-{i}.rdm")), b"x").unwrap();
			thread::sleep(Duration::from_millis(5));
		}
		assert!(wait_for_signal(&watcher, Duration::from_secs(5)), "the burst ended and was reported");
		thread::sleep(QUIET * 2);
		assert!(!watcher.try_signal(), "one burst, one signal");
		std::fs::remove_file(dir.join("file-0.rdm")).unwrap();
		assert!(wait_for_signal(&watcher, Duration::from_secs(5)), "a removal counts");
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
