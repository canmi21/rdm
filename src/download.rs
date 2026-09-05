//! What a download is, how the list is filtered, and how numbers are shown.

use std::time::Duration;

use chrono::{DateTime, Local};
use serde::Serialize;

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
}

/// A built-in category: a name, an icon and the extensions it starts with. The list is the
/// application's and grows with releases; a user's changes to it are kept apart, so a release
/// that adds an extension reaches every user who did not remove it on purpose.
#[derive(Debug)]
pub struct Preset {
	pub name: &'static str,
	pub icon: crate::ui::icon::Icon,
	pub tint: crate::ui::theme::Tint,
	pub extensions: &'static str,
}

impl Preset {
	pub fn base(&self) -> Vec<String> {
		split_extensions(self.extensions)
	}
}

/// A user's changes to a preset's extension list: what they added, what they took away. Both
/// are lists of extensions, not a copy of the whole list, so the built-in list can change
/// under them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Overrides {
	pub added: Vec<String>,
	pub removed: Vec<String>,
}

impl Overrides {
	pub fn is_empty(&self) -> bool {
		self.added.is_empty() && self.removed.is_empty()
	}
}

/// The list a preset runs with: the built-in extensions less the removed ones, then the added
/// ones after them. The built-in order is the application's and the additions keep the order
/// they were typed in.
// TODO: whether the additions should be able to sit among the built-in ones rather than after
// them is the author's to decide; the file already keeps them apart, so either order is a
// change here alone.
pub fn merged_extensions(base: &[String], overrides: &Overrides) -> Vec<String> {
	let mut list: Vec<String> =
		base.iter().filter(|e| !overrides.removed.contains(e)).cloned().collect();
	for extension in &overrides.added {
		if !list.contains(extension) {
			list.push(extension.clone());
		}
	}
	list
}

/// A user-shaped bucket: a name, an icon, and a pattern over the file name. A preset is the same
/// thing with its pattern derived from an extension list the application maintains, so the
/// sidebar reads one kind of list.
#[derive(Clone, Debug)]
pub struct Category {
	pub id: u64,
	pub name: String,
	pub icon: crate::ui::icon::Icon,
	/// As written for a custom rule; derived from the extensions for a preset. `regex` is its
	/// compiled form, or None for the catch-all.
	pub pattern: String,
	regex: Option<fancy_regex::Regex>,
	/// The color its icon shows when lit, as `0xrrggbb`: a preset's own or the next in the
	/// cycle to start with, and whatever the user picked or typed after that.
	pub color: u32,
	/// A color the user wrote for this category, as they wrote it, kept beside the named ones so
	/// it can be chosen again after a named one was.
	pub custom_color: Option<String>,
	/// Which preset this is, with the user's changes to its list; None for a custom rule.
	pub preset: Option<(&'static Preset, Overrides)>,
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
		let color = if pattern.is_empty() { Tint::Snow } else { Tint::cycle(id) }.rgb();
		Ok(Category {
			id,
			name: name.to_owned(),
			icon,
			pattern: pattern.to_owned(),
			regex,
			color,
			custom_color: None,
			preset: None,
		})
	}

	/// Matches the file name only: a URL's path is what the name came from, and a pattern over the
	/// whole address would catch hosts as often as files.
	pub fn matches(&self, download: &Download) -> bool {
		match &self.regex {
			Some(regex) => regex.is_match(&download.name).unwrap_or(false),
			None => false,
		}
	}

