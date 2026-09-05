//! What the tests share and the application does not: a place on disk to write.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A fresh, empty directory under the temp directory, named for the test, unique to this
/// process and this call -- tests run at once, and two clearing the same directory collide.
/// Whatever an earlier run left there is removed first. Never inside the repository: one test
/// that downloaded into the working directory left its files in three commits.
pub fn scratch(name: &str) -> PathBuf {
	static NEXT: AtomicUsize = AtomicUsize::new(0);
	let n = NEXT.fetch_add(1, Ordering::Relaxed);
	let dir = std::env::temp_dir().join(format!("rdm-{name}-{}-{n}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}
