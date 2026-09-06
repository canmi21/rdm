//! What a download is, how the list is filtered, and how numbers are shown.

use std::time::Duration;

use chrono::{DateTime, Local};
use serde::Serialize;

use crate::category::{Category, categories_of};
use crate::ui::theme::Tint;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Status {
	Queued,
	Downloading,
	Paused,
	Completed,
	Failed,
}

impl Status {
	/// The engine's word for it, as the list shows it. Queued and Running are the engine's; the
	/// list says Downloading for the one that moves.
	pub fn from_engine(status: &crate::engine::Status) -> Status {
		use crate::engine::Status as S;
		match status {
			S::Queued => Status::Queued,
			S::Running => Status::Downloading,
			S::Paused => Status::Paused,
			S::Completed(_) => Status::Completed,
			S::Failed(_) => Status::Failed,
		}
	}

	pub const ALL: [Status; 5] =
		[Status::Queued, Status::Downloading, Status::Paused, Status::Completed, Status::Failed];

	pub fn label(self) -> &'static str {
		match self {
			Status::Queued => crate::i18n::t("status.queued"),
			Status::Downloading => crate::i18n::t("status.downloading"),
			Status::Paused => crate::i18n::t("status.paused"),
			Status::Completed => crate::i18n::t("status.completed"),
			Status::Failed => crate::i18n::t("status.failed"),
		}
	}

	/// The word the database keeps, and back. Lowercase, so a hand query reads naturally.
	pub fn name(self) -> &'static str {
		match self {
			Status::Queued => "queued",
			Status::Downloading => "downloading",
			Status::Paused => "paused",
			Status::Completed => "completed",
			Status::Failed => "failed",
		}
	}

	pub fn parse(name: &str) -> Option<Status> {
		Status::ALL.into_iter().find(|s| s.name() == name)
	}
}

#[derive(Clone, Debug, Serialize)]
pub struct Download {
	pub id: u64,
	pub name: String,
	pub url: String,
	/// Total size in bytes; zero while the server has not said.
	pub size: u64,
	pub received: u64,
	/// Bytes per second, zero unless downloading.
	pub speed: u64,
	pub status: Status,
	pub added: DateTime<Local>,
	/// The page the address was found on, when it came from one.
	pub source: Option<String>,
	/// Where the finished file landed, once it has.
	pub path: Option<String>,
	/// Why it failed, in the engine's words, while it is failed.
	pub error: Option<String>,
	/// How many connections were asked for at Add Task; None is the engine's own judgement.
	pub connections: Option<u16>,
	/// The rest of what Add Task can ask for, each empty when it was not: where the file goes
	/// instead of the download folder, other addresses of the same file, a checksum the result
	/// must match, the part of the file wanted as `start-end`, and a limit of its own.
	pub directory: Option<String>,
	pub mirrors: Vec<String>,
	pub checksum: Option<String>,
	pub range: Option<String>,
	pub speed_limit: Option<u64>,
}

/// A part of a file as a person writes it, `start-end` in bytes with either side optional,
/// `1000-` to the end and `-1000` for the first thousand; empty is the whole file. Read back
/// out of the row as written, so what it means is the engine's `range`.
pub fn parse_range(text: &str) -> Result<Option<(u64, Option<u64>)>, String> {
	let text = text.trim().replace(' ', "");
	if text.is_empty() {
		return Ok(None);
	}
	let Some((start, end)) = text.split_once('-') else {
		return Err("A range is start-end, in bytes.".to_owned());
	};
	let start = if start.is_empty() {
		0
	} else {
		start.parse().map_err(|_| "A range is start-end, in bytes.".to_owned())?
	};
	let end = if end.is_empty() {
		None
	} else {
		Some(end.parse().map_err(|_| "A range is start-end, in bytes.".to_owned())?)
	};
	if end.is_some_and(|e| e <= start) {
		return Err("A range ends after it starts.".to_owned());
	}
	Ok(Some((start, end)))
}

impl Download {
	pub fn progress(&self) -> f32 {
		if self.size == 0 {
			0.0
		} else {
			(self.received as f64 / self.size as f64).clamp(0.0, 1.0) as f32
		}
	}

	pub fn remaining(&self) -> Option<Duration> {
		if self.speed == 0 || self.size <= self.received {
			return None;
		}
		Some(Duration::from_secs((self.size - self.received) / self.speed))
	}
}

