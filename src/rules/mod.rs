//! What rdm knows about the places files come from, kept as data: which addresses are mirrors of
//! one another, and where a file's checksum can be read before it is downloaded. Three layers --
//! built in, synced, the user's own -- are read and merged into one compiled set at start. See
//! spec/rules.md.

pub mod checksum;
pub mod resolve;
pub mod sync;
pub mod template;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The compiled set's format; a reader checks it before anything else. See spec/json.md.
pub const VERSION: u32 = 1;

/// The rules compiled into the binary: the few close to fact that are the floor when nothing has
/// synced. Their text is the repository's own `rules/`, so the built-in layer and the synced one
/// never disagree about a file they share.
const BUILT_IN: &[(&str, &str)] = &[
	("cdn/jsdelivr.toml", include_str!("../../rules/cdn/jsdelivr.toml")),
	("code/github.toml", include_str!("../../rules/code/github.toml")),
	("packages/npm.toml", include_str!("../../rules/packages/npm.toml")),
	("packages/pypi.toml", include_str!("../../rules/packages/pypi.toml")),
];

/// Where a rule came from. Ordered as it ranks at equal priority: the user's over the synced over
/// the built in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layer {
	#[default]
	BuiltIn,
	Synced,
	Custom,
}

/// One rule file as written.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
	#[serde(default)]
	entry: Vec<Entry>,
	#[serde(default)]
	family: Vec<Family>,
	#[serde(default)]
	authority: Vec<Authority>,
	#[serde(default)]
	domain: Vec<Domain>,
	#[serde(default)]
	order: Vec<Order>,
}

/// A rule's place set by the user in the rules window: its priority, by id, whatever layer it is
/// in. Honoured from the custom layer only, since the order is the user's; the rule's own file is
/// left alone, a synced one being the sync's to replace. See spec/rules.md.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Order {
	id: String,
	priority: i32,
}

/// One kind of address, matched by a template with named parts, with where its file's checksum is
/// read and which other addresses serve it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
	pub id: String,
	#[serde(default)]
	pub priority: i32,
	#[serde(rename = "match")]
	pub pattern: String,
	/// Tried in order; the first that answers is the checksum.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub checksum: Vec<checksum::Source>,
	/// Address templates filled from the match: other places the same file is served.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub mirror: Vec<String>,
	/// Addresses the pattern has to match, checked by the tests and by nothing at run time.
	#[serde(default, skip_serializing)]
	pub examples: Vec<String>,
	#[serde(default)]
	pub layer: Layer,
	#[serde(default)]
	pub file: String,
}

/// Prefixes that serve the same tree: swap one for another and the same file is there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
	pub id: String,
	#[serde(default)]
	pub priority: i32,
	pub prefixes: Vec<String>,
	#[serde(default, skip_serializing)]
	pub examples: Vec<String>,
	#[serde(default)]
	pub layer: Layer,
	#[serde(default)]
	pub file: String,
}

/// A host trusted to answer for a source it mirrors. Honoured from the built-in and custom layers
/// only, so a sync cannot make a host an authority. See spec/rules.md.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
	pub host: String,
}

/// What the user chose for a source's domain when it had a mirror and no checksum.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
	/// Not asked again; a mirror when the checksum is found by itself, the source alone otherwise.
	Auto,
	/// Not asked again, and never a mirror.
	Never,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Domain {
	pub host: String,
	pub mirror: Choice,
}

/// Every layer merged: rules in the order they are tried, the authorities that may answer for a
/// source, and the choices made for domains. Written out as JSON beside the state as well, so what
/// the application is working from can be read. See spec/rules.md.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Compiled {
	pub version: u32,
	pub entries: Vec<Entry>,
	pub families: Vec<Family>,
	pub authorities: Vec<String>,
	pub domains: Vec<Domain>,
	/// What could not be read, a line each: a file that is not TOML, an authority from the synced
	/// layer. Shown by the rules window; nothing stops on them.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub problems: Vec<String>,
}

/// Where the synced and custom layers live, and where the compiled set is written.
#[derive(Clone, Debug)]
pub struct Places {
	pub synced: PathBuf,
	pub custom: PathBuf,
	pub compiled: PathBuf,
}

/// One layer's files, as (path within the layer, text).
pub type Texts = Vec<(String, String)>;

pub fn built_in() -> Texts {
	BUILT_IN.iter().map(|(path, text)| (path.to_string(), text.to_string())).collect()
}

