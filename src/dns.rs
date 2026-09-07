//! How a name becomes an address.
//!
//! **This application resolves names itself, and does it the same way on every platform.** That
//! is the whole reason: one Rust stack, one cache, one set of timeouts, so a download that will
//! not start behaves the same on macOS, Windows and Linux and can be reasoned about from one
//! place. It is not a security measure. Asking the machine's own servers with our own client
//! gets the machine's own answers, lies included; what buys trust is changing who is asked or
//! how, and both of those are the user's to turn on.
//!
//! What the system's stack knows and no unicast server does is `.local`, which is answered by
//! multicast, and whatever a VPN's own scoped resolver answers for. So a name our resolver
//! cannot find is put to the system once before the download fails. The fallback is an escape
//! hatch and not a second opinion: it runs where nobody could have answered, never where an
//! answer came back that somebody might not like.
//!
//! Four things the user can change, and each turns something off:
//!
//! - **Force the system's resolver.** Off. On, nothing here is built and reqwest resolves the way
//!   the machine does, which is the way out if this arrangement is ever the problem.
//! - **DNS over HTTPS.** Off. On, the question cannot be read or rewritten on the way, which is
//!   what somebody whose network answers `github.com` with a lie is after.
//! - **Force DNS over HTTPS.** Off, and only there to be turned on beside the one above. On,
//!   HTTPS is the only way a question goes out: nothing below it in the chain, and a name that
//!   cannot be resolved that way is a download that does not start.
//! - **Which servers.** The machine's own, one of the two anybody in that position already knows,
//!   or whatever is written in the field.
//!
//! **The chain, once the first rung has not answered.** DNS over HTTPS falls to our own stack on
//! port 53, and that to the machine's. A request carried by a proxy skips the middle rung: a
//! second question of ours from the wrong place is not what it wants, and the machine's stack is.
//! Forcing HTTPS has no chain at all, which is the whole of what forcing it means.
//!
//! **A download that goes through a proxy resolves nothing here, unless HTTPS is on.** The name
//! travels to the proxy and the proxy resolves it, because the address a CDN gives depends on who
//! asked and the one that matters is the one seen from where the connection is made. Turning
//! HTTPS on says something stronger -- who may see and answer the question at all -- so it wins,
//! and somebody who turns it on beside a proxy has pointed two things at one job. The switch is
//! off until they do. See src/engine/client.rs.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex, OnceLock};

use hickory_resolver::TokioResolver;
use hickory_resolver::config::{LookupIpStrategy, NameServerConfig, ResolverConfig};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::net::{DnsError, NetError};
use serde::{Deserialize, Serialize};

/// Which servers are asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Servers {
	/// Whatever the machine is configured with, which is what everything else on it uses. On
	/// Apple that is read from the SystemConfiguration store and not from `/etc/resolv.conf`, so
	/// the search domains arrive with the addresses.
	#[default]
	System,
	Cloudflare,
	Google,
	/// Whatever is written in the field. `named` is what an older file calls this.
	#[serde(alias = "named")]
	Custom,
}

