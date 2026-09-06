//! What the window remembers between launches: its frame, the column widths, the view. Kept in
//! `state.json` under the platform's state directory, versioned, and written a moment after each
//! change rather than at quit, since a quit is not always given. See spec/state.md.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::View;
use crate::identity::{ORGANIZATION, QUALIFIER};

/// The shape of `state.json`. Bumped only when an older file can no longer be read as-is; fields
/// added or dropped without breaking that stay at the same version.
pub const VERSION: u64 = 1;

/// Where the application keeps what it owns.
pub struct Paths {
	/// `state.json`: the window's memory, small, rewritten whole.
	pub state: PathBuf,
	/// `config.json`: what the user shaped and may edit by hand -- the categories, later the settings.
	pub config: PathBuf,
	/// `internal.sqlite`: the downloads themselves, once they persist; rows, appended and updated.
	pub database: PathBuf,
	/// Where downloads land: the platform's Downloads folder as the user has it -- the XDG
	/// user-dirs entry on Linux, the known folder on Windows, `~/Downloads` on macOS, which
	/// offers no way to move it -- and the home directory if there is no such folder.
	pub downloads: PathBuf,
}

impl Paths {
	/// Every file under one directory, with the downloads in a folder beside them. What the
	/// tests use in place of the platform's directories.
	#[cfg(test)]
	pub fn under(dir: &Path) -> Paths {
		Paths {
			state: dir.join("state.json"),
			config: dir.join("config.json"),
			database: dir.join("internal.sqlite"),
			downloads: dir.join("downloads"),
		}
	}

	pub fn resolve() -> Option<Paths> {
		// The third word carries the suffix, so a development build's state, config and database
		// sit beside the installed application's rather than in them. See src/identity.rs.
		let dirs = directories::ProjectDirs::from(QUALIFIER, ORGANIZATION, &crate::identity::instance())?;
		// Linux has a directory for state as distinct from data; the others fold them together.
		let root = dirs.state_dir().unwrap_or_else(|| dirs.data_local_dir()).to_path_buf();
		let user = directories::UserDirs::new()?;
		let downloads =
			user.download_dir().map(Path::to_path_buf).unwrap_or_else(|| user.home_dir().to_path_buf());
		Some(Paths {
			state: root.join("state.json"),
			config: dirs.config_dir().join("config.json"),
			database: root.join("internal.sqlite"),
			downloads,
		})
	}
}

/// A window's frame in screen points. Kept as plain numbers so the file has no gpui types in it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
	pub version: u64,
	#[serde(default)]
	pub window: Option<Frame>,
	#[serde(default)]
	pub maximized: bool,
	#[serde(default)]
	pub widths: Option<[f32; 5]>,
	#[serde(default)]
	pub view: Option<View>,
	/// The header's funnel: whether the lists also hold the download folder's other files.
	/// Absent only in a file this application has never written -- a first launch -- since a save
	/// writes the field whether the funnel was ever touched or not. That is what makes the
	/// absence mean "nobody has chosen", which is the one case the default is allowed to change
	/// under. See src/app/mod.rs and spec/ui.md.
	#[serde(default)]
	pub folder_shown: Option<bool>,
	/// The build that last ran, so the next can tell what it came after: absent in a file an
	/// older build wrote, or a hand build. See src/update/install.rs on the legacy names.
	#[serde(default)]
	pub last_build: Option<u64>,
}

impl Default for State {
	fn default() -> Self {
		State {
			version: VERSION,
			window: None,
			maximized: false,
			widths: None,
			view: None,
			folder_shown: None,
			last_build: None,
		}
	}
}

impl State {
	/// The saved frame if any of it would land on one of the displays given; a window saved on a
	/// monitor that is gone comes back centred instead of invisible.
	pub fn frame_on(&self, displays: &[Frame]) -> Option<Frame> {
		let frame = self.window?;
		displays.iter().any(|d| frame.overlaps(d)).then_some(frame)
	}
}

impl Frame {
	fn overlaps(&self, other: &Frame) -> bool {
		self.x < other.x + other.width
			&& other.x < self.x + self.width
			&& self.y < other.y + other.height
			&& other.y < self.y + self.height
	}
}