/// Every `.toml` under `root`, at any depth -- folders are groups -- in path order, so the same
/// tree always merges the same way. A missing folder is an empty layer.
pub fn read_tree(root: &Path) -> Texts {
	fn walk(dir: &Path, root: &Path, out: &mut Texts) {
		let Ok(entries) = std::fs::read_dir(dir) else { return };
		let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
		paths.sort();
		for path in paths {
			if path.is_dir() {
				walk(&path, root, out);
			} else if path.extension().is_some_and(|e| e == "toml")
				&& let Ok(text) = std::fs::read_to_string(&path)
			{
				let name = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
				out.push((name, text));
			}
		}
	}
	let mut out = Vec::new();
	walk(root, root, &mut out);
	out
}

/// The three layers merged. Rules are ordered by priority, highest first, then by layer, the user's
/// first, then by file and place in it; where several match one address the first is used whole.
pub fn compile(layers: &[(Layer, Texts)]) -> Compiled {
	let mut compiled = Compiled { version: VERSION, ..Compiled::default() };
	let mut entries = Vec::new();
	let mut families = Vec::new();
	let mut domains: Vec<(Layer, Domain)> = Vec::new();
	let mut orders: Vec<Order> = Vec::new();
	let mut synced_authorities: Vec<(String, String)> = Vec::new();
	for (layer, texts) in layers {
		for (path, text) in texts {
			let file: File = match toml::from_str(text) {
				Ok(file) => file,
				Err(error) => {
					compiled.problems.push(format!("{layer:?} {path}: {}", error.message()));
					continue;
				}
			};
			for (index, mut entry) in file.entry.into_iter().enumerate() {
				if let Err(problem) = template::Template::parse(&entry.pattern) {
					compiled.problems.push(format!("{layer:?} {path}: entry {}: {problem}", entry.id));
					continue;
				}
				entry.layer = *layer;
				entry.file = path.clone();
				entries.push((index, entry));
			}
			for (index, mut family) in file.family.into_iter().enumerate() {
				family.layer = *layer;
				family.file = path.clone();
				families.push((index, family));
			}
			for authority in file.authority {
				if *layer == Layer::Synced {
					synced_authorities.push((path.clone(), authority.host));
				} else if !compiled.authorities.contains(&authority.host) {
					compiled.authorities.push(authority.host);
				}
			}
			domains.extend(file.domain.into_iter().map(|d| (*layer, d)));
			if *layer == Layer::Custom {
				orders.extend(file.order);
			}
		}
	}
	// A synced authority is a problem only when it would have made a host one: the synced copy of
	// the built-in jsDelivr file names the same hosts the built-in layer already does.
	for (path, host) in synced_authorities {
		if !compiled.authorities.contains(&host) {
			compiled.problems.push(format!(
				"Synced {path}: authority {host} ignored; only built-in and custom rules name one"
			));
		}
	}
	// One rule to an id: the copy in the highest layer, so a synced rule replaces the built-in one it
	// was built from, and a user's rule of the same id replaces either.
	entries = keep_highest(entries, |e| (e.id.clone(), e.layer));
	families = keep_highest(families, |f| (f.id.clone(), f.layer));
	for order in &orders {
		for (_, entry) in entries.iter_mut().filter(|(_, e)| e.id == order.id) {
			entry.priority = order.priority;
		}
		for (_, family) in families.iter_mut().filter(|(_, f)| f.id == order.id) {
			family.priority = order.priority;
		}
	}
	entries.sort_by(|(i, a), (j, b)| {
		b.priority.cmp(&a.priority).then(b.layer.cmp(&a.layer)).then(a.file.cmp(&b.file)).then(i.cmp(j))
	});
	families.sort_by(|(i, a), (j, b)| {
		b.priority.cmp(&a.priority).then(b.layer.cmp(&a.layer)).then(a.file.cmp(&b.file)).then(i.cmp(j))
	});
	compiled.entries = entries.into_iter().map(|(_, e)| e).collect();
	compiled.families = families.into_iter().map(|(_, f)| f).collect();
	// One choice per domain, the higher layer's.
	domains.sort_by(|(a, _), (b, _)| b.cmp(a));
	for (_, domain) in domains {
		if !compiled.domains.iter().any(|d| d.host.eq_ignore_ascii_case(&domain.host)) {
			compiled.domains.push(domain);
		}
	}
	compiled
}

/// Each id's rule from the highest layer that has one, in the order they came.
fn keep_highest<T>(rules: Vec<(usize, T)>, key: impl Fn(&T) -> (String, Layer)) -> Vec<(usize, T)> {
	let mut best: std::collections::HashMap<String, Layer> = std::collections::HashMap::new();
	for (_, rule) in &rules {
		let (id, layer) = key(rule);
		let top = best.entry(id).or_insert(layer);
		*top = (*top).max(layer);
	}
	rules
		.into_iter()
		.filter(|(_, rule)| {
			let (id, layer) = key(rule);
			best.get(&id) == Some(&layer)
		})
		.collect()
}