impl Servers {
	/// What is offered, which depends on the transport: a machine's DNS configuration names
	/// addresses and never URLs, so following it is an answer on port 53 and not over HTTPS.
	pub fn offered(transport: Transport) -> &'static [Servers] {
		match transport {
			Transport::Plain => &[Servers::System, Servers::Cloudflare, Servers::Google, Servers::Custom],
			Transport::Https => &[Servers::Cloudflare, Servers::Google, Servers::Custom],
		}
	}

	/// What the option is called. The two servers offered are named by their address on port 53
	/// and by their operator over HTTPS, because that is what somebody looking for them knows:
	/// everybody remembers 1.1.1.1 and nobody remembers a DoH URL.
	pub fn name(self, transport: Transport) -> &'static str {
		match (self, transport) {
			(Servers::System, _) => crate::i18n::t("dns.servers.system"),
			(Servers::Custom, _) => crate::i18n::t("dns.servers.custom"),
			(Servers::Cloudflare, Transport::Plain) => "1.1.1.1",
			(Servers::Google, Transport::Plain) => "8.8.8.8",
			(Servers::Cloudflare, Transport::Https) => "Cloudflare",
			(Servers::Google, Transport::Https) => "Google",
		}
	}

	/// What choosing it writes into the field beside it, so what is being asked is on screen
	/// rather than implied -- the same reason a chosen user agent fills its field. The machine's
	/// own servers and Custom write nothing: one has nothing to show and the other is the field.
	pub fn written(self, transport: Transport) -> &'static str {
		match (self, transport) {
			(Servers::Cloudflare, Transport::Plain) => "1.1.1.1",
			(Servers::Google, Transport::Plain) => "8.8.8.8",
			(Servers::Cloudflare, Transport::Https) => "https://cloudflare-dns.com/dns-query",
			(Servers::Google, Transport::Https) => "https://dns.google/dns-query",
			(Servers::System | Servers::Custom, _) => "",
		}
	}
}

/// How they are asked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transport {
	/// Port 53, which anything between here and there can read and rewrite. UDP first; hickory
	/// moves the same question to TCP when an answer comes back truncated, and also when it comes
	/// back with the wrong case, which is how a forged reply gives itself away.
	#[default]
	Plain,
	/// DNS over HTTPS, which it cannot.
	Https,
}

impl Transport {
	pub fn of(https: bool) -> Transport {
		match https {
			true => Transport::Https,
			false => Transport::Plain,
		}
	}

	pub fn is_https(self) -> bool {
		self == Transport::Https
	}
}

/// Names that are the machine's own business, whatever else is set: they go to its stack and
/// nothing here is asked. Exactly one is built in.
///
/// `.local` is answered by multicast and no unicast server has it, so where it should go is not a
/// policy question -- there is no other right answer, and a resolver that sends it to Cloudflare
/// is a resolver that has broken `nas.local` for nothing. **Every other internal domain is
/// somebody's arrangement and not ours to guess.** `.lan`, `.home`, `.internal`, `.corp` and a
/// company's own name are all real, all different and all in use for public names somewhere; a
/// list of guesses would quietly take names away from the servers the user chose, which is the
/// one thing naming servers is meant to prevent. So the rest is a field.
pub const ALWAYS_THE_SYSTEM: [&str; 1] = ["local"];

/// The two servers offered, and the addresses they answer on, written down so that choosing one
/// does not need a question answered before it can ask its own. A DoH server named by a host we
/// have not got an address for is looked up the ordinary way, which is not a circle -- one
/// question before the first, and every question after it over HTTPS -- but it does mean the
/// bootstrap goes through whatever is already working, and these two never have to.
const PINNED: [(&str, &str); 2] = [("cloudflare-dns.com", "1.1.1.1"), ("dns.google", "8.8.8.8")];

/// What the settings come to, gathered so the resolver is built from one thing and so two
/// settings that come to the same thing share a resolver.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
	/// Nothing of ours is built, and reqwest resolves the way the machine does.
	pub force_system: bool,
	pub transport: Transport,
	/// HTTPS or nothing: no rung below it, and a question that cannot go that way does not go.
	/// Means nothing while the transport is port 53.
	pub force_https: bool,
	pub servers: Servers,
	/// The addresses or URLs, as the user wrote them: comma, space or newline apart.
	pub written: String,
	/// Domains the system resolves whatever the rest of this says, as the user wrote them.
	/// `ALWAYS_THE_SYSTEM` is in force beside these and cannot be taken out.
	pub system_domains: String,
}

impl Choice {
	/// The servers to ask, as text: the field where the choice is Custom, the chosen server's own
	/// address or URL otherwise, and nothing where the machine's own are the ones to read.
	fn text(&self) -> &str {
		let written = match self.servers {
			Servers::Custom => self.written.trim(),
			other => other.written(self.transport),
		};
		match (written.is_empty(), self.transport) {
			// Over HTTPS there is no such thing as following the machine, which names addresses
			// and never URLs. A choice that comes to nothing there is the server offered first,
			// not a quiet drop back to port 53 -- somebody who asked for HTTPS did not ask for
			// their questions to go out in the clear because a field was empty.
			(true, Transport::Https) => Servers::Cloudflare.written(Transport::Https),
			_ => written,
		}
	}

