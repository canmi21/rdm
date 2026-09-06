//! What a category is: a rule over file names, the presets the application ships with their
//! extension lists, and the user's changes kept apart from those lists. Which categories a
//! download falls into is decided here; the download itself is in `download`.

use serde::Serialize;

use crate::download::Download;
use crate::ui::icon::Icon;
use crate::ui::theme::Tint;

/// A built-in category: a name, an icon and the extensions it starts with. The list is the
/// application's and grows with releases; a user's changes to it are kept apart, so a release
/// that adds an extension reaches every user who did not remove it on purpose.
#[derive(Debug)]
pub struct Preset {
	pub name: &'static str,
	pub icon: Icon,
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
	pub icon: Icon,
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
	pub fn new(id: u64, name: &str, icon: Icon, pattern: &str) -> Result<Category, String> {
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
		self.matches_name(&download.name)
	}

	/// The rule against a bare name: a download's, or a file's inside an archive.
	pub fn matches_name(&self, name: &str) -> bool {
		match &self.regex {
			Some(regex) => regex.is_match(name).unwrap_or(false),
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
	pub const PRESETS: [Preset; 15] = {
		use crate::ui::theme::Tint;
		use Icon;
		[
			Preset {
				name: "Videos",
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
				name: "Plain Text",
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
				name: "eBooks",
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
				name: "Disk Images",
				icon: Icon::Disc,
				tint: Tint::Navy,
				// What holds a filesystem: an installer's image, an optical one, a machine's disk.
				// `bin` and `hex` were here and are Firmware's now -- a firmware image is a chip's
				// contents and not a filesystem, and the two were being counted as one thing.
				extensions: "iso img dmg cue nrg mdf mds toast cdr vhd vhdx vmdk vdi qcow qcow2 wim \
					esd hdd sparseimage sparsebundle",
			},
			Preset {
				name: "Firmware",
				icon: Icon::Cpu,
				tint: Tint::Teal,
				// A chip's contents rather than a disk's, in five families. `img` is deliberately
				// not here: it is a filesystem image far more often than a firmware one, and
				// Disk Images has the better claim on it. `cap` is not here either -- a packet
				// capture answers to it more often than a UEFI capsule does.
				extensions: "\
					bin hex ihex ihx srec s19 s28 s37 mot sre exo eep \
					elf axf out lss \
					uf2 dfu fwu fw swu ota gbl cyacd cyacd2 apj px4 \
					rbf sof pof jed jedec svf xsvf jam jbc mcs rpd jic bit bitstream \
					rom bios fd capsule wph bio trx chk ipsw kdz ftf",
			},
			Preset {
				name: "3D Models",
				icon: Icon::Box,
				tint: Tint::Purple,
				extensions: "stl obj 3mf step stp iges igs fbx dae ply gltf glb usd usda usdc usdz blend \
					3ds max c4d skp scad amf wrl x3d off",
			},
			Preset {
				name: "Torrents",
				icon: Icon::Magnet,
				tint: Tint::Orange,
				extensions: "torrent",
			},
		]
	};

	/// The category with this id, if it is still there.
	pub fn find(categories: &[Category], id: u64) -> Option<&Category> {
		categories.iter().find(|c| c.id == id)
	}

	pub fn find_preset(name: &str) -> Option<&'static Preset> {
		// The names a config.json carried before a preset was renamed are still that preset: the
		// sidebar took Title Case and the plural for what can be counted, as Finder's does.
		let name = match name {
			"Ebooks" => "eBooks",
			"Video" => "Videos",
			"Plain text" => "Plain Text",
			"Disk images" => "Disk Images",
			other => other,
		};
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

	/// Back to the preset as shipped: its list, its icon and its color.
	pub fn reset_preset(&mut self) {
		if let Some((preset, overrides)) = &mut self.preset {
			*overrides = Overrides::default();
			self.icon = preset.icon;
			self.color = preset.tint.rgb();
			self.custom_color = None;
		}
		self.recompile();
	}

	/// Whether anything differs from the preset as shipped; false for a custom rule.
	pub fn differs_from_preset(&self) -> bool {
		match &self.preset {
			Some((preset, overrides)) => {
				!overrides.is_empty() || self.icon != preset.icon || self.color != preset.tint.rgb()
			}
			None => false,
		}
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
		all.push(Category::new(all.len() as u64 + 1, "Other", Icon::File, "").expect("empty is fine"));
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
	categories_with_contents(categories, download, &[])
}

/// What an archive is by what it holds: the first category that every one of its contents
/// matches and its own name does not -- Programs for a zip of one `.exe`, Audio for a tar of
/// songs. None for a row that is not an archive, or one whose contents say nothing more than
/// its name. The row's icon is this one's wherever the row is listed, since what is inside is
/// what the thing is; the wrapper is how it arrived.
pub fn nature<'a>(
	categories: &'a [Category],
	download: &Download,
	contents: &[String],
) -> Option<&'a Category> {
	if contents.is_empty() {
		return None;
	}
	categories.iter().find(|c| {
		!c.is_catch_all() && !c.matches(download) && contents.iter().all(|n| c.matches_name(n))
	})
}

/// The same, for a download that is an archive whose top-level names are known: it is in every
/// category its own name matches, and also in every category that every one of its contents
/// matches -- a zip of one `.exe` is a program, a tar of `.mp3`s is audio, a zip of a `.pdf`
/// and a `.mp4` is only an archive. Other takes it when nothing else does. See spec/ui.md.
pub fn categories_with_contents<'a>(
	categories: &'a [Category],
	download: &Download,
	contents: &[String],
) -> Vec<&'a Category> {
	let mut matched: Vec<&Category> = categories.iter().filter(|c| c.matches(download)).collect();
	if !contents.is_empty() {
		for category in categories.iter().filter(|c| !c.is_catch_all()) {
			if !matched.iter().any(|m| m.id == category.id)
				&& contents.iter().all(|name| category.matches_name(name))
			{
				matched.push(category);
			}
		}
		matched.sort_by_key(|c| categories.iter().position(|k| k.id == c.id));
	}
	if matched.is_empty() {
		categories.iter().filter(|c| c.is_catch_all()).take(1).collect()
	} else {
		matched
	}
}

