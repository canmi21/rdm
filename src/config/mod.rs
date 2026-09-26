//! What the user shaped: the categories here, and the settings in `preferences`. `config.json` in
//! the platform's configuration directory, versioned like state.json, seeded once and then the
//! user's to edit. See spec/state.md.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::category::{Category, Overrides, Preset, extensions_of_pattern};
use crate::state::{parse_versioned, write_json};
use crate::ui::icon::Icon;
use crate::ui::theme::{format_hex, parse_color};

mod preferences;

pub use preferences::Preferences;

pub const VERSION: u64 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
	pub version: u64,
	#[serde(default)]
	pub categories: Vec<CategoryConfig>,
	/// What the settings sheet offers; absent in a file from before it did, and then the
	/// defaults.
	#[serde(default)]
	pub settings: Preferences,
	/// Every preset this file has been offered, whether or not it is still among the categories.
	/// A preset added to the application after this file was written is not in here, and is
	/// seeded on the next load: without it a new category would exist only for somebody starting
	/// fresh. With it, a preset the user took away stays away, since taking it away leaves the
	/// name here. Absent in a file from before this, which is read as having been offered
	/// whatever it holds. See spec/state.md.
	#[serde(default)]
	pub offered: Vec<String>,
}

/// A category as the file spells it. A custom rule carries its pattern as written and its
/// color as hex. A preset carries its name under `preset` and the user's changes to its list
/// -- extensions added, and built-in ones removed -- and no pattern, since the pattern is
/// derived from the list the application ships, which a release may extend; its icon is
/// written always and its color only when it is not the preset's own, so a preset the user
/// left alone follows the application's choice. A file from before presets were kept this way
/// spells them as patterns; one whose name and pattern are a preset's is read as that preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CategoryConfig {
	pub name: String,
	pub icon: String,
	#[serde(default, skip_serializing_if = "String::is_empty")]
	pub pattern: String,
	/// `#rrggbb`; absent for a preset drawn in its own color.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub color: Option<String>,
	/// A color the user wrote, as written; offered beside the named ones whether or not it is
	/// the one in use.
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub custom_color: Option<String>,
	/// A colour for one extension within this category, `#rrggbb` by extension. Only what differs
	/// from the preset is written, so a preset's own shades can change under a file that never
	/// touched them; an extension the user set back to the category's colour is written as an
	/// empty string, which is how "inherit" is told from "never said".
	#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
	pub shades: BTreeMap<String, String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub preset: Option<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub added: Vec<String>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub removed: Vec<String>,
}

impl Config {
	/// The starting file: the built-in categories, so a user who wants to change them finds them
	/// written down rather than baked in.
	pub fn seed() -> Config {
		Config {
			version: VERSION,
			categories: Category::defaults().iter().map(CategoryConfig::from).collect(),
			settings: Preferences::default(),
			offered: Category::PRESETS.iter().map(|preset| preset.name.to_owned()).collect(),
		}
	}

	/// Adds the presets this file has never been offered, before the catch-all at the end, and
	/// says whether it added any -- the caller writes the file back when it did. A file that
	/// predates the record is taken to have been offered the presets it holds, so nothing the
	/// user removed comes back; only what the application has learned since arrives.
	pub fn offer_new_presets(&mut self) -> bool {
		if self.offered.is_empty() {
			self.offered = self.categories.iter().map(|c| c.name.clone()).collect();
		}
		let new: Vec<&Preset> = Category::PRESETS
			.iter()
			.filter(|preset| !self.offered.iter().any(|name| name == preset.name))
			.collect();
		if new.is_empty() {
			return false;
		}
		// Before the catch-all, which is last by rule: a category that matches everything after
		// one that matches something is never reached. The catch-all is the one with no preset
		// and no pattern -- a preset writes no pattern either, so an empty pattern alone finds
		// the first preset in the file and puts every new category at the very top.
		let at = self
			.categories
			.iter()
			.position(|c| c.preset.is_none() && c.pattern.is_empty())
			.unwrap_or(self.categories.len());
		for (offset, preset) in new.iter().enumerate() {
			let category =
				Category::from_preset(0, preset.name, Overrides::default()).expect("a preset compiles");
			self.categories.insert(at + offset, CategoryConfig::from(&category));
			self.offered.push(preset.name.to_owned());
		}
		true
	}