	/// The middle rung: our own stack on port 53, on whatever servers the machine is configured
	/// with, which is what the default choice already is.
	fn plain(&self) -> Choice {
		Choice { system_domains: self.system_domains.clone(), ..Choice::default() }
	}

	/// Every domain the system answers for: the one built in, and the ones written down. Leading
	/// dots are allowed and dropped, `.corp.example.com` and `corp.example.com` being the same
	/// thing to anybody who writes either.
	fn system_domain_list(&self) -> impl Iterator<Item = &str> {
		ALWAYS_THE_SYSTEM.into_iter().chain(
			self
				.system_domains
				.split([',', ' ', '\n'])
				.map(|domain| domain.trim().trim_start_matches('.'))
				.filter(|domain| !domain.is_empty()),
		)
	}

	/// The same domains as reqwest wants them for `no_proxy`: comma-separated, which is what
	/// `NO_PROXY` has always been. None where there is nothing to say, which cannot happen while
	/// anything is built in but is the honest shape.
	pub fn no_proxy(&self) -> Option<String> {
		let list: Vec<&str> = self.system_domain_list().collect();
		(!list.is_empty()).then(|| list.join(","))
	}

	/// Whether this name is one of them. A domain matches itself and everything under it, which
	/// is what anybody writing `corp.example.com` in such a field means and what `NO_PROXY` has
	/// meant for as long as it has existed.
	fn goes_to_the_system(&self, name: &str) -> bool {
		let name = name.trim_end_matches('.');
		self.system_domain_list().any(|domain| {
			name.len() >= domain.len()
				&& name[name.len() - domain.len()..].eq_ignore_ascii_case(domain)
				&& (name.len() == domain.len() || name.as_bytes()[name.len() - domain.len() - 1] == b'.')
		})
	}

	/// Whether a name our resolver could not find is worth putting to the system. It is only when
	/// the servers being asked are the machine's own: what the system knows and they do not is
	/// `.local` and whatever a VPN answers for, and both come from the same machine either way.
	/// Somebody who named servers said they do not trust this machine's, and a fallback that
	/// asked it anyway would hand back the answers they refused.
	fn asks_system_for_missing(&self) -> bool {
		self.servers == Servers::System && self.transport == Transport::Plain
	}
}

/// The resolver this process has, and the choice it was built for.
///
/// **One, for the life of the process.** A resolver holds a cache, and a cache thrown away with
/// the client that made it answers nothing twice: a download builds a client per connection, so
/// this is the difference between one query for a name and sixteen. The choice is kept beside it
/// so a settings change replaces it rather than being answered by the servers it used to name.
static CURRENT: OnceLock<Mutex<Option<(Made, Resolver)>>> = OnceLock::new();

/// What a kept resolver was made for: the settings, and whether a proxy carries the requests.
/// The second is not a setting but it decides the chain, so it belongs to the identity.
type Made = (Choice, bool);

/// The resolver for a choice, or None where the choice is to let the system do it and there is
/// nothing to build. `proxied` says a proxy carries the requests this is for, which is not a
/// setting but a fact about them, and which decides whether the middle rung is in the chain.
pub fn resolver(choice: &Choice, proxied: bool) -> Option<Resolver> {
	if choice.force_system {
		return None;
	}
	let key = (choice.clone(), proxied);
	let held = CURRENT.get_or_init(|| Mutex::new(None));
	let mut held = held.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
	if let Some((made_for, resolver)) = held.as_ref()
		&& *made_for == key
	{
		return Some(resolver.clone());
	}
	let resolver = Resolver(Arc::new(Held {
		choice: choice.clone(),
		proxied,
		first: tokio::sync::OnceCell::new(),
		plain: tokio::sync::OnceCell::new(),
	}));
	*held = Some((key, resolver.clone()));
	Some(resolver)
}

