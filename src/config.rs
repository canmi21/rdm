//! What the user shaped: the categories, and later the settings. `config.json` in the platform's
//! configuration directory, versioned like state.json, seeded once and then the user's to edit.
//! See spec/state.md.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::category::{Category, Overrides, extensions_of_pattern};
use crate::state::{parse_versioned, write_json};
use crate::ui::icon::Icon;
use crate::ui::theme::{format_hex, parse_color};
use crate::update::{Channel, Policy};

pub const VERSION: u64 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
	pub version: u64,
	#[serde(default)]
	pub categories: Vec<CategoryConfig>,
	/// What the settings sheet offers; absent in a file from before it did, and then the
	/// defaults.
	#[serde(default)]
	pub settings: Preferences,
}

/// The switches a user sets, as the file spells them. Every field has a default, so a file that
/// predates a switch reads as if the switch had been left alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
	/// The sidebar's category icons keep their hues always rather than only when chosen or
	/// hovered; the state filters above them are not covered. On to start with; the window's
	/// inactive grey overrides it either way.
	#[serde(default = "yes")]
	pub colorful_categories: bool,
	/// Which channel's builds the update check follows. Nightly, the only one there is.
	#[serde(default)]
	pub update_channel: Channel,
	/// The check runs on its own every few minutes; off, only Check now asks.
	#[serde(default = "yes")]
	pub check_updates: bool,
	/// A build the check finds is acted on without asking, as `update_policy` says.
	#[serde(default = "yes")]
	pub auto_update: bool,
	#[serde(default)]
	pub update_policy: Policy,
}

fn yes() -> bool {
	true
}

impl Default for Preferences {
	fn default() -> Self {
		Preferences {
			colorful_categories: true,
			update_channel: Channel::default(),
			check_updates: true,
			auto_update: true,
			update_policy: Policy::default(),
		}
	}
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
		}
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
					return Some(category);
				}
				let icon = Icon::by_name(&c.icon).unwrap_or(Icon::File);
				match Category::new(id, &c.name, icon, &c.pattern) {
					Ok(mut category) => {
						if let Some(color) = color {
							category.color = color;
						}
						category.custom_color = custom;
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
		}
	}
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

fn migrate(from: u64, _value: Value) -> Result<Value> {
	bail!("no migration from config.json version {from}")
}

/// The file if it is there and readable; the seed, written, if it is not there at all. A file
/// that is there but unreadable is left exactly as it is and the seed is used for the run, so a
/// hand edit that went wrong is not overwritten by the application correcting it.
pub fn load_or_seed(path: &Path) -> Config {
	match std::fs::read_to_string(path) {
		Ok(text) => parse(&text).unwrap_or_else(|error| {
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
	use crate::testing::scratch;

	#[test]
	fn a_missing_file_is_seeded_with_the_defaults_and_written() {
		let dir = scratch("seed");
		let path = dir.join("config.json");
		let config = load_or_seed(&path);
		assert_eq!(config.categories.len(), Category::PRESETS.len() + 1, "every preset, then Other");
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
