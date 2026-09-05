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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Kind {
	Video,
	Audio,
	Document,
	Archive,
	Program,
	Other,
}

impl Kind {
	pub const ALL: [Kind; 6] =
		[Kind::Video, Kind::Audio, Kind::Document, Kind::Archive, Kind::Program, Kind::Other];

	pub fn label(self) -> &'static str {
		match self {
			Kind::Video => "Video",
			Kind::Audio => "Audio",
			Kind::Document => "Documents",
			Kind::Archive => "Archives",
			Kind::Program => "Programs",
			Kind::Other => "Other",
		}
	}

	pub fn from_name(name: &str) -> Kind {
		let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
		match extension.as_str() {
			"mp4" | "mkv" | "mov" | "webm" | "avi" => Kind::Video,
			"mp3" | "flac" | "aac" | "wav" | "m4a" => Kind::Audio,
			"pdf" | "epub" | "doc" | "docx" | "txt" | "md" => Kind::Document,
			"zip" | "tar" | "gz" | "xz" | "7z" | "rar" => Kind::Archive,
			"dmg" | "pkg" | "app" | "exe" | "msi" => Kind::Program,
			_ => Kind::Other,
		}
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
}

impl Download {
	pub fn kind(&self) -> Kind {
		Kind::from_name(&self.name)
	}

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
	Kind(Kind),
}

impl Filter {
	pub const STATES: [Filter; 4] =
		[Filter::All, Filter::Downloading, Filter::Unfinished, Filter::Completed];

	pub fn label(self) -> &'static str {
		match self {
			Filter::All => "All",
			Filter::Downloading => "Downloading",
			Filter::Unfinished => "Unfinished",
			Filter::Completed => "Completed",
			Filter::Kind(kind) => kind.label(),
		}
	}

	/// Every filter the sidebar offers, in its order.
	pub fn all() -> impl Iterator<Item = Filter> {
		Filter::STATES.into_iter().chain(Kind::ALL.into_iter().map(Filter::Kind))
	}

	pub fn matches(self, download: &Download) -> bool {
		match self {
			Filter::All => true,
			Filter::Downloading => download.status == Status::Downloading,
			Filter::Unfinished => download.status != Status::Completed,
			Filter::Completed => download.status == Status::Completed,
			Filter::Kind(kind) => download.kind() == kind,
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