/// What reqwest is handed. Cloning one clones a handle: every copy is the same resolver, the
/// same connections and the same cache.
#[derive(Clone)]
pub struct Resolver(Arc<Held>);

struct Held {
	choice: Choice,
	proxied: bool,
	/// Built at the first name asked rather than where this is made. Building it may have to look
	/// a DoH server's own address up, which is a question, and a question wants a runtime; this is
	/// made where a client is made, which is not always inside one.
	first: tokio::sync::OnceCell<TokioResolver>,
	/// The middle rung, built only if the chain ever reaches it.
	plain: tokio::sync::OnceCell<TokioResolver>,
}

impl Held {
	/// Whether our own stack on port 53 is a rung here: under DNS over HTTPS, over the system's,
	/// and only where no proxy carries the request -- one that gets this far wants the machine's
	/// stack, not a second question of ours asked from a place the connection is not made from.
	fn has_plain_rung(&self) -> bool {
		self.choice.transport.is_https() && !self.choice.force_https && !self.proxied
	}

	/// What is left of the chain once the first rung has not answered. Forcing HTTPS leaves
	/// nothing: both of the rungs below it would send the question out in the clear, which is
	/// the one thing forcing it is for.
	async fn below(&self, name: &str, why: NetError) -> Result<reqwest::dns::Addrs, Failure> {
		if self.choice.force_https && self.choice.transport.is_https() {
			return Err(Box::new(why));
		}
		if self.has_plain_rung() {
			let rung = self.choice.plain();
			if let Ok(plain) = self.plain.get_or_try_init(|| build(&rung)).await
				&& let Ok(lookup) = plain.lookup_ip(name).await
			{
				let found: Vec<SocketAddr> = lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
				if !found.is_empty() {
					return Ok(Box::new(found.into_iter()));
				}
			}
		}
		system(name).await
	}
}

/// What a resolution comes back as when it does not come back with an address.
type Failure = Box<dyn std::error::Error + Send + Sync>;

impl reqwest::dns::Resolve for Resolver {
	fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
		let held = self.0.clone();
		Box::pin(async move {
			let name = name.as_str().to_owned();
			// Before any of the chain: a name the machine answers for goes to the machine. This
			// is a routing rule and not a fallback -- it holds whatever else is set, forced HTTPS
			// included, because no DoH server has ever been able to answer for `nas.local` and
			// asking one is not stricter, only broken.
			if held.choice.goes_to_the_system(&name) {
				return system(&name).await;
			}
			let first = match held.first.get_or_try_init(|| build(&held.choice)).await {
				Ok(first) => first,
				// A resolver that will not build at all -- nothing usable in the settings, a DoH
				// server whose own address cannot be found -- is one we do without rather than a
				// download that fails before it starts.
				Err(why) => return held.below(&name, NetError::Msg(why.clone())).await,
			};
			match first.lookup_ip(name.as_str()).await {
				Ok(lookup) => {
					let addresses: Vec<SocketAddr> =
						// The port is reqwest's to fill in: it says so, and fills a zero with
						// the scheme's.
						lookup.iter().map(|ip| SocketAddr::new(ip, 0)).collect();
					match addresses.is_empty() {
						false => Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs),
						// An answer with nothing in it is the same as no answer.
						true => held.below(&name, NetError::Message("no addresses")).await,
					}
				}
				// A name our servers do not know may be one the system does: `.local` is answered
				// by multicast and a VPN's own names by a resolver scoped to it, and no unicast
				// server has either. But a name that does not exist is only worth asking about
				// again while the servers asked were the machine's own; somebody who named
				// servers said this machine's are not to be trusted.
				Err(error) if missing(&error) => match held.choice.asks_system_for_missing() {
					true => held.below(&name, error).await,
					false => Err(Box::new(error) as _),
				},
				// Anything else is the question not getting through, which is what the rungs
				// below are for whatever the servers were.
				Err(error) => held.below(&name, error).await,
			}
		})
	}
}

