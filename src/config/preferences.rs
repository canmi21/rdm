//! The switches a user sets, the half of `config.json` beside the categories: how the window
//! looks and speaks, what it says and where, and everything a download is told, carried to the
//! engine as its settings. See spec/state.md.

use serde::{Deserialize, Serialize};

use crate::download::Folders;
use crate::engine::HttpVersion;
use crate::notify::{Occasion, Style};
use crate::update::{Channel, Policy};

/// The switches a user sets, as the file spells them. Every field has a default, so a file that
/// predates a switch reads as if the switch had been left alone.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
	/// The sidebar's category icons keep their hues always rather than only when chosen or
	/// hovered; the state filters above them are not covered. On to start with; the window's
	/// inactive grey overrides it either way.
	#[serde(default = "yes")]
	pub colorful_categories: bool,
	/// The window goes grey while it is not in front. On to start with; off, it keeps its
	/// colors whether it is in front or not.
	#[serde(default = "yes")]
	pub dim_inactive: bool,
	/// Which channel's builds the update check follows. Nightly, the only one there is.
	#[serde(default)]
	pub update_channel: Channel,
	/// The check runs on its own every few minutes; off, only Check now asks.
	#[serde(default = "yes")]
	pub check_updates: bool,
	/// A build the check finds is acted on without asking, as `update_policy` says.
	#[serde(default = "yes")]
	pub auto_update: bool,
	#[serde(default)]
	pub update_policy: Policy,
	/// What shows a file where it lives, on the system that has no one answer: a command given
	/// the file's path. Empty is `xdg-open` on the folder, which every desktop answers even
	/// though none of them selects the file. macOS and Windows ignore it -- the Finder and File
	/// Explorer are what those systems mean by this, and there is nothing to choose.
	#[serde(default)]
	pub file_manager: String,
	/// What the download folder's own directories become in the list. See `Folders`.
	#[serde(default)]
	pub folders: Folders,
	/// The download folder's junk is kept out of the lists: what the system leaves behind, what
	/// an editor writes beside an open file, the pointers a browser saves instead of a file. On
	/// to start with. A torrent is the exception it makes: filed rather than dropped, so it has a
	/// row under Torrents and nowhere else. See `download::junk`.
	#[serde(default = "yes")]
	pub hide_junk: bool,
	/// Where each kind of notice is said, one field an occasion so one can be turned down without
	/// touching the others. A finished download opens the dialog, which is the one notice with
	/// something to do next -- open the file, show it where it lives, come back to the window --
	/// and the reason the dialog exists. A failed one speaks to the system, since the point of it
	/// is to reach somebody who has looked away and there is nothing to do but look. The queue
	/// emptying says nothing to start with, or the last download of a batch would say it twice;
	/// a newer build shows the card in the corner. See src/notify.rs.
	#[serde(default = "in_a_window")]
	pub notice_finished: Style,
	#[serde(default = "to_the_system")]
	pub notice_failed: Style,
	#[serde(default)]
	pub notice_queue: Style,
	#[serde(default = "in_the_window")]
	pub notice_update: Style,
	/// Bytes per second across every download; None is unlimited, the default.
	#[serde(default)]
	pub speed_limit: Option<u64>,
	/// What Add Task offers first: None is the engine's own judgement, Some a fixed count.
	#[serde(default)]
	pub connections: Option<u16>,
	/// The two ends of the speed limit slider New Task offers, in MB/s: None is 1 and 100. Past the
	/// high end the slider is no limit. See spec/ui.md.
	#[serde(default)]
	pub limit_slider_from: Option<u32>,
	#[serde(default)]
	pub limit_slider_to: Option<u32>,
	/// How many downloads run at once; the rest wait their turn.
	#[serde(default = "three")]
	pub max_active: usize,
	/// Which running download goes back to the queue when a waiting one is started now and every
	/// place is taken. See spec/engine.md, "The queue can be reordered".
	#[serde(default)]
	pub bump: crate::engine::Bump,
	/// The engine's defaults for every new download, each None where the engine's own value
	/// stands. See spec/engine.md for what each does.
	#[serde(default)]
	pub min_segment: Option<u64>,
	#[serde(default)]
	pub connect_timeout: Option<u64>,
	#[serde(default)]
	pub idle_timeout: Option<u64>,
	#[serde(default)]
	pub retries: Option<u32>,
	#[serde(default)]
	pub retry_wait: Option<u64>,
	#[serde(default)]
	pub max_size: Option<u64>,
	#[serde(default = "auto_http")]
	pub http: HttpVersion,
	#[serde(default)]
	pub user_agent: Option<String>,
	#[serde(default)]
	pub headers: Vec<(String, String)>,
	#[serde(default)]
	pub proxy: Option<String>,
	/// Where the proxy comes from: nothing, whatever is running on this machine, or the address
	/// above. Looking is the default -- the address a proxy listens on is its choice and not the
	/// user's, and the machine can be asked. See src/proxy.rs.
	#[serde(default)]
	pub proxy_source: crate::proxy::Source,
	/// How names are resolved. This application does it itself by default, the same way on every
	/// platform; these are the three things that can change that. See src/dns.rs.
	/// What the window is read in, as one of the three there are -- never "follow the machine".
	/// The machine is asked at the seed and at the migration off version 1, and the answer is
	/// written here; after that this field is the whole of it. See src/i18n.rs.
	#[serde(default)]
	pub language: crate::i18n::Language,
	/// Starts with the machine. Off to begin with: an application that put itself in the login
	/// items without being asked would be one of those applications. The switch is what the user
	/// last chose; whether the entry is really there is asked of the system. See src/startup.rs.
	#[serde(default)]
	pub start_at_login: bool,
	/// What this application calls itself to a server: itself, one of the two disguises offered,
	/// or whatever is written in the field. See src/agent.rs.
	#[serde(default)]
	pub agent: crate::agent::Agent,
	/// Off. On, nothing of ours is built and the machine resolves the way it does for everything
	/// else on it -- the way out if resolving here is ever the problem.
	#[serde(default)]
	pub dns_force_system: bool,
	#[serde(default)]
	pub dns_transport: crate::dns::Transport,
	/// HTTPS or nothing: no rung under it, and a name it cannot resolve does not get resolved.
	/// Off, and only there to be turned on beside the switch above.
	#[serde(default)]
	pub dns_force_https: bool,
	#[serde(default)]
	pub dns_servers: crate::dns::Servers,
	/// The servers as the user wrote them, which choosing one of the offered servers fills in.
	#[serde(default)]
	pub dns_servers_written: String,
	/// Domains the machine's own stack resolves whatever the rest of this says, and which never
	/// go through a proxy either. `.local` is built in beside whatever is written here and cannot
	/// be taken out; every other internal domain is the user's to name. See src/dns.rs.
	#[serde(default)]
	pub dns_system_domains: String,
	#[serde(default)]
	pub max_redirects: Option<usize>,
	#[serde(default = "yes")]
	pub preallocate: bool,
}

