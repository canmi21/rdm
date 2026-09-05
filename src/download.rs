//! What a download is, how the list is filtered, and how numbers are shown.

use std::time::Duration;

use chrono::{DateTime, Local};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Status {
	Queued,
	Downloading,
	Paused,
	Completed,
	Failed,
}

impl Status {
	pub const ALL: [Status; 5] =
		[Status::Queued, Status::Downloading, Status::Paused, Status::Completed, Status::Failed];

	pub fn label(self) -> &'static str {
		match self {
			Status::Queued => "Queued",
			Status::Downloading => "Downloading",
			Status::Paused => "Paused",
			Status::Completed => "Completed",
			Status::Failed => "Failed",
		}
	}
}

/// A user-shaped bucket: a name, an icon, and a pattern over the file name. The built-in set is
/// seeded the same way, so there is one kind of category and the sidebar reads a list.
#[derive(Clone, Debug)]
pub struct Category {
	pub id: u64,
	pub name: String,
	pub icon: crate::ui::icon::Icon,
	/// As written; `regex` is its compiled form, or None for the catch-all.
	pub pattern: String,
	regex: Option<fancy_regex::Regex>,
}

impl Category {
	/// Compiles the pattern; an error is the pattern's own message, shown where it was typed.
	pub fn new(
		id: u64,
		name: &str,
		icon: crate::ui::icon::Icon,
		pattern: &str,
	) -> Result<Category, String> {
		let regex = if pattern.is_empty() {
			None
		} else {
			Some(fancy_regex::Regex::new(pattern).map_err(|e| e.to_string())?)
		};
		Ok(Category { id, name: name.to_owned(), icon, pattern: pattern.to_owned(), regex })
	}

	/// Matches the file name only: a URL's path is what the name came from, and a pattern over the
	/// whole address would catch hosts as often as files.
	pub fn matches(&self, download: &Download) -> bool {
		match &self.regex {
			Some(regex) => regex.is_match(&download.name).unwrap_or(false),
			None => false,
		}
	}

	/// The starting set, roughly what a download folder holds. Other has no pattern and takes
	/// whatever nothing else did.
	pub fn defaults() -> Vec<Category> {
		use crate::ui::icon::Icon;
		let seed = [
			("Video", Icon::Film, r"(?i)\.(mp4|mkv|mov|webm|avi)$"),
			("Audio", Icon::Music, r"(?i)\.(mp3|flac|aac|wav|m4a)$"),
			("Documents", Icon::FileText, r"(?i)\.(pdf|epub|docx?|txt|md)$"),
			("Archives", Icon::Archive, r"(?i)\.(zip|tar|gz|xz|7z|rar)$"),
			("Programs", Icon::Package, r"(?i)\.(dmg|pkg|app|exe|msi)$"),
			("Other", Icon::File, ""),
		];
		seed
			.iter()
			.enumerate()
			.map(|(i, (name, icon, pattern))| {
				Category::new(i as u64 + 1, name, *icon, pattern).expect("a seed pattern compiles")
			})
			.collect()
	}
}

/// The first category whose pattern matches, else the catch-all, else nothing.
pub fn category_of<'a>(categories: &'a [Category], download: &Download) -> Option<&'a Category> {
	categories
		.iter()
		.find(|c| c.matches(download))
		.or_else(|| categories.iter().find(|c| c.regex.is_none()))
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
			Filter::All => "All".to_owned(),
			Filter::Downloading => "Downloading".to_owned(),
			Filter::Unfinished => "Unfinished".to_owned(),
			Filter::Completed => "Completed".to_owned(),
			Filter::Category(id) => {
				categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default()
			}
		}
	}

	pub fn matches(self, download: &Download, categories: &[Category]) -> bool {
		match self {
			Filter::All => true,
			Filter::Downloading => download.status == Status::Downloading,
			Filter::Unfinished => download.status != Status::Completed,
			Filter::Completed => download.status == Status::Completed,
			Filter::Category(id) => category_of(categories, download).is_some_and(|c| c.id == id),
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

// TODO: stands in for a persistent store and a transfer engine, neither of which exists yet.
// The rows are shaped like real ones so the UI can be built against them.
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
	use crate::ui::icon::Icon;

	fn named(name: &str) -> Download {
		let mut d = sample().remove(0);
		d.name = name.to_owned();
		d
	}

	#[test]
	fn the_first_matching_category_wins_and_other_takes_the_rest() {
		let categories = Category::defaults();
		assert_eq!(category_of(&categories, &named("talk.MP4")).unwrap().name, "Video");
		assert_eq!(category_of(&categories, &named("notes.md")).unwrap().name, "Documents");
		assert_eq!(category_of(&categories, &named("disk.iso")).unwrap().name, "Other");
	}

	#[test]
	fn a_user_category_ahead_of_the_defaults_captures_first() {
		let mut categories = Category::defaults();
		categories.insert(0, Category::new(99, "Code", Icon::Code, r"\.(rs|py)$").unwrap());
		assert_eq!(category_of(&categories, &named("main.rs")).unwrap().name, "Code");
	}

	#[test]
	fn a_bad_pattern_is_an_error_with_the_engine_message_and_lookaround_is_allowed() {
		assert!(Category::new(1, "x", Icon::File, "(").is_err());
		let lookahead = Category::new(1, "Not video", Icon::File, r"^(?!.*\.mp4$).*$").unwrap();
		assert!(lookahead.matches(&named("a.pdf")));
		assert!(!lookahead.matches(&named("a.mp4")));
	}
}
