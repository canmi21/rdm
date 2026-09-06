//! What the window knows about getting out: the proxy in use, and the look that finds one.
//!
//! The look runs once at launch and again whenever it is asked for, off the window's thread --
//! seven connections at a tenth of a second each is most of a second in the worst case, which is
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
			Source::Fixed => {
				self.preferences.proxy.clone().filter(|address| !address.trim().is_empty())
			}
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
		let receiver = self.engine.run(async move {
			tokio::task::spawn_blocking(proxy::discover).await.unwrap_or(None)
		});
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

	/// Settings' rows: who is asked, how, and by what. Each is written as it is chosen, and the
	/// engine reads them when it builds the next client.
	pub(crate) fn set_dns_servers(&mut self, servers: crate::dns::Servers, cx: &mut Context<Self>) {
		self.preferences.dns_servers = servers;
		self.save_config();
		cx.notify();
	}

	pub(crate) fn set_dns_transport(
		&mut self,
		transport: crate::dns::Transport,
		cx: &mut Context<Self>,
	) {
		self.preferences.dns_transport = transport;
		// The field holds addresses for one and URLs for the other; what was written for the old
		// transport is not an answer for the new one, so it goes back to the offered pair.
		self.preferences.dns_servers_written.clear();
		self.save_config();
		cx.notify();
	}

	pub(crate) fn set_dns_stack(&mut self, stack: crate::dns::Stack, cx: &mut Context<Self>) {
		self.preferences.dns_stack = stack;
		self.save_config();
		cx.notify();
	}

	/// What Settings says about the proxy: the address in use and where it came from, or why
	/// there is none.
	pub(crate) fn proxy_status(&self) -> String {
		match self.preferences.proxy_source {
			Source::Direct => "Straight out".to_owned(),
			Source::Fixed => match self.proxy_in_use() {
				Some(address) => address,
				None => "No address set".to_owned(),
			},
			Source::Found if self.looking_for_proxy => "Looking...".to_owned(),
			Source::Found => match &self.found_proxy {
				Some(address) => format!("Found {address}"),
				None => "Nothing found; straight out".to_owned(),
			},
		}
	}
}
