//! What the window knows about getting out: the proxy in use, and the look that finds one.
//!
//! The look runs once at launch and again whenever it is asked for, off the window's thread --
//! four connections at an eighth of a second each is half a second in the worst case, which is
//! nothing on a background thread and a visible stall on the main one. See src/proxy.rs.

use gpui::Context;

use crate::app::Rdm;
use crate::proxy::{self, Source};

impl Rdm {
	/// The address every download goes through, or None for straight out. This is what the engine
	/// is given, and what Settings reports.
	pub(crate) fn proxy_in_use(&self) -> Option<String> {
		match self.preferences.proxy_source {
			Source::Direct => None,
			Source::Fixed => self.preferences.proxy.clone().filter(|address| !address.trim().is_empty()),
			Source::Found => self.found_proxy.clone(),
		}
	}

	/// Looks for a proxy on this machine and keeps what it finds. Runs at launch and whenever the
	/// user asks; the answer is not written to the config, being a fact about the machine now
	/// rather than a choice the user made.
	pub(crate) fn look_for_proxy(&mut self, cx: &mut Context<Self>) {
		if self.looking_for_proxy {
			return;
		}
		self.looking_for_proxy = true;
		cx.notify();
		let receiver = self
			.engine
			.run(async move { tokio::task::spawn_blocking(proxy::discover).await.unwrap_or(None) });
		self.proxy_look = Some(receiver);
	}

	/// The look's answer, if it has come. Called from the window's tick.
	pub(crate) fn poll_proxy_look(&mut self, cx: &mut Context<Self>) {
		let Some(receiver) = &self.proxy_look else { return };
		match receiver.try_recv() {
			Ok(found) => {
				self.proxy_look = None;
				self.looking_for_proxy = false;
				self.found_proxy = found;
				cx.notify();
			}
			Err(std::sync::mpsc::TryRecvError::Disconnected) => {
				self.proxy_look = None;
				self.looking_for_proxy = false;
				cx.notify();
			}
			Err(std::sync::mpsc::TryRecvError::Empty) => {}
		}
	}

	/// Settings' row: where the proxy comes from. Looking again is part of choosing to look,
	/// since the answer is about the machine as it is now.
	pub(crate) fn set_proxy_source(&mut self, source: Source, cx: &mut Context<Self>) {
		self.preferences.proxy_source = source;
		self.save_config();
		if source == Source::Found {
			self.look_for_proxy(cx);
		}
		cx.notify();
	}

	/// Settings' row: what the window is read in. It takes effect at the next frame, which is
	/// what "immediately" looks like; nothing is restarted and nothing is rebuilt.
	pub(crate) fn set_language(&mut self, language: crate::i18n::Language, cx: &mut Context<Self>) {
		self.preferences.language = language;
		crate::i18n::use_language(language);
		self.save_config();
		cx.notify();
	}

	/// Settings' row: whether this build starts with the machine. What the system says after the
	/// attempt is what the switch shows, so a write that failed reads as off rather than as on.
	pub(crate) fn set_start_at_login(&mut self, on: bool, cx: &mut Context<Self>) {
		if let Err(error) = crate::startup::set(on) {
			eprintln!("could not change the login item: {error:#}");
		}
		self.preferences.start_at_login = crate::startup::enabled();
		self.save_config();
		cx.notify();
	}

	/// Settings' row: what this application calls itself to a server. Choosing one of the
	/// disguises fills the field beside it, so what is being sent is on screen rather than
	/// implied -- a disguise nobody can read is a disguise nobody can check.
	pub(crate) fn set_agent(&mut self, agent: crate::agent::Agent, cx: &mut Context<Self>) {
		self.preferences.agent = agent;
		if agent != crate::agent::Agent::Custom {
			let own = crate::engine::Settings::default().user_agent;
			self.preferences.user_agent = Some(agent.string(&own, ""));
		}
		self.save_config();
		self.show_setting("settings.label.user_agent", cx);
		cx.notify();
	}

	/// Settings' row: hand the whole business back to the machine. On, nothing of ours is built
	/// and reqwest resolves the way everything else on this machine does -- the way out if
	/// resolving here is ever the problem. See src/dns.rs.
	pub(crate) fn set_dns_force_system(&mut self, on: bool, cx: &mut Context<Self>) {
		self.preferences.dns_force_system = on;
		self.save_config();
		cx.notify();
	}

	/// Settings' row: whether the questions go over HTTPS. Turning it changes what a server is --
	/// an address for port 53, a URL for HTTPS -- so what was written for the old transport is not
	/// an answer for the new one, and the choice goes back to the first server offered.
	pub(crate) fn set_dns_https(&mut self, on: bool, cx: &mut Context<Self>) {
		let transport = crate::dns::Transport::of(on);
		self.preferences.dns_transport = transport;
		self.set_dns_servers(crate::dns::Servers::offered(transport)[0], cx);
	}

	/// Settings' row: which servers. Choosing one of the offered fills the field beside it, so
	/// what is being asked is on screen rather than implied -- the same reason a chosen user agent
	/// fills its field. Custom leaves the field alone, the field being the choice.
	pub(crate) fn set_dns_servers(&mut self, servers: crate::dns::Servers, cx: &mut Context<Self>) {
		self.preferences.dns_servers = servers;
		if servers != crate::dns::Servers::Custom {
			self.preferences.dns_servers_written =
				servers.written(self.preferences.dns_transport).to_owned();
		}
		self.save_config();
		self.show_setting("settings.label.name_servers", cx);
		cx.notify();
	}

	/// What Settings says about the proxy: the address in use and where it came from, or why
	/// there is none.
	pub(crate) fn proxy_status(&self) -> String {
		match self.preferences.proxy_source {
			Source::Direct => crate::i18n::t("proxy.status.direct").to_owned(),
			Source::Fixed => match self.proxy_in_use() {
				Some(address) => address,
				None => crate::i18n::t("proxy.status.unset").to_owned(),
			},
			Source::Found if self.looking_for_proxy => crate::i18n::t("proxy.status.looking").to_owned(),
			Source::Found => match &self.found_proxy {
				Some(address) => format!("Found {address}"),
				None => crate::i18n::t("proxy.status.nothing").to_owned(),
			},
		}
	}
}
