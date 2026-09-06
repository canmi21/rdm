//! Settings are a sheet inside the main window, like Add Task; only a download gets a window of
//! its own, because a download is a thing to keep beside the list while it moves. The sheet is
//! shaped for the many settings to come: a rail of sections on the left with a search field
//! over it, and the chosen section's rows on the right. A search cuts across every section and
//! shows what matches under its section's name, so a setting is found without knowing where
//! it was filed. See spec/ui.md.

use gpui::{Context, Entity, IntoElement, Role, SharedString, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::identity;
use crate::ui::icon::{Icon, hover_icon};
use crate::ui::text_input::TextInput;
use crate::ui::{LeavesFocus, backdrop, icon_button};
use std::collections::HashMap;

use crate::engine::HttpVersion;
use crate::download::Folders;
use crate::notify::{Occasion, Style};
use crate::update::Policy;

// TODO: every value row here is a label until there is a setting behind it and a store to keep it
// in; the folder is the one the engine writes to, the rest are the engine's defaults, read only.

/// The sheet while it is up: which section is open, and the field that searches all of them.
pub struct SettingsSheet {
	pub section: Section,
	pub search: Entity<TextInput>,
	/// The fields, by their row's label: each applies on Enter and reads back what was kept.
	pub fields: HashMap<&'static str, Entity<TextInput>>,
	/// What the last field said no to, under the row.
	pub complaint: Option<(&'static str, String)>,
}

/// Every field there is: its row's label, its placeholder, and a word on what it takes.
const FIELDS: [(&str, &str, &str); 14] = [
	("settings.label.concurrent_downloads", "3", "How many run at once; the rest wait"),
	("settings.label.speed_limit", "Off", "KB/s, or with m or g; empty for none"),
	("settings.label.connections", "Auto", "Auto, or a number up to 256, offered first at Add Task"),
	("settings.label.smallest_segment", "1m", "A file below this is never split; bytes, or with k, m or g"),
	("settings.label.connect_timeout", "30", "Seconds to establish a connection"),
	("settings.label.idle_timeout", "60", "Seconds without a byte before a connection is dropped and retried"),
	("settings.label.retries", "5", "Times a failing connection is tried again"),
	("settings.label.retry_wait", "1", "Seconds before the first retry, doubling each time"),
	("settings.label.size_limit", "Off", "A file the server declares larger is refused; empty for none"),
	("settings.label.user_agent", "rdm/version", "Sent with every request; empty for the engine's own"),
	(
		"settings.label.proxy",
		"Address",
		"http://, https:// or socks5://, credentials in the address",
	),
	(
		"settings.label.name_servers",
		"Cloudflare, Google",
		"Addresses for port 53, https:// URLs for HTTPS; empty for the offered pair",
	),
	("settings.label.headers", "", "Name: value, several apart by semicolons"),
	("settings.label.redirects", "10", "How many a request follows"),
];

/// The sections down the rail, in their order. A setting belongs to exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
	General,
	Transfers,
	Folder,
	Notifications,
	Network,
	Updates,
	Appearance,
	About,
}

impl Section {
	pub const ALL: [Section; 8] = [
		Section::General,
		Section::Transfers,
		Section::Folder,
		Section::Notifications,
		Section::Network,
		Section::Updates,
		Section::Appearance,
		Section::About,
	];

	pub fn name(self) -> &'static str {
		match self {
			Section::General => crate::i18n::t("settings.section.general"),
			Section::Transfers => crate::i18n::t("settings.section.transfers"),
			Section::Folder => crate::i18n::t("settings.section.folder"),
			Section::Notifications => crate::i18n::t("settings.section.notifications"),
			Section::Network => crate::i18n::t("settings.section.network"),
			Section::Updates => crate::i18n::t("settings.section.updates"),
			Section::Appearance => crate::i18n::t("settings.section.appearance"),
			Section::About => crate::i18n::t("settings.section.about"),
		}
	}

	fn icon(self) -> Icon {
		match self {
			Section::General => Icon::SlidersHorizontal,
			Section::Transfers => Icon::Download,
			Section::Folder => Icon::FolderOpen,
			Section::Notifications => Icon::Bell,
			Section::Network => Icon::Globe,
			Section::Updates => Icon::Download,
			Section::Appearance => Icon::Palette,
			Section::About => Icon::Info,
		}
	}
}