/// What the download folder's own directories become in the list. Nothing here moves a file: it
/// is how the folder reads, not how it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Folders {
	/// Nothing at all: a directory is not a download, and what is inside it is somebody else's
	/// business. This is what it does to start with -- a download folder with a checked-out
	/// repository in it has a hundred thousand files in it, and none of them was downloaded.
	#[default]
	Ignore,
	/// Every file inside, listed at the top level beside the loose ones, for a folder somebody
	/// really does keep downloads in.
	Flatten,
	/// The directory itself, as a row that opens onto what it holds.
	Tree,
}

impl Folders {
	pub const ALL: [Folders; 3] = [Folders::Ignore, Folders::Flatten, Folders::Tree];

	pub fn name(self) -> &'static str {
		match self {
			Folders::Ignore => crate::i18n::t("folders.ignore"),
			Folders::Flatten => crate::i18n::t("folders.flatten"),
			Folders::Tree => crate::i18n::t("folders.tree"),
		}
	}
}

/// Why a file in the download folder is not worth a row of its own. The folder collects things
/// nobody downloaded and nobody wants listed: what the operating system leaves behind, what an
/// editor writes beside a file it has open, and the small pointers a browser saves instead of a
/// file. Hiding them is a preference and it is on to start with, since a list of eighty rows of
/// which nine are `.DS_Store` is a worse list than one of seventy-one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Junk {
	/// The system's leavings and an editor's scratch: never a row, whatever is being looked at.
	Noise,
	/// Worth keeping and worth filing, but not worth a place among downloads: a row only under
	/// the category it belongs to, which is where somebody looking for one would look.
	Filed,
}

