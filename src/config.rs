//! What the user shaped: the categories, and later the settings. `config.json` in the platform's
//! configuration directory, versioned like state.json, seeded once and then the user's to edit.
//! See spec/state.md.

use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::download::Category;
use crate::state::{parse_versioned, write_json};
use crate::ui::icon::Icon;

pub const VERSION: u64 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Config {
	pub version: u64,
	#[serde(default)]
	pub categories: Vec<CategoryConfig>,
}

/// A category as the file spells it: the icon by its Lucide name, the pattern as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CategoryConfig {
	pub name: String,
	pub icon: String,
	pub pattern: String,
}

impl Config {
	/// The starting file: the built-in categories, so a user who wants to change them finds them
	/// written down rather than baked in.
	pub fn seed() -> Config {
		Config {
			version: VERSION,
			categories: Category::defaults().iter().map(CategoryConfig::from).collect(),
		}
	}

	/// The categories in the file's order, ids assigned by position. A pattern that does not
	/// compile is reported and skipped rather than taking the rest down with it; an icon name
	/// that is not one of the choices draws as a plain file.
	pub fn categories(&self) -> Vec<Category> {
		self
			.categories
			.iter()
			.enumerate()
			.filter_map(|(i, c)| {
				let icon = Icon::by_name(&c.icon).unwrap_or(Icon::File);
				match Category::new(i as u64 + 1, &c.name, icon, &c.pattern) {
					Ok(category) => Some(category),
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

	pub fn from_categories(categories: &[Category]) -> Config {
		Config { version: VERSION, categories: categories.iter().map(CategoryConfig::from).collect() }
	}
}

impl From<&Category> for CategoryConfig {
	fn from(c: &Category) -> Self {
		CategoryConfig {
			name: c.name.clone(),
			icon: c.icon.name().to_owned(),
			pattern: c.pattern.clone(),
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

	fn scratch(name: &str) -> std::path::PathBuf {
		std::env::temp_dir().join(format!("rdm-config-{}-{name}", std::process::id()))
	}

	#[test]
	fn a_missing_file_is_seeded_with_the_defaults_and_written() {
		let dir = scratch("seed");
		let path = dir.join("config.json");
		let config = load_or_seed(&path);
		assert_eq!(config.categories.len(), Category::PRESETS.len() + 1, "every preset, then Other");
		assert_eq!(config.categories[0].name, "Video");
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
