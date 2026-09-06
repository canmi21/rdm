//! The mark a system puts on a file that came from the internet, and taking it off.
//!
//! macOS writes `com.apple.quarantine` on anything a browser or a download manager saves, and
//! reads it back when the file is opened: an application carrying it is the one that asks whether
//! you are sure, and a `.app` from an unidentified developer carrying it is the one that refuses
//! outright. Taking the mark off is what somebody does in a terminal, and it is one call.
//!
//! **No privileges are asked for.** The mark lives on the file, and a file the user owns is a
//! file the user may write the attributes of; `xattr -d` in a terminal needs no `sudo` for a file
//! in one's own downloads, and neither does this. Where it does fail -- a file owned by somebody
//! else, a read-only volume -- it is reported and nothing is escalated. An application that
//! raised an administrator prompt to change an attribute on a file you own would be asking for
//! something it does not need, and a prompt nobody is there to answer is a prompt that hangs.
//!
//! Only macOS keeps a mark of this shape. Windows writes a `Zone.Identifier` stream and Linux
//! keeps nothing; both read as unmarked here until somebody writes them. See spec/ui.md.

use std::path::Path;

/// The attribute itself. One name, in one place.
#[cfg(target_os = "macos")]
const ATTRIBUTE: &[u8] = b"com.apple.quarantine\0";

/// Whether this file is marked as having come from the internet.
#[cfg(target_os = "macos")]
pub fn marked(path: &Path) -> bool {
	let Some(name) = c_path(path) else { return false };
	// Asking for nothing and reading the size back: the value is not wanted, only whether there
	// is one. A missing attribute answers -1, which is the whole of the question.
	let size = unsafe {
		libc::getxattr(
			name.as_ptr(),
			ATTRIBUTE.as_ptr().cast(),
			std::ptr::null_mut(),
			0,
			0,
			libc::XATTR_NOFOLLOW,
		)
	};
	size >= 0
}

/// Takes the mark off. `Ok(())` when the file no longer carries it, however it came to not carry
/// it; an error names why it still does.
#[cfg(target_os = "macos")]
pub fn clear(path: &Path) -> Result<(), String> {
	let name = c_path(path).ok_or_else(|| "the path is not a name this system can take".to_owned())?;
	let removed = unsafe {
		libc::removexattr(name.as_ptr(), ATTRIBUTE.as_ptr().cast(), libc::XATTR_NOFOLLOW)
	};
	if removed == 0 {
		return Ok(());
	}
	let error = std::io::Error::last_os_error();
	// Already gone is what was wanted; the caller asked for the file to be unmarked, not for the
	// call to have done the unmarking.
	if error.raw_os_error() == Some(libc::ENOATTR) {
		return Ok(());
	}
	Err(format!("{error}"))
}

#[cfg(target_os = "macos")]
fn c_path(path: &Path) -> Option<std::ffi::CString> {
	use std::os::unix::ffi::OsStrExt;
	std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

/// Windows writes a `Zone.Identifier` stream and Linux keeps nothing of the kind; until either is
/// written, nothing here is marked and there is nothing to take off.
#[cfg(not(target_os = "macos"))]
pub fn marked(_path: &Path) -> bool {
	false
}

#[cfg(not(target_os = "macos"))]
pub fn clear(_path: &Path) -> Result<(), String> {
	Ok(())
}

/// Whether a file is the kind whose mark is worth showing. Every downloaded file carries one and
/// almost none of them matters: the mark only does anything when the file is opened as a program,
/// so a flag on a `.txt` would be a flag on everything and would mean nothing.
pub fn worth_flagging(name: &str) -> bool {
	const KINDS: [&str; 12] = [
		"app", "dmg", "pkg", "mpkg", "exe", "msi", "appimage", "deb", "rpm", "jar", "run", "sh",
	];
	crate::category::extension_of(name).is_some_and(|e| KINDS.contains(&e.as_str()))
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The mark is on nearly every downloaded file and matters on almost none of them: it only
	/// does anything when the file is opened as a program.
	#[test]
	fn only_the_kinds_that_run_are_worth_a_flag() {
		for name in ["Thing.app", "installer.dmg", "setup.exe", "package.deb", "Tool.AppImage"] {
			assert!(worth_flagging(name), "{name}");
		}
		for name in ["notes.txt", "photo.jpg", "debian.iso", "model.stl", "noextension"] {
			assert!(!worth_flagging(name), "{name}");
		}
	}

	/// A file nobody marked is unmarked, and taking a mark off a file that has none is what was
	/// wanted rather than an error.
	#[test]
	fn a_plain_file_is_unmarked_and_clearing_it_is_no_error() {
		let dir = crate::testing::scratch("quarantine");
		let file = dir.join("plain.txt");
		std::fs::write(&file, b"nothing came from anywhere").unwrap();
		assert!(!marked(&file), "nothing wrote a mark on it");
		assert!(clear(&file).is_ok(), "and taking off a mark it does not have is not a failure");
		assert!(!marked(&file));
	}

	/// The whole point: a mark that is written is seen, and taken off without asking anybody for
	/// anything. Writing one needs the same permission taking one off does, so a machine where
	/// this test could not write is a machine where the feature would not work either.
	#[cfg(target_os = "macos")]
	#[test]
	fn a_mark_that_is_written_is_seen_and_taken_off_without_privileges() {
		let dir = crate::testing::scratch("quarantine-mark");
		let file = dir.join("Thing.dmg");
		std::fs::write(&file, b"pretend disk image").unwrap();
		let name = c_path(&file).unwrap();
		let value = b"0081;00000000;rdm;";
		let wrote = unsafe {
			libc::setxattr(
				name.as_ptr(),
				ATTRIBUTE.as_ptr().cast(),
				value.as_ptr().cast(),
				value.len(),
				0,
				libc::XATTR_NOFOLLOW,
			)
		};
		assert_eq!(wrote, 0, "a file we just made is one we can mark");
		assert!(marked(&file), "and the mark is seen");
		clear(&file).expect("taken off, with no privileges asked for");
		assert!(!marked(&file), "and it is gone");
	}
}
