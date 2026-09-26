//! What each field of Settings shows, and what a value typed into one changes.

use super::*;

impl Rdm {
	/// What a field shows for its setting now: empty where the engine's own value stands.
	pub(super) fn setting_text(&self, key: &str) -> String {
		use crate::download::{format_bytes, format_rate};
		let p = &self.preferences;
		let size = |n: Option<u64>| n.map(format_bytes).unwrap_or_default();
		let number = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_default();
		match key {
			"settings.label.concurrent_downloads" => p.max_active.to_string(),
			"settings.label.speed_limit" => {
				p.speed_limit.map(|l| format_rate(Some(l))).unwrap_or_default()
			}
			"settings.label.connections" => number(p.connections.map(u64::from)),
			"settings.label.limit_slider_from" => number(p.limit_slider_from.map(u64::from)),
			"settings.label.limit_slider_to" => number(p.limit_slider_to.map(u64::from)),
			"settings.label.smallest_segment" => size(p.min_segment),
			"settings.label.connect_timeout" => number(p.connect_timeout),
			"settings.label.idle_timeout" => number(p.idle_timeout),
			"settings.label.retries" => number(p.retries.map(u64::from)),
			"settings.label.retry_wait" => number(p.retry_wait),
			"settings.label.size_limit" => size(p.max_size),
			"settings.label.user_agent" => p.user_agent.clone().unwrap_or_default(),
			"settings.label.proxy" => p.proxy.clone().unwrap_or_default(),
			"settings.label.name_servers" => p.dns_servers_written.clone(),
			"settings.label.system_domains" => p.dns_system_domains.clone(),
			"settings.label.headers" => {
				p.headers.iter().map(|(n, v)| format!("{n}: {v}")).collect::<Vec<_>>().join("; ")
			}
			"settings.label.redirects" => number(p.max_redirects.map(|n| n as u64)),
			_ => String::new(),
		}
	}

	/// Reads a setting back into the field that shows it. The rows where choosing something fills
	/// a field beside it -- a server, a user agent -- change the setting and not the field, and
	/// what is on screen has to follow or the field says what was there before.
	pub(crate) fn show_setting(&mut self, key: &'static str, cx: &mut Context<Self>) {
		let shown = self.setting_text(key);
		if let Some(sheet) = &self.settings
			&& let Some(field) = sheet.fields.get(key)
		{
			field.update(cx, |field, cx| field.set_content(&shown, cx));
		}
	}

