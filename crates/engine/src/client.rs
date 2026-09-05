//! The HTTP client, built from the settings. One is built per connection of a split download,
//! forced to HTTP/1.1, because the point of several connections is several TCP connections and
//! HTTP/2 would fold them into one. See spec/engine.md.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::error::{Error, Result};
use crate::settings::{HttpVersion, Settings};

/// A client for one connection. `split` says the download has several, which forces HTTP/1.1
/// whatever the setting says.
pub fn build(settings: &Settings, split: bool) -> Result<reqwest::Client> {
	let mut headers = HeaderMap::new();
	for (name, value) in &settings.headers {
		if let (Ok(name), Ok(value)) =
			(HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value))
		{
			headers.insert(name, value);
		}
	}
	let mut builder = reqwest::Client::builder()
		.user_agent(&settings.user_agent)
		.default_headers(headers)
		.connect_timeout(settings.connect_timeout)
		// The idle timeout is enforced per chunk by the worker, which knows when bytes stop; a
		// whole-request timeout would cut a long download that is doing fine.
		.redirect(reqwest::redirect::Policy::limited(settings.max_redirects))
		// Downloads want the bytes as they are on the server: a transfer encoding decoded on
		// the way would make Content-Length and byte ranges lie.
		.no_gzip()
		.no_brotli()
		.no_deflate()
		.no_zstd();
	builder = match (split, settings.http) {
		(true, _) | (false, HttpVersion::Http1) => builder.http1_only(),
		(false, HttpVersion::Http2) => builder.http2_prior_knowledge(),
		(false, HttpVersion::Auto) => builder,
	};
	if let Some(proxy) = &settings.proxy {
		builder = builder.proxy(reqwest::Proxy::all(proxy)?);
	}
	builder.build().map_err(Error::Http)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_client_builds_from_the_defaults_and_from_every_version() {
		let settings = Settings::default();
		assert!(build(&settings, false).is_ok());
		assert!(build(&settings, true).is_ok());
		for http in [HttpVersion::Http1, HttpVersion::Http2] {
			let settings = Settings { http, ..Settings::default() };
			assert!(build(&settings, false).is_ok());
		}
		let with_proxy =
			Settings { proxy: Some("socks5://127.0.0.1:1".to_owned()), ..Settings::default() };
		assert!(build(&with_proxy, false).is_ok());
		let bad_proxy = Settings { proxy: Some("::".to_owned()), ..Settings::default() };
		assert!(build(&bad_proxy, false).is_err());
	}
}
