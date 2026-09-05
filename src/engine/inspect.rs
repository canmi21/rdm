//! What an address is before it becomes a download: the probe's answer, and -- when the
//! answer is a web page rather than a file -- the files that page links to, so the window can
//! say "this is a page" and offer what is behind it instead of saving the HTML. See
//! spec/engine.md.

use reqwest::{Client, Url};

use crate::engine::error::Result;
use crate::engine::probe::{Probe, probe, safe_name};

/// A file a page links to, with the name it would be saved under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
	pub url: Url,
	pub name: String,
}

#[derive(Clone, Debug)]
pub struct Inspection {
	pub probe: Probe,
	/// The probe says the address is a page, not a file.
	pub is_page: bool,
	/// Files the page links to, when it is one; a page with none is still a page.
	pub links: Vec<Link>,
}

/// How much of a page is read for links. A page of downloads is a few hundred kilobytes at
/// the most; past this the rest is not read.
const PAGE_LIMIT: usize = 2 * 1024 * 1024;

/// Extensions that mark a link as another page rather than a file.
const PAGE_EXTENSIONS: [&str; 10] =
	["html", "htm", "xhtml", "php", "asp", "aspx", "jsp", "cgi", "shtml", "do"];

pub async fn inspect(client: &Client, url: Url) -> Result<Inspection> {
	let probed = probe(client, url.clone()).await?;
	let is_page = probed.content_type.as_deref().is_some_and(is_html);
	let links =
		if is_page { links_of(client, &probed.url).await.unwrap_or_default() } else { Vec::new() };
	Ok(Inspection { probe: probed, is_page, links })
}

pub fn is_html(content_type: &str) -> bool {
	let kind = content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
	matches!(kind.as_str(), "text/html" | "application/xhtml+xml")
}

/// Reads the page and returns the file-shaped links on it, resolved and deduplicated, in the
/// order they appear. The page is not parsed as a document: every `href` and `src` value is
/// taken as written, which is what a page of downloads offers and what a page of anything else
/// mostly lacks.
async fn links_of(client: &Client, page: &Url) -> Result<Vec<Link>> {
	use futures::StreamExt;
	let response = client.get(page.clone()).send().await?;
	let mut body = Vec::new();
	let mut stream = response.bytes_stream();
	while let Some(chunk) = stream.next().await {
		let chunk = chunk?;
		body.extend_from_slice(&chunk);
		if body.len() >= PAGE_LIMIT {
			break;
		}
	}
	let text = String::from_utf8_lossy(&body);
	Ok(extract_links(&text, page))
}

pub fn extract_links(html: &str, page: &Url) -> Vec<Link> {
	let mut links: Vec<Link> = Vec::new();
	for attribute in ["href", "src"] {
		let mut rest = html;
		while let Some(at) = rest.find(attribute) {
			rest = &rest[at + attribute.len()..];
			let trimmed = rest.trim_start();
			let Some(after_eq) = trimmed.strip_prefix('=') else { continue };
			let after_eq = after_eq.trim_start();
			let (value, tail) = match after_eq.chars().next() {
				Some(q @ ('"' | '\'')) => {
					let inner = &after_eq[1..];
					match inner.find(q) {
						Some(end) => (&inner[..end], &inner[end + 1..]),
						None => break,
					}
				}
				Some(_) => {
					let end = after_eq.find([' ', '>', '\n', '\t']).unwrap_or(after_eq.len());
					(&after_eq[..end], &after_eq[end..])
				}
				None => break,
			};
			rest = tail;
			let Some(link) = file_link(value, page) else { continue };
			if !links.iter().any(|l| l.url == link.url) {
				links.push(link);
			}
		}
	}
	links
}

/// An attribute value as a file link: absolute or relative to the page, http or https, with a
/// last path segment that has an extension and is not itself a page.
fn file_link(value: &str, page: &Url) -> Option<Link> {
	let value = value.trim();
	if value.is_empty() || value.starts_with('#') || value.starts_with("javascript:") {
		return None;
	}
	let mut url = page.join(value).ok()?;
	if !matches!(url.scheme(), "http" | "https") {
		return None;
	}
	url.set_fragment(None);
	let segment = url.path_segments()?.rfind(|s| !s.is_empty())?.to_owned();
	let (_, extension) = segment.rsplit_once('.')?;
	let ok = (1..=5).contains(&extension.len())
		&& extension.chars().all(|c| c.is_ascii_alphanumeric())
		&& !PAGE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str());
	if !ok {
		return None;
	}
	let name = safe_name(&percent_encoding::percent_decode_str(&segment).decode_utf8().ok()?)?;
	Some(Link { url, name })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::testing::{Options, TestServer};

	#[test]
	fn links_are_pulled_out_of_a_page_resolved_and_filtered() {
		let page = Url::parse("https://host.example/downloads/index.html").unwrap();
		let html = r##"<html><body>
			<a href="release-1.2.0.tar.gz">tarball</a>
			<a href='/files/app%20setup.dmg'>dmg</a>
			<a href=https://cdn.example/big.iso>iso</a>
			<a href="other.html">another page</a>
			<a href="#top">top</a>
			<a href="javascript:void(0)">nothing</a>
			<img src="../logo.png">
			<a href="release-1.2.0.tar.gz">again</a>
			<a href="mailto:x@y.z">mail</a>
		</body></html>"##;
		let links = extract_links(html, &page);
		let names: Vec<&str> = links.iter().map(|l| l.name.as_str()).collect();
		assert_eq!(names, ["release-1.2.0.tar.gz", "app setup.dmg", "big.iso", "logo.png"]);
		assert_eq!(links[0].url.as_str(), "https://host.example/downloads/release-1.2.0.tar.gz");
		assert_eq!(links[1].url.as_str(), "https://host.example/files/app%20setup.dmg");
		assert_eq!(links[3].url.as_str(), "https://host.example/logo.png");
	}

	#[tokio::test]
	async fn a_page_is_told_from_a_file_and_its_links_offered() {
		let html = b"<a href=\"a.zip\">a</a><a href=\"b/c.pdf\">c</a>".to_vec();
		let server = TestServer::start(
			html,
			Options { content_type: Some("text/html; charset=utf-8".into()), ..Options::default() },
		);
		let client = crate::engine::client::build(&crate::engine::Settings::default(), false).unwrap();
		let seen = inspect(&client, server.url("/dir/page")).await.unwrap();
		assert!(seen.is_page);
		let names: Vec<&str> = seen.links.iter().map(|l| l.name.as_str()).collect();
		assert_eq!(names, ["a.zip", "c.pdf"]);
		assert_eq!(seen.links[1].url.path(), "/dir/b/c.pdf", "resolved against the page");

		let file = TestServer::start(
			vec![1; 100],
			Options { content_type: Some("application/zip".into()), ..Options::default() },
		);
		let seen = inspect(&client, file.url("/x")).await.unwrap();
		assert!(!seen.is_page && seen.links.is_empty());
	}

	#[test]
	fn html_is_recognised_with_or_without_parameters() {
		assert!(is_html("text/html; charset=UTF-8"));
		assert!(is_html("TEXT/HTML"));
		assert!(!is_html("text/plain"));
		assert!(!is_html("application/octet-stream"));
	}
}
