//! The HTTP client, built from the settings. One is built per connection of a split download,
//! forced to HTTP/1.1, because the point of several connections is several TCP connections and
//! HTTP/2 would fold them into one. See spec/engine.md.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use crate::engine::error::{Error, Result};
use crate::engine::settings::{HttpVersion, Settings};

/// A client for one connection. `split` says the download has several, which forces HTTP/1.1
/// whatever the setting says.
pub fn build(settings: &Settings, split: bool) -> Result<reqwest::Client> {
	crate::tls::install();
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
	// A proxy carries the name, and resolving it here would be answering from the wrong place: a
	// CDN's answer depends on who asked, and the one that matters is the one seen from where the
	// connection is made. It would also cost a rule-based proxy the domain it routes on. So a
	// request that goes through one is handed the name and nothing here resolves anything -- not
	// our stack, and not the system's either, the name travelling in the CONNECT line or in the
	// SOCKS request. See src/proxy.rs and src/dns.rs.
	let proxied = settings.proxy.is_some();
	if let Some(proxy) = &settings.proxy {
		let mut through = reqwest::Proxy::all(crate::proxy::resolved_there(proxy))?;
		// The domains the machine answers for do not go through a proxy either. `nas.local` is on
		// the network this machine is on, and a proxy can neither resolve it nor reach it -- so
		// without this the list would do nothing at all on a machine with a proxy running, which
		// is most of them here.
		if let Some(names) = settings.dns.no_proxy() {
			through = through.no_proxy(reqwest::NoProxy::from_string(&names));
		}
		builder = builder.proxy(through);
	}
	// Names go to the proxy where there is one -- unless DNS over HTTPS is on, which says
	// something stronger than where an answer should come from: who may see and answer the
	// question at all. Somebody who turns it on beside a proxy has pointed two things at one job,
	// and that is theirs to settle; the switch is off until they do.
	if (!proxied || settings.dns.transport.is_https())
		&& let Some(resolver) = crate::dns::resolver(&settings.dns, proxied)
	{
		builder = builder.dns_resolver(std::sync::Arc::new(resolver));
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