fn to_the_system() -> Style {
	Style::System
}

fn in_the_window() -> Style {
	Style::InApp
}

fn in_a_window() -> Style {
	Style::Window
}

fn three() -> usize {
	3
}

fn auto_http() -> HttpVersion {
	HttpVersion::Auto
}

impl Preferences {
	/// Where this occasion's notice is said. One accessor rather than four call sites reaching
	/// for four fields, so a new occasion is a field, an arm and a row and nothing else.
	pub fn notice(&self, occasion: Occasion) -> Style {
		match occasion {
			Occasion::Finished => self.notice_finished,
			Occasion::Failed => self.notice_failed,
			Occasion::Queue => self.notice_queue,
			Occasion::Update => self.notice_update,
		}
	}

	pub fn set_notice(&mut self, occasion: Occasion, style: Style) {
		*match occasion {
			Occasion::Finished => &mut self.notice_finished,
			Occasion::Failed => &mut self.notice_failed,
			Occasion::Queue => &mut self.notice_queue,
			Occasion::Update => &mut self.notice_update,
		} = style;
	}

	/// The engine's settings for a new download: its own defaults, with what the user set
	/// written over them. `found` is the proxy the last look turned up, which only matters when
	/// the source is what is running on this machine.
	/// The limit slider's low and high ends, in MB/s.
	pub fn limit_slider(&self) -> (f64, f64) {
		(f64::from(self.limit_slider_from.unwrap_or(1)), f64::from(self.limit_slider_to.unwrap_or(100)))
	}

