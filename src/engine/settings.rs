//! What a download can be told about how to behave, and the HTTP client built from it. Every
//! knob lives here with the value it has until somebody changes it. One client is built per
//! connection of a split download, forced to HTTP/1.1, because the point of several connections
//! is several TCP connections and HTTP/2 would fold them into one. See spec/engine.md.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::engine::error::{Error, Result};

/// How many connections a download may open at once. `auto` lets the engine start with one
/// and grow towards `max` as the server proves it can take more; off, it opens `max` at once
/// when the file is large enough to split and one otherwise. Never more than `MAX`, which is
/// what the window lets a person ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connections {
	pub min: u16,
	pub max: u16,
	pub auto: bool,
}

impl Default for Connections {
	fn default() -> Self {
		Connections::auto()
	}
}

impl Connections {
	/// The most a download may open, whatever is asked.
	pub const MAX: u16 = 32;

	/// The engine's own judgement: four connections at once, then two more each time one delivers
	/// its first byte -- twice as many each round, as TCP's slow start grows -- up to thirty-two,
	/// and never a segment shorter than `min_segment`, so a small file stays on few and a large
	/// one grows as far as the server and the file allow. A server that takes fewer turns the
	/// rest away and the count comes down to what it takes. See spec/engine.md.
	pub fn auto() -> Connections {
		Connections { min: 4, max: 32, auto: true }
	}

	/// Exactly this many, opened at once when the file can be split.
	pub fn fixed(count: u16) -> Connections {
		let count = count.clamp(1, Connections::MAX);
		Connections { min: count, max: count, auto: false }
	}

	/// What was asked for, made sane: at least one, at most `MAX`, and `min` never above `max`.
	pub fn clamped(self) -> Connections {
		let max = self.max.clamp(1, Connections::MAX);
		Connections { min: self.min.clamp(1, max), max, auto: self.auto }
	}
}

/// Which HTTP the client speaks. Auto lets it negotiate; a download split across several
/// connections is forced to HTTP/1.1 regardless, because HTTP/2 multiplexes every request onto
/// one TCP connection and the point of several connections is several TCP connections.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HttpVersion {
	Auto,
	Http1,
	Http2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
	pub connections: Connections,
	/// A file smaller than this is never split: the connections would spend longer being set up
	/// than transferring. aria2 calls this `min-split-size`.
	pub min_segment: u64,
	/// The connection is given this long to be established.
	pub connect_timeout: Duration,
	/// A connection that sends nothing for this long is dropped and its segment retried.
	pub idle_timeout: Duration,
	/// How long a connection may go without a byte, or crawl far behind the others, before it is
	/// dropped and reopened from where it stands. Much shorter than `idle_timeout`, which is the
	/// transport's last word; this is the scheduler noticing one connection holding the rest up.
	/// See spec/engine.md, "A stuck connection is reopened".
	pub stall_timeout: Duration,
	/// How many times a failing segment is retried before the download fails; the wait between
	/// tries doubles from `retry_wait` each time.
	pub retries: u32,
	pub retry_wait: Duration,
	/// Bytes per second for this download; None is unlimited. The engine has a global limit of
	/// its own on top.
	pub speed_limit: Option<u64>,
	/// A file the server declares larger than this is refused before a byte is transferred.
	pub max_size: Option<u64>,
	pub http: HttpVersion,
	pub user_agent: String,
	/// Sent with every request, after the ones the engine sets itself.
	pub headers: Vec<(String, String)>,
	/// `http://`, `https://` or `socks5://`, with credentials in the URL; None uses the system's.
	pub proxy: Option<String>,
	/// Who resolves names and how. What this comes to is one resolver the whole process shares;
	/// the built thing is not a setting and does not live here. A download that goes through a
	/// proxy uses none of it, the name being the proxy's to resolve. See src/dns.rs.
	pub dns: crate::dns::Choice,
	pub max_redirects: usize,
	/// The file is grown to its full length before the first byte lands, so a segment can be
	/// written at its offset and a full disk fails the download at the start rather than the end.
	pub preallocate: bool,
}

