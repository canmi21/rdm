//! The first request: what the server has, and what it allows. Asked with a GET for the first
//! byte alone rather than a HEAD, because a server that ignores Range on HEAD and honours it on
//! GET is common and the reverse is not, and the answer to this one request decides everything
//! after it -- the size, whether the file can be split and resumed, its name, and the marks
//! that say whether it is still the same file later. See spec/engine.md.

use percent_encoding::percent_decode_str;
use reqwest::header::{
	ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG,
	LAST_MODIFIED, RANGE,
};
use reqwest::{Client, StatusCode, Url};

use crate::engine::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probe {
	/// Where the bytes actually are, after redirects; every later request goes here directly.
	pub url: Url,
	/// None when the server did not say, which is what a chunked response looks like.
	pub size: Option<u64>,
	/// The server answered a one-byte range with 206: it can be resumed and split.
	pub ranges: bool,
	pub etag: Option<String>,
	pub last_modified: Option<String>,
	/// From Content-Disposition when the server named the file, else the address's last path
	/// segment, else `download`. Never contains a path separator.
	pub file_name: String,
	pub content_type: Option<String>,
}

impl Probe {
	/// What `If-Range` is sent with, so a resumed request is answered with 200 and the whole
	/// file if the file changed rather than with a slice of a different file. The ETag when
	/// there is one; Last-Modified otherwise; nothing when the server marks nothing, in which
	/// case a change goes unnoticed.
	pub fn validator(&self) -> Option<&str> {
		self.etag.as_deref().or(self.last_modified.as_deref())
	}
}

pub async fn probe(client: &Client, url: Url) -> Result<Probe> {
	match url.scheme() {
		"http" | "https" => {}
		other => return Err(Error::Scheme(other.to_owned())),
	}
	let response = client.get(url.clone()).header(RANGE, "bytes=0-0").send().await?;
	let status = response.status();
	if !status.is_success() {
		return Err(Error::Refused { status: status.as_u16() });
	}
	let headers = response.headers();
	let text =
		|name| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim).map(str::to_owned);
	let (ranges, size) = if status == StatusCode::PARTIAL_CONTENT {
		// `bytes 0-0/1234`, or `bytes 0-0/*` from a server that will not say.
		let total = text(CONTENT_RANGE)
			.and_then(|v| v.rsplit('/').next().map(str::to_owned))
			.and_then(|t| t.parse::<u64>().ok());
		(true, total)
	} else {
		let accepts = text(ACCEPT_RANGES).is_some_and(|v| v.eq_ignore_ascii_case("bytes"));
		// A 200 to a Range request is the whole file; the length is the size. `Accept-Ranges:
		// bytes` alone is a promise some servers make and do not keep, so it is not believed
		// without a 206 -- but it is remembered, so a resume can try.
		(accepts, text(CONTENT_LENGTH).and_then(|v| v.parse().ok()))
	};
	let final_url = response.url().clone();
	let file_name = text(CONTENT_DISPOSITION)
		.and_then(|d| disposition_name(&d))
		.or_else(|| url_name(&final_url))
		.unwrap_or_else(|| "download".to_owned());
	Ok(Probe {
		url: final_url,
		size,
		ranges,
		etag: text(ETAG),
		last_modified: text(LAST_MODIFIED),
		file_name,
		content_type: text(CONTENT_TYPE),
	})
}

/// `attachment; filename="a.zip"` or `filename*=UTF-8''a%20b.zip`; the starred form wins, as
/// the standard says, since it is the one that can spell a name outside ASCII.
pub fn disposition_name(header: &str) -> Option<String> {
	let mut plain = None;
	for part in header.split(';').map(str::trim) {
		let Some((key, value)) = part.split_once('=') else { continue };
		match key.trim().to_ascii_lowercase().as_str() {
			"filename*" => {
				// `charset'language'value`
				let mut pieces = value.splitn(3, '\'');
				let (_charset, _language, encoded) = (pieces.next()?, pieces.next()?, pieces.next()?);
				let decoded = percent_decode_str(encoded).decode_utf8().ok()?.into_owned();
				if let Some(name) = safe_name(&decoded) {
					return Some(name);
				}
			}
			"filename" => plain = safe_name(value.trim().trim_matches('"')),
			_ => {}
		}
	}
	plain
}

