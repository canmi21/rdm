//! What the rules say about one file New Task has looked at: its checksum where a rule says how to
//! find it, and the other addresses serving it -- each probed first, and kept only when it serves a
//! file of the same size with ranges. Whether a kept mirror is then used is the caller's to decide
//! against the checksum and the user's choices. See spec/rules.md.

use std::sync::Arc;

use reqwest::Url;

use crate::engine::{Checksum, Probe};
use crate::rules::template::{self, Captures};
use crate::rules::{Choice, Compiled, Layer};

/// How many candidates are probed at once.
const AT_ONCE: usize = 6;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolution {
	/// The checksum and the host it was read from.
	pub checksum: Option<(Checksum, String)>,
	/// Other addresses serving the same file, each probed and found the same size with ranges.
	pub mirrors: Vec<Url>,
	/// What the user chose for this source's domain, if anything.
	pub choice: Option<Choice>,
	/// The rule that matched, for saying so.
	pub rule: Option<String>,
}

impl Resolution {
	/// The mirrors that may be used with this checksum: none from the host the checksum came from,
	/// since a mirror cannot vouch for itself, unless that host is an authority. With a checksum
	/// typed by the user, every mirror: the user is the source of that one.
	pub fn usable(&self, rules: &Compiled, typed: bool) -> Vec<Url> {
		if typed {
			return self.mirrors.clone();
		}
		let Some((_, from)) = &self.checksum else { return Vec::new() };
		self
			.mirrors
			.iter()
			.filter(|m| {
				rules.is_authority(from) || m.host_str().is_none_or(|host| !same_site(host, from))
			})
			.cloned()
			.collect()
	}
}

/// Whether two hosts belong to one site, by their last two labels: close enough to tell a mirror's
/// host from a source's, which is all it is asked.
pub fn same_site(a: &str, b: &str) -> bool {
	fn site(host: &str) -> String {
		let labels: Vec<&str> = host.trim_end_matches('.').rsplit('.').take(2).collect();
		labels.into_iter().rev().collect::<Vec<_>>().join(".").to_ascii_lowercase()
	}
	site(a) == site(b)
}

/// The rules' answer for `url`, which New Task was given, and `probe`, what the server said of it.
pub async fn resolve(
	rules: Arc<Compiled>,
	client: reqwest::Client,
	url: Url,
	probe: Probe,
) -> Resolution {
	let origin_host = url.host_str().unwrap_or_default().to_owned();
	let mut resolution =
		Resolution { choice: rules.choice_for(&origin_host), ..Resolution::default() };
	let matched = rules.entry_for(url.as_str()).or_else(|| rules.entry_for(probe.url.as_str()));
	let mut candidates: Vec<Url> = Vec::new();
	if let Some((entry, captures)) = &matched {
		resolution.rule = Some(entry.id.clone());
		resolution.checksum = checksum(&rules, &client, entry, captures, &origin_host).await;
		for mirror in &entry.mirror {
			if let Some(address) = template::fill(mirror, captures).ok().and_then(|a| Url::parse(&a).ok())
			{
				candidates.push(address);
			}
		}
	}
	for address in [url.as_str(), probe.url.as_str()] {
		candidates.extend(family_swaps(&rules, address));
	}
	if resolution.choice == Some(Choice::Never) {
		return resolution;
	}
	let mut seen = vec![url.clone(), probe.url.clone()];
	candidates.retain(|c| {
		let fresh = !seen.contains(c);
		seen.push(c.clone());
		fresh
	});
	resolution.mirrors = verified(&client, candidates, &probe).await;
	resolution
}

/// The first checksum any of the entry's sources gives. A synced rule may only read one from the
/// source's own site or an authority: the list of mirrors trusted to answer for a source is not a
/// sync's to extend. See spec/rules.md.
async fn checksum(
	rules: &Compiled,
	client: &reqwest::Client,
	entry: &crate::rules::Entry,
	captures: &Captures,
	origin: &str,
) -> Option<(Checksum, String)> {
	for source in &entry.checksum {
		let Some(first) = source.first_url(captures).and_then(|u| Url::parse(&u).ok()) else {
			continue;
		};
		let host = first.host_str().unwrap_or_default().to_owned();
		if entry.layer == Layer::Synced && !same_site(&host, origin) && !rules.is_authority(&host) {
			continue;
		}
		if let Some(found) = crate::rules::checksum::read(client, source, captures).await {
			return Some((found, host));
		}
	}
	None
}

/// The address under each other prefix of every family one of whose prefixes it starts with.
pub fn family_swaps(rules: &Compiled, address: &str) -> Vec<Url> {
	let mut out = Vec::new();
	for family in &rules.families {
		let Some(prefix) = family.prefixes.iter().find(|p| address.starts_with(p.as_str())) else {
			continue;
		};
		let rest = &address[prefix.len()..];
		for other in family.prefixes.iter().filter(|p| *p != prefix) {
			if let Ok(url) = Url::parse(&format!("{other}{rest}")) {
				out.push(url);
			}
		}
	}
	out
}

/// The candidates that serve the same file: probed, a few at a time, and kept when the size is the
/// one the source gave and ranges are served, which a download split across sources needs.
async fn verified(client: &reqwest::Client, candidates: Vec<Url>, probe: &Probe) -> Vec<Url> {
	let Some(size) = probe.size else { return Vec::new() };
	let mut kept = Vec::new();
	for chunk in candidates.chunks(AT_ONCE) {
		let answers = futures::future::join_all(chunk.iter().map(|candidate| async move {
			let answer = tokio::time::timeout(
				std::time::Duration::from_secs(10),
				crate::engine::probe(client, candidate.clone()),
			)
			.await;
			(candidate.clone(), answer)
		}))
		.await;
		for (candidate, answer) in answers {
			if let Ok(Ok(found)) = answer
				&& found.size == Some(size)
				&& found.ranges
			{
				kept.push(candidate);
			}
		}
	}
	kept
}