/// Whether the answer was "no such name" rather than "no answer". hickory reports both as an
/// error and they are the two ends of the fallback: a name that does not exist is worth putting
/// to the system only when the system's own servers were the ones asked, and a question that
/// never got through is worth putting to it however it was sent.
fn missing(error: &NetError) -> bool {
	matches!(error, NetError::Dns(DnsError::NoRecordsFound(_)))
}

/// The machine's own stack, asked the way everything else on it asks. `lookup_host` is
/// `getaddrinfo` on tokio's blocking pool, so the wait is not on a worker thread.
async fn system(
	name: &str,
) -> Result<reqwest::dns::Addrs, Box<dyn std::error::Error + Send + Sync>> {
	let addresses: Vec<SocketAddr> = tokio::net::lookup_host((name, 0)).await?.collect();
	Ok(Box::new(addresses.into_iter()))
}

/// The resolver for a choice, made inside a runtime because a DoH server named by a host has to
/// be looked up before it can be asked anything.
async fn build(choice: &Choice) -> Result<TokioResolver, String> {
	// DNS over HTTPS is TLS like any other, and this may be the first thing to want it.
	crate::tls::install();
	let provider = TokioRuntimeProvider::default();
	let mut builder = match config(choice).await {
		Some(config) => TokioResolver::builder_with_config(config, provider),
		None => TokioResolver::builder(provider).map_err(|error| error.to_string())?,
	};
	// A and AAAA in parallel, A ordered first. hickory orders AAAA first by default, and the
	// system's stack does too -- but it also knows whether this machine has a route to a v6
	// address at all and demotes them when it does not, which we cannot. On a machine with
	// half-working IPv6 that difference is happy eyeballs' wait on every connection, sixteen
	// times over for a split download. reqwest's own hickory client makes the same override.
	builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
	builder.build().map_err(|error| error.to_string())
}