fn url_name(url: &Url) -> Option<String> {
	let segment = url.path_segments()?.rfind(|s| !s.is_empty())?;
	let decoded = percent_decode_str(segment).decode_utf8().ok()?;
	safe_name(&decoded)
}

/// The name with anything that could make it a path removed: separators, control characters,
/// a leading dot that would hide it. None when nothing is left.
pub fn safe_name(raw: &str) -> Option<String> {
	let cleaned: String =
		raw.chars().filter(|c| !matches!(c, '/' | '\\' | ':' | '\0') && !c.is_control()).collect();
	let trimmed = cleaned.trim().trim_start_matches('.').trim();
	(!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::testing::{Options, TestServer};

	fn client() -> Client {
		crate::engine::client::build(&crate::engine::Settings::default(), false).unwrap()
	}

	#[tokio::test]
	async fn a_server_with_ranges_says_its_size_and_marks() {
		let server = TestServer::start(
			vec![7u8; 5000],
			Options { etag: Some("\"v1\"".into()), ..Options::default() },
		);
		let probe = probe(&client(), server.url("/files/data.bin")).await.unwrap();
		assert_eq!((probe.size, probe.ranges), (Some(5000), true));
		assert_eq!(probe.etag.as_deref(), Some("\"v1\""));
		assert_eq!(probe.validator(), Some("\"v1\""));
		assert_eq!(probe.file_name, "data.bin");
		assert_eq!(server.requests().len(), 1, "one request, and its body was one byte");
		assert_eq!(server.requests()[0].range, Some((0, Some(0))));
	}

	#[tokio::test]
	async fn a_server_without_ranges_is_believed_and_the_length_still_read() {
		let server = TestServer::start(vec![1u8; 300], Options { ranges: false, ..Options::default() });
		let probe = probe(&client(), server.url("/a%20b.iso")).await.unwrap();
		assert_eq!((probe.size, probe.ranges), (Some(300), false));
		assert_eq!(probe.file_name, "a b.iso", "the path segment is decoded");
	}

	#[tokio::test]
	async fn the_disposition_names_the_file_and_redirects_are_followed() {
		let server = TestServer::start(
			vec![0u8; 10],
			Options {
				disposition: Some(
					"attachment; filename=\"plain.txt\"; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf".into(),
				),
				redirect_from: Some("/go".into()),
				..Options::default()
			},
		);
		let probe = probe(&client(), server.url("/go")).await.unwrap();
		assert_eq!(probe.file_name, "résumé.pdf", "the starred form wins");
		assert_eq!(probe.url.path(), "/target", "the final address is what later requests use");
	}

	#[tokio::test]
	async fn refusals_and_wrong_schemes_are_errors_the_window_can_name() {
		let server = TestServer::start(vec![], Options { status: Some(403), ..Options::default() });
		assert!(matches!(
			probe(&client(), server.url("/x")).await,
			Err(Error::Refused { status: 403 })
		));
		let ftp = Url::parse("ftp://example.org/x").unwrap();
		assert!(matches!(probe(&client(), ftp).await, Err(Error::Scheme(s)) if s == "ftp"));
	}

	#[test]
	fn names_are_taken_apart_safely() {
		assert_eq!(disposition_name("inline; filename=\"../../etc/passwd\""), Some("etcpasswd".into()));
		assert_eq!(disposition_name("attachment; filename=report.pdf"), Some("report.pdf".into()));
		assert_eq!(disposition_name("attachment"), None);
		assert_eq!(safe_name(".hidden"), Some("hidden".into()));
		assert_eq!(safe_name("  "), None);
		assert_eq!(url_name(&Url::parse("https://h/a/b/").unwrap()), Some("b".into()));
		assert_eq!(url_name(&Url::parse("https://h/").unwrap()), None);
	}
}
