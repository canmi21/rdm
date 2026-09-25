//! Where a rule says a file's checksum can be read, and reading it: a field of a JSON document, a
//! sum file beside the file, or a line of a list of sums. Every form a checksum arrives in --
//! `sha256:hex`, `sha512-base64` as npm writes it, bare hex, base64 with the algorithm said
//! beside it -- comes out as one `Checksum`. See spec/rules.md.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::Checksum;
use crate::rules::template::{self, Captures};

/// How long a checksum may take to arrive before the download goes on without one.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(10);
/// The most read of any document a checksum is looked for in.
const MOST: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algo {
	Sha256,
	Sha512,
	Md5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
	Hex,
	Base64,
}

/// One place to read a checksum from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Source {
	/// A field of a JSON document, reached by `pick`: `dist.integrity`, `assets[name={file}].digest`.
	Json {
		url: String,
		pick: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		algo: Option<Algo>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		encoding: Option<Encoding>,
	},
	/// A file holding the sum alone, or as the first word of a line naming the file.
	Sidecar {
		url: String,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		algo: Option<Algo>,
	},
	/// A list of sums, one file a line, found among the items of a JSON document by the item's
	/// name: a release's `checksums.txt` or `SHA256SUMS` among its assets.
	Sums {
		/// The document listing the candidates.
		url: String,
		/// Where the list of items is, as `pick` reaches a field.
		items: String,
		/// Each item's name and address fields.
		name: String,
		link: String,
		/// Names that are lists of sums, `*` for any run of characters.
		like: Vec<String>,
		#[serde(default, skip_serializing_if = "Option::is_none")]
		algo: Option<Algo>,
	},
}

impl Source {
	/// Every address this source asks, as templates, for checking a rule against its pattern.
	#[cfg(test)]
	pub fn templates(&self) -> Vec<&str> {
		match self {
			Source::Json { url, pick, .. } => vec![url, pick],
			Source::Sidecar { url, .. } => vec![url],
			Source::Sums { url, .. } => vec![url],
		}
	}

	/// The document asked first, filled: whose host the checksum is from.
	pub fn first_url(&self, captures: &Captures) -> Option<String> {
		let url = match self {
			Source::Json { url, .. } | Source::Sidecar { url, .. } | Source::Sums { url, .. } => url,
		};
		template::fill(url, captures).ok()
	}
}

/// The checksum this source gives for the file the captures describe, or None: not there, not
/// reachable, not a checksum. A source failing is the next one's turn, never an error.
pub async fn read(
	client: &reqwest::Client,
	source: &Source,
	captures: &Captures,
) -> Option<Checksum> {
	let file = captures
		.get("file")
		.cloned()
		.or_else(|| captures.get("url").and_then(|u| u.rsplit('/').next()).map(str::to_owned))?;
	match source {
		Source::Json { url, pick, algo, encoding } => {
			let document = fetch_json(client, &template::fill(url, captures).ok()?).await?;
			let value = select(&document, &template::fill(pick, captures).ok()?)?;
			normalize(value.as_str()?, *algo, *encoding)
		}
		Source::Sidecar { url, algo } => {
			let text = fetch(client, &template::fill(url, captures).ok()?).await?;
			from_sums(&text, &file, *algo).or_else(|| {
				let first = text.split_whitespace().next()?;
				normalize(first, *algo, Some(Encoding::Hex))
			})
		}
		Source::Sums { url, items, name, link, like, algo } => {
			let document = fetch_json(client, &template::fill(url, captures).ok()?).await?;
			let list = select(&document, &template::fill(items, captures).ok()?)?.as_array()?;
			for item in list {
				let (Some(called), Some(address)) =
					(item.get(name).and_then(Value::as_str), item.get(link).and_then(Value::as_str))
				else {
					continue;
				};
				if called == file || !like.iter().any(|pattern| glob(pattern, called)) {
					continue;
				}
				if let Some(text) = fetch(client, address).await
					&& let Some(checksum) = from_sums(&text, &file, *algo)
				{
					return Some(checksum);
				}
			}
			None
		}
	}
}

async fn fetch(client: &reqwest::Client, url: &str) -> Option<String> {
	let response = tokio::time::timeout(PATIENCE, client.get(url).send()).await.ok()?.ok()?;
	if !response.status().is_success() || response.content_length().is_some_and(|n| n as usize > MOST)
	{
		return None;
	}
	let bytes = tokio::time::timeout(PATIENCE, response.bytes()).await.ok()?.ok()?;
	(bytes.len() <= MOST).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

async fn fetch_json(client: &reqwest::Client, url: &str) -> Option<Value> {
	serde_json::from_str(&fetch(client, url).await?).ok()
}

/// A field reached by a path: keys joined by dots, and a list narrowed to the item whose field has
/// a value by `list[field=value]`. The value is everything after `=` up to the closing bracket.
pub fn select<'a>(document: &'a Value, path: &str) -> Option<&'a Value> {
	let mut at = document;
	let mut rest = path;
	while !rest.is_empty() {
		// A segment ends at the next dot outside brackets.
		let mut depth = 0;
		let end = rest
			.char_indices()
			.find(|(_, c)| {
				match c {
					'[' => depth += 1,
					']' => depth -= 1,
					'.' if depth == 0 => return true,
					_ => {}
				}
				false
			})
			.map_or(rest.len(), |(i, _)| i);
		let segment = &rest[..end];
		rest = rest.get(end + 1..).unwrap_or("");
		let (key, filter) = match segment.split_once('[') {
			Some((key, filter)) => (key, Some(filter.strip_suffix(']')?)),
			None => (segment, None),
		};
		if !key.is_empty() {
			at = at.get(key)?;
		}
		if let Some(filter) = filter {
			let (field, value) = filter.split_once('=')?;
			at = at
				.as_array()?
				.iter()
				.find(|item| item.get(field).and_then(Value::as_str) == Some(value))?;
		}
	}
	Some(at)
}

