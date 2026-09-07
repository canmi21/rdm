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
pub const VERSION: u64 = 3;

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

/// A display: the name the system keeps for it across a restart and a replug, and how big it is.
/// Only the name is written down. The size is not, because the size that decides where a window
/// fits is the size the display is when the window comes back, not the size it was when the
/// window left; and where the display sits is not, because no frame here is in the desktop's
/// coordinates. See `State::frame_on` and src/screens.rs.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
	pub uuid: String,
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
	/// The display the frame above is a frame on, by the name the system keeps for it. Absent on a
	/// system whose displays have no name to keep, and in a file written before there was a window
	/// to record; the window is then centred at the size it was left. See `State::frame_on`.
	#[serde(default)]
	pub display: Option<String>,
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
	/// Where to open, given the displays there are now. The frame is a place on one display and
	/// nothing without it: unplug a monitor and plug it in on the other side, and the same numbers
	/// still mean the same corner of that monitor, while the desktop underneath has been renumbered
	/// entirely. So the display is looked up by name, and the frame is put back on it, cut down to
	/// it if it came back smaller.
	///
	/// A display that is not there gives nothing, and nothing means centred -- on the main display,
	/// at the size the window was left, which the caller reads from `window`. A name that answers
	/// to no display is no better than no name: it is not a reason to open where nobody can see.
	/// See spec/state.md.
	pub fn frame_on(&self, screens: &[Screen]) -> Option<Frame> {
		let frame = self.window?;
		let uuid = self.display.as_ref()?;
		let screen = screens.iter().find(|s| &s.uuid == uuid)?;
		Some(frame.within(screen.width, screen.height))
	}
}

impl Frame {
	/// The same frame kept whole on a display this size. A side longer than the display is cut down
	/// to it, and a frame that would hang off an edge is pulled in, so a display that came back
	/// smaller than it was shows the whole window rather than a corner of it.
	fn within(self, width: f32, height: f32) -> Frame {
		let across = self.width.min(width);
		let down = self.height.min(height);
		Frame {
			x: self.x.clamp(0.0, (width - across).max(0.0)),
			y: self.y.clamp(0.0, (height - down).max(0.0)),
			width: across,
			height: down,
		}
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
		// 2 -> 3: the frame is the window's place on its own display, which is what GPUI reports
		// and what it takes back; before this it was read as a place on the desktop, which it never
		// was. The display beside it kept a rectangle nothing reads any more, so the name alone is
		// left. The name is kept rather than dropped: the old shape recorded the main display
		// whatever display the window was on, and on the main display the two readings agree, so a
		// window left there comes back where it was and one left elsewhere is no worse off than the
		// build that wrote the file left it.
		2 => {
			match value.get("display").and_then(|display| display.get("uuid")).cloned() {
				Some(uuid) => value["display"] = uuid,
				None => {
					if let Some(object) = value.as_object_mut() {
						object.remove("display");
					}
				}
			}
			value["version"] = Value::from(3u64);
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

	fn screen(uuid: &str, width: f32, height: f32) -> Screen {
		Screen { uuid: uuid.to_owned(), width, height }
	}

	/// The window comes back to the display it was left on, at the place on that display it was
	/// left at. That the display has moved on the desktop since does not enter into it: the frame
	/// was never in the desktop's coordinates, so there is nothing in it to correct.
	#[test]
	fn a_window_comes_back_to_the_display_it_was_left_on_wherever_that_display_moved_to() {
		let laptop = screen("laptop", 1512.0, 982.0);
		// The window sat 100 in and 50 down on the monitor, whichever side of the laptop it was on.
		let state = State {
			window: Some(Frame { x: 100.0, y: 50.0, width: 800.0, height: 600.0 }),
			display: Some("desk".to_owned()),
			..State::default()
		};
		let desk = screen("desk", 2560.0, 1440.0);
		assert_eq!(state.frame_on(&[laptop, desk]), state.window);
	}

	/// A display that came back smaller keeps the window whole rather than showing a corner of
	/// it: a side that still fits keeps its length and is pulled in until it is on the screen, and
	/// only a side that cannot fit is cut down to what there is.
	#[test]
	fn a_smaller_display_pulls_the_window_in_and_cuts_only_what_cannot_fit() {
		let state = State {
			window: Some(Frame { x: 1400.0, y: 900.0, width: 1200.0, height: 800.0 }),
			display: Some("desk".to_owned()),
			..State::default()
		};
		let smaller = screen("desk", 1280.0, 720.0);
		let back = state.frame_on(&[smaller]).expect("the display is here");
		// 1200 still fits across 1280, so it is kept and the left edge comes in to 80; 800 does
		// not fit down 720, so it is cut to 720 and there is nowhere left to be but the top.
		assert_eq!(back, Frame { x: 80.0, y: 0.0, width: 1200.0, height: 720.0 });
	}

	/// A display that is not there gives nothing, and so does a file that names no display at all.
	/// Nothing is what centres the window, and the size it was left at is read from the frame
	/// regardless -- a window that has to be centred is still the size the user made it.
	#[test]
	fn a_display_that_is_gone_leaves_the_window_to_be_centred_at_the_size_it_was() {
		let window = Some(Frame { x: 100.0, y: 100.0, width: 800.0, height: 600.0 });
		let unplugged = State { window, display: Some("desk".to_owned()), ..State::default() };
		let laptop = [screen("laptop", 1512.0, 982.0)];
		assert_eq!(unplugged.frame_on(&laptop), None, "the display it was on is not here");
		assert_eq!(unplugged.window, window, "and the size it was is still on record");
		let nameless = State { window, display: None, ..State::default() };
		assert_eq!(nameless.frame_on(&laptop), None, "no name is no display");
	}

	/// The window's frame used to be read as a place on the desktop and is now a place on its own
	/// display, and the display beside it used to carry a rectangle nothing reads any more.
	#[test]
	fn a_file_that_named_its_display_with_a_rectangle_keeps_the_name() {
		let state = parse(
			r#"{ "version": 2, "window": { "x": 100.0, "y": 50.0, "width": 800.0, "height": 600.0 },
			     "display": { "uuid": "desk", "frame": { "x": 0.0, "y": 0.0, "width": 2560.0, "height": 1440.0 } } }"#,
		)
		.unwrap();
		assert_eq!(state.display.as_deref(), Some("desk"));
		assert_eq!(state.frame_on(&[screen("desk", 2560.0, 1440.0)]), state.window);
		let never_had_one = parse(r#"{ "version": 2, "widths": [1, 2, 3, 4, 5] }"#).unwrap();
		assert_eq!(never_had_one.display, None);
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
