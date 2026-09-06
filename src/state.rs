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
pub const VERSION: u64 = 2;

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

/// A display, as the system names it and as it sat when the window was last on it. The name is
/// what the system keeps across a restart and a replug, which is what makes it worth writing
/// down; the frame beside it is what turns the window's coordinates into a place on this screen
/// rather than a place on the desktop. See `State::frame_on`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Screen {
	pub uuid: String,
	pub frame: Frame,
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
	/// The display the window was on, and where that display was. Absent in a file written before
	/// this was recorded, and on a system whose displays have no name to keep; the frame above is
	/// then read as it always was. See `State::frame_on`.
	#[serde(default)]
	pub display: Option<Screen>,
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
			display: None,
			folder_shown: None,
			last_build: None,
		}
	}
}

impl State {
	/// Where to open, given the displays there are now. The window is put back on the display it
	/// was left on, at the place on that display it was left at -- which is not the same as the
	/// coordinates it was left at, since a desktop is one plane and a display moves about in it:
	/// unplug the laptop's second monitor and plug it in on the other side, and the numbers that
	/// meant "top left of the right-hand screen" now mean somewhere else entirely, or nowhere.
	///
	/// So the frame is read as an offset into the display it belongs to, and the offset is what
	/// survives. A display that is not there falls back to the old rule -- the coordinates as
	/// they were, if any of the window would land on any screen -- and then to None, which is
	/// centred. See spec/state.md.
	pub fn frame_on(&self, screens: &[Screen]) -> Option<Frame> {
		let frame = self.window?;
		if let Some(was) = &self.display
			&& let Some(now) = screens.iter().find(|s| s.uuid == was.uuid)
		{
			return Some(frame.moved_from(&was.frame, &now.frame));
		}
		let displays: Vec<Frame> = screens.iter().map(|s| s.frame).collect();
		displays.iter().any(|d| frame.overlaps(d)).then_some(frame)
	}
}

impl Frame {
	/// How much of this frame lies on that one, in square points. Which display a window is on is
	/// decided by this rather than by asking the window: a window can straddle two screens, and
	/// the one it is on is the one it is mostly on.
	pub fn overlap_with(&self, other: &Frame) -> f32 {
		let across = (self.x + self.width).min(other.x + other.width) - self.x.max(other.x);
		let down = (self.y + self.height).min(other.y + other.height) - self.y.max(other.y);
		across.max(0.0) * down.max(0.0)
	}

	fn overlaps(&self, other: &Frame) -> bool {
		self.x < other.x + other.width
			&& other.x < self.x + self.width
			&& self.y < other.y + other.height
			&& other.y < self.y + self.height
	}

