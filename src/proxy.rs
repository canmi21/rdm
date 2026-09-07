//! Which proxy the downloads go through, and finding one that is already running.
//!
//! The machines this is used on usually have a proxy on them, and it is usually listening on one
//! of a handful of ports. Asking the user to type an address they did not choose -- mihomo picked
//! 7890, not them -- is asking them to know something about their own machine that the machine can
//! be asked instead. So the default is to look: open a socket to each of the known ports, take the
//! first that answers, and go straight out when none does.
//!
//! **Three things are looked for, not seven programs.** A catalogue of every proxy's default port
//! is a list to keep up with, and what matters is not which program is listening but what it
//! speaks: an HTTP proxy, a SOCKS5 proxy, and the 78xx ports the Clash family and its descendants
//! have made the ones a machine here is most likely to have. Ports that a program other than a
//! proxy commonly holds are left out -- 8080 is somebody's development server far more often than
//! it is a proxy, and sending every download through it would be worse than finding nothing.
//!
//! Nothing is guessed beyond that. A port that answers is a program listening, not necessarily a
//! proxy, so the address found is shown in Settings and can be overruled by one typed there.
//!
//! **A proxy resolves the names it is given.** See `resolved_there` at the bottom of this file for
//! why that is not an option, and src/dns.rs for what happens when there is no proxy.
//! See spec/engine.md.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where a proxy comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
	/// Straight out: no proxy, whatever is running on this machine.
	Direct,
	/// Whatever is running on this machine, found by trying the ports the common tools listen
	/// on. Nothing found is the same as Direct.
	#[default]
	Found,
	/// The address the user typed, and only that one.
	Fixed,
}

impl Source {
	pub const ALL: [Source; 3] = [Source::Found, Source::Fixed, Source::Direct];

	pub fn name(self) -> &'static str {
		match self {
			Source::Found => crate::i18n::t("proxy.source.found"),
			Source::Fixed => crate::i18n::t("proxy.source.fixed"),
			Source::Direct => crate::i18n::t("proxy.source.direct"),
		}
	}
}

/// The addresses tried, in the order they are tried: the mixed port first, because it is the one
/// most of these machines have, then the rest of the 78xx family, then the port SOCKS5 has
/// listened on since before any of this.
///
/// A mixed port is written as `http://`: reqwest will send HTTP through it, which the port
/// accepts, and a SOCKS-only listener is written as `socks5://`. Nothing here is a guess about
/// what the program is -- only about what it speaks on that number.
pub const KNOWN: [&str; 4] = [
	// The mixed port, HTTP and SOCKS on one number, on its old and new defaults.
	"http://127.0.0.1:7890",
	"http://127.0.0.1:7897",
	// The SOCKS port that usually sits beside it.
	"socks5://127.0.0.1:7891",
	// The address a SOCKS5 proxy has had since before any of the above.
	"socks5://127.0.0.1:1080",
];

/// How long a port is given to answer. A proxy on this machine answers in under a millisecond;
/// anything that does not is either not there or not worth waiting for at launch.
const PATIENCE: Duration = Duration::from_millis(120);

/// The first known address that answers, or None. Blocking, and meant for a background thread:
/// four connections at an eighth of a second each is half a second in the worst case, which is
/// nothing off the main thread and a visible stall on it.
pub fn discover() -> Option<String> {
	KNOWN.iter().find(|address| answers(address)).map(|address| (*address).to_owned())
}

/// Whether something is listening at this address. A connection that opens is all that is asked:
/// speaking the protocol to find out whether it is really a proxy would mean sending a request
/// through a program the user has not agreed to send anything through.
fn answers(address: &str) -> bool {
	let Some(socket) = socket_of(address) else { return false };
	TcpStream::connect_timeout(&socket, PATIENCE).is_ok()
}

/// The host and port out of a proxy address, for the connection test. Only the loopback
/// addresses this looks for are parsed; anything else is not something to probe.
fn socket_of(address: &str) -> Option<SocketAddr> {
	let (_, rest) = address.split_once("://")?;
	rest.parse().ok()
}

/// The address as the client should be given it, which for SOCKS means the form that sends the
/// name rather than an address.
///
/// `socks5://` and `socks5h://` are the same wire protocol -- SOCKS5 has carried domain names
/// since it was written -- and differ only in whether the client resolves before it connects.
/// That distinction is curl trivia and not something a download manager's settings field should
/// make somebody know: whoever types `socks5://` means "a SOCKS5 proxy", and the proxy is who
/// should be resolving. An address that is not SOCKS5 is handed back as it came, an HTTP proxy
/// already being given the name in the CONNECT line.
pub fn resolved_there(address: &str) -> String {
	match address.strip_prefix("socks5://") {
		Some(rest) => format!("socks5h://{rest}"),
		None => address.to_owned(),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// What is looked for is three things and not a catalogue of programs: a mixed port, a SOCKS
	/// port, and nothing on a number that something other than a proxy commonly holds.
	#[test]
	fn the_list_is_short_and_holds_no_port_a_development_server_wants() {
		assert!(KNOWN.len() <= 4, "a list to keep up with is a list that falls behind");
		for address in KNOWN {
			for taken in [":8080", ":3000", ":8000", ":5000"] {
				assert!(!address.ends_with(taken), "{address} is somebody's own server");
			}
		}
	}

	/// A SOCKS proxy is given the name, not an address: it is the one that can route on a domain,
	/// and the one whose answer comes from where the connection is made.
	#[test]
	fn a_socks_proxy_is_handed_the_name() {
		assert_eq!(resolved_there("socks5://127.0.0.1:7891"), "socks5h://127.0.0.1:7891");
		assert_eq!(resolved_there("socks5h://127.0.0.1:1080"), "socks5h://127.0.0.1:1080");
		assert_eq!(resolved_there("http://127.0.0.1:7890"), "http://127.0.0.1:7890");
		for address in KNOWN {
			assert!(!resolved_there(address).starts_with("socks5://"), "{address}");
		}
	}

	#[test]
	fn every_known_address_parses_and_names_a_scheme() {
		for address in KNOWN {
			assert!(address.starts_with("http://") || address.starts_with("socks5://"), "{address}");
			let socket = socket_of(address).unwrap_or_else(|| panic!("{address} is a socket"));
			assert!(socket.ip().is_loopback(), "only this machine's own ports are probed: {address}");
		}
	}

	/// A port nothing is on answers nothing, and quickly. The one this uses is in the range the
	/// system hands out for a moment and lets go of, so it is as close to certainly free as a
	/// port gets without holding one open to find out.
	#[test]
	fn a_port_with_nothing_on_it_does_not_answer() {
		assert!(!answers("http://127.0.0.1:1"), "port 1 needs privileges nobody here has");
		assert!(!answers("not-an-address"), "and something that is not an address is not tried");
		assert!(!answers("http://example.com:80"), "nor anything that is not this machine");
	}

	#[test]
	fn the_source_offered_first_is_the_one_that_asks_the_machine() {
		assert_eq!(Source::default(), Source::Found);
		assert_eq!(Source::ALL[0], Source::Found);
		for source in Source::ALL {
			assert!(!source.name().is_empty());
		}
	}
}
