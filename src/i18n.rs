//! What the window says, in the language it is read in. Three: American English, simplified
//! Chinese and Japanese.
//!
//! Every string the user reads is a key, and the keys are flat -- `settings.section.network`
//! rather than a tree -- because a tree of translations is a tree to walk in three files that
//! must agree, and flat keys are a set that can simply be compared. The files are one a language
//! under `locales/`, embedded at build time: a translation that can go missing at run time is a
//! window that can come up blank.
//!
//! **Not everything is translated, on purpose.** A name is a name: `rdm`, `Downloads`, `Finder`,
//! `Chrome`, `Hickory`, `HTTPS`, `SOCKS5`, `.DS_Store`. A Chinese or Japanese sentence with those
//! left in English is what somebody who uses this software writes; one with them translated is
//! what a machine writes. The English file is the source of truth for the set of keys and the
//! fallback for anything a translation has not caught up with, so a missing string shows in
//! English rather than as a key or a blank.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

/// The languages there are. The value is what `config.json` carries and what the files are named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
	/// Follow the system, which is what a first launch does before anybody has chosen.
	#[default]
	System,
	En,
	Zh,
	Ja,
}

impl Language {
	pub const ALL: [Language; 4] = [Language::System, Language::En, Language::Zh, Language::Ja];

	/// What Settings shows. A language is named in itself: somebody looking for their own
	/// language is looking for the word they call it by, not the word English calls it by.
	pub fn name(self) -> &'static str {
		match self {
			Language::System => "System",
			Language::En => "English",
			Language::Zh => "简体中文",
			Language::Ja => "日本語",
		}
	}

	/// The code the file is named for; System resolves to whatever the machine is set to.
	fn code(self) -> &'static str {
		match self {
			Language::System => Language::of_system().code(),
			Language::En => "en",
			Language::Zh => "zh",
			Language::Ja => "ja",
		}
	}

	/// What the machine is set to, of the three there are, English for anything else. Read once:
	/// a system language that changes under a running application is not a thing worth watching
	/// for, and the next launch will see it.
	pub fn of_system() -> Language {
		static SYSTEM: OnceLock<Language> = OnceLock::new();
		*SYSTEM.get_or_init(|| {
			let tag = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
			// A tag is a language and then some: `zh-Hans-CN`, `ja-JP`, `en-GB`. Only the first
			// part decides, and only three answers exist.
			match tag.split(['-', '_']).next().unwrap_or("") {
				"zh" => Language::Zh,
				"ja" => Language::Ja,
				_ => Language::En,
			}
		})
	}
}

/// The language in use, as an index into `Language::ALL`. An atomic rather than a lock because it
/// is read once a string a frame and written when somebody picks from a menu; switching takes
/// effect at the next frame, which is what "immediately" looks like.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

/// Sets the language every later `t` reads in.
pub fn use_language(language: Language) {
	let at = Language::ALL.iter().position(|l| *l == language).unwrap_or(0);
	ACTIVE.store(at as u8, Ordering::Relaxed);
}

fn active() -> Language {
	Language::ALL[ACTIVE.load(Ordering::Relaxed) as usize % Language::ALL.len()]
}

/// The three files, embedded. A translation that can go missing at run time is a window that can
/// come up blank.
const EN: &str = include_str!("../locales/en.json");
const ZH: &str = include_str!("../locales/zh.json");
const JA: &str = include_str!("../locales/ja.json");

fn table(code: &str) -> &'static HashMap<String, String> {
	static TABLES: OnceLock<HashMap<&'static str, HashMap<String, String>>> = OnceLock::new();
	let tables = TABLES.get_or_init(|| {
		[("en", EN), ("zh", ZH), ("ja", JA)]
			.into_iter()
			.map(|(code, text)| {
				let table: HashMap<String, String> = serde_json::from_str(text).unwrap_or_else(|error| {
					// The files are ours and are checked by a test; a broken one is a bug rather
					// than a thing to handle, and an empty table falls back to English anyway.
					eprintln!("locales/{code}.json did not parse: {error}");
					HashMap::new()
				});
				(code, table)
			})
			.collect()
	});
	tables.get(code).or_else(|| tables.get("en")).expect("English is embedded")
}