	/// A field's text, applied: parsed for its setting, kept, handed to the engine where the
	/// engine takes it live, and read back into the field as kept; or refused under its row.
	pub(crate) fn apply_setting(&mut self, key: &'static str, text: &str, cx: &mut Context<Self>) {
		use crate::download::{parse_number, parse_rate, parse_size};
		let text = text.trim();
		let result: Result<(), String> = (|| {
			match key {
				"settings.label.concurrent_downloads" => {
					let n = parse_number(text)?.unwrap_or(3).clamp(1, 64) as usize;
					self.preferences.max_active = n;
					self.engine.set_max_active(n);
				}
				"settings.label.speed_limit" => {
					let limit = parse_rate(text)?;
					self.preferences.speed_limit = limit;
					self.engine.set_speed_limit(limit);
				}
				"settings.label.connections" => {
					self.preferences.connections = crate::ui::add_dialog::parse_connections(text)?;
				}
				"settings.label.limit_slider_from" | "settings.label.limit_slider_to" => {
					let value = match parse_number(text)? {
						None => None,
						Some(n) if (1..=100_000).contains(&n) => Some(n as u32),
						Some(_) => return Err("The slider is counted in whole MB/s, from 1.".to_owned()),
					};
					let (mut from, mut to) =
						(self.preferences.limit_slider_from, self.preferences.limit_slider_to);
					if key.ends_with("from") {
						from = value;
					} else {
						to = value;
					}
					if from.unwrap_or(1) >= to.unwrap_or(100) {
						return Err("The slider has to start below where it ends.".to_owned());
					}
					self.preferences.limit_slider_from = from;
					self.preferences.limit_slider_to = to;
				}
				"settings.label.smallest_segment" => self.preferences.min_segment = parse_size(text)?,
				"settings.label.connect_timeout" => self.preferences.connect_timeout = parse_number(text)?,
				"settings.label.idle_timeout" => self.preferences.idle_timeout = parse_number(text)?,
				"settings.label.retries" => {
					self.preferences.retries = parse_number(text)?.map(|n| n as u32)
				}
				"settings.label.retry_wait" => self.preferences.retry_wait = parse_number(text)?,
				"settings.label.size_limit" => self.preferences.max_size = parse_size(text)?,
				"settings.label.user_agent" => {
					self.preferences.user_agent = (!text.is_empty()).then(|| text.to_owned())
				}
				"settings.label.proxy" => {
					// `socks5h://` is taken as well as `socks5://` and means the same thing here:
					// either way the proxy is the one that resolves. See src/proxy.rs.
					let schemed =
						["http://", "https://", "socks5://", "socks5h://"].iter().any(|s| text.starts_with(s));
					if !text.is_empty() && !schemed {
						return Err("A proxy starts with http://, https:// or socks5://.".to_owned());
					}
					self.preferences.proxy = (!text.is_empty()).then(|| text.to_owned());
				}
				"settings.label.name_servers" => {
					// Only what the transport in use can be given: an address where the question
					// goes over 53, a URL where it goes over HTTPS. Writing one where the other
					// belongs is the mistake worth catching, since it fails silently otherwise.
					let https = self.preferences.dns_transport == crate::dns::Transport::Https;
					let parts: Vec<&str> =
						text.split([',', ' ', '\n']).map(str::trim).filter(|p| !p.is_empty()).collect();
					for part in &parts {
						if https && !part.starts_with("https://") {
							return Err("Over HTTPS, a server is an https:// URL.".to_owned());
						}
						if !https && part.parse::<std::net::IpAddr>().is_err() {
							return Err("On port 53, a server is an address like 1.1.1.1.".to_owned());
						}
					}
					self.preferences.dns_servers_written = text.to_owned();
					// Writing servers is choosing them: leaving the row above on one of the
					// offered pair while the field says otherwise would show one thing and ask
					// another.
					self.preferences.dns_servers = crate::dns::Servers::Custom;
				}
				"settings.label.system_domains" => {
					// A domain and nothing else: an address here would look like it worked and
					// would never match, since what is compared is the name being resolved.
					for part in text.split([',', ' ', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
						if part.trim_start_matches('.').parse::<std::net::IpAddr>().is_ok() {
							return Err("A domain, not an address: names are what is matched.".to_owned());
						}
					}
					self.preferences.dns_system_domains = text.to_owned();
				}
				"settings.label.headers" => {
					let mut headers = Vec::new();
					for part in text.split(';').map(str::trim).filter(|p| !p.is_empty()) {
						let Some((name, value)) = part.split_once(':') else {
							return Err("A header is Name: value.".to_owned());
						};
						headers.push((name.trim().to_owned(), value.trim().to_owned()));
					}
					self.preferences.headers = headers;
				}
				"settings.label.redirects" => {
					self.preferences.max_redirects = parse_number(text)?.map(|n| n as usize)
				}
				_ => {}
			}
			Ok(())
		})();
		match result {
			Ok(()) => {
				self.save_config();
				let shown = self.setting_text(key);
				if let Some(sheet) = &mut self.settings {
					sheet.complaint = None;
					if let Some(field) = sheet.fields.get(key) {
						field.update(cx, |field, cx| field.set_content(&shown, cx));
					}
				}
			}
			Err(message) => {
				if let Some(sheet) = &mut self.settings {
					sheet.complaint = Some((key, message));
				}
			}
		}
		cx.notify();
	}
}