/// All three layers read from their places and merged, and the result written out as JSON.
pub fn load(places: &Places) -> Compiled {
	let compiled = compile(&[
		(Layer::BuiltIn, built_in()),
		(Layer::Synced, read_tree(&places.synced)),
		(Layer::Custom, read_tree(&places.custom)),
	]);
	if let Some(parent) = places.compiled.parent() {
		let _ = std::fs::create_dir_all(parent);
	}
	if let Ok(text) = serde_json::to_string_pretty(&compiled) {
		let _ = std::fs::write(&places.compiled, text);
	}
	compiled
}

impl Compiled {
	/// The entry that answers for this address: the first in order that matches it.
	pub fn entry_for(&self, url: &str) -> Option<(&Entry, template::Captures)> {
		self.entries.iter().find_map(|entry| {
			let template = template::Template::parse(&entry.pattern).ok()?;
			template.matches(url).map(|captures| (entry, captures))
		})
	}

	/// What was chosen for this host's domain, matching the host or any domain above it.
	pub fn choice_for(&self, host: &str) -> Option<Choice> {
		let host = host.to_ascii_lowercase();
		self
			.domains
			.iter()
			.find(|d| {
				let domain = d.host.to_ascii_lowercase();
				host == domain || host.ends_with(&format!(".{domain}"))
			})
			.map(|d| d.mirror)
	}

	pub fn is_authority(&self, host: &str) -> bool {
		self.authorities.iter().any(|a| a.eq_ignore_ascii_case(host))
	}
}

/// Remembers a choice for a domain in the custom layer, in a file of its own that nothing but
/// this writes. A choice already there for the domain is replaced.
pub fn remember(custom: &Path, host: &str, choice: Choice) -> std::io::Result<()> {
	let path = custom.join("choices.toml");
	let mut file: File =
		std::fs::read_to_string(&path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
	file.domain.retain(|d| !d.host.eq_ignore_ascii_case(host));
	file.domain.push(Domain { host: host.to_ascii_lowercase(), mirror: choice });
	std::fs::create_dir_all(custom)?;
	write_choices(&path, &file.domain)
}

fn write_choices(path: &Path, domains: &[Domain]) -> std::io::Result<()> {
	let mut text = String::from(
		"# What was chosen in New Task for a source that had a mirror and no checksum. Written by rdm;\n# edit or delete a line to change it. See spec/rules.md.\n",
	);
	for domain in domains {
		let mirror = match domain.mirror {
			Choice::Auto => "auto",
			Choice::Never => "never",
		};
		text.push_str(&format!("\n[[domain]]\nhost = \"{}\"\nmirror = \"{mirror}\"\n", domain.host));
	}
	std::fs::write(path, text)
}

/// Forgets a choice made for a domain.
pub fn forget(custom: &Path, host: &str) -> std::io::Result<()> {
	let path = custom.join("choices.toml");
	let Ok(text) = std::fs::read_to_string(&path) else { return Ok(()) };
	let mut file: File = toml::from_str(&text).unwrap_or_default();
	file.domain.retain(|d| !d.host.eq_ignore_ascii_case(host));
	write_choices(&path, &file.domain)
}

/// Sets rules' priorities in the custom layer's `order.toml`, which nothing but the rules window
/// writes, replacing what each had there.
pub fn set_priorities(custom: &Path, priorities: &[(String, i32)]) -> std::io::Result<()> {
	let path = custom.join("order.toml");
	let mut file: File =
		std::fs::read_to_string(&path).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
	file.order.retain(|o| !priorities.iter().any(|(id, _)| *id == o.id));
	file
		.order
		.extend(priorities.iter().map(|(id, priority)| Order { id: id.clone(), priority: *priority }));
	let mut text = String::from(
		"# Where rules were moved to in the rules window, by id and priority, whatever layer they are\n# in. Written by rdm; delete a block to put a rule back. See spec/rules.md.\n",
	);
	for order in &file.order {
		text.push_str(&format!("\n[[order]]\nid = \"{}\"\npriority = {}\n", order.id, order.priority));
	}
	std::fs::create_dir_all(custom)?;
	std::fs::write(path, text)
}

/// The domain a choice is remembered for: the host with a leading `www.` taken off.
pub fn domain_of(host: &str) -> String {
	host.trim_start_matches("www.").to_ascii_lowercase()
}

#[cfg(test)]
mod tests;