/// A checksum in any of the forms rules meet, as one `Checksum`.
pub fn normalize(text: &str, algo: Option<Algo>, encoding: Option<Encoding>) -> Option<Checksum> {
	let text = text.trim();
	// npm's integrity, which is the web's subresource integrity: `sha512-<base64>`.
	for (prefix, algo) in [("sha512-", Algo::Sha512), ("sha256-", Algo::Sha256)] {
		if let Some(encoded) = text.strip_prefix(prefix) {
			return from_bytes(algo, &base64(encoded)?);
		}
	}
	if encoding == Some(Encoding::Base64) {
		return from_bytes(algo?, &base64(text)?);
	}
	// `sha256:hex` and bare hex are what `Checksum::parse` already reads.
	match algo {
		Some(algo) if !text.contains(':') => Checksum::parse(&format!("{}:{text}", name(algo))),
		_ => Checksum::parse(text),
	}
}

fn name(algo: Algo) -> &'static str {
	match algo {
		Algo::Sha256 => "sha256",
		Algo::Sha512 => "sha512",
		Algo::Md5 => "md5",
	}
}

fn from_bytes(algo: Algo, bytes: &[u8]) -> Option<Checksum> {
	let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
	Checksum::parse(&format!("{}:{hex}", name(algo)))
}

/// The sum for `file` in a list of sums: `hex  name` or `hex *name` as the coreutils write them, or
/// `SHA256 (name) = hex` as BSD does.
pub fn from_sums(text: &str, file: &str, algo: Option<Algo>) -> Option<Checksum> {
	for line in text.lines() {
		let line = line.trim();
		if let Some((head, hex)) = line.split_once(") = ") {
			let Some((kind, named)) = head.split_once(" (") else { continue };
			if named == file {
				let algo = algo.or(match kind.to_ascii_uppercase().as_str() {
					"SHA256" => Some(Algo::Sha256),
					"SHA512" => Some(Algo::Sha512),
					"MD5" => Some(Algo::Md5),
					_ => None,
				});
				return normalize(hex, algo, Some(Encoding::Hex));
			}
			continue;
		}
		let mut words = line.split_whitespace();
		let (Some(hex), Some(named)) = (words.next(), words.next()) else { continue };
		let named = named.trim_start_matches('*');
		if named == file || named.rsplit('/').next() == Some(file) {
			return normalize(hex, algo, Some(Encoding::Hex));
		}
	}
	None
}

/// Whether `text` fits `pattern`, where `*` is any run of characters and case is ignored.
pub fn glob(pattern: &str, text: &str) -> bool {
	let (pattern, text) = (pattern.to_ascii_lowercase(), text.to_ascii_lowercase());
	let parts: Vec<&str> = pattern.split('*').collect();
	let mut at = 0;
	for (index, part) in parts.iter().enumerate() {
		if index == 0 {
			if !text.starts_with(part) {
				return false;
			}
			at = part.len();
		} else if index == parts.len() - 1 {
			return text.len() >= at + part.len() && text.ends_with(part);
		} else {
			match text[at..].find(part) {
				Some(found) => at += found + part.len(),
				None => return false,
			}
		}
	}
	parts.len() > 1 || text == pattern
}

/// Standard base64, padded or not; None for anything else.
fn base64(text: &str) -> Option<Vec<u8>> {
	let value = |c: u8| -> Option<u32> {
		Some(match c {
			b'A'..=b'Z' => c - b'A',
			b'a'..=b'z' => c - b'a' + 26,
			b'0'..=b'9' => c - b'0' + 52,
			b'+' | b'-' => 62,
			b'/' | b'_' => 63,
			_ => return None,
		} as u32)
	};
	let digits: Vec<u32> = text.trim_end_matches('=').bytes().map(value).collect::<Option<_>>()?;
	let mut out = Vec::with_capacity(digits.len() * 3 / 4);
	for chunk in digits.chunks(4) {
		let mut acc = 0u32;
		for (i, d) in chunk.iter().enumerate() {
			acc |= d << (18 - 6 * i);
		}
		let bytes = [(acc >> 16) as u8, (acc >> 8) as u8, acc as u8];
		out.extend_from_slice(&bytes[..chunk.len().saturating_sub(1)]);
	}
	Some(out)
}