/// What makes a file junk, or None if it is a file like any other. The name is judged whole,
/// since most of these are known by their whole name rather than their extension.
pub fn junk(name: &str) -> Option<Junk> {
	let lower = name.to_ascii_lowercase();
	// A torrent is the one kind that is filed rather than dropped: somebody who wants one goes
	// to Torrents in the sidebar and finds it there.
	if lower.ends_with(".torrent") {
		return Some(Junk::Filed);
	}
	// Microsoft Office writes `~$name.docx` beside a document it has open, and leaves it behind
	// when it does not close cleanly.
	if lower.starts_with("~$") || lower.starts_with(".~lock.") {
		return Some(Junk::Noise);
	}
	const NAMED: [&str; 10] = [
		".ds_store",
		"thumbs.db",
		"ehthumbs.db",
		"ehthumbs_vista.db",
		"desktop.ini",
		"icon\r",
		".localized",
		".directory",
		"$recycle.bin",
		".spotlight-v100",
	];
	if NAMED.contains(&lower.as_str()) {
		return Some(Junk::Noise);
	}
	// A pointer to something rather than the something: what a browser or a desktop saves when
	// what was dragged was a link.
	const POINTERS: [&str; 6] = [".lnk", ".url", ".webloc", ".desktop", ".alias", ".symlink"];
	if POINTERS.iter().any(|suffix| lower.ends_with(suffix)) {
		return Some(Junk::Noise);
	}
	// What a download leaves behind when it is interrupted somewhere else.
	const PARTIALS: [&str; 5] = [".crdownload", ".part", ".partial", ".download", ".tmp"];
	if PARTIALS.iter().any(|suffix| lower.ends_with(suffix)) {
		return Some(Junk::Noise);
	}
	None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Filter {
	All,
	Downloading,
	Unfinished,
	Completed,
	Category(u64),
}

impl Filter {
	pub const STATES: [Filter; 4] =
		[Filter::All, Filter::Downloading, Filter::Unfinished, Filter::Completed];

	/// The state filters' names; a category's is the category's own.
	pub fn label(self, categories: &[Category]) -> String {
		match self {
			Filter::All => crate::i18n::t("filter.all").to_owned(),
			Filter::Downloading => crate::i18n::t("filter.downloading").to_owned(),
			Filter::Unfinished => crate::i18n::t("filter.unfinished").to_owned(),
			Filter::Completed => crate::i18n::t("filter.completed").to_owned(),
			Filter::Category(id) => {
				Category::find(categories, id).map(|c| c.name.clone()).unwrap_or_default()
			}
		}
	}

	/// The color a filter's icon shows when lit: All is white, the states take their status
	/// colors, a category its own.
	pub fn color(self, categories: &[Category]) -> u32 {
		match self {
			Filter::All => Tint::Snow.rgb(),
			Filter::Downloading => Tint::Frost.rgb(),
			Filter::Unfinished => Tint::Yellow.rgb(),
			Filter::Completed => Tint::Green.rgb(),
			Filter::Category(id) => Category::find(categories, id).map_or(Tint::Snow.rgb(), |c| c.color),
		}
	}

	pub fn matches(self, download: &Download, categories: &[Category]) -> bool {
		match self {
			Filter::All => true,
			Filter::Downloading => download.status == Status::Downloading,
			Filter::Unfinished => download.status != Status::Completed,
			Filter::Completed => download.status == Status::Completed,
			Filter::Category(id) => categories_of(categories, download).iter().any(|c| c.id == id),
		}
	}
}

pub fn format_bytes(bytes: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
	let mut value = bytes as f64;
	let mut unit = 0;
	while value >= 1000.0 && unit < UNITS.len() - 1 {
		value /= 1000.0;
		unit += 1;
	}
	if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// A limit as a person writes it: empty, `off` or `unlimited` is none; a bare number is
/// kilobytes a second, since that is the unit a limit is thought in; `k`, `m` or `g`, with or
/// without `B/s` after, says otherwise.
pub fn parse_rate(text: &str) -> Result<Option<u64>, String> {
	let text = text.trim().to_ascii_lowercase();
	if text.is_empty() || text == "off" || text == "unlimited" || text == "none" {
		return Ok(None);
	}
	let digits: String = text.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
	let unit = text[digits.len()..].trim().trim_end_matches("/s").trim_end_matches('b').trim();
	let number: f64 =
		digits.parse().map_err(|_| "A limit is a number, like 500k or 2m.".to_owned())?;
	let scale = match unit {
		"" | "k" | "kb" => 1024.0,
		"m" | "mb" => 1024.0 * 1024.0,
		"g" | "gb" => 1024.0 * 1024.0 * 1024.0,
		_ => return Err("A limit is a number with k, m or g after it.".to_owned()),
	};
	let bytes = (number * scale) as u64;
	if bytes == 0 {
		Err("A limit is more than nothing; leave it empty for none.".to_owned())
	} else {
		Ok(Some(bytes))
	}
}

/// A size as a person writes it: bytes, or with `k`, `m` or `g`; empty is none.
pub fn parse_size(text: &str) -> Result<Option<u64>, String> {
	let text = text.trim().to_ascii_lowercase();
	if text.is_empty() || text == "off" || text == "none" {
		return Ok(None);
	}
	let digits: String = text.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
	let unit = text[digits.len()..].trim().trim_end_matches('b').trim();
	let number: f64 =
		digits.parse().map_err(|_| "A size is a number, like 512k or 2m.".to_owned())?;
	let scale = match unit {
		"" => 1.0,
		"k" => 1024.0,
		"m" => 1024.0 * 1024.0,
		"g" => 1024.0 * 1024.0 * 1024.0,
		_ => return Err("A size is a number with k, m or g after it.".to_owned()),
	};
	let bytes = (number * scale) as u64;
	if bytes == 0 {
		Err("A size is more than nothing; leave it empty for none.".to_owned())
	} else {
		Ok(Some(bytes))
	}
}

/// A count or a number of seconds as a person writes it; empty is none.
pub fn parse_number(text: &str) -> Result<Option<u64>, String> {
	let text = text.trim();
	if text.is_empty() {
		return Ok(None);
	}
	text.parse::<u64>().map(Some).map_err(|_| "A whole number.".to_owned())
}

/// A limit as the settings show it: `Off`, or the rate.
pub fn format_rate(limit: Option<u64>) -> String {
	match limit {
		None => "Off".to_owned(),
		Some(bytes) => format_speed(bytes),
	}
}

pub fn format_speed(bytes_per_second: u64) -> String {
	format!("{}/s", format_bytes(bytes_per_second))
}

pub fn format_added(added: DateTime<Local>) -> String {
	added.format("%b %-d, %H:%M").to_string()
}

pub fn format_duration(duration: Duration) -> String {
	let seconds = duration.as_secs();
	match seconds {
		0..60 => format!("{seconds}s"),
		60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
		_ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
	}
}

/// Rows shaped like real ones, for the headless tests to click on: the list is otherwise
/// empty until the engine fills it. Not persisted yet; see spec/state.md.
#[cfg(test)]
pub fn sample() -> Vec<Download> {
	let now = Local::now();
	let entry = |id: u64, name: &str, url: &str, size, received, speed, status| Download {
		id,
		name: name.to_owned(),
		url: url.to_owned(),
		size,
		received,
		speed,
		status,
		// Spread over the past days so the Added column has something to order by.
		added: now - chrono::Duration::hours(id as i64 * 7),
		source: None,
		path: None,
		error: None,
		connections: None,
		directory: None,
		mirrors: Vec::new(),
		checksum: None,
		range: None,
		speed_limit: None,
	};
	vec![
		entry(
			1,
			"ubuntu-26.04-desktop-arm64.iso",
			"https://releases.ubuntu.com/26.04/ubuntu-26.04-desktop-arm64.iso",
			5_800_000_000,
			2_310_000_000,
			48_000_000,
			Status::Downloading,
		),
		entry(
			2,
			"talk-recording.mp4",
			"https://example.org/media/talk-recording.mp4",
			1_240_000_000,
			1_240_000_000,
			0,
			Status::Completed,
		),
		entry(
			3,
			"rust-book.pdf",
			"https://example.org/docs/rust-book.pdf",
			18_400_000,
			6_100_000,
			0,
			Status::Paused,
		),
		entry(
			4,
			"soundtrack.flac",
			"https://example.org/audio/soundtrack.flac",
			312_000_000,
			0,
			0,
			Status::Queued,
		),
		entry(
			5,
			"zed-macos-aarch64.dmg",
			"https://example.org/releases/zed-macos-aarch64.dmg",
			168_000_000,
			168_000_000,
			0,
			Status::Completed,
		),
		entry(
			6,
			"dataset-2026.tar.xz",
			"https://example.org/data/dataset-2026.tar.xz",
			0,
			94_000_000,
			0,
			Status::Failed,
		),
		entry(
			7,
			"fedora-workstation-42.iso",
			"https://download.fedoraproject.org/fedora-workstation-42.iso",
			2_400_000_000,
			800_000_000,
			21_000_000,
			Status::Downloading,
		),
		entry(
			8,
			"lecture-03.mkv",
			"https://example.org/media/lecture-03.mkv",
			3_100_000_000,
			2_950_000_000,
			9_500_000,
			Status::Downloading,
		),
		entry(
			9,
			"node-modules-cache.tar.gz",
			"https://example.org/ci/node-modules-cache.tar.gz",
			640_000_000,
			120_000_000,
			64_000_000,
			Status::Downloading,
		),
	]
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The folder collects what nobody downloaded. Most of it is never worth a row; a torrent is
	/// worth one, but only under Torrents, which is what `Filed` means.
	#[test]
	fn the_folders_leavings_are_junk_and_a_torrent_is_the_filed_kind() {
		for name in [
			".DS_Store",
			"Thumbs.db",
			"desktop.ini",
			"~$Report.docx",
			".~lock.notes.odt#",
			"Shortcut.lnk",
			"Bookmark.webloc",
			"debian.iso.crdownload",
			"half.part",
		] {
			assert_eq!(junk(name), Some(Junk::Noise), "{name} is never a row");
		}
		assert_eq!(junk("ubuntu-24.04.torrent"), Some(Junk::Filed), "a row under Torrents alone");
		for name in ["debian.iso", "notes.txt", "Report.docx", "firmware.hex", "model.stl"] {
			assert_eq!(junk(name), None, "{name} is a file like any other");
		}
		assert_eq!(junk(".ds_store"), Some(Junk::Noise), "the name is judged without its case");
	}


	#[test]
	fn a_rate_is_read_in_kilobytes_by_default_and_off_when_empty() {
		assert_eq!(parse_rate(""), Ok(None));
		assert_eq!(parse_rate("off"), Ok(None));
		assert_eq!(parse_rate("500"), Ok(Some(500 * 1024)));
		assert_eq!(parse_rate("2m"), Ok(Some(2 * 1024 * 1024)));
		assert_eq!(parse_rate("1.5 MB/s"), Ok(Some(1536 * 1024)));
		assert!(parse_rate("fast").is_err());
		assert!(parse_rate("0").is_err());
		assert_eq!(format_rate(None), "Off");
		assert_eq!(parse_size("2m"), Ok(Some(2 * 1024 * 1024)));
		assert_eq!(parse_size("4096"), Ok(Some(4096)));
		assert_eq!(parse_size(""), Ok(None));
		assert_eq!(parse_number(" 12 "), Ok(Some(12)));
		assert!(parse_number("twelve").is_err());
		assert_eq!(parse_range("1000-2000"), Ok(Some((1000, Some(2000)))));
		assert_eq!(parse_range("1000-"), Ok(Some((1000, None))));
		assert_eq!(parse_range("-500"), Ok(Some((0, Some(500)))));
		assert_eq!(parse_range(""), Ok(None));
		assert!(parse_range("9-3").is_err() && parse_range("abc").is_err());
	}
}