#[cfg(test)]
mod tests {
	/// Firmware is five families of extension and none of them is a disk's. The two that had to
	/// be argued over are checked by name: `img` is a filesystem far more often than a chip, and
	/// `cap` is a packet capture far more often than a UEFI capsule.
	#[test]
	fn firmware_covers_its_families_and_leaves_the_disks_alone() {
		let firmware = Category::find_preset("Firmware").expect("a preset");
		let has = |ext: &str| firmware.base().iter().any(|e| e == ext);
		// The toolchain's output, the record formats, the flashing containers, the bitstreams,
		// and the vendors' whole-device images.
		for ext in ["bin", "hex", "elf", "srec", "uf2", "dfu", "rbf", "jed", "trx", "ipsw"] {
			assert!(has(ext), "firmware takes {ext}");
		}
		for ext in ["img", "cap", "iso", "dmg"] {
			assert!(!has(ext), "firmware leaves {ext} alone");
		}
		let disks = Category::find_preset("Disk Images").expect("a preset");
		let disk_has = |ext: &str| disks.base().iter().any(|e| e == ext);
		assert!(disk_has("img") && disk_has("iso"), "which is where img and iso belong");
		assert!(!disk_has("bin") && !disk_has("hex"), "and where bin and hex no longer do");
	}

	use super::*;
	use crate::download::{Download, sample};

	fn named(name: &str) -> Download {
		let mut d = sample().remove(0);
		d.name = name.to_owned();
		d
	}

	#[test]
	fn an_archive_is_also_where_all_of_its_contents_are() {
		let categories = Category::defaults();
		let names = |name: &str, contents: &[&str]| -> Vec<String> {
			let contents: Vec<String> = contents.iter().map(|c| (*c).to_owned()).collect();
			categories_with_contents(&categories, &named(name), &contents)
				.iter()
				.map(|c| c.name.clone())
				.collect()
		};
		assert_eq!(names("tool.zip", &["setup.exe"]), ["Archives", "Programs"]);
		assert_eq!(names("Foo.zip", &["Foo.app"]), ["Archives", "Programs"]);
		assert_eq!(names("album.tar", &["01.mp3", "02.flac"]), ["Audio", "Archives"]);
		assert_eq!(names("mixed.zip", &["a.pdf", "b.mp4"]), ["Archives"]);
		assert_eq!(names("mixed.zip", &[]), ["Archives"]);
		assert_eq!(names("plain.xyz", &["a.mp3"]), ["Audio"], "the contents alone can place it");
		let contents = |c: &[&str]| -> Vec<String> { c.iter().map(|c| (*c).to_owned()).collect() };
		let what = |name: &str, c: &[&str]| {
			nature(&categories, &named(name), &contents(c)).map(|c| c.name.clone())
		};
		assert_eq!(what("tool.zip", &["setup.exe"]).as_deref(), Some("Programs"));
		assert_eq!(what("mixed.zip", &["a.pdf", "b.mp4"]), None);
		assert_eq!(what("inner.zip", &["a.zip", "b.zip"]), None, "archives of archives stay so");
		assert_eq!(what("tool.zip", &[]), None);
	}

	#[test]
	fn a_file_is_in_every_category_that_matches_and_other_takes_the_rest() {
		let mut categories = Category::defaults();
		categories.push(Category::new(99, "Ubuntu", Icon::Disc, "(?i)^ubuntu").unwrap());
		let names = |name: &str| -> Vec<String> {
			categories_of(&categories, &named(name)).iter().map(|c| c.name.clone()).collect()
		};
		assert_eq!(names("talk.MP4"), ["Videos"]);
		assert_eq!(names("ubuntu-26.04.iso"), ["Disk Images", "Ubuntu"]);
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
		let mut video = Category::preset("Videos").unwrap();
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
		assert_eq!(video.pattern, Category::preset("Videos").unwrap().pattern);
	}

	#[test]
	fn a_bad_pattern_is_an_error_with_the_engine_message_and_lookaround_is_allowed() {
		assert!(Category::new(1, "x", Icon::File, "(").is_err());
		let lookahead = Category::new(1, "Not video", Icon::File, r"^(?!.*\.mp4$).*$").unwrap();
		assert!(lookahead.matches(&named("a.pdf")));
		assert!(!lookahead.matches(&named("a.mp4")));
	}
}
