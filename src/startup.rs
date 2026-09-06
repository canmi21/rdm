//! Starting with the machine, or not. Off to begin with: an application that put itself in the
//! login items without being asked would be one of those applications.
//!
//! Each system keeps its login items somewhere different and none of them is a library call worth
//! taking a dependency for. macOS reads a launch agent out of `~/Library/LaunchAgents`; Windows
//! reads a value out of the current user's `Run` key, written with the tool that ships with it;
//! Linux reads a desktop entry out of `~/.config/autostart`. All three are the user's own, need
//! no privileges, and are undone by taking them away again.
//!
//! The entry names itself after this build's identifier, so a development build and an installed
//! one keep separate entries and neither turns the other on. See spec/state.md.

use std::path::PathBuf;

use anyhow::{Context as _, Result};

/// Whether this build is in the login items.
pub fn enabled() -> bool {
	#[cfg(windows)]
	{
		std::process::Command::new("reg")
			.args(["query", RUN_KEY, "/v"])
			.arg(crate::identity::id())
			.output()
			.is_ok_and(|out| out.status.success())
	}
	#[cfg(not(windows))]
	{
		entry().is_some_and(|path| path.exists())
	}
}

/// Puts this build in the login items, or takes it out. What is written points at the binary as
/// it is running now, so a build moved after this was switched on starts nothing -- which is
/// better than starting whatever is at the old path.
pub fn set(on: bool) -> Result<()> {
	if on { install() } else { remove() }
}

/// Where the entry lives, or None where the home directory could not be found.
#[cfg(not(windows))]
fn entry() -> Option<PathBuf> {
	let home = directories::UserDirs::new()?.home_dir().to_path_buf();
	let id = crate::identity::id();
	Some(if cfg!(target_os = "macos") {
		home.join("Library/LaunchAgents").join(format!("{id}.plist"))
	} else {
		home.join(".config/autostart").join(format!("{id}.desktop"))
	})
}

#[cfg(not(windows))]
fn install() -> Result<()> {
	let path = entry().context("no home directory to write a login item into")?;
	let binary = std::env::current_exe().context("this build's own path")?;
	let binary = binary.to_string_lossy().into_owned();
	let parent = path.parent().context("a login item has a directory")?;
	std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
	let text = if cfg!(target_os = "macos") {
		// A launch agent, which is what macOS calls a thing that starts when the user logs in.
		// `RunAtLoad` is the whole of it; nothing is kept alive, since somebody who quits the
		// application meant to quit it.
		format!(
			"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
			 <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
			 \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
			 <plist version=\"1.0\">\n\
			 <dict>\n\
			 \t<key>Label</key>\n\t<string>{id}</string>\n\
			 \t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>{binary}</string>\n\t</array>\n\
			 \t<key>RunAtLoad</key>\n\t<true/>\n\
			 </dict>\n\
			 </plist>\n",
			id = crate::identity::id(),
		)
	} else {
		format!(
			"[Desktop Entry]\n\
			 Type=Application\n\
			 Name={name}\n\
			 Exec={binary}\n\
			 X-GNOME-Autostart-enabled=true\n",
			name = crate::identity::DISPLAY_NAME,
		)
	};
	std::fs::write(&path, text).with_context(|| format!("write {}", path.display()))
}

#[cfg(not(windows))]
fn remove() -> Result<()> {
	let Some(path) = entry() else { return Ok(()) };
	match std::fs::remove_file(&path) {
		Ok(()) => Ok(()),
		// Not there is what was wanted.
		Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
		Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
	}
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// Windows keeps its login items in the registry, and `reg` is how one writes there without a
/// crate that speaks the registry API. It ships with the system and needs no privileges for the
/// current user's own key.
#[cfg(windows)]
fn install() -> Result<()> {
	let binary = std::env::current_exe().context("this build's own path")?;
	let status = std::process::Command::new("reg")
		.args(["add", RUN_KEY, "/v"])
		.arg(crate::identity::id())
		.args(["/t", "REG_SZ", "/d"])
		.arg(format!("\"{}\"", binary.display()))
		.arg("/f")
		.status()
		.context("run reg")?;
	anyhow::ensure!(status.success(), "reg add did not succeed");
	Ok(())
}

#[cfg(windows)]
fn remove() -> Result<()> {
	// A value that is not there is not an error worth reporting: it is the state that was asked
	// for, and `reg` says so in a way that cannot be told from a real failure.
	let _ = std::process::Command::new("reg")
		.args(["delete", RUN_KEY, "/v"])
		.arg(crate::identity::id())
		.arg("/f")
		.status();
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The entry is named after this build, so a development build and an installed one keep
	/// separate ones and neither turns the other on.
	#[cfg(not(windows))]
	#[test]
	fn the_entry_is_named_after_this_build() {
		let Some(path) = entry() else { return };
		let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
		assert!(name.starts_with(&crate::identity::id()), "{name}");
		assert!(name.ends_with(".plist") || name.ends_with(".desktop"), "{name}");
		#[cfg(debug_assertions)]
		assert!(name.contains(".dev."), "a development build keeps its own: {name}");
	}
}