/// A Choice row takes a plain function and so cannot carry the occasion with it; each occasion
/// names its own instead, which is four lines against changing every other row in the sheet.
fn notice_setter(occasion: Occasion) -> fn(&mut Rdm, usize, &mut Context<Rdm>) {
	match occasion {
		Occasion::Finished => |this, at, cx| this.set_notice(Occasion::Finished, Style::ALL[at], cx),
		Occasion::Failed => |this, at, cx| this.set_notice(Occasion::Failed, Style::ALL[at], cx),
		Occasion::Queue => |this, at, cx| this.set_notice(Occasion::Queue, Style::ALL[at], cx),
		Occasion::Update => |this, at, cx| this.set_notice(Occasion::Update, Style::ALL[at], cx),
	}
}

/// What a setting's row shows on its right.
enum Control {
	/// A value that cannot be changed here yet.
	Value(String),
	/// A switch, and what flipping it does.
	Switch { on: bool, set: fn(&mut Rdm, bool, &mut Context<Rdm>) },
	/// A word that does something when pressed, with a note on how it last went.
	Action { word: &'static str, note: String, run: fn(&mut Rdm, &mut Context<Rdm>) },
	/// A field, applied on Enter, with a word on what it takes.
	Field { input: Entity<TextInput>, note: &'static str },
	/// One of a few words, the chosen one lit.
	Choice { options: Vec<&'static str>, chosen: usize, set: fn(&mut Rdm, usize, &mut Context<Rdm>) },
}

impl Row {
	/// Puts a row under a heading. The rows written out in full name their own; this is for the
	/// ones built by a helper, which cannot know where they are going.
	fn under(mut self, group: &'static str) -> Row {
		self.group = group;
		self
	}
}

struct Row {
	section: Section,
	/// The heading this row sits under within its section, empty for the rows that open it. A
	/// section of a dozen rows in one run is a list to read rather than a page to use; the
	/// headings are what make it three short lists.
	group: &'static str,
	label: &'static str,
	/// One line under the label saying what the setting does, empty where the label already says
	/// it. Most labels do not: `Auto update` names itself and says nothing about what happens,
	/// and what happens used to be a second row away.
	note: &'static str,
	control: Control,
}

impl Rdm {
	pub(crate) fn settings_open(&self) -> bool {
		self.settings.is_some()
	}

	/// Opens on General. The search field is not given the keyboard: Settings is a place to look
	/// around, not a form to fill in, so the keyboard stays with the window until the field is
	/// pressed. See spec/ui.md.
	pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
		if self.settings.is_none() {
			let rdm = cx.entity();
			let search = cx.new(|cx| {
				TextInput::new("Search settings", cx)
					.with_leading(Icon::Search)
					.on_cancel(move |_, cx| rdm.update(cx, |this, cx| this.close_settings(cx)))
			});
			let mut fields = HashMap::new();
			for (key, placeholder, _) in FIELDS {
				let rdm = cx.entity();
				let shown = self.setting_text(key);
				let field = cx.new(|cx| {
					let mut field = TextInput::new(placeholder, cx).on_confirm(move |text, _, cx| {
						let text = text.to_owned();
						rdm.update(cx, |this, cx| this.apply_setting(key, &text, cx))
					});
					if !shown.is_empty() {
						field.set_content(&shown, cx);
					}
					field
				});
				fields.insert(key, field);
			}
			self.settings =
				Some(SettingsSheet { section: Section::General, search, fields, complaint: None });
		}
		cx.notify();
	}

	pub(crate) fn close_settings(&mut self, cx: &mut Context<Self>) {
		self.settings = None;
		cx.notify();
	}

	/// The control socket's verb, built where the socket is. See spec/workflow.md.
	#[cfg(all(debug_assertions, unix))]
	pub(crate) fn toggle_settings(&mut self, open: bool, cx: &mut Context<Self>) {
		if open { self.open_settings(cx) } else { self.close_settings(cx) }
	}

	pub(crate) fn set_settings_section(&mut self, section: Section, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings {
			sheet.section = section;
			cx.notify();
		}
	}

	/// What a field shows for its setting now: empty where the engine's own value stands.
	fn setting_text(&self, key: &str) -> String {
		use crate::download::{format_bytes, format_rate};
		let p = &self.preferences;
		let size = |n: Option<u64>| n.map(format_bytes).unwrap_or_default();
		let number = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_default();
		match key {
			"settings.label.concurrent_downloads" => p.max_active.to_string(),
			"settings.label.speed_limit" => p.speed_limit.map(|l| format_rate(Some(l))).unwrap_or_default(),
			"settings.label.connections" => number(p.connections.map(u64::from)),
			"settings.label.smallest_segment" => size(p.min_segment),
			"settings.label.connect_timeout" => number(p.connect_timeout),
			"settings.label.idle_timeout" => number(p.idle_timeout),
			"settings.label.retries" => number(p.retries.map(u64::from)),
			"settings.label.retry_wait" => number(p.retry_wait),
			"settings.label.size_limit" => size(p.max_size),
			"settings.label.user_agent" => p.user_agent.clone().unwrap_or_default(),
			"settings.label.proxy" => p.proxy.clone().unwrap_or_default(),
			"settings.label.name_servers" => p.dns_servers_written.clone(),
			"settings.label.headers" => {
				p.headers.iter().map(|(n, v)| format!("{n}: {v}")).collect::<Vec<_>>().join("; ")
			}
			"settings.label.redirects" => number(p.max_redirects.map(|n| n as u64)),
			_ => String::new(),
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
					self.preferences.connections = if text.is_empty() || text.eq_ignore_ascii_case("auto") {
						None
					} else {
						Some(crate::ui::add_dialog::parse_count(text)?)
					};
				}
				"settings.label.smallest_segment" => self.preferences.min_segment = parse_size(text)?,
				"settings.label.connect_timeout" => self.preferences.connect_timeout = parse_number(text)?,
				"settings.label.idle_timeout" => self.preferences.idle_timeout = parse_number(text)?,
				"settings.label.retries" => self.preferences.retries = parse_number(text)?.map(|n| n as u32),
				"settings.label.retry_wait" => self.preferences.retry_wait = parse_number(text)?,
				"settings.label.size_limit" => self.preferences.max_size = parse_size(text)?,
				"settings.label.user_agent" => self.preferences.user_agent = (!text.is_empty()).then(|| text.to_owned()),
				"settings.label.proxy" => {
					let schemed = ["http://", "https://", "socks5://"].iter().any(|s| text.starts_with(s));
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
				"settings.label.redirects" => self.preferences.max_redirects = parse_number(text)?.map(|n| n as usize),
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

	/// A row for one of the fields: the field while the sheet is up, its value otherwise.
	/// A row whose control is a text field, in a group of its own choosing.
	fn field_row(&self, section: Section, key: &'static str) -> Row {
		let (_, _, note) = FIELDS.iter().find(|(k, _, _)| *k == key).copied().unwrap_or((key, "", ""));
		let control = match self.settings.as_ref().and_then(|s| s.fields.get(key)) {
			Some(input) => Control::Field { input: input.clone(), note },
			None => Control::Value(self.setting_text(key)),
		};
		Row { section, group: "", label: key, note: "", control }
	}

	/// Every setting there is, in the rail's order, with what it shows now.
	fn settings_rows(&self) -> Vec<Row> {
		let folder = self
			.paths
			.as_ref()
			.map(|p| p.downloads.display().to_string())
			.unwrap_or_else(|| "the working directory".to_owned());
		let mut rows = vec![
			Row {
				section: Section::General,
				group: "settings.group.language",
				label: "settings.label.language",
				note: "settings.note.language",
				control: Control::Choice {
					options: crate::i18n::Language::ALL.iter().map(|l| l.name()).collect(),
					chosen: crate::i18n::Language::ALL
						.iter()
						.position(|l| *l == self.preferences.language)
						.unwrap_or(0),
					set: |this, index, cx| {
						this.set_language(crate::i18n::Language::ALL[index], cx);
					},
				},
			},
			Row {
				section: Section::General,
				group: "settings.group.starting",
				label: "settings.label.start_at_login",
				note: "settings.note.start_at_login",
				control: Control::Switch {
					on: self.preferences.start_at_login,
					set: Rdm::set_start_at_login,
				},
			},
			Row {
				section: Section::General,
				group: "settings.group.where_things_go",
				label: "settings.label.download_folder",
				note: "settings.note.download_folder",
				control: Control::Value(folder),
			},
			Row {
				section: Section::General,
				group: "settings.group.where_things_go",
				note: "settings.note.on_completion",
				label: "settings.label.on_completion",
				control: Control::Value("Do nothing".to_owned()),
			},
			// TODO: a picker once there is a second channel to pick.
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.update_channel",
				label: "settings.label.update_channel",
				control: Control::Value(self.preferences.update_channel.name().to_owned()),
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.check_for_updates",
				label: "settings.label.check_for_updates",
				control: Control::Switch {
					on: self.preferences.check_updates,
					set: Rdm::set_check_updates,
				},
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.check_for_updates",
				label: "settings.label.automatic_updates",
				control: Control::Switch { on: self.preferences.auto_update, set: Rdm::set_auto_update },
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.when_found",
				label: "settings.label.when_a_build_is_found",
				control: Control::Choice {
					options: Policy::ALL.iter().map(|p| p.name()).collect(),
					chosen: Policy::ALL
						.iter()
						.position(|p| *p == self.preferences.update_policy)
						.unwrap_or(0),
					set: |this, index, cx| this.set_update_policy(Policy::ALL[index], cx),
				},
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.latest_build",
				label: "settings.label.latest_build",
				control: Control::Action {
					word: "Check now",
					note: self.update_status(),
					run: |this, cx| this.check_for_updates(true, cx),
				},
			},
			Row {
				section: Section::Folder,
				group: "settings.group.what_is_listed",
				note: "settings.note.folders",
				label: "settings.label.folders",
				control: Control::Choice {
					options: Folders::ALL.iter().map(|f| f.name()).collect(),
					chosen: Folders::ALL.iter().position(|f| *f == self.preferences.folders).unwrap_or(0),
					set: |this, index, cx| this.set_folders(Folders::ALL[index], cx),
				},
			},
			Row {
				section: Section::Folder,
				group: "settings.group.opening_a_file",
				note: "settings.note.show_with",
				label: "settings.label.show_with",
				control: Control::Value(if cfg!(any(target_os = "macos", windows)) {
					crate::reveal::manager_name().to_owned()
				} else if self.preferences.file_manager.trim().is_empty() {
					"xdg-open".to_owned()
				} else {
					self.preferences.file_manager.clone()
				}),
			},
			Row {
				section: Section::Folder,
				group: "settings.group.what_is_listed",
				note: "settings.note.hide_junk",
				label: "settings.label.hide_junk",
				control: Control::Switch {
					on: self.preferences.hide_junk,
					set: Rdm::set_hide_junk,
				},
			},
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_source",
				note: "settings.note.proxy_source",
				control: Control::Choice {
					options: crate::proxy::Source::ALL.iter().map(|s| s.name()).collect(),
					chosen: crate::proxy::Source::ALL
						.iter()
						.position(|s| *s == self.preferences.proxy_source)
						.unwrap_or(0),
					set: |this, index, cx| this.set_proxy_source(crate::proxy::Source::ALL[index], cx),
				},
			},
			Row {
				section: Section::Network,
				group: "settings.group.what_we_call_ourselves",
				label: "settings.label.user_agent",
				note: "settings.note.user_agent",
				control: Control::Choice {
					options: crate::agent::Agent::offered().iter().map(|a| a.name()).collect(),
					chosen: crate::agent::Agent::offered()
						.iter()
						.position(|a| *a == self.preferences.agent)
						.unwrap_or(0),
					set: |this, index, cx| {
						let chosen = crate::agent::Agent::offered()[index];
						this.set_agent(chosen, cx);
					},
				},
			},
			self.field_row(Section::Network, "settings.label.user_agent").under("settings.group.what_we_call_ourselves"),
			self.field_row(Section::Network, "settings.label.proxy").under("settings.group.proxy"),
			Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_who",
				note: "settings.note.dns_who",
				control: Control::Choice {
					options: crate::dns::Servers::ALL.iter().map(|s| s.name()).collect(),
					chosen: crate::dns::Servers::ALL
						.iter()
						.position(|s| *s == self.preferences.dns_servers)
						.unwrap_or(0),
					set: |this, index, cx| this.set_dns_servers(crate::dns::Servers::ALL[index], cx),
				},
			},
			Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_how",
				note: "settings.note.dns_how",
				control: Control::Choice {
					options: crate::dns::Transport::ALL.iter().map(|t| t.name()).collect(),
					chosen: crate::dns::Transport::ALL
						.iter()
						.position(|t| *t == self.preferences.dns_transport)
						.unwrap_or(0),
					set: |this, index, cx| this.set_dns_transport(crate::dns::Transport::ALL[index], cx),
				},
			},
			self.field_row(Section::Network, "settings.label.name_servers").under("settings.group.names"),
			Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_what",
				note: "settings.note.dns_what",
				control: Control::Choice {
					options: crate::dns::Stack::ALL.iter().map(|s| s.name()).collect(),
					chosen: crate::dns::Stack::ALL
						.iter()
						.position(|s| *s == self.preferences.dns_stack)
						.unwrap_or(0),
					set: |this, index, cx| this.set_dns_stack(crate::dns::Stack::ALL[index], cx),
				},
			},
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_in_use",
				note: "settings.note.proxy_in_use",
				control: Control::Action {
					word: "Look again",
					note: self.proxy_status(),
					run: |this, cx| this.look_for_proxy(cx),
				},
			},
			self.field_row(Section::Transfers, "settings.label.concurrent_downloads").under("settings.group.at_once"),
			self.field_row(Section::Transfers, "settings.label.speed_limit").under("settings.group.at_once"),
			self.field_row(Section::Transfers, "settings.label.connections").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.smallest_segment").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.connect_timeout").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.idle_timeout").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.retries").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.retry_wait").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.size_limit").under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.http_version",
				label: "settings.label.http_version",
				control: Control::Choice {
					options: vec!["Auto", "HTTP/1.1", "HTTP/2"],
					chosen: match self.preferences.http {
						HttpVersion::Auto => 0,
						HttpVersion::Http1 => 1,
						HttpVersion::Http2 => 2,
					},
					set: |this, index, cx| {
						this.preferences.http =
							[HttpVersion::Auto, HttpVersion::Http1, HttpVersion::Http2][index];
						this.save_config();
						cx.notify();
					},
				},
			},
			self.field_row(Section::Transfers, "settings.label.headers").under("settings.group.per_download"),
			self.field_row(Section::Transfers, "settings.label.redirects").under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.preallocate",
				label: "settings.label.preallocate",
				control: Control::Switch {
					on: self.preferences.preallocate,
					set: |this, on, cx| {
						this.preferences.preallocate = on;
						this.save_config();
						cx.notify();
					},
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.the_table",
				note: "settings.note.column_widths",
				label: "settings.label.column_widths",
				control: Control::Action {
					word: "Reset",
					note: String::new(),
					run: |this, cx| this.reset_widths(cx),
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.colors",
				note: "settings.note.colorful",
				label: "settings.label.colorful",
				control: Control::Switch {
					on: self.preferences.colorful_categories,
					set: Rdm::set_colorful_categories,
				},
			},
			Row {
				section: Section::Appearance,
				group: "settings.group.colors",
				note: "settings.note.dim",
				label: "settings.label.dim",
				control: Control::Switch { on: self.preferences.dim_inactive, set: Rdm::set_dim_inactive },
			},
			// What this build is: the name in full lives here, and the numbers that tell one
			// build from another. See spec/release.md.
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.application",
				control: Control::Value(identity::NAME.to_owned()),
			},
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.version",
				control: Control::Value(match self.updates.this {
					Some(build) => format!("{} ({build})", identity::VERSION),
					None => format!("{}, built by hand", identity::VERSION),
				}),
			},
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.commit",
				control: Control::Value(
					identity::COMMIT
						.map(|sha| sha[..sha.len().min(12)].to_owned())
						.unwrap_or_else(|| "none".to_owned()),
				),
			},
			Row {
				section: Section::About,
				group: "",
				note: "settings.note.identifier",
				label: "settings.label.identifier",
				control: Control::Value(identity::id()),
			},
		];
		// One row an occasion, in the order src/notify.rs lists them, so a new occasion is a
		// variant and nothing here.
		rows.extend(Occasion::ALL.map(|occasion| Row {
			section: Section::Notifications,
			group: "settings.group.where_each_is_said",
			note: occasion.note(),
			label: occasion.label(),
			control: Control::Choice {
				options: Style::ALL.iter().map(|style| style.name()).collect(),
				// A style this build no longer offers lands on the first: a row has to light
				// something, and one lighting nothing reads as broken rather than as unset.
				chosen: Style::ALL
					.iter()
					.position(|style| *style == self.preferences.notice(occasion))
					.unwrap_or(0),
				set: notice_setter(occasion),
			},
		}));
		rows
	}

	pub(crate) fn settings_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(sheet) = &self.settings else { return deferred(div()).priority(2) };
		let query = sheet.search.read(cx).content.trim().to_lowercase();
		let searching = !query.is_empty();
		let rows = self.settings_rows();

		// The rail: the search field, then one row per section, lit while it is the one shown.
		// While a search is on, no section is lit, since the pane shows every section's matches.
		let sections = Section::ALL.into_iter().map(|section| {
			let on = !searching && sheet.section == section;
			let name = section.name();
			div()
				.id(SharedString::from(format!("settings-section:{name}")))
				.role(Role::Tab)
				.aria_label(format!("Settings: {name}"))
				.aria_selected(on)
				.debug_selector(move || format!("section:{name}"))
				.flex()
				.items_center()
				.gap_2()
				.px_2()
				.py_1()
				.rounded_sm()
				.cursor_pointer()
				.group("settings-section")
				.text_color(if on { p.text } else { p.muted })
				.when(on, |s| s.bg(p.selection))
				.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
				.on_click(cx.listener(move |this, _, _, cx| this.set_settings_section(section, cx)))
				.child(
					hover_icon(
						section.icon(),
						"settings-section",
						if on { p.text } else { p.muted },
						(!on).then_some(p.text),
					)
					.size_3p5(),
				)
				.child(name)
		});
		let rail = div()
			.flex()
			.flex_col()
			.gap_0p5()
			.w(px(176.0))
			.flex_none()
			.p_2()
			.border_r_1()
			.border_color(p.border)
			.child(div().mb_1p5().child(sheet.search.clone()))
			.children(sections);

		// The pane: the section's rows under its name, or every match under each section's name.
		let auto = self.preferences.auto_update;
		let mut shown: Vec<&Row> = rows
			.iter()
			.filter(|row| auto || row.label != "settings.label.when_a_build_is_found")
			.filter(|row| {
				if searching {
					// A search reads what is on screen -- the label, the line under it and the
					// heading -- rather than the keys behind them: somebody looking for "proxy"
					// is looking for what a setting does, and in the language they are reading.
					let seen = |key: &str| crate::i18n::t(key).to_lowercase();
					seen(row.label).contains(&query)
						|| seen(row.note).contains(&query)
						|| seen(row.group).contains(&query)
				} else {
					row.section == sheet.section
				}
			})
			.collect();
		// Rows of one group are gathered together, in the order their groups first appear. The
		// heading is emitted when the group changes, so a group split in two by a row from
		// another gets its heading twice -- which it did, and read as two lists of the same name.
		if !searching {
			let mut order: Vec<&'static str> = Vec::new();
			for row in &shown {
				if !order.contains(&row.group) {
					order.push(row.group);
				}
			}
			shown.sort_by_key(|row| order.iter().position(|g| *g == row.group).unwrap_or(0));
		}
		let complaint = sheet.complaint.clone();
		// The pane scrolls. A row is a label, a line saying what it does and a control, and a
		// section of a dozen of those is taller than the sheet; without this the rows past the
		// bottom were drawn outside it, where a press reaches the backdrop and closes the sheet.
		let mut pane = div()
			.id("settings-pane")
			.flex()
			.flex_col()
			.flex_1()
			.min_w_0()
			// Without this the pane is as tall as its rows and grows past the sheet, whatever the
			// sheet's own height says: a flex child does not shrink below its content unless it
			// is told it may.
			.min_h_0()
			.overflow_y_scroll()
			.p_4()
			.gap_1();
		if searching && shown.is_empty() {
			pane = pane.child(div().text_color(p.muted).child(format!("Nothing matches \"{query}\"")));
		} else if searching {
			let mut last: Option<Section> = None;
			for row in shown {
				if last != Some(row.section) {
					let first = last.is_none();
					last = Some(row.section);
					pane = pane.child(section_title(p, row.section.name()).when(!first, |s| s.mt_2()));
				}
				pane = pane.child(self.setting_row(row, cx));
			}
		} else {
			pane = pane.child(section_title(p, sheet.section.name()));
			// The headings within a section, emitted as the rows walk past them: a dozen rows in
			// one run is a list to read, and three short lists is a page to use.
			let mut group: Option<&'static str> = None;
			for row in shown {
				if group != Some(row.group) && !row.group.is_empty() {
					pane = pane.child(group_title(p, crate::i18n::t(row.group)));
				}
				group = Some(row.group);
				pane = pane.child(self.setting_row(row, cx));
				if let Some((label, message)) = &complaint
					&& *label == row.label
				{
					pane = pane.child(
						div()
							.text_xs()
							.text_color(p.failure)
							.debug_selector(|| "settings-complaint".to_owned())
							.child(message.clone()),
					);
				}
			}
		}

		deferred(
			backdrop(p).child(
				div()
					.id("settings-sheet")
					.debug_selector(|| "settings-sheet".to_owned())
					.flex()
					.flex_col()
					.w(px(640.0))
					.h(px(480.0))
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.bg(p.panel)
					.shadow_lg()
					.overflow_hidden()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_settings(cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.px_4()
							.pt_3()
							.pb_2()
							.border_b_1()
							.border_color(p.border)
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Settings"))
							.child(icon_button(
								p,
								"settings-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_settings(cx)),
							)),
					)
					.child(div().flex().flex_1().min_h_0().child(rail).child(pane)),
			),
		)
		.priority(2)
	}

	/// One setting: its name on the left, and on the right the value it has or the switch that
	/// changes it. The switch is a track with a knob, lit while on.
	fn setting_row(&self, row: &Row, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let label = row.label;
		let right = match &row.control {
			Control::Value(value) => {
				div().text_color(p.muted).truncate().child(value.clone()).into_any_element()
			}
			Control::Switch { on, set } => {
				let (on, set) = (*on, *set);
				div()
					.id(SharedString::from(format!("switch:{label}")))
					.role(Role::CheckBox)
					.aria_label(label)
					.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(move || format!("switch:{label}"))
					.flex()
					.items_center()
					.w(px(30.0))
					.h(px(18.0))
					.p_px()
					.rounded_full()
					.cursor_pointer()
					.leaves_focus()
					.bg(if on { p.accent } else { p.track })
					.when(!on, |s| s.justify_start())
					.when(on, |s| s.justify_end())
					.on_click(cx.listener(move |this, _, _, cx| set(this, !on, cx)))
					.child(div().size(px(14.0)).rounded_full().bg(p.text))
					.into_any_element()
			}
			Control::Choice { options, chosen, set } => {
				let (chosen, set) = (*chosen, *set);
				div()
					.flex()
					.items_center()
					.gap_1()
					.children(options.iter().enumerate().map(|(index, option)| {
						let on = index == chosen;
						div()
							.id(SharedString::from(format!("choice:{label}:{option}")))
							.role(Role::RadioButton)
							.aria_label(*option)
							.aria_selected(on)
							.debug_selector(move || format!("choice:{option}"))
							.px_2()
							.py_0p5()
							.rounded_sm()
							.cursor_pointer()
							.leaves_focus()
							.text_color(if on { p.text } else { p.muted })
							.when(on, |s| s.bg(p.selection))
							.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
							.on_click(cx.listener(move |this, _, _, cx| set(this, index, cx)))
							.child(*option)
					}))
					.into_any_element()
			}
			Control::Field { input, note } => div()
				.flex()
				.items_center()
				.gap_3()
				.min_w_0()
				.child(div().text_xs().text_color(p.muted).truncate().child(*note))
				.child(div().w(px(112.0)).flex_none().child(input.clone()))
				.into_any_element(),
			Control::Action { word, note, run } => {
				let (word, run) = (*word, *run);
				div()
					.flex()
					.items_center()
					.gap_3()
					.min_w_0()
					.child(div().text_color(p.muted).truncate().child(note.clone()))
					.child(
						div()
							.id(SharedString::from(format!("action:{label}")))
							.role(Role::Button)
							.aria_label(word)
							.debug_selector(move || format!("button:{word}"))
							.flex_none()
							.px_2()
							.py_0p5()
							.rounded_sm()
							.text_color(p.accent)
							.cursor_pointer()
							.leaves_focus()
							.hover(move |s| s.bg(p.hover))
							.on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
							.child(word),
					)
					.into_any_element()
			}
		};
		// A choice of several words does not fit beside its label, so it goes under it.
		let stacked = matches!(row.control, Control::Choice { .. });
		let note = crate::i18n::t(row.note);
		let fixed = matches!(row.control, Control::Switch { .. } | Control::Choice { .. });
		div()
			.debug_selector(move || format!("setting:{label}"))
			.flex()
			.when(!stacked, |s| s.justify_between().items_start().gap_4())
			.when(stacked, |s| s.flex_col().items_start().gap_1p5())
			.py_1p5()
			.border_b_1()
			.border_color(p.border)
			// The label and its note give way, and the control does not: a note is a sentence and
			// will take every point it is given, and a control clipped to nothing is a control
			// that cannot be pressed -- which is what happened when these were the other way
			// round, and the switches stopped answering.
			.child(
				div()
					.flex()
					.flex_col()
					// Beside a control it gives way; under one there is nothing to give way to,
					// and saying it may shrink to nothing there leaves the note a character wide
					// and a row two thousand points tall.
					.when(!stacked, |s| s.flex_1().min_w_0())
					.gap_0p5()
					.child(div().truncate().child(crate::i18n::t(label)))
					.when(!note.is_empty(), |s| {
						s.child(div().text_xs().text_color(p.muted).child(note))
					}),
			)
			// A switch and a row of words are the size they are; a value, a note or a path is as
			// long as it happens to be, and one of those given its natural width leaves the note
			// beside it a character wide and the row a thousand points tall.
			.child(
				div()
					.when(fixed, |s| s.flex_none())
					.when(!fixed, |s| s.min_w_0().max_w(gpui::relative(0.6)).truncate())
					.child(right),
			)
	}
}

/// A heading within a section: smaller than the section's own and set off above the rows it
/// gathers, so the eye can skip a group whole rather than reading every label in it.
fn group_title(p: crate::ui::theme::Palette, name: &'static str) -> gpui::Div {
	div()
		.debug_selector(move || format!("group:{name}"))
		.pt_3()
		.pb_0p5()
		.text_xs()
		.text_color(p.muted)
		.child(name)
}

fn section_title(p: crate::ui::theme::Palette, name: &'static str) -> gpui::Div {
	div().text_xs().text_color(p.muted).pb_1().child(name)
}