/// What to show for a key, in the language in use. English is the fallback for anything a
/// translation has not caught up with; the key itself is the last resort, and seeing one on
/// screen means somebody wrote a key that no file has.
pub fn t(key: &str) -> &'static str {
	let language = active();
	if let Some(text) = table(language.code()).get(key) {
		return text;
	}
	if let Some(text) = table("en").get(key) {
		return text;
	}
	// A key no file has, and a string that is not a key at all -- English written where a key
	// belongs -- both land here, and both have to come back as `'static` so they can be handed to
	// gpui without a copy at every call site. Leaked once each and remembered: this is called
	// once a string a frame, and leaking per call would be a leak in the render loop.
	static LEAKED: OnceLock<std::sync::Mutex<HashMap<String, &'static str>>> = OnceLock::new();
	let leaked = LEAKED.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
	let mut leaked = leaked.lock().unwrap_or_else(|held| held.into_inner());
	if let Some(text) = leaked.get(key) {
		return text;
	}
	let text: &'static str = Box::leak(key.to_owned().into_boxed_str());
	leaked.insert(key.to_owned(), text);
	text
}

#[cfg(test)]
mod tests {
	use super::*;

	fn keys(text: &str) -> std::collections::BTreeSet<String> {
		serde_json::from_str::<HashMap<String, String>>(text)
			.expect("a locale file is a flat object of strings")
			.into_keys()
			.collect()
	}

	/// Every file has the same keys. A translation missing one falls back to English, which is
	/// the right behaviour at run time and the wrong thing to discover then.
	#[test]
	fn the_three_files_carry_the_same_keys() {
		let (en, zh, ja) = (keys(EN), keys(ZH), keys(JA));
		assert!(!en.is_empty(), "there is something to translate");
		let missing_zh: Vec<&String> = en.difference(&zh).collect();
		let missing_ja: Vec<&String> = en.difference(&ja).collect();
		assert!(missing_zh.is_empty(), "zh.json is missing: {missing_zh:?}");
		assert!(missing_ja.is_empty(), "ja.json is missing: {missing_ja:?}");
		let extra_zh: Vec<&String> = zh.difference(&en).collect();
		let extra_ja: Vec<&String> = ja.difference(&en).collect();
		assert!(extra_zh.is_empty(), "zh.json has keys English does not: {extra_zh:?}");
		assert!(extra_ja.is_empty(), "ja.json has keys English does not: {extra_ja:?}");
	}

	/// Keys are flat. A key with a nested object under it is a tree, and three trees that must
	/// agree is the thing this arrangement exists to avoid.
	#[test]
	fn every_key_is_flat_and_every_value_is_a_string() {
		for (code, text) in [("en", EN), ("zh", ZH), ("ja", JA)] {
			let parsed: serde_json::Value = serde_json::from_str(text).expect(code);
			let object = parsed.as_object().unwrap_or_else(|| panic!("{code} is an object"));
			for (key, value) in object {
				assert!(value.is_string(), "{code}: {key} is not a string");
				assert!(!key.is_empty(), "{code}: an empty key");
			}
		}
	}

	#[test]
	fn a_missing_key_falls_back_to_english_and_the_language_switches() {
		use_language(Language::En);
		let english = t("settings.section.general");
		use_language(Language::Zh);
		assert_ne!(t("settings.section.general"), english, "Chinese says it differently");
		use_language(Language::En);
		assert_eq!(t("settings.section.general"), english, "and switching back switches back");
	}

	/// A tag is a language and then some, and only the first part decides.
	#[test]
	fn a_language_is_named_in_itself() {
		assert_eq!(Language::Zh.name(), "简体中文");
		assert_eq!(Language::Ja.name(), "日本語");
		assert_eq!(Language::default(), Language::System);
	}
}