	/// The presets a user picks from. The seed is all of them plus Other; a user who wants fewer
	/// removes them, and one who wants more writes a custom one or adds to a preset's list.
	///
	/// The lists aim to be complete for what a download manager meets rather than short: every
	/// vendor's and every open format for the kind, the older ones still in circulation, and the
	/// newer ones -- AVIF, JPEG XL, HEIF, Zstandard, MSIX -- that a list written a few years ago
	/// would miss. A file that two lists describe is in both categories, so overlap is not a
	/// problem to solve; a disk image is also a program when it installs one.
	pub const PRESETS: [Preset; 12] = {
		use crate::ui::icon::Icon;
		use crate::ui::theme::Tint;
		[
			Preset {
				name: "Video",
				icon: Icon::Film,
				tint: Tint::Purple,
				extensions: "mp4 m4v mkv mov webm avi wmv flv f4v ts m2ts mts mpg mpeg mpe m2v vob \
					3gp 3g2 ogv ogm rm rmvb asf divx mxf hevc h264 h265 av1",
			},
			Preset {
				name: "Audio",
				icon: Icon::Music,
				tint: Tint::Teal,
				extensions: "mp3 flac aac m4a m4b m4p wav aiff aif aifc ogg oga opus wma alac ape wv \
					dsf dff mka mid midi amr ac3 dts caf tta mpc spx au ra",
			},
			Preset {
				name: "Images",
				icon: Icon::Image,
				tint: Tint::Yellow,
				extensions: "jpg jpeg jpe jfif png apng gif webp avif heic heif heics heifs jxl svg svgz \
					tif tiff bmp dib ico icns psd psb xcf raw dng cr2 cr3 nef nrw arw orf rw2 raf pef \
					srw exr hdr tga pcx ppm pgm pbm pnm",
			},
			Preset {
				name: "Documents",
				icon: Icon::FileText,
				tint: Tint::Frost,
				extensions: "pdf rtf doc docx docm dot dotx dotm odt ott fodt pages wpd wps abw tex xps \
					oxps",
			},
			Preset {
				name: "Plain text",
				icon: Icon::Text,
				tint: Tint::Teal,
				extensions: "txt text md markdown mdx rst adoc asciidoc org log nfo",
			},
			Preset {
				name: "Presentations",
				icon: Icon::Presentation,
				tint: Tint::Orange,
				extensions: "ppt pptx pptm pps ppsx ppsm pot potx potm odp otp fodp key",
			},
			Preset {
				name: "Spreadsheets",
				icon: Icon::FileSpreadsheet,
				tint: Tint::Green,
				extensions: "xls xlsx xlsm xlsb xlt xltx xltm ods ots fods numbers csv tsv",
			},
			Preset {
				name: "Ebooks",
				icon: Icon::BookOpen,
				tint: Tint::Green,
				extensions: "epub mobi azw azw3 azw4 kfx kpf prc fb2 fb3 lit lrf pdb djvu djv cbz cbr cb7 \
					cbt ibooks",
			},
			Preset {
				name: "Code",
				icon: Icon::Code,
				tint: Tint::Blue,
				extensions: "rs py ts tsx js jsx mjs cjs go c h cpp hpp cc cxx hh java kt kts swift m mm \
					cs fs rb php pl lua dart scala hs ex exs erl clj zig sh bash zsh fish ps1 sql json \
					toml yaml yml xml html htm css scss less vue svelte ipynb",
			},
			Preset {
				name: "Archives",
				icon: Icon::Archive,
				tint: Tint::Orange,
				extensions: "zip zipx 7z rar tar gz tgz bz2 tbz2 xz txz zst tzst lz lz4 lzma tlz z cab \
					arj lha lzh sit sitx ace",
			},
			Preset {
				name: "Programs",
				icon: Icon::Package,
				tint: Tint::Red,
				extensions: "dmg pkg mpkg app exe msi msix msixbundle appx appxbundle deb rpm appimage \
					flatpak snap apk aab xapk ipa jar run",
			},
			Preset {
				name: "Disk images",
				icon: Icon::Disc,
				tint: Tint::Navy,
				extensions: "iso img dmg bin cue nrg mdf mds toast cdr vhd vhdx vmdk vdi qcow qcow2 wim \
					esd hdd sparseimage sparsebundle",
			},
		]
	};

	pub fn find_preset(name: &str) -> Option<&'static Preset> {
		Category::PRESETS.iter().find(|p| p.name == name)
	}

	/// A preset with the user's changes applied; None for a name that is not a preset.
	pub fn from_preset(id: u64, name: &str, overrides: Overrides) -> Option<Category> {
		let preset = Category::find_preset(name)?;
		let pattern = pattern_for_extensions(&merged_extensions(&preset.base(), &overrides).join(" "));
		let mut category = Category::new(id, preset.name, preset.icon, &pattern).ok()?;
		category.color = preset.tint.rgb();
		category.preset = Some((preset, overrides));
		Some(category)
	}

	#[cfg(test)]
	pub fn preset(name: &str) -> Option<Category> {
		Category::from_preset(0, name, Overrides::default())
	}

	/// The extensions a preset runs with; empty for a custom rule, which has no list.
	pub fn extensions(&self) -> Vec<String> {
		match &self.preset {
			Some((preset, overrides)) => merged_extensions(&preset.base(), overrides),
			None => Vec::new(),
		}
	}

	/// Rewrites a preset's list, one extension at a time. A built-in extension is switched off
	/// by naming it in `removed` and back on by taking it out; an added one is dropped outright.
	/// Does nothing to a custom rule.
	pub fn set_extension(&mut self, extension: &str, on: bool) {
		let Some((preset, overrides)) = &mut self.preset else { return };
		let extension = extension.trim().trim_start_matches('.').to_lowercase();
		if preset.base().contains(&extension) {
			overrides.removed.retain(|e| *e != extension);
			if !on {
				overrides.removed.push(extension);
			}
		} else {
			overrides.added.retain(|e| *e != extension);
			if on {
				overrides.added.push(extension);
			}
		}
		self.recompile();
	}

	pub fn reset_preset(&mut self) {
		if let Some((_, overrides)) = &mut self.preset {
			*overrides = Overrides::default();
		}
		self.recompile();
	}

	fn recompile(&mut self) {
		if self.preset.is_some() {
			self.pattern = pattern_for_extensions(&self.extensions().join(" "));
			self.regex = fancy_regex::Regex::new(&self.pattern).ok();
		}
	}

	pub fn defaults() -> Vec<Category> {
		let mut all: Vec<Category> = Category::PRESETS
			.iter()
			.enumerate()
			.map(|(i, preset)| {
				Category::from_preset(i as u64 + 1, preset.name, Overrides::default())
					.expect("a preset compiles")
			})
			.collect();
		all.push(
			Category::new(all.len() as u64 + 1, "Other", crate::ui::icon::Icon::File, "")
				.expect("empty is fine"),
		);
		all
	}

	pub fn is_catch_all(&self) -> bool {
		self.regex.is_none()
	}
}