	pub fn engine_settings(&self, found: Option<&str>) -> crate::engine::Settings {
		let mut settings = crate::engine::Settings::default();
		if let Some(n) = self.min_segment {
			settings.min_segment = n;
		}
		if let Some(s) = self.connect_timeout {
			settings.connect_timeout = std::time::Duration::from_secs(s);
		}
		if let Some(s) = self.idle_timeout {
			settings.idle_timeout = std::time::Duration::from_secs(s);
		}
		if let Some(n) = self.retries {
			settings.retries = n;
		}
		if let Some(s) = self.retry_wait {
			settings.retry_wait = std::time::Duration::from_secs(s);
		}
		settings.max_size = self.max_size;
		settings.http = self.http;
		// The field is what `Something else` sends and nothing else does; the rest is a table.
		settings.user_agent =
			self.agent.string(&settings.user_agent, self.user_agent.as_deref().unwrap_or_default());
		settings.headers = self.headers.clone();
		// What the engine is given: the address typed, whatever was found, or nothing. `found` is
		// what the last look came to and is None until it has looked. See src/app/network.rs.
		// The four of them are one thing to the engine: what a choice comes to is src/dns.rs's to
		// work out, and one resolver is built for it and shared by every download that asks.
		settings.dns = crate::dns::Choice {
			force_system: self.dns_force_system,
			transport: self.dns_transport,
			force_https: self.dns_force_https,
			servers: self.dns_servers,
			written: self.dns_servers_written.clone(),
			system_domains: self.dns_system_domains.clone(),
		};
		settings.proxy = match self.proxy_source {
			crate::proxy::Source::Direct => None,
			crate::proxy::Source::Fixed => self.proxy.clone().filter(|a| !a.trim().is_empty()),
			crate::proxy::Source::Found => found.map(str::to_owned),
		};
		if let Some(n) = self.max_redirects {
			settings.max_redirects = n;
		}
		settings.preallocate = self.preallocate;
		settings
	}
}

fn yes() -> bool {
	true
}

impl Default for Preferences {
	fn default() -> Self {
		Preferences {
			colorful_categories: true,
			dim_inactive: true,
			file_manager: String::new(),
			folders: Folders::default(),
			hide_junk: true,
			notice_finished: Style::Window,
			notice_failed: Style::System,
			notice_queue: Style::Silent,
			notice_update: Style::InApp,
			update_channel: Channel::default(),
			check_updates: true,
			auto_update: true,
			update_policy: Policy::default(),
			speed_limit: None,
			connections: None,
			limit_slider_from: None,
			limit_slider_to: None,
			max_active: 3,
			bump: crate::engine::Bump::default(),
			min_segment: None,
			connect_timeout: None,
			idle_timeout: None,
			retries: None,
			retry_wait: None,
			max_size: None,
			http: HttpVersion::Auto,
			user_agent: None,
			headers: Vec::new(),
			proxy: None,
			proxy_source: crate::proxy::Source::default(),
			// Asked of the machine once, at the only moment there is nothing written down to
			// read instead. See src/i18n.rs.
			language: crate::i18n::Language::detected(),
			start_at_login: false,
			agent: crate::agent::Agent::default(),
			dns_force_system: false,
			dns_transport: crate::dns::Transport::default(),
			dns_force_https: false,
			dns_servers: crate::dns::Servers::default(),
			dns_servers_written: String::new(),
			dns_system_domains: String::new(),
			max_redirects: None,
			preallocate: true,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::config::{Config, parse};

	#[test]
	fn the_engine_settings_are_its_own_with_the_user_written_over() {
		let plain = Preferences::default().engine_settings(None);
		assert_eq!(plain, crate::engine::Settings::default());
		let set = Preferences {
			retries: Some(9),
			proxy: Some("socks5://h:1080".into()),
			http: HttpVersion::Http1,
			headers: vec![("X-A".into(), "b".into())],
			..Preferences::default()
		};
		let settings = set.engine_settings(None);
		assert_eq!((settings.retries, settings.http), (9, HttpVersion::Http1));
		// An address alone is not enough now: the source says whether to use it, and what is
		// running on the machine is what a fresh preferences file asks for.
		assert_eq!(settings.proxy, None, "the address is set but the source is not This address");
		let typed = Preferences { proxy_source: crate::proxy::Source::Fixed, ..set.clone() };
		assert_eq!(typed.engine_settings(None).proxy.as_deref(), Some("socks5://h:1080"));
		let looked = Preferences { proxy_source: crate::proxy::Source::Found, ..set.clone() };
		assert_eq!(
			looked.engine_settings(Some("http://127.0.0.1:7890")).proxy.as_deref(),
			Some("http://127.0.0.1:7890"),
			"and what was found is used in its place"
		);
		assert_eq!(looked.engine_settings(None).proxy, None, "nothing found is straight out");
		assert_eq!(settings.headers.len(), 1);
		assert_eq!(settings.user_agent, crate::engine::Settings::default().user_agent);
		let text = serde_json::to_string(&Config::from_parts(&[], &set)).unwrap();
		assert_eq!(parse(&text).unwrap().settings, set, "round trip");
	}
}