	/// The categories in the file's order, ids assigned by position. A pattern that does not
	/// compile is reported and skipped rather than taking the rest down with it; an icon name
	/// that is not one of the choices draws as a plain file; a preset name the application does
	/// not know is read as a custom rule over whatever pattern is there.
	pub fn categories(&self) -> Vec<Category> {
		self
			.categories
			.iter()
			.enumerate()
			.filter_map(|(i, c)| {
				let id = i as u64 + 1;
				let mut overrides = Overrides { added: c.added.clone(), removed: c.removed.clone() };
				// A file from before presets kept their lists spells one as its pattern. A preset's
				// name over a plain list of extensions is that preset, with whatever the list had
				// beyond the built-in one kept as additions; nothing is marked removed, so the
				// extensions a release added since arrive as they do for everyone.
				let preset = c.preset.as_deref().or_else(|| {
					let preset = Category::find_preset(&c.name)?;
					let old = extensions_of_pattern(&c.pattern)?;
					let base = preset.base();
					overrides.added.extend(old.into_iter().filter(|e| !base.contains(e)));
					Some(preset.name)
				});
				let color = c.color.as_deref().and_then(parse_color);
				let custom = c.custom_color.clone().filter(|text| parse_color(text).is_some());
				if let Some(mut category) =
					preset.and_then(|name| Category::from_preset(id, name, overrides))
				{
					if let Some(icon) = Icon::by_name(&c.icon) {
						category.icon = icon;
					}
					if let Some(color) = color {
						category.color = color;
					}
					category.custom_color = custom;
					apply_shades(&mut category.shades, &c.shades);
					return Some(category);
				}
				let icon = Icon::by_name(&c.icon).unwrap_or(Icon::File);
				match Category::new(id, &c.name, icon, &c.pattern) {
					Ok(mut category) => {
						if let Some(color) = color {
							category.color = color;
						}
						category.custom_color = custom;
						apply_shades(&mut category.shades, &c.shades);
						Some(category)
					}
					Err(error) => {
						eprintln!(
							"config.json: category {:?} skipped, its pattern does not compile: {error}",
							c.name
						);
						None
					}
				}
			})
			.collect()
	}

	pub fn from_parts(categories: &[Category], settings: &Preferences) -> Config {
		Config {
			version: VERSION,
			categories: categories.iter().map(CategoryConfig::from).collect(),
			settings: settings.clone(),
			// Every preset has been offered by the time anything is saved: the load offers what
			// the file had never seen, so there is nothing to carry through the window for this.
			offered: Category::PRESETS.iter().map(|preset| preset.name.to_owned()).collect(),
		}
	}
}

/// The file's shades over whatever the preset seeded: a colour replaces one, an empty string
/// takes one away, and an extension the file does not mention keeps what the preset gave it.
fn apply_shades(shades: &mut BTreeMap<String, u32>, written: &BTreeMap<String, String>) {
	for (extension, text) in written {
		let extension = extension.to_ascii_lowercase();
		match parse_color(text) {
			Some(color) => {
				shades.insert(extension, color);
			}
			None => {
				shades.remove(&extension);
			}
		}
	}
}

