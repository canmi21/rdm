//! An address with named parts, both ways: matched against a URL to pull the parts out, and filled
//! from them to make another. `{name}` is one path segment, `{name+}` one or more, `{name:regex}`
//! whatever the expression says; every part matches as little as it can, so a name followed by a
//! version is split where the version starts. `{url}` fills with the whole address matched. See
//! spec/rules.md.

use std::collections::HashMap;

use fancy_regex::Regex;

pub type Captures = HashMap<String, String>;

#[derive(Debug)]
pub struct Template {
	regex: Regex,
}

impl Template {
	pub fn parse(pattern: &str) -> Result<Template, String> {
		let mut expression = String::from("^");
		let mut rest = pattern;
		while let Some(open) = rest.find('{') {
			expression.push_str(&fancy_regex::escape(&rest[..open]));
			let close = close_of(rest, open).ok_or_else(|| format!("unclosed {{ in {pattern}"))?;
			let part = &rest[open + 1..close];
			let (name, inner) = match part.split_once(':') {
				Some((name, inner)) => (name, inner.to_owned()),
				None if part.ends_with('+') => (&part[..part.len() - 1], ".+?".to_owned()),
				None => (part, "[^/]+?".to_owned()),
			};
			if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
				return Err(format!("a part is named in letters, digits and _: {{{part}}}"));
			}
			if name == "url" {
				return Err("{url} is filled, never matched".to_owned());
			}
			expression.push_str(&format!("(?P<{name}>{inner})"));
			rest = &rest[close + 1..];
		}
		expression.push_str(&fancy_regex::escape(rest));
		expression.push('$');
		let regex = Regex::new(&expression).map_err(|e| format!("{pattern}: {e}"))?;
		Ok(Template { regex })
	}

	/// The parts of `url` if it matches, the query and fragment left off first: an address with a
	/// signature or a tracking tag after it is the same file.
	pub fn matches(&self, url: &str) -> Option<Captures> {
		let bare = url.split(['?', '#']).next().unwrap_or(url);
		let found = self.regex.captures(bare).ok()??;
		let mut captures: Captures = self
			.regex
			.capture_names()
			.flatten()
			.filter_map(|name| found.name(name).map(|m| (name.to_owned(), m.as_str().to_owned())))
			.collect();
		captures.insert("url".to_owned(), bare.to_owned());
		Some(captures)
	}
}

/// `text` with every `{name}` replaced by its part; a name with no part is an error, since a rule
/// that fills an address it cannot complete would ask for the wrong one.
pub fn fill(text: &str, captures: &Captures) -> Result<String, String> {
	let mut out = String::new();
	let mut rest = text;
	while let Some(open) = rest.find('{') {
		out.push_str(&rest[..open]);
		let close = close_of(rest, open).ok_or_else(|| format!("unclosed {{ in {text}"))?;
		let name = rest[open + 1..close].split(':').next().unwrap_or_default().trim_end_matches('+');
		out.push_str(
			captures.get(name).ok_or_else(|| format!("{{{name}}} is not a part of the match"))?,
		);
		rest = &rest[close + 1..];
	}
	out.push_str(rest);
	Ok(out)
}

/// The `}` that closes the `{` at `open`, counting the braces an expression inside may hold:
/// `{sha:[0-9a-f]{40}}` is one part.
fn close_of(text: &str, open: usize) -> Option<usize> {
	let mut depth = 0;
	for (index, c) in text[open..].char_indices() {
		match c {
			'{' => depth += 1,
			'}' => {
				depth -= 1;
				if depth == 0 {
					return Some(open + index);
				}
			}
			_ => {}
		}
	}
	None
}