/// Reads a versioned file, bringing an older version up to `current` one step at a time through
/// `migrate`. A file from a newer version is refused rather than guessed at: it will be read
/// correctly by the build that wrote it, and overwriting it here would lose what that build knew.
/// A file with no integer version is refused the same way. Shared by state.json and config.json.
pub fn parse_versioned<T: serde::de::DeserializeOwned>(
	text: &str,
	current: u64,
	migrate: fn(u64, Value) -> Result<Value>,
) -> Result<T> {
	let mut value: Value = serde_json::from_str(text).context("not JSON")?;
	let version = value.get("version").and_then(Value::as_u64).context("no integer version")?;
	if version > current {
		bail!("version {version} is newer than this build's {current}");
	}
	for step in version..current {
		value = migrate(step, value)?;
	}
	Ok(serde_json::from_value(value)?)
}

pub fn parse(text: &str) -> Result<State> {
	parse_versioned(text, VERSION, migrate)
}

/// One step, from `from` to `from + 1`. Each breaking change adds an arm and bumps VERSION; the
/// arms are the history of the file's shape and are never removed.
fn migrate(from: u64, _value: Value) -> Result<Value> {
	// No breaking change has happened yet, so no arm exists; the first one replaces this line.
	bail!("no migration from state.json version {from}")
}

pub fn load(path: &Path) -> State {
	match std::fs::read_to_string(path) {
		Ok(text) => parse(&text).unwrap_or_else(|error| {
			eprintln!("ignoring {}: {error:#}", path.display());
			State::default()
		}),
		Err(_) => State::default(),
	}
}

/// Written whole, to a sibling and then renamed over the old file, so a crash mid-write leaves
/// the previous file rather than half of the new one.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
	if let Some(parent) = path.parent() {
		std::fs::create_dir_all(parent)?;
	}
	let temporary = path.with_extension("json.tmp");
	std::fs::write(&temporary, serde_json::to_string_pretty(value)?)?;
	std::fs::rename(&temporary, path)?;
	Ok(())
}

pub fn save(path: &Path, state: &State) -> Result<()> {
	write_json(path, state)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fields_this_build_does_not_know_are_ignored_and_missing_ones_default() {
		let state =
			parse(r#"{ "version": 1, "widths": [1, 2, 3, 4, 5], "later_field": true }"#).unwrap();
		assert_eq!(state.widths, Some([1.0, 2.0, 3.0, 4.0, 5.0]));
		assert_eq!(state.window, None);
		assert_eq!(state.view, None);
	}

	/// The funnel's default was off and is now on, which is allowed to move only for somebody who
	/// has never chosen. A save writes the field either way, so a file that names it has been
	/// chosen for and is left alone; only a file this application has never written says nothing.
	#[test]
	fn a_funnel_never_chosen_for_is_absent_and_one_chosen_for_is_kept() {
		assert_eq!(State::default().folder_shown, None, "nobody has chosen yet");
		let off = parse(r#"{ "version": 1, "folder_shown": false }"#).unwrap();
		assert_eq!(off.folder_shown, Some(false), "off on purpose stays off");
		let on = parse(r#"{ "version": 1, "folder_shown": true }"#).unwrap();
		assert_eq!(on.folder_shown, Some(true));
		let never = parse(r#"{ "version": 1 }"#).unwrap();
		assert_eq!(never.folder_shown, None, "and a file from before the field says nothing");
	}

	#[test]
	fn a_newer_file_is_refused_and_a_versionless_one_too() {
		assert!(parse(r#"{ "version": 99 }"#).is_err());
		assert!(parse(r#"{ "widths": [1, 2, 3, 4, 5] }"#).is_err());
		assert!(parse(r#"{ "version": 1.5 }"#).is_err(), "the version is an integer");
	}

	#[test]
	fn a_frame_on_a_display_that_is_gone_is_not_restored() {
		let state = State {
			window: Some(Frame { x: 3000.0, y: 100.0, width: 800.0, height: 600.0 }),
			..State::default()
		};
		let laptop = [Frame { x: 0.0, y: 0.0, width: 1512.0, height: 982.0 }];
		assert_eq!(state.frame_on(&laptop), None);
		let with_external = [laptop[0], Frame { x: 1512.0, y: 0.0, width: 2560.0, height: 1440.0 }];
		assert_eq!(state.frame_on(&with_external), state.window);
	}

	#[test]
	fn what_is_saved_is_read_back() {
		let dir = crate::testing::scratch("state");
		let path = dir.join("state.json");
		let state = State {
			window: Some(Frame { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }),
			widths: Some([1.0; 5]),
			view: Some(View::Grid),
			folder_shown: Some(true),
			..State::default()
		};
		save(&path, &state).unwrap();
		assert_eq!(load(&path), state);
		assert!(!dir.join("state.json.tmp").exists(), "the temporary is renamed away");
		std::fs::remove_dir_all(dir).ok();
	}
}
