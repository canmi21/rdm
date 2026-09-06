//! How a name becomes an address. Three questions, each its own answer, because the reasons for
//! changing one are not the reasons for changing the others:
//!
//! - **Who is asked.** The system's own servers, which is what everything else on the machine
//!   uses, or a pair named here. A machine on a network that answers `github.com` with a lie is
//!   the reason anybody sets this, and the pair offered first is the pair such a person means:
//!   Cloudflare and Google.
//! - **How they are asked.** Plain DNS on port 53, which anything between here and there can read
//!   and rewrite, or DNS over HTTPS, which it cannot.
//! - **What does the asking.** The system's resolver, which knows about the machine's search
//!   domains, its `/etc/hosts` and its VPN, or one written in Rust that knows only what it is
//!   told. The system's is right far more often; the other exists because the system's cannot be
//!   pointed at a server of one's choosing on every platform.
//!
//! Nothing here is on by default. The system's stack, asking the system's servers, is what a
//! download manager should do until somebody says otherwise. See spec/engine.md.

use std::net::SocketAddr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Who is asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Servers {
	/// Whatever the machine is configured with, which is what everything else on it uses.
	#[default]
	System,
	/// The addresses named in the settings, and only those.
	Named,
}

impl Servers {
	pub const ALL: [Servers; 2] = [Servers::System, Servers::Named];

	pub fn name(self) -> &'static str {
		match self {
			Servers::System => crate::i18n::t("dns.servers.system"),
			Servers::Named => crate::i18n::t("dns.servers.named"),
		}
	}
}

/// How they are asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
	/// Port 53, in the clear, which anything between here and there can read and rewrite.
	#[default]
	Plain,
	/// DNS over HTTPS, which it cannot.
	Https,
}

impl Transport {
	pub const ALL: [Transport; 2] = [Transport::Plain, Transport::Https];

	pub fn name(self) -> &'static str {
		match self {
			Transport::Plain => crate::i18n::t("dns.transport.plain"),
			Transport::Https => crate::i18n::t("dns.transport.https"),
		}
	}
}

/// What does the asking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stack {
	/// The system's own resolver, which knows the machine's search domains, its hosts file and
	/// its VPN.
	#[default]
	System,
	/// Hickory, which knows only what it is told, and can be told to ask anyone.
	Hickory,
}

impl Stack {
	pub const ALL: [Stack; 2] = [Stack::System, Stack::Hickory];

	pub fn name(self) -> &'static str {
		match self {
			Stack::System => crate::i18n::t("dns.stack.system"),
			Stack::Hickory => crate::i18n::t("dns.stack.hickory"),
		}
	}
}

/// The servers offered first when somebody chooses to name their own: the two public resolvers
/// anybody in this position already knows the addresses of. Written as text because that is what
/// the settings field holds and what the user edits.
pub const DEFAULT_PLAIN: &str = "1.1.1.1, 8.8.8.8";

/// And the same two over HTTPS.
pub const DEFAULT_HTTPS: &str = "https://cloudflare-dns.com/dns-query, https://dns.google/dns-query";

/// What the settings come to, gathered so the resolver is built from one thing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
	pub servers: Servers,
	pub transport: Transport,
	pub stack: Stack,
	/// The addresses or URLs, as the user wrote them: comma or space apart.
	pub written: String,
}

impl Choice {
	/// Whether anything here asks for a resolver of our own. The system's stack asking the
	/// system's servers is the whole of what reqwest does already.
	pub fn is_default(&self) -> bool {
		self.stack == Stack::System && self.servers == Servers::System
	}
}

/// A resolver built to a choice, or None where the choice is the system's own and there is
/// nothing to build. An address that will not parse is left out rather than taken as a reason to
/// fail: a settings field is typed into a character at a time, and a resolver that refused to
/// exist while it was half-typed would take the downloads with it.
pub fn resolver(choice: &Choice) -> Option<Arc<Resolver>> {
	if choice.is_default() {
		return None;
	}
	let provider = hickory_resolver::net::runtime::TokioRuntimeProvider::default();
	let builder = match choice.servers {
		// The system's servers, asked by hickory: what the machine is configured with, read the
		// way the machine reads it.
		Servers::System => hickory_resolver::TokioResolver::builder(provider).ok()?,
		Servers::Named => {
			hickory_resolver::TokioResolver::builder_with_config(named_config(choice)?, provider)
		}
	};
	// A resolver that will not build is a resolver we do without: the system's stack answers.
	Some(Arc::new(Resolver { inner: builder.build().ok()? }))
}