/// The servers to ask, or None to read the machine's own. An address that will not parse is left
/// out rather than taken as a reason to fail: a settings field is typed into a character at a
/// time, and a resolver that refused to exist while it was half-typed would take the downloads
/// with it.
async fn config(choice: &Choice) -> Option<ResolverConfig> {
	let mut servers = Vec::new();
	for one in choice.text().split([',', ' ', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
		match choice.transport {
			Transport::Plain => match one.parse::<IpAddr>() {
				Ok(ip) => servers.push(NameServerConfig::udp_and_tcp(ip)),
				Err(_) => continue,
			},
			Transport::Https => match over_https(one).await {
				Some(server) => servers.push(server),
				None => continue,
			},
		}
	}
	if servers.is_empty() {
		// Nothing usable is the same as having named nothing, and the machine's own servers are
		// what that comes to -- except over HTTPS, where going back to port 53 would be answering
		// a question nobody asked. See `Choice::text`.
		return match choice.transport {
			Transport::Plain => None,
			Transport::Https => {
				let offered = over_https(Servers::Cloudflare.written(Transport::Https)).await?;
				Some(ResolverConfig::from_parts(None, Vec::new(), vec![offered]))
			}
		};
	}
	Some(ResolverConfig::from_parts(None, Vec::new(), servers))
}

/// A DoH server from its URL: its address from the table where we have it, from the URL itself
/// where the URL names one, and from the ordinary lookup otherwise. The host is kept whatever the
/// address came from, because it is the name the certificate is checked against -- an address
/// somebody rewrote on the way fails the handshake rather than answering the questions.
async fn over_https(url: &str) -> Option<NameServerConfig> {
	let rest = url.strip_prefix("https://")?;
	let (host, path) = rest.split_once('/').unwrap_or((rest, "dns-query"));
	let ip = match PINNED.iter().find(|(known, _)| *known == host) {
		Some((_, address)) => address.parse().ok()?,
		None => match host.parse::<IpAddr>() {
			Ok(ip) => ip,
			Err(_) => tokio::net::lookup_host((host, 443)).await.ok()?.next()?.ip(),
		},
	};
	Some(NameServerConfig::https(ip, host.into(), Some(format!("/{path}").into())))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn choice(transport: Transport, servers: Servers, written: &str) -> Choice {
		Choice {
			force_system: false,
			transport,
			force_https: false,
			servers,
			written: written.to_owned(),
			system_domains: String::new(),
		}
	}

	#[test]
	fn nothing_is_built_for_the_choice_to_let_the_system_do_it() {
		let forced = Choice { force_system: true, ..Choice::default() };
		assert!(resolver(&forced, false).is_none());
		assert!(resolver(&forced, true).is_none());
	}

	/// The default is our own stack asking the machine's own servers, which is a resolver to
	/// build and not a reason to skip one.
	#[test]
	fn the_default_is_our_stack_on_the_machines_servers() {
		let default = Choice::default();
		assert!(!default.force_system);
		assert_eq!(default.servers, Servers::System);
		assert_eq!(default.transport, Transport::Plain);
		assert!(default.text().is_empty(), "which is read from the machine, not written here");
		assert!(default.asks_system_for_missing(), "and a name it cannot find goes to the system");
	}

	/// Naming servers is saying this machine's are not to be trusted. A fallback that asked them
	/// anyway on a name the named servers do not have would hand back what was refused.
	#[test]
	fn only_the_machines_own_servers_earn_the_fallback() {
		assert!(!choice(Transport::Plain, Servers::Cloudflare, "").asks_system_for_missing());
		assert!(!choice(Transport::Plain, Servers::Custom, "9.9.9.9").asks_system_for_missing());
		assert!(!choice(Transport::Https, Servers::Cloudflare, "").asks_system_for_missing());
	}

	/// Choosing one of the offered servers fills the field beside it, so what is being asked is
	/// on screen. Custom is the one that reads the field instead of writing it.
	#[test]
	fn choosing_a_server_says_what_it_means() {
		assert_eq!(Servers::Cloudflare.written(Transport::Plain), "1.1.1.1");
		assert_eq!(Servers::Google.written(Transport::Plain), "8.8.8.8");
		assert_eq!(choice(Transport::Plain, Servers::Google, "ignored").text(), "8.8.8.8");
		assert_eq!(choice(Transport::Plain, Servers::Custom, " 9.9.9.9 ").text(), "9.9.9.9");
		for transport in [Transport::Plain, Transport::Https] {
			for servers in Servers::offered(transport) {
				assert!(!servers.name(transport).is_empty());
			}
		}
	}

	/// Every URL the two offered options can come to has its address written down, so the common
	/// way to turn DoH on never asks the network where its DoH server is.
	#[test]
	fn the_offered_doh_servers_need_no_lookup() {
		for offered in [Servers::Cloudflare, Servers::Google] {
			let url = offered.written(Transport::Https);
			let host = url.strip_prefix("https://").and_then(|r| r.split('/').next()).unwrap();
			assert!(PINNED.iter().any(|(known, _)| *known == host), "{host} has no address");
		}
		for (_, address) in PINNED {
			assert!(address.parse::<IpAddr>().is_ok(), "{address}");
		}
	}

	/// Asking for HTTPS and getting port 53 because a field was empty would be answering a
	/// question nobody asked, so what an unusable choice comes to over HTTPS is still a URL.
	#[test]
	fn an_empty_choice_over_https_is_still_over_https() {
		assert!(choice(Transport::Https, Servers::Custom, "").text().starts_with("https://"));
		assert!(choice(Transport::Https, Servers::System, "").text().starts_with("https://"));
		assert!(
			choice(Transport::Plain, Servers::Custom, "").text().is_empty(),
			"port 53 reads the machine"
		);
	}

	/// Two settings that come to the same thing share a resolver, and a settings change replaces
	/// it: one cache for the process, and never one that answers with the servers it used to name.
	#[test]
	fn the_resolver_is_kept_and_replaced_rather_than_rebuilt() {
		let one = choice(Transport::Plain, Servers::Custom, "1.1.1.1");
		let first = resolver(&one, false).expect("a resolver is built");
		let again = resolver(&one, false).expect("and kept");
		assert!(Arc::ptr_eq(&first.0, &again.0), "the same choice is the same resolver");
		let other = choice(Transport::Plain, Servers::Custom, "8.8.8.8");
		let changed = resolver(&other, false).expect("a changed choice is a new resolver");
		assert!(!Arc::ptr_eq(&first.0, &changed.0));
		// And a proxy carrying the requests is a different chain under the same choice, so it is
		// a different resolver even where nothing in the settings moved.
		let proxied = resolver(&one, true).expect("a resolver is built");
		assert!(!Arc::ptr_eq(&first.0, &proxied.0));
	}

	/// `.local` is built in and cannot be taken out, because where it goes is not a policy
	/// question: no unicast server has it. Nothing else is guessed at.
	#[test]
	fn only_local_is_built_in() {
		let bare = choice(Transport::Https, Servers::Cloudflare, "");
		assert!(bare.goes_to_the_system("nas.local"));
		assert!(bare.goes_to_the_system("NAS.LOCAL"), "a name is not case");
		assert!(bare.goes_to_the_system("local"));
		assert!(bare.goes_to_the_system("nas.local."), "a root dot is still the same name");
		for guess in ["host.lan", "host.home", "host.internal", "host.corp", "example.com"] {
			assert!(!bare.goes_to_the_system(guess), "{guess} is somebody's arrangement, not ours");
		}
		assert!(!bare.goes_to_the_system("notlocal"), "a suffix is a label, not a substring");
		assert!(!bare.goes_to_the_system("local.example.com"), "and it is the last one");
	}

	/// Everything else is written down, a domain standing for itself and all beneath it. A
	/// written domain never takes `.local` away.
	#[test]
	fn what_is_written_joins_it_and_never_replaces_it() {
		let mut written = choice(Transport::Https, Servers::Cloudflare, "");
		written.system_domains = " .corp.example.com, lan ".to_owned();
		assert!(written.goes_to_the_system("git.corp.example.com"));
		assert!(written.goes_to_the_system("corp.example.com"), "the domain itself as well");
		assert!(written.goes_to_the_system("host.lan"));
		assert!(written.goes_to_the_system("nas.local"), "and the built-in one is still there");
		assert!(!written.goes_to_the_system("example.com"), "not what it is a subdomain of");
		let names = written.no_proxy().expect("something to say");
		assert!(names.starts_with("local,"), "reqwest wants them comma-separated");
		assert!(names.contains("corp.example.com") && names.contains("lan"));
	}

	/// The chain under the first rung. Our own stack on port 53 sits there only under HTTPS --
	/// on port 53 it is already the first rung -- only while HTTPS is not forced, and only where
	/// no proxy carries the request.
	#[test]
	fn the_middle_rung_is_under_https_and_nowhere_else() {
		let held = |transport, force_https, proxied| Held {
			choice: Choice {
				force_system: false,
				transport,
				force_https,
				servers: Servers::Cloudflare,
				written: String::new(),
				system_domains: String::new(),
			},
			proxied,
			first: tokio::sync::OnceCell::new(),
			plain: tokio::sync::OnceCell::new(),
		};
		assert!(held(Transport::Https, false, false).has_plain_rung());
		assert!(!held(Transport::Https, true, false).has_plain_rung(), "forcing leaves no rung");
		assert!(!held(Transport::Https, false, true).has_plain_rung(), "proxied wants the system");
		assert!(!held(Transport::Plain, false, false).has_plain_rung(), "already the first rung");
	}

	/// An older file says `named` where this now says `custom`, and it means the same thing: the
	/// servers in the field. A value that will not read is a config.json thrown away whole.
	#[test]
	fn an_older_files_word_for_the_field_still_reads() {
		let old: Servers = serde_json::from_str("\"named\"").expect("named still reads");
		assert_eq!(old, Servers::Custom);
		let now: Servers = serde_json::from_str("\"custom\"").expect("and so does custom");
		assert_eq!(now, Servers::Custom);
	}
}

/// Against a real network, and ignored by default as the engine's own network tests are:
/// `cargo test -- --ignored` runs them. What they prove is the part no unit test can -- that a
/// resolver built from a choice actually answers, over port 53 and over HTTPS, and that a name
/// nobody has comes back as a failure rather than a wait.
#[cfg(test)]
mod network {
	use std::time::Duration;

	use reqwest::dns::Resolve;

	use super::*;

	async fn answer(choice: Choice, name: &str) -> Result<Vec<SocketAddr>, ()> {
		let resolver = resolver(&choice, false).expect("a resolver is built");
		let name: reqwest::dns::Name = name.parse().expect("a name");
		tokio::time::timeout(Duration::from_secs(30), resolver.resolve(name))
			.await
			.expect("answered within thirty seconds")
			.map(|addresses| addresses.collect())
			.map_err(|_| ())
	}

	/// The default: our own stack on whatever servers the machine is configured with.
	#[tokio::test]
	#[ignore = "needs the network"]
	async fn the_machines_own_servers_answer_through_our_stack() {
		let addresses = answer(Choice::default(), "one.one.one.one").await.expect("resolved");
		assert!(!addresses.is_empty());
	}

	/// Both offered servers, both ways of asking them. The HTTPS half is what proves the pinned
	/// addresses are right and that the certificate is checked against the host beside them.
	#[tokio::test]
	#[ignore = "needs the network"]
	async fn the_offered_servers_answer_over_53_and_over_https() {
		for transport in [Transport::Plain, Transport::Https] {
			for servers in [Servers::Cloudflare, Servers::Google] {
				let choice = Choice {
					force_system: false,
					transport,
					force_https: false,
					servers,
					written: String::new(),
					system_domains: String::new(),
				};
				let addresses = answer(choice, "example.com").await.expect("{servers:?} answered");
				assert!(!addresses.is_empty(), "{servers:?} over {transport:?}");
			}
		}
	}

	/// `.invalid` is reserved and nobody answers for it, so this is the fallback running its whole
	/// length -- our servers say no such name, the system is asked, and the answer is still no.
	#[tokio::test]
	#[ignore = "needs the network"]
	async fn a_name_nobody_has_fails_rather_than_hanging() {
		assert!(answer(Choice::default(), "no-such-host.rdm.invalid").await.is_err());
	}

	/// The routing rule runs before the chain, so a name on the list resolves even where the
	/// resolver in front of it could answer nothing at all. A DoH server on this machine's own
	/// port 443, where nothing is listening, is the clearest way to say that: forced, so there is
	/// no chain under it either, and the only way an answer comes back is the rule.
	#[tokio::test]
	#[ignore = "needs the network"]
	async fn a_name_the_machine_answers_for_never_reaches_the_resolver() {
		let nowhere = Choice {
			force_system: false,
			transport: Transport::Https,
			force_https: true,
			servers: Servers::Custom,
			written: "https://127.0.0.1/dns-query".to_owned(),
			system_domains: "example.com".to_owned(),
		};
		let found = answer(nowhere.clone(), "example.com").await.expect("routed to the system");
		assert!(!found.is_empty());
		// And a name that is not on the list gets that resolver and nothing after it.
		assert!(answer(nowhere, "example.org").await.is_err(), "no chain under a forced HTTPS");
	}

	/// The way out, on its own: the machine's stack, asked the way everything else on it asks.
	#[tokio::test]
	#[ignore = "needs the network"]
	async fn the_fallback_is_the_machines_own_stack() {
		let addresses: Vec<SocketAddr> =
			system("example.com").await.expect("the system answers").collect();
		assert!(!addresses.is_empty());
	}
}