	/// The same place on a display that has moved or changed size. The offset into the display is
	/// what is kept; a window wider or taller than the display it comes back to is cut down to it,
	/// and one that would hang off an edge is pulled in, so a smaller screen than last time still
	/// shows the whole window rather than a corner of it.
	fn moved_from(self, was: &Frame, now: &Frame) -> Frame {
		let width = self.width.min(now.width);
		let height = self.height.min(now.height);
		let offset_x = (self.x - was.x).min(now.width - width).max(0.0);
		let offset_y = (self.y - was.y).min(now.height - height).max(0.0);
		Frame { x: now.x + offset_x, y: now.y + offset_y, width, height }
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
fn migrate(from: u64, mut value: Value) -> Result<Value> {
	match from {
		// 1 -> 2: the Compact view was dropped, since the table beside it said everything Compact
		// said and more. A file remembering it names a view this build has no variant for, and
		// serde fails the whole object over one field it cannot read -- which would take the
		// window's frame and the column widths down with a view nobody would miss. So it is
		// rewritten to the table rather than left to fail.
		1 => {
			if value.get("view").and_then(Value::as_str) == Some("Compact") {
				value["view"] = Value::from("Detailed");
			}
			value["version"] = Value::from(2u64);
			Ok(value)
		}
		_ => bail!("no migration from state.json version {from}"),
	}
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

	/// The Compact view is gone, and a file that remembers it must not take the rest of the file
	/// down with it: the frame and the widths are what the file is for, and serde fails a whole
	/// object over one field it cannot read.
	#[test]
	fn a_file_remembering_the_compact_view_comes_back_as_the_table() {
		let state = parse(r#"{ "version": 1, "widths": [1, 2, 3, 4, 5], "view": "Compact" }"#).unwrap();
		assert_eq!(state.view, Some(View::Detailed), "the view it named is now the table");
		assert_eq!(state.widths, Some([1.0, 2.0, 3.0, 4.0, 5.0]), "and everything beside it survived");
		assert_eq!(state.version, VERSION, "read as the shape this build writes");
		let kept = parse(r#"{ "version": 1, "view": "Grid" }"#).unwrap();
		assert_eq!(kept.view, Some(View::Grid), "a view this build still has is left where it was");
	}

	#[test]
	fn a_newer_file_is_refused_and_a_versionless_one_too() {
		assert!(parse(r#"{ "version": 99 }"#).is_err());
		assert!(parse(r#"{ "widths": [1, 2, 3, 4, 5] }"#).is_err());
		assert!(parse(r#"{ "version": 1.5 }"#).is_err(), "the version is an integer");
	}

	fn screen(uuid: &str, x: f32, y: f32, width: f32, height: f32) -> Screen {
		Screen { uuid: uuid.to_owned(), frame: Frame { x, y, width, height } }
	}

	/// The window comes back to the display it was left on, at the place on that display it was
	/// left at. The coordinates are not that place: unplug a second monitor and plug it in on the
	/// other side and the same numbers point somewhere else, or off the desk entirely.
	#[test]
	fn a_window_comes_back_to_the_display_it_was_left_on_wherever_that_display_moved_to() {
		let laptop = screen("laptop", 0.0, 0.0, 1512.0, 982.0);
		// The window sat 100 in and 50 down on a monitor that was then to the right of the laptop.
		let state = State {
			window: Some(Frame { x: 1612.0, y: 50.0, width: 800.0, height: 600.0 }),
			display: Some(screen("desk", 1512.0, 0.0, 2560.0, 1440.0)),
			..State::default()
		};
		// Plugged in on the left this time, so the same monitor now starts at -2560.
		let moved = screen("desk", -2560.0, 0.0, 2560.0, 1440.0);
		let back = state.frame_on(&[laptop.clone(), moved]).expect("the display is here");
		assert_eq!(back, Frame { x: -2460.0, y: 50.0, width: 800.0, height: 600.0 });
		// And where it has not moved, nothing moves.
		let same = screen("desk", 1512.0, 0.0, 2560.0, 1440.0);
		assert_eq!(state.frame_on(&[laptop, same]), state.window);
	}

	/// A display that came back smaller keeps the window whole rather than showing a corner of
	/// it: a side that still fits keeps its length and is pulled in until it is on the screen, and
	/// only a side that cannot fit is cut down to what there is.
	#[test]
	fn a_smaller_display_pulls_the_window_in_and_cuts_only_what_cannot_fit() {
		let state = State {
			window: Some(Frame { x: 1400.0, y: 900.0, width: 1200.0, height: 800.0 }),
			display: Some(screen("desk", 0.0, 0.0, 2560.0, 1440.0)),
			..State::default()
		};
		let smaller = screen("desk", 0.0, 0.0, 1280.0, 720.0);
		let back = state.frame_on(&[smaller]).expect("the display is here");
		// 1200 still fits across 1280, so it is kept and the left edge comes in to 80; 800 does
		// not fit down 720, so it is cut to 720 and there is nowhere left to be but the top.
		assert_eq!(back, Frame { x: 80.0, y: 0.0, width: 1200.0, height: 720.0 });
	}

	/// A display that is not there at all falls back to the older rule, which asks only whether
	/// any of the window would land on any screen; a name nobody answers to is no better than
	/// none, so it is not an excuse to open somewhere nobody can see.
	#[test]
	fn a_display_that_is_gone_falls_back_to_the_coordinates_and_then_to_nothing() {
		let state = State {
			window: Some(Frame { x: 100.0, y: 100.0, width: 800.0, height: 600.0 }),
			display: Some(screen("unplugged", 0.0, 0.0, 2560.0, 1440.0)),
			..State::default()
		};
		let laptop = screen("laptop", 0.0, 0.0, 1512.0, 982.0);
		assert_eq!(state.frame_on(&[laptop]), state.window, "the coordinates still land on a screen");
		let elsewhere = screen("laptop", 4000.0, 0.0, 1512.0, 982.0);
		assert_eq!(state.frame_on(&[elsewhere]), None, "and off every screen is centred instead");
	}

	#[test]
	fn a_frame_on_a_display_that_is_gone_is_not_restored() {
		let state = State {
			window: Some(Frame { x: 3000.0, y: 100.0, width: 800.0, height: 600.0 }),
			..State::default()
		};
		let laptop = [screen("laptop", 0.0, 0.0, 1512.0, 982.0)];
		assert_eq!(state.frame_on(&laptop), None);
		let with_external =
			[laptop[0].clone(), screen("desk", 1512.0, 0.0, 2560.0, 1440.0)];
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
