//! Handing a file to the rest of the system: opening it with whatever the system opens it with,
//! and showing it where it lives. Both are one command per platform and neither waits for it --
//! a file manager takes as long as it takes, and the window has a list to draw meanwhile.
//!
//! What "showing it where it lives" is called differs by system, and so does what does it: the
//! Finder on macOS, File Explorer on Windows, and on Linux whatever the user names, since a
//! Linux desktop has no one answer and `xdg-open` on the folder is only the most likely one. See
//! spec/ui.md.

use std::path::Path;
use std::process::Command;

/// What the button that shows a file where it lives is called on this system. A word somebody
/// recognises beats a word that is merely accurate: on macOS the Finder is the Finder.
pub fn manager_name() -> &'static str {
	if cfg!(target_os = "macos") {
		"Finder"
	} else if cfg!(windows) {
		"File Explorer"
	} else {
		"file manager"
	}
}

/// Opens the file with whatever the system opens it with. Errors are reported and no more: the
/// press was to open a file, and a window that could not is not a window that should stop.
pub fn open(path: &Path) {
	let mut command = if cfg!(target_os = "macos") {
		let mut command = Command::new("open");
		command.arg(path);
		command
	} else if cfg!(windows) {
		// `start` is a shell built-in rather than a program, and its first argument is the title
		// of the window it would open, which is why the empty string is there.
		let mut command = Command::new("cmd");
		command.args(["/C", "start", ""]).arg(path);
		command
	} else {
		let mut command = Command::new("xdg-open");
		command.arg(path);
		command
	};
	spawn(&mut command, "open");
}

/// Shows the file where it lives, with it selected where the system can select it. `command` is
/// what the user named for this on Linux, ignored on the systems that have one answer.
pub fn show(path: &Path, command: &str) {
	let mut command = if cfg!(target_os = "macos") {
		let mut open = Command::new("open");
		open.arg("-R").arg(path);
		open
	} else if cfg!(windows) {
		let mut explorer = Command::new("explorer");
		// No space after the comma: Explorer takes the whole of the rest as the path, and a
		// space is part of it.
		explorer.arg(format!("/select,{}", path.display()));
		explorer
	} else {
		let named = command.trim();
		let folder = path.parent().unwrap_or(path);
		if named.is_empty() {
			let mut open = Command::new("xdg-open");
			open.arg(folder);
			open
		} else {
			// What the user named, given the file: a file manager that can select it will, and
			// one that cannot opens the folder, which is what `xdg-open` would have done.
			let mut named = Command::new(named);
			named.arg(path);
			named
		}
	};
	spawn(&mut command, "show");
}

fn spawn(command: &mut Command, what: &str) {
	if let Err(error) = command.spawn() {
		eprintln!("could not {what} the file: {error}");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The word on the button is the system's own word for the thing.
	#[test]
	fn the_file_manager_is_called_what_this_system_calls_it() {
		let name = manager_name();
		assert!(!name.is_empty());
		#[cfg(target_os = "macos")]
		assert_eq!(name, "Finder");
	}
}