impl Default for Settings {
	fn default() -> Self {
		Settings {
			connections: Connections::default(),
			min_segment: 1024 * 1024,
			connect_timeout: Duration::from_secs(30),
			idle_timeout: Duration::from_secs(60),
			stall_timeout: Duration::from_secs(10),
			retries: 5,
			retry_wait: Duration::from_secs(1),
			speed_limit: None,
			max_size: None,
			http: HttpVersion::Auto,
			user_agent: concat!("rdm/", env!("CARGO_PKG_VERSION")).to_owned(),
			headers: Vec::new(),
			proxy: None,
			dns: crate::dns::Choice::default(),
			max_redirects: 10,
			preallocate: true,
		}
	}
}

impl Settings {
	/// A client for one connection. `split` says the download has several, which forces HTTP/1.1
	/// whatever the setting says.
	pub fn client(&self, split: bool) -> Result<reqwest::Client> {
		crate::tls::install();
		let mut headers = HeaderMap::new();
		for (name, value) in &self.headers {
			if let (Ok(name), Ok(value)) =
				(HeaderName::from_bytes(name.as_bytes()), HeaderValue::from_str(value))
			{
				headers.insert(name, value);
			}
		}
		let mut builder = reqwest::Client::builder()
			.user_agent(&self.user_agent)
			.default_headers(headers)
			.connect_timeout(self.connect_timeout)
			// The idle timeout is enforced per chunk by the worker, which knows when bytes stop; a
			// whole-request timeout would cut a long download that is doing fine.
			.redirect(reqwest::redirect::Policy::limited(self.max_redirects))
			// Downloads want the bytes as they are on the server: a transfer encoding decoded on
			// the way would make Content-Length and byte ranges lie.
			.no_gzip()
			.no_brotli()
			.no_deflate()
			.no_zstd();
		builder = match (split, self.http) {
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
		let proxied = self.proxy.is_some();
		if let Some(proxy) = &self.proxy {
			let mut through = reqwest::Proxy::all(crate::proxy::resolved_there(proxy))?;
			// The domains the machine answers for do not go through a proxy either. `nas.local` is on
			// the network this machine is on, and a proxy can neither resolve it nor reach it -- so
			// without this the list would do nothing at all on a machine with a proxy running, which
			// is most of them here.
			if let Some(names) = self.dns.no_proxy() {
				through = through.no_proxy(reqwest::NoProxy::from_string(&names));
			}
			builder = builder.proxy(through);
		}
		// Names go to the proxy where there is one -- unless DNS over HTTPS is on, which says
		// something stronger than where an answer should come from: who may see and answer the
		// question at all. Somebody who turns it on beside a proxy has pointed two things at one job,
		// and that is theirs to settle; the switch is off until they do.
		if (!proxied || self.dns.transport.is_https())
			&& let Some(resolver) = crate::dns::resolver(&self.dns, proxied)
		{
			builder = builder.dns_resolver(std::sync::Arc::new(resolver));
		}
		builder.build().map_err(Error::Http)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn connections_are_made_sane_rather_than_refused() {
		assert_eq!(
			Connections { min: 0, max: 0, auto: true }.clamped(),
			Connections { min: 1, max: 1, auto: true }
		);
		assert_eq!(
			Connections { min: 9, max: 4, auto: false }.clamped(),
			Connections { min: 4, max: 4, auto: false }
		);
	}

	#[test]
	fn a_client_builds_from_the_defaults_and_from_every_version() {
		let settings = Settings::default();
		assert!(settings.client(false).is_ok());
		assert!(settings.client(true).is_ok());
		for http in [HttpVersion::Http1, HttpVersion::Http2] {
			let settings = Settings { http, ..Settings::default() };
			assert!(settings.client(false).is_ok());
		}
		let with_proxy =
			Settings { proxy: Some("socks5://127.0.0.1:1".to_owned()), ..Settings::default() };
		assert!(with_proxy.client(false).is_ok());
		let bad_proxy = Settings { proxy: Some("::".to_owned()), ..Settings::default() };
		assert!(bad_proxy.client(false).is_err());
	}
}