/// What to write for a category's shades: only where they differ from the preset's, so the
/// built-in list can change under a file that never touched it. An extension the preset shades
/// and the user set back to the category's colour is written as an empty string, which is the
/// only way to tell "inherit, deliberately" from "never said".
fn shades_against(
	shades: &BTreeMap<String, u32>,
	preset: &[(&'static str, crate::ui::theme::Tint)],
) -> BTreeMap<String, String> {
	let mut written = BTreeMap::new();
	for (extension, color) in shades {
		let built_in = preset.iter().find(|(e, _)| e == extension).map(|(_, t)| t.rgb());
		if built_in != Some(*color) {
			written.insert(extension.clone(), format_hex(*color));
		}
	}
	for (extension, _) in preset {
		if !shades.contains_key(*extension) {
			written.insert((*extension).to_owned(), String::new());
		}
	}
	written
}

impl From<&Category> for CategoryConfig {
	fn from(c: &Category) -> Self {
		match &c.preset {
			Some((preset, overrides)) => CategoryConfig {
				name: c.name.clone(),
				icon: c.icon.name().to_owned(),
				pattern: String::new(),
				color: (c.color != preset.tint.rgb()).then(|| format_hex(c.color)),
				custom_color: c.custom_color.clone(),
				shades: shades_against(&c.shades, preset.shades),
				preset: Some(preset.name.to_owned()),
				added: overrides.added.clone(),
				removed: overrides.removed.clone(),
			},
			None => CategoryConfig {
				name: c.name.clone(),
				icon: c.icon.name().to_owned(),
				pattern: c.pattern.clone(),
				// Always written: the cycle it started in is by position, which reordering moves.
				color: Some(format_hex(c.color)),
				custom_color: c.custom_color.clone(),
				shades: shades_against(&c.shades, &[]),
				preset: None,
				added: Vec::new(),
				removed: Vec::new(),
			},
		}
	}
}

pub fn parse(text: &str) -> Result<Config> {
	parse_versioned(text, VERSION, migrate)
}

/// One step, from `from` to `from + 1`. Each breaking change adds an arm and bumps VERSION; the
/// arms are the history of the file's shape and are never removed. See spec/state.md.
fn migrate(from: u64, mut value: Value) -> Result<Value> {
	match from {
		// 1 -> 2: `System` was taken out of the languages, so a file naming it names a variant
		// this build has no arm for -- and one field it cannot read fails the whole object,
		// which would cost the reader their categories and every switch beside them. The machine
		// is asked once, here, and the answer written down: which is the same act a first launch
		// performs, arriving at the same place. See src/i18n.rs.
		1 => {
			if let Some(settings) = value.get_mut("settings").and_then(Value::as_object_mut)
				&& settings.get("language").and_then(Value::as_str) == Some("system")
			{
				let detected = serde_json::to_value(crate::i18n::Language::detected())?;
				settings.insert("language".to_owned(), detected);
			}
			Ok(value)
		}
		_ => bail!("no migration from config.json version {from}"),
	}
}

/// The file if it is there and readable; the seed, written, if it is not there at all. A file
/// that is there but unreadable is left exactly as it is and the seed is used for the run, so a
/// hand edit that went wrong is not overwritten by the application correcting it.
pub fn load_or_seed(path: &Path) -> Config {
	match std::fs::read_to_string(path) {
		Ok(text) => parse(&text)
			.map(|mut config| {
				// A preset the application learned since this file was written arrives now, and
				// the file is rewritten so it is not offered a second time after the user removes
				// it.
				if config.offer_new_presets()
					&& let Err(error) = write_json(path, &config)
				{
					eprintln!("could not write {}: {error:#}", path.display());
				}
				config
			})
			.unwrap_or_else(|error| {
				eprintln!("ignoring {}: {error:#}", path.display());
				Config::seed()
			}),
		Err(_) => {
			let seed = Config::seed();
			if let Err(error) = write_json(path, &seed) {
				eprintln!("could not write {}: {error:#}", path.display());
			}
			seed
		}
	}
}

pub fn save(path: &Path, config: &Config) -> Result<()> {
	write_json(path, config)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::update::Policy;

	/// A preset the application learned after a file was written has to reach that file, or the
	/// category exists only for somebody starting fresh. What the user took away stays away: its
	/// name is in the record of what has been offered, and only what is missing from that record
	/// arrives.
	#[test]
	fn a_preset_added_since_a_file_was_written_arrives_and_a_removed_one_stays_away() {
		let mut config = Config::seed();
		let before = config.categories.len();
		// The seed takes the common presets and records the rest as offered but not taken, so a
		// fresh file is shorter than the list of presets and stays that way.
		assert_eq!(before, Category::COMMON.len() + 1, "the common presets, then the catch-all");
		assert!(!config.offer_new_presets(), "a fresh file has been offered everything");
		assert_eq!(config.categories.len(), before);
		// The user takes one away; it is still on the record, so it does not come back.
		config.categories.retain(|c| c.name != "Archives");
		assert!(!config.offer_new_presets(), "what was taken away stays away");
		assert!(!config.categories.iter().any(|c| c.name == "Archives"));
		// A file from before the record is read as having been offered what it holds.
		config.offered.clear();
		assert!(config.offer_new_presets(), "every preset it does not hold is news to it now");
		let names: Vec<&str> = config.categories.iter().map(|c| c.name.as_str()).collect();
		assert!(names.contains(&"Torrents") && names.contains(&"Archives"));
		assert_eq!(names.last(), Some(&"Other"), "and the catch-all is still last");
		// Just before the catch-all, not at the top: a preset writes no pattern either, so
		// looking for an empty one finds the first preset in the file. In the order the
		// application lists them, which ends with 3D Models and then Torrents.
		assert_eq!(&names[names.len() - 3..], ["3D Models", "Torrents", "Other"], "{names:?}");
		assert_eq!(names[0], "Videos", "and what was there stays where it was");
	}
	use crate::testing::scratch;

	#[test]
	fn a_missing_file_is_seeded_with_the_defaults_and_written() {
		let dir = scratch("seed");
		let path = dir.join("config.json");
		let config = load_or_seed(&path);
		assert_eq!(config.categories.len(), Category::COMMON.len() + 1, "the common ones, then Other");
		assert_eq!(config.categories[0].name, "Videos");
		assert_eq!(parse(&std::fs::read_to_string(&path).unwrap()).unwrap(), config);
		std::fs::remove_dir_all(dir).ok();
	}

	#[test]
	fn an_existing_file_is_read_as_the_user_left_it() {
		let text = r#"{ "version": 1, "categories": [
			{ "name": "Papers", "icon": "book-open", "pattern": "(?i)\\.pdf$" },
			{ "name": "Everything else", "icon": "no-such-icon", "pattern": "" }
		] }"#;
		let categories = parse(text).unwrap().categories();
		assert_eq!(categories.len(), 2);
		assert_eq!((categories[0].name.as_str(), categories[0].icon), ("Papers", Icon::BookOpen));
		assert_eq!(categories[1].icon, Icon::File, "an unknown icon name draws as a plain file");
	}

	#[test]
	fn a_preset_is_read_by_name_with_its_changes_and_an_old_file_by_its_pattern() {
		let text = r##"{ "version": 1, "categories": [
			{ "name": "Video", "icon": "disc", "color": "#abc", "custom_color": "rgb(1, 2, 3)", "preset": "Video", "added": ["xyz"], "removed": ["mkv"] },
			{ "name": "Audio", "icon": "music", "pattern": "(?i)\\.(mp3|flac|aac|wav|m4a|ogg|xyz)$" },
			{ "name": "Films", "icon": "film", "pattern": "(?i)\\.(mp4)$" },
			{ "name": "Ebooks", "icon": "code", "preset": "Ebooks" },
			{ "name": "Disk images", "icon": "disc", "preset": "Disk images" }
		] }"##;
		let categories = parse(text).unwrap().categories();
		assert_eq!(
			(categories[3].name.as_str(), categories[3].icon),
			("eBooks", Icon::Code),
			"a preset and an icon under the names an older file wrote"
		);
		assert_eq!(categories[4].name, "Disk Images", "a preset renamed for the sidebar's style");
		assert_eq!(categories[0].name, "Videos", "the preset's name, not the file's, is shown");
		let video = categories[0].extensions();
		assert!(!video.contains(&"mkv".to_owned()) && video.last() == Some(&"xyz".to_owned()));
		assert_eq!((categories[0].icon, categories[0].color), (Icon::Disc, 0xaabbcc), "overrides hold");
		assert_eq!(categories[0].custom_color.as_deref(), Some("rgb(1, 2, 3)"), "kept as written");
		assert!(
			categories[1].preset.is_some(),
			"a preset's name and pattern from before is that preset"
		);
		let audio = categories[1].extensions();
		assert!(audio.contains(&"opus".to_owned()), "the built-in list has grown under it");
		assert_eq!(audio.last().map(String::as_str), Some("xyz"), "what it had beyond is kept");
		assert!(categories[2].preset.is_none(), "a custom rule stays a custom rule");
		let written = Config::from_parts(&categories, &Preferences::default());
		assert_eq!(written.categories[0].added, ["xyz"]);
		assert_eq!(written.categories[0].pattern, "", "a preset's pattern is not written");
		assert_eq!(written.categories[0].color.as_deref(), Some("#aabbcc"));
		assert_eq!(written.categories[1].color, None, "a preset in its own color writes none");
		assert!(written.categories[2].color.is_some(), "a custom rule always writes its color");
		assert_eq!(written.categories[1].preset.as_deref(), Some("Audio"));
		assert_eq!(parse(&serde_json::to_string(&written).unwrap()).unwrap(), written);
	}

	/// `System` came out of the languages, so a file naming it names a variant with no arm. One
	/// field it cannot read fails the whole object, which would have cost the reader their
	/// categories and every switch beside them -- so the arm resolves it rather than refusing.
	#[test]
	fn a_file_that_followed_the_machine_is_given_the_language_it_meant() {
		let old = parse(
			r#"{ "version": 1, "categories": [], "settings": { "language": "system", "retries": 9 } }"#,
		)
		.unwrap();
		assert_eq!(old.settings.language, crate::i18n::Language::detected(), "asked once, here");
		assert!(
			crate::i18n::Language::ALL.contains(&old.settings.language),
			"and it is one of the three, whatever the machine is set to"
		);
		assert_eq!(old.settings.retries, Some(9), "the rest of the file came through with it");
		// A file that had already chosen is left alone: only `system` was unreadable.
		let chosen =
			parse(r#"{ "version": 1, "categories": [], "settings": { "language": "ja" } }"#).unwrap();
		assert_eq!(chosen.settings.language, crate::i18n::Language::Ja);
		// And one written by this build reads back as itself.
		let now =
			parse(r#"{ "version": 2, "categories": [], "settings": { "language": "zh" } }"#).unwrap();
		assert_eq!(now.settings.language, crate::i18n::Language::Zh);
	}

	#[test]
	fn a_switch_missing_from_the_file_reads_as_its_default_and_a_set_one_holds() {
		let old = parse(r#"{ "version": 1, "categories": [] }"#).unwrap();
		assert!(old.settings.colorful_categories, "a file from before the switch");
		let off = parse(r#"{ "version": 1, "settings": { "colorful_categories": false } }"#).unwrap();
		assert!(!off.settings.colorful_categories);
		assert!(
			off.settings.check_updates && off.settings.auto_update,
			"the update switches default on"
		);
		let quiet = parse(
			r#"{ "version": 1, "settings": { "check_updates": false, "update_policy": "notify" } }"#,
		)
		.unwrap();
		assert!(!quiet.settings.check_updates);
		assert_eq!(quiet.settings.update_policy, Policy::Notify);
		let text = serde_json::to_string(&off).unwrap();
		assert_eq!(parse(&text).unwrap().settings, off.settings);
	}

	#[test]
	fn a_pattern_that_does_not_compile_drops_only_its_own_category() {
		let text = r#"{ "version": 1, "categories": [
			{ "name": "Broken", "icon": "code", "pattern": "(" },
			{ "name": "Fine", "icon": "code", "pattern": "x" }
		] }"#;
		let categories = parse(text).unwrap().categories();
		assert_eq!(categories.len(), 1);
		assert_eq!(categories[0].name, "Fine");
	}
}