/// The extensions a pattern of the shape `pattern_for_extensions` writes stands for; None for
/// any other pattern. How a file from before presets kept their lists is recognised.
pub fn extensions_of_pattern(pattern: &str) -> Option<Vec<String>> {
	let inner = pattern.strip_prefix(r"(?i)\.(")?.strip_suffix(")$")?;
	let list: Vec<String> = inner.split('|').map(|e| e.replace('\\', "")).collect();
	if list.is_empty() || list.iter().any(|e| e.is_empty() || e.contains(['(', ')', '[', '.'])) {
		return None;
	}
	Some(list)
}

/// `rs, py` or `rs py` as a list: trimmed, a leading dot dropped, lowercase, empties gone.
pub fn split_extensions(extensions: &str) -> Vec<String> {
	extensions
		.split(|c: char| c == ',' || c.is_whitespace())
		.map(|e| e.trim().trim_start_matches('.').to_lowercase())
		.filter(|e| !e.is_empty())
		.collect()
}

/// `rs, py` or `rs py` becomes `(?i)\.(rs|py)$`: what a user means by "these file types", spelled
/// as the regular expression the category actually runs.
pub fn pattern_for_extensions(extensions: &str) -> String {
	let list: Vec<String> =
		split_extensions(extensions).iter().map(|e| fancy_regex::escape(e).into_owned()).collect();
	if list.is_empty() { String::new() } else { format!(r"(?i)\.({})$", list.join("|")) }
}

/// How the custom form's two fields combine when both are filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Combine {
	And,
	Or,
}

/// The basic rule the custom form offers: extensions, text the name contains, or both, spelled
/// as the one regular expression the category runs. The text is taken literally; `ignore_case`
/// does what it says, and `ignore_space` lets any run of whitespace in the text match any run of
/// whitespace or none, so "rust book" finds "rust  book" and "RustBook" alike. The extensions
/// are always matched without regard to case, as they are alone.
pub fn pattern_for_rule(
	extensions: &str,
	contains: &str,
	combine: Combine,
	ignore_case: bool,
	ignore_space: bool,
) -> String {
	let suffix = pattern_for_extensions(extensions);
	let mut text = if ignore_space {
		contains
			.split_whitespace()
			.map(|part| fancy_regex::escape(part).into_owned())
			.collect::<Vec<_>>()
			.join(r"\s*")
	} else {
		let trimmed = contains.trim();
		if trimmed.is_empty() { String::new() } else { fancy_regex::escape(trimmed).into_owned() }
	};
	if ignore_case && !text.is_empty() {
		text = format!("(?i:{text})");
	}
	match (text.is_empty(), suffix.is_empty()) {
		(true, true) => String::new(),
		(true, false) => suffix,
		(false, true) => text,
		// The suffix pattern carries its own (?i); beside text it becomes a group so the text keeps
		// its own case rule.
		(false, false) => {
			let suffix = format!("(?i:{})", suffix.trim_start_matches("(?i)").trim_end_matches('$'));
			match combine {
				Combine::And => format!("^(?=.*{text}).*{suffix}$"),
				Combine::Or => format!("(?:{text}|{suffix}$)"),
			}
		}
	}
}