/// The servers the user named, as a hickory config. None where nothing in the field parsed,
/// which is the same as having named nothing.
fn named_config(choice: &Choice) -> Option<hickory_resolver::config::ResolverConfig> {
	use hickory_resolver::config::{NameServerConfig, ResolverConfig};
	let written: Vec<&str> =
		choice.written.split([',', ' ', '\n']).map(str::trim).filter(|s| !s.is_empty()).collect();
	let servers: Vec<NameServerConfig> = match choice.transport {
		Transport::Plain => written
			.iter()
			.filter_map(|text| text.parse().ok())
			.map(NameServerConfig::udp_and_tcp)
			.collect(),
		Transport::Https => written
			.iter()
			.filter_map(|url| {
				// A DoH server is named by its URL, and its address is found the ordinary way --
				// which is not a circle: the URL's host is resolved once, through whatever is
				// already working, and every question after it goes over HTTPS.
				let rest = url.strip_prefix("https://")?;
				let (host, path) = rest.split_once('/').unwrap_or((rest, "dns-query"));
				let addresses = std::net::ToSocketAddrs::to_socket_addrs(&(host, 443)).ok()?;
				let ip = addresses.map(|a: SocketAddr| a.ip()).next()?;
				Some(NameServerConfig::https(ip, host.into(), Some(format!("/{path}").into())))
			})
			.collect(),
	};
	if servers.is_empty() {
		return None;
	}
	Some(ResolverConfig::from_parts(None, Vec::new(), servers))
}

/// What reqwest is handed. The trait is reqwest's; the work is hickory's.
pub struct Resolver {
	inner: hickory_resolver::TokioResolver,
}

impl reqwest::dns::Resolve for Resolver {
	fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
		let inner = self.inner.clone();
		Box::pin(async move {
			let lookup = inner.lookup_ip(name.as_str()).await?;
			// The port is reqwest's to fill in: it says so, and fills a zero with the scheme's.
			let addresses: Vec<SocketAddr> =
				lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
			Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn choice(servers: Servers, transport: Transport, stack: Stack, written: &str) -> Choice {
		Choice { servers, transport, stack, written: written.to_owned() }
	}

	#[test]
	fn the_system_asking_the_system_builds_nothing() {
		let plain = choice(Servers::System, Transport::Plain, Stack::System, "");
		assert!(plain.is_default(), "which is what reqwest does already");
		assert!(resolver(&plain).is_none());
	}

	/// A settings field is typed into a character at a time. A resolver that refused to exist
	/// while the field was half-typed would take the downloads with it, so what does not parse is
	/// left out and what is left over is used.
	#[test]
	fn addresses_that_do_not_parse_are_left_out_rather_than_fatal() {
		let half = choice(Servers::Named, Transport::Plain, Stack::Hickory, "1.1.1.1, 8.8.8");
		assert!(named_config(&half).is_some(), "one good address is enough");
		let none = choice(Servers::Named, Transport::Plain, Stack::Hickory, "nonsense");
		assert!(named_config(&none).is_none(), "and none is none");
		let empty = choice(Servers::Named, Transport::Plain, Stack::Hickory, "");
		assert!(named_config(&empty).is_none());
	}

	#[test]
	fn the_offered_servers_are_the_two_anybody_in_this_position_knows() {
		assert!(DEFAULT_PLAIN.contains("1.1.1.1") && DEFAULT_PLAIN.contains("8.8.8.8"));
		assert!(DEFAULT_HTTPS.contains("cloudflare-dns.com") && DEFAULT_HTTPS.contains("dns.google"));
		let named = choice(Servers::Named, Transport::Plain, Stack::Hickory, DEFAULT_PLAIN);
		assert!(named_config(&named).is_some(), "and the default pair parses");
	}

	#[test]
	fn every_choice_is_named_and_the_defaults_are_the_systems() {
		assert_eq!(Servers::default(), Servers::System);
		assert_eq!(Transport::default(), Transport::Plain);
		assert_eq!(Stack::default(), Stack::System);
		for name in Servers::ALL.map(Servers::name).into_iter().chain(Transport::ALL.map(Transport::name)) {
			assert!(!name.is_empty());
		}
		for stack in Stack::ALL {
			assert!(!stack.name().is_empty());
		}
	}
}
