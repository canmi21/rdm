//! The window's side of the mark a system puts on a downloaded file: which rows carry one, and
//! taking it off when the flag is pressed. The reading and the writing are `src/quarantine.rs`;
//! this is the cache in front of them and the one action. See spec/ui.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::Context;

use crate::app::Rdm;

/// Which files carry the mark, by path. One attribute lookup a file, kept for the run: the list
/// draws every row it has, and asking the filesystem once a row a frame is asking too often.
#[derive(Default)]
pub struct Marks {
	seen: HashMap<PathBuf, bool>,
}

impl Marks {
	pub fn of(&mut self, path: &Path) -> bool {
		if let Some(known) = self.seen.get(path) {
			return *known;
		}
		let marked = crate::quarantine::marked(path);
		self.seen.insert(path.to_path_buf(), marked);
		marked
	}

	fn forget(&mut self, path: &Path) {
		self.seen.remove(path);
	}
}

impl Rdm {
	/// Takes the mark off the row's file. No privileges are asked for: the mark is on a file the
	/// user owns, and a prompt for something that does not need one is a prompt to regret. What
	/// fails is said once and the flag stays, which is the truth about the file.
	pub(crate) fn clear_quarantine(&mut self, id: u64, cx: &mut Context<Self>) {
		let Some(path) = self.download(id).and_then(|d| d.path.clone()) else { return };
		let path = PathBuf::from(path);
		match crate::quarantine::clear(&path) {
			Ok(()) => self.marked.borrow_mut().forget(&path),
			Err(error) => eprintln!("could not clear the mark on {}: {error}", path.display()),
		}
		cx.notify();
	}
}