/// Every category whose pattern matches, in the sidebar's order; the catch-all alone when none
/// does. A file that two rules describe is in both, which is what makes an order between rules
/// unnecessary.
pub fn categories_of<'a>(categories: &'a [Category], download: &Download) -> Vec<&'a Category> {
	let matched: Vec<&Category> = categories.iter().filter(|c| c.matches(download)).collect();
	if matched.is_empty() {
		categories.iter().filter(|c| c.is_catch_all()).take(1).collect()
	} else {
		matched
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
	/// Why it failed, in the engine's words, while it is failed.
	pub error: Option<String>,
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
				categories.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_default()
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
			Filter::Category(id) => {
				categories.iter().find(|c| c.id == id).map_or(Tint::Snow.rgb(), |c| c.color)
			}
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
		error: None,
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
	fn a_file_is_in_every_category_that_matches_and_other_takes_the_rest() {
		let mut categories = Category::defaults();
		categories.push(Category::new(99, "Ubuntu", Icon::Disc, "(?i)^ubuntu").unwrap());
		let names = |name: &str| -> Vec<String> {
			categories_of(&categories, &named(name)).iter().map(|c| c.name.clone()).collect()
		};
		assert_eq!(names("talk.MP4"), ["Video"]);
		assert_eq!(names("ubuntu-26.04.iso"), ["Disk images", "Ubuntu"]);
		assert_eq!(names("notes.unknown"), ["Other"]);
	}

	#[test]
	fn extensions_become_one_anchored_case_insensitive_pattern() {
		assert_eq!(pattern_for_extensions("rs, py .ts"), r"(?i)\.(rs|py|ts)$");
		assert_eq!(pattern_for_extensions("c++"), r"(?i)\.(c\+\+)$");
		assert_eq!(pattern_for_extensions(" , "), "");
	}

	#[test]
	fn a_basic_rule_is_extensions_or_text_or_both() {
		let matches = |pattern: &str, name: &str| {
			Category::new(1, "x", Icon::File, pattern).unwrap().matches(&named(name))
		};
		let rule = |e, c, and, case, space| {
			pattern_for_rule(e, c, if and { Combine::And } else { Combine::Or }, case, space)
		};
		assert_eq!(rule("", "", true, false, false), "");
		assert_eq!(rule("rs", "", true, false, false), r"(?i)\.(rs)$");
		let strict = rule("", "Rust book", true, false, false);
		assert!(matches(&strict, "The Rust book.pdf") && !matches(&strict, "rust-book.pdf"));
		let loose = rule("", "Rust book", true, true, true);
		assert!(matches(&loose, "rust  book.pdf") && matches(&loose, "RUSTBOOK.epub"));
		let case_only = rule("", "Rust book", true, true, false);
		assert!(matches(&case_only, "rust BOOK.pdf") && !matches(&case_only, "rustbook.pdf"));
		let both = rule("pdf epub", "rust book", true, true, true);
		assert!(matches(&both, "RustBook.PDF") && !matches(&both, "rust book.mp4"));
		let both_strict = rule("pdf", "rust", true, false, false);
		assert!(matches(&both_strict, "rust.PDF") && !matches(&both_strict, "Rust.pdf"));
		let either = rule("pdf", "rust", false, false, false);
		assert!(matches(&either, "notes.PDF") && matches(&either, "rust.mp4"));
		assert!(!matches(&either, "Rust.mp4"));
	}

	#[test]
	fn a_pattern_of_extensions_reads_back_as_its_list() {
		assert_eq!(extensions_of_pattern(r"(?i)\.(rs|c\+\+)$").unwrap(), ["rs", "c++"]);
		assert_eq!(extensions_of_pattern(r"(?i)^ubuntu"), None);
		assert_eq!(extensions_of_pattern(r"(?i)\.(a|(b))$"), None);
	}

	#[test]
	fn a_preset_keeps_the_built_in_list_apart_from_the_users_changes() {
		let mut video = Category::preset("Video").unwrap();
		video.set_extension("mkv", false);
		video.set_extension("xyz", true);
		video.set_extension(".XYZ", true);
		let list = video.extensions();
		assert!(!list.contains(&"mkv".to_owned()) && list.last() == Some(&"xyz".to_owned()));
		assert!(!video.matches(&named("a.mkv")) && video.matches(&named("a.xyz")));
		let (_, overrides) = video.preset.clone().unwrap();
		assert_eq!(
			(overrides.added, overrides.removed),
			(vec!["xyz".to_owned()], vec!["mkv".to_owned()])
		);
		video.set_extension("mkv", true);
		video.set_extension("xyz", false);
		assert!(video.preset.as_ref().unwrap().1.is_empty());
		video.reset_preset();
		assert_eq!(video.pattern, Category::preset("Video").unwrap().pattern);
	}

	#[test]
	fn a_bad_pattern_is_an_error_with_the_engine_message_and_lookaround_is_allowed() {
		assert!(Category::new(1, "x", Icon::File, "(").is_err());
		let lookahead = Category::new(1, "Not video", Icon::File, r"^(?!.*\.mp4$).*$").unwrap();
		assert!(lookahead.matches(&named("a.pdf")));
		assert!(!lookahead.matches(&named("a.mp4")));
	}
}
