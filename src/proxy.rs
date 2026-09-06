//! Which proxy the downloads go through, and finding one that is already running.
//!
//! The machines this is used on usually have a proxy on them, and it is usually one of a handful
//! of programs listening on one of a handful of ports. Asking the user to type an address they
//! did not choose -- mihomo picked 7890, not them -- is asking them to know something about their
//! own machine that the machine can be asked instead. So the default is to look: open a socket to
//! each of the known ports, take the first that answers, and go straight out when none does.
//!
//! Nothing is guessed beyond that. A port that answers is a program listening, not necessarily a
//! proxy, so the address found is shown in Settings and can be overruled by one typed there. See
//! spec/engine.md.

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

/// The addresses tried, in the order they are tried. The first is mihomo and Clash's mixed port,
/// which speaks HTTP and SOCKS on one number and is what most of these machines have; the rest
/// are the other defaults those programs and their neighbours ship with.
///
/// A mixed port is written as `http://`: reqwest will send HTTP through it, which the port
/// accepts, and a SOCKS-only listener is written as `socks5://`. Nothing here is a guess about
/// what the program is -- only about what it speaks on that number.
pub const KNOWN: [&str; 7] = [
	// mihomo, Clash and Clash Verge: the mixed port, old and new defaults.
	"http://127.0.0.1:7890",
	"http://127.0.0.1:7897",
	// Clash's separate SOCKS port, beside the mixed one.
	"socks5://127.0.0.1:7891",
	// V2Ray, Xray and sing-box as their templates ship them.
	"socks5://127.0.0.1:10808",
	"http://127.0.0.1:10809",
	// The address a SOCKS proxy has had since before any of these.
	"socks5://127.0.0.1:1080",
	// Privoxy, which a good many of the above chain through.
	"http://127.0.0.1:8118",
];

/// How long a port is given to answer. A proxy on this machine answers in under a millisecond;
/// anything that does not is either not there or not worth waiting for at launch.
const PATIENCE: Duration = Duration::from_millis(120);

/// The first known address that answers, or None. Blocking, and meant for a background thread:
/// seven connections at a tenth of a second each is most of a second in the worst case, which is
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

#[cfg(test)]
mod tests {
	use super::*;

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
