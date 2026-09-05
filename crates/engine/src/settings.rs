//! What a download can be told about how to behave. Every knob a settings window will one day
//! show lives here, with the value it has until somebody changes it; the window is not built,
//! so nothing here is read from a file yet. See spec/engine.md.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How many connections a download may open at once. `auto` lets the engine start with one
/// and grow towards `max` as the server proves it can take more; off, it opens `max` at once
/// when the file is large enough to split and one otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connections {
	pub min: u8,
	pub max: u8,
	pub auto: bool,
}

impl Default for Connections {
	fn default() -> Self {
		// aria2's defaults: one connection until there is a reason for more, sixteen at most.
		Connections { min: 1, max: 16, auto: true }
	}
}

impl Connections {
	/// What was asked for, made sane: at least one, and `min` never above `max`.
	pub fn clamped(self) -> Connections {
		let max = self.max.max(1);
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
			retries: 5,
			retry_wait: Duration::from_secs(1),
			speed_limit: None,
			max_size: None,
			http: HttpVersion::Auto,
			user_agent: concat!("rdm/", env!("CARGO_PKG_VERSION")).to_owned(),
			headers: Vec::new(),
			proxy: None,
			max_redirects: 10,
			preallocate: true,
		}
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
}
