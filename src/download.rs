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
			Status::Queued => "Queued",
			Status::Downloading => "Downloading",
			Status::Paused => "Paused",
			Status::Completed => "Completed",
			Status::Failed => "Failed",
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
			Filter::All => "All Tasks".to_owned(),
			Filter::Downloading => "Downloading".to_owned(),
			Filter::Unfinished => "Unfinished".to_owned(),
			Filter::Completed => "Completed".to_owned(),
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
	}
}
