//! Settings is a sheet inside the main window, like Add Task; only a download gets a window of
//! its own, because a download is a thing to keep beside the list while it moves. It carries a
//! strip of its own at the top -- its name and the cross that closes it -- laid out as every
//! other sheet's header is.
//!
//! It is shaped for the many settings to come: a rail of sections on the left with a search field
//! over it, and the chosen section's rows on the right. A search cuts across every section and
//! shows what matches under its section's name, so a setting is found without knowing where it
//! was filed. See spec/ui.md.

use gpui::{
	Context, Entity, IntoElement, Role, SharedString, deferred, div, prelude::*, px,
};

use crate::app::Rdm;
use crate::identity;
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::text_input::TextInput;
use crate::ui::tooltip::tooltip_wrapped;
use crate::ui::{LeavesFocus, backdrop, floating, icon_button};
use std::collections::HashMap;

use crate::download::Folders;
use crate::engine::HttpVersion;
use crate::notify::{Occasion, Style};
use crate::update::Policy;

// TODO: every value row here is a label until there is a setting behind it and a store to keep it
// in; the folder is the one the engine writes to, the rest are the engine's defaults, read only.

/// The card's size. Fixed, so changing sections moves nothing.
const SHEET_W: f32 = 680.0;
const SHEET_H: f32 = 520.0;

/// The strip at the top of the card: its name and the button that closes it.
const HEADER_H: f32 = 36.0;


/// The sheet while it is up: which section is open, and the field that searches all of them.
pub struct SettingsSheet {
	pub section: Section,
	pub search: Entity<TextInput>,
	/// The fields, by their row's label: each applies on Enter and reads back what was kept.
	pub fields: HashMap<&'static str, Entity<TextInput>>,
	/// What the last field said no to, under the row.
	pub complaint: Option<(&'static str, String)>,
	/// The row whose dropdown is open, by its label. One at a time: a second opening closes the
	/// first, since two menus down the same pane would each be answering for the other's row.
	pub menu: Option<&'static str>,
}

/// Every field there is: the key that is its setting's identity, its placeholder, and the note
/// saying what it takes. The note is a key like every other row's rather than the English it used
/// to be: it lands in `Row::note` and is read there, so a field's explanation is translated and
/// reaches the pointer by the same path as a switch's. It used to be a second note living inside
/// `Control::Field`, drawn between the label and the input where there was never room for it --
/// every row of Transfers showed a sentence cut off at four words. One note, one home.
const FIELDS: [(&str, &str, &str); 15] = [
	("settings.label.concurrent_downloads", "3", "settings.note.concurrent"),
	("settings.label.speed_limit", "Off", "settings.note.speed_limit"),
	("settings.label.connections", "Auto", "settings.note.connections"),
	("settings.label.smallest_segment", "1m", "settings.note.smallest_segment"),
	("settings.label.connect_timeout", "30", "settings.note.connect_timeout"),
	("settings.label.idle_timeout", "60", "settings.note.idle_timeout"),
	("settings.label.retries", "5", "settings.note.retries"),
	("settings.label.retry_wait", "1", "settings.note.retry_wait"),
	("settings.label.size_limit", "Off", "settings.note.size_limit"),
	("settings.label.user_agent", "rdm/version", "settings.note.user_agent_sent"),
	("settings.label.proxy", "Address", "settings.note.proxy"),
	("settings.label.name_servers", "1.1.1.1", "settings.note.name_servers"),
	("settings.label.system_domains", "corp.example.com", "settings.note.system_domains"),
	("settings.label.headers", "", "settings.note.headers"),
	("settings.label.redirects", "10", "settings.note.redirects"),
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
	/// A word that does something when pressed, with how it last went beside it. That is a value
	/// and not an explanation -- it changes as the thing runs -- so it stays on screen while the
	/// row's own note goes to the pointer with every other.
	Action { word: &'static str, note: String, run: fn(&mut Rdm, &mut Context<Rdm>) },
	/// A field, applied on Enter. What it takes is the row's note, like every other row's.
	Field { input: Entity<TextInput> },
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

	/// Calls the row something other than its setting's key, for the one place where two rows
	/// share a setting and would otherwise share a name.
	fn titled(mut self, title: &'static str) -> Row {
		self.title = Some(title);
		self
	}
}

struct Row {
	section: Section,
	/// The heading this row sits under within its section, empty for the rows that open it. A
	/// section of a dozen rows in one run is a list to read rather than a page to use; the
	/// headings are what make it three short lists.
	group: &'static str,
	/// The key that is the setting's identity: what `apply_setting` dispatches on, what the field
	/// is filed under, and what names the row to a test and to the accessibility tree.
	label: &'static str,
	/// What the row is called on screen, where that is not the identity above. One row needs it:
	/// the user agent is chosen on one row and written on another, and both were called `User
	/// agent` -- two identical labels in one group, which says nothing about which is which.
	title: Option<&'static str>,
	/// What the setting does, said in a sentence. It reaches the reader as a tooltip on the label
	/// rather than as a line under it, so a row is one line; the search still reads it, since a
	/// setting is looked for by what it does. See spec/ui.md.
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
			self.settings = Some(SettingsSheet {
				section: Section::General,
				search,
				fields,
				complaint: None,
				menu: None,
			});
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

	/// Opens one row's dropdown, or closes whatever is open. Moving to another section closes it
	/// too: a menu belongs to a row, and the row is gone.
	pub(crate) fn toggle_settings_menu(&mut self, label: &'static str, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings {
			sheet.menu = if sheet.menu == Some(label) { None } else { Some(label) };
			cx.notify();
		}
	}

	pub(crate) fn close_settings_menu(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings
			&& sheet.menu.is_some()
		{
			sheet.menu = None;
			cx.notify();
		}
	}

	pub(crate) fn settings_menu_open(&self) -> bool {
		self.settings.as_ref().is_some_and(|s| s.menu.is_some())
	}

	pub(crate) fn set_settings_section(&mut self, section: Section, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings {
			sheet.section = section;
			sheet.menu = None;
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
			"settings.label.speed_limit" => {
				p.speed_limit.map(|l| format_rate(Some(l))).unwrap_or_default()
			}
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
					self.preferences.connections = if text.is_empty() || text.eq_ignore_ascii_case("auto") {
						None
					} else {
						Some(crate::ui::add_dialog::parse_count(text)?)
					};
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

	/// A row for one of the fields: the field while the sheet is up, its value otherwise.
	/// A row whose control is a text field, in a group of its own choosing.
	fn field_row(&self, section: Section, key: &'static str) -> Row {
		let (_, _, note) = FIELDS.iter().find(|(k, _, _)| *k == key).copied().unwrap_or((key, "", ""));
		let control = match self.settings.as_ref().and_then(|s| s.fields.get(key)) {
			Some(input) => Control::Field { input: input.clone() },
			None => Control::Value(self.setting_text(key)),
		};
		Row { section, group: "", label: key, title: None, note, control }
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
				title: None,
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
				title: None,
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
				title: None,
				note: "settings.note.download_folder",
				control: Control::Value(folder),
			},
			Row {
				section: Section::General,
				group: "settings.group.where_things_go",
				note: "settings.note.on_completion",
				label: "settings.label.on_completion",
				title: None,
				control: Control::Value("Do nothing".to_owned()),
			},
			// TODO: a picker once there is a second channel to pick.
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.update_channel",
				label: "settings.label.update_channel",
				title: None,
				control: Control::Value(self.preferences.update_channel.name().to_owned()),
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.check_for_updates",
				label: "settings.label.check_for_updates",
				title: None,
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
				title: None,
				control: Control::Switch { on: self.preferences.auto_update, set: Rdm::set_auto_update },
			},
			Row {
				section: Section::Updates,
				group: "",
				note: "settings.note.when_found",
				label: "settings.label.when_a_build_is_found",
				title: None,
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
				title: None,
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
				title: None,
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
				title: None,
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
				title: None,
				control: Control::Switch { on: self.preferences.hide_junk, set: Rdm::set_hide_junk },
			},
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_source",
				title: None,
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
				title: None,
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
			// The row above chooses a disguise and fills this one, so the two are one setting seen
			// twice: what was picked, and what will actually be sent. They were both called `User
			// agent`, which named the pair rather than either half of it.
			self
				.field_row(Section::Network, "settings.label.user_agent")
				.titled("settings.label.user_agent_sent")
				.under("settings.group.what_we_call_ourselves"),
			self.field_row(Section::Network, "settings.label.proxy").under("settings.group.proxy"),
			Row {
				section: Section::Network,
				group: "settings.group.proxy",
				label: "settings.label.proxy_in_use",
				title: None,
				note: "settings.note.proxy_in_use",
				control: Control::Action {
					word: "Look again",
					note: self.proxy_status(),
					run: |this, cx| this.look_for_proxy(cx),
				},
			},
			self
				.field_row(Section::Transfers, "settings.label.concurrent_downloads")
				.under("settings.group.at_once"),
			self
				.field_row(Section::Transfers, "settings.label.speed_limit")
				.under("settings.group.at_once"),
			self
				.field_row(Section::Transfers, "settings.label.connections")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.smallest_segment")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.connect_timeout")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.idle_timeout")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.retries")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.retry_wait")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.size_limit")
				.under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.http_version",
				label: "settings.label.http_version",
				title: None,
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
			self
				.field_row(Section::Transfers, "settings.label.headers")
				.under("settings.group.per_download"),
			self
				.field_row(Section::Transfers, "settings.label.redirects")
				.under("settings.group.per_download"),
			Row {
				section: Section::Transfers,
				group: "settings.group.per_download",
				note: "settings.note.preallocate",
				label: "settings.label.preallocate",
				title: None,
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
				title: None,
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
				title: None,
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
				title: None,
				control: Control::Switch { on: self.preferences.dim_inactive, set: Rdm::set_dim_inactive },
			},
			// What this build is: the name in full lives here, and the numbers that tell one
			// build from another. See spec/release.md.
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.application",
				title: None,
				control: Control::Value(identity::NAME.to_owned()),
			},
			Row {
				section: Section::About,
				group: "settings.group.this_build",
				note: "",
				label: "settings.label.version",
				title: None,
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
				title: None,
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
				title: None,
				control: Control::Value(identity::id()),
			},
		];
		// One row an occasion, in the order src/notify.rs lists them, so a new occasion is a
		// variant and nothing here.
		rows.extend(Occasion::ALL.map(|occasion| {
			Row {
				section: Section::Notifications,
				group: "settings.group.where_each_is_said",
				note: occasion.note(),
				label: occasion.label(),
				title: None,
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
			}
		}));
		// How names are resolved. The switch that hands the whole business back to the machine
		// comes first, and while it is on the rows under it are not shown: none of them does
		// anything then, and a row that cannot matter is a row read for nothing. See src/dns.rs.
		rows.push(Row {
			section: Section::Network,
			group: "settings.group.names",
			label: "settings.label.dns_force_system",
			title: None,
			note: "settings.note.dns_force_system",
			control: Control::Switch {
				on: self.preferences.dns_force_system,
				set: Rdm::set_dns_force_system,
			},
		});
		if !self.preferences.dns_force_system {
			let transport = self.preferences.dns_transport;
			let offered = crate::dns::Servers::offered(transport);
			rows.push(Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_https",
				title: None,
				note: "settings.note.dns_https",
				control: Control::Switch { on: transport.is_https(), set: Rdm::set_dns_https },
			});
			// Only beside the switch above: forcing a transport that is not in use says nothing.
			if transport.is_https() {
				rows.push(Row {
					section: Section::Network,
					group: "settings.group.names",
					label: "settings.label.dns_force_https",
					title: None,
					note: "settings.note.dns_force_https",
					control: Control::Switch {
						on: self.preferences.dns_force_https,
						set: Rdm::set_dns_force_https,
					},
				});
			}
			rows.push(Row {
				section: Section::Network,
				group: "settings.group.names",
				label: "settings.label.dns_servers",
				title: None,
				note: "settings.note.dns_servers",
				control: Control::Choice {
					options: offered.iter().map(|s| s.name(transport)).collect(),
					chosen: offered.iter().position(|s| *s == self.preferences.dns_servers).unwrap_or(0),
					set: |this, index, cx| {
						let transport = this.preferences.dns_transport;
						this.set_dns_servers(crate::dns::Servers::offered(transport)[index], cx);
					},
				},
			});
			// Only where the choice above reads it. Following the machine's own servers means
			// there is nothing to write, and a field that is ignored is worse than no field.
			if self.preferences.dns_servers != crate::dns::Servers::System {
				rows.push(
					self
						.field_row(Section::Network, "settings.label.name_servers")
						.under("settings.group.names"),
				);
			}
			rows.push(
				self
					.field_row(Section::Network, "settings.label.system_domains")
					.under("settings.group.names"),
			);
		}
		rows
	}

	pub(crate) fn settings_body(
		&self,
		p: crate::ui::theme::Palette,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let Some(sheet) = &self.settings else { return div().into_any_element() };
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
						|| row.title.is_some_and(|title| seen(title).contains(&query))
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
				pane = pane.child(self.setting_row(p, row, cx));
			}
		} else {
			// No section title here: the rail two inches to the left already shows which section
			// is open, lit, and the word repeated at the top of the pane was the same answer to a
			// question nobody had asked twice. It stays under a search, where the rail is lit by
			// nothing and the section is the only thing saying where a match came from.
			//
			// The headings within a section, emitted as the rows walk past them: a dozen rows in
			// one run is a list to read, and three short lists is a page to use.
			let mut group: Option<&'static str> = None;
			for row in shown {
				if group != Some(row.group) && !row.group.is_empty() {
					pane = pane.child(group_title(p, crate::i18n::t(row.group)));
				}
				group = Some(row.group);
				pane = pane.child(self.setting_row(p, row, cx));
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

		// The card, drawn the same whether it is a sheet in the main window or a window of its
		// own: its own strip at the top, the rail and the pane under it. The frame around it is
		// the only difference between the two, which is what makes dragging it out move nothing
		// but the frame.
		div()
			.id("settings-sheet")
			.debug_selector(|| "settings-sheet".to_owned())
			.flex()
			.flex_col()
			.size_full()
			.overflow_hidden()
			// Zed's density, the same as the main window's: this is the same application and not
			// a dialog with a face of its own.
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.child(self.settings_header(p, cx))
			.child(div().flex().flex_1().min_h_0().child(rail).child(pane))
			.into_any_element()
	}

	/// The strip at the top of the card: its name and the one button that closes it, laid out as
	/// every other sheet's is -- the name at the left, the cross at the right, on every system.
	/// A sheet is not a window and its cross is not a window button, so there is nothing here for
	/// a system's own arrangement to be followed.
	fn settings_header(
		&self,
		p: crate::ui::theme::Palette,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		div()
			.debug_selector(|| "settings-header".to_owned())
			.flex()
			.items_center()
			.justify_between()
			.flex_none()
			.h(px(HEADER_H))
			.px_3()
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
			))
	}

	/// Settings inside the main window, which is where it opens: the card over the dimmed list.
	/// Dragging its strip takes it out. See spec/ui.md.
	pub(crate) fn settings_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		deferred(
			backdrop(p).child(
				div()
					.id("settings-card")
					.w(px(SHEET_W))
					.h(px(SHEET_H))
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.shadow_lg()
					.overflow_hidden()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_settings(cx)))
					.child(self.settings_body(p, cx)),
			),
		)
		.priority(2)
	}

	/// One setting: its name on the left, and on the right the value it has or the switch that
	/// changes it. The switch is a track with a knob, lit while on.
	fn setting_row(
		&self,
		p: crate::ui::theme::Palette,
		row: &Row,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let label = row.label;
		// A choice is drawn one of two ways and the words decide which: see `segments_fit`.
		let dropdown = match &row.control {
			Control::Choice { options, .. } => !segments_fit(options),
			_ => false,
		};
		let open = dropdown && self.settings.as_ref().is_some_and(|sheet| sheet.menu == Some(label));
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
			// One word and a chevron, the options in a panel under the row. It reads as the
			// closed thing it is, which a row of words too long to draw did not: four disguises
			// ran off the pane and the fourth was never on screen at all.
			Control::Choice { options, chosen, .. } if dropdown => {
				let shown = options.get(*chosen).copied().unwrap_or_default();
				div()
					.id(SharedString::from(format!("choice:{label}")))
					.role(Role::Button)
					.aria_label(label)
					.debug_selector(move || format!("choice:{label}"))
					.flex()
					.items_center()
					.justify_between()
					.gap_2()
					.w(px(200.0))
					.flex_none()
					.px_2()
					.py_0p5()
					.rounded_sm()
					.border_1()
					.border_color(if open { p.accent } else { p.border })
					.bg(p.track)
					.cursor_pointer()
					.leaves_focus()
					.on_click(cx.listener(move |this, _, _, cx| this.toggle_settings_menu(label, cx)))
					.child(div().min_w_0().truncate().child(shown))
					.child(icon(Icon::ChevronDown, p.muted).size_3())
					.into_any_element()
			}
			// A segmented control: one track with the segments inside it, so the alternatives
			// read as one control offering a choice. Lit and unlit words with nothing around them
			// read as a button and some loose text, which is what they were.
			Control::Choice { options, chosen, set } => {
				let (chosen, set) = (*chosen, *set);
				div()
					.flex()
					.items_center()
					.p_px()
					.rounded_md()
					.bg(p.track)
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
			Control::Field { input } => {
				div().w(px(132.0)).flex_none().child(input.clone()).into_any_element()
			}
			// The status wraps rather than truncating, up to the width below. What it says is the
			// whole point of the row -- which build was found, or why the check could not be
			// read -- and a sentence cut at `2026.9.6 (102) is the la` has answered nothing. The
			// word beside it keeps to the first line, so the row still reads as one action.
			Control::Action { word, note, run } => {
				let (word, run) = (*word, *run);
				div()
					.flex()
					.items_start()
					.gap_3()
					.min_w_0()
					.child(div().max_w(px(240.0)).text_color(p.muted).child(note.clone()))
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
		// A segmented control is as wide as all of its words at once and does not fit beside a
		// label, so it goes under one. A dropdown is one word and a chevron and stays on the line
		// with everything else.
		let stacked = matches!(row.control, Control::Choice { .. }) && !dropdown;
		let note = crate::i18n::t(row.note);
		// An action sizes itself: its status wraps within its own ceiling, so capping and
		// truncating the whole thing here would undo the wrapping a line below.
		let fixed = matches!(
			row.control,
			Control::Switch { .. } | Control::Field { .. } | Control::Action { .. }
		) || dropdown;
		let title = crate::i18n::t(row.title.unwrap_or(row.label));
		let line = div()
			.flex()
			.when(!stacked, |s| s.justify_between().items_center().gap_4())
			.when(stacked, |s| s.flex_col().items_start().gap_1p5())
			// The label gives way and the control does not: a control clipped to nothing is a
			// control that cannot be pressed, which is what happened when these were the other way
			// round and the switches stopped answering.
			.child(
				div()
					// An id because a tooltip needs one: GPUI tracks how long the pointer has
					// rested on an element, and an element with no id is not one it can follow.
					.id(SharedString::from(format!("label:{label}")))
					.when(!stacked, |s| s.flex_1().min_w_0())
					.truncate()
					// The row is one line, so what the setting does lives under the pointer. A
					// label with nothing behind it gets no tooltip rather than an empty one.
					.when(!note.is_empty(), |s| s.tooltip(tooltip_wrapped(note)))
					.child(title),
			)
			// A switch, a field and a dropdown are the size they are; a value or a status is as
			// long as it happens to be, and one of those given its natural width pushes the label
			// out of the row -- so beside a label it is capped and truncates instead.
			//
			// The cap is for sharing a line and nothing else. A segmented control has the line to
			// itself, and holding it to six tenths of one cut `No proxy` to `No`.
			.child(
				div()
					.when(fixed || stacked, |s| s.flex_none())
					.when(!fixed && !stacked, |s| s.min_w_0().max_w(gpui::relative(0.6)).truncate())
					.child(right),
			);
		div()
			.debug_selector(move || format!("setting:{label}"))
			.flex()
			.flex_col()
			.gap_1()
			.py_1p5()
			.child(line)
			.when(open, |s| s.child(self.choice_menu(p, row, cx)))
	}

	/// The options of a dropdown, in a panel under its row. It is laid out in the pane rather than
	/// floated over the window, which the funnel's menu is: that one hangs off the window root and
	/// is positioned in window space, and a row here has no window position to be given -- it is
	/// inside a pane that scrolls, so an anchored panel would part company with its row on the
	/// first turn of the wheel. In the pane it moves with the row and is clipped by the same
	/// edges. It occludes, like everything drawn over the window. See spec/ui.md.
	fn choice_menu(
		&self,
		p: crate::ui::theme::Palette,
		row: &Row,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let label = row.label;
		let Control::Choice { options, chosen, set } = &row.control else {
			return div().into_any_element();
		};
		let (chosen, set) = (*chosen, *set);
		floating(p, SharedString::from(format!("menu:{label}")))
			.debug_selector(move || format!("menu:{label}"))
			.flex()
			.flex_col()
			.gap_px()
			.w(px(200.0))
			.p_1()
			.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_settings_menu(cx)))
			.children(options.iter().enumerate().map(|(index, option)| {
				let on = index == chosen;
				div()
					.id(SharedString::from(format!("option:{label}:{option}")))
					.role(Role::RadioButton)
					.aria_label(*option)
					.aria_selected(on)
					.debug_selector(move || format!("choice:{option}"))
					.flex()
					.items_center()
					.px_1p5()
					.py_0p5()
					.rounded_sm()
					.cursor_pointer()
					.leaves_focus()
					.text_color(if on { p.text } else { p.muted })
					.when(on, |s| s.bg(p.selection))
					.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
					.on_click(cx.listener(move |this, _, _, cx| {
						set(this, index, cx);
						this.close_settings_menu(cx);
					}))
					.child(*option)
			}))
			.into_any_element()
	}
}

/// Whether a choice's options can be drawn side by side as a segmented control, or want a
/// dropdown instead. What decides is how much room the words ask for, measured in the columns
/// they draw in rather than in characters: a CJK glyph is twice the width of a Latin one, so
/// `简体中文` is four characters and eight columns, and counting characters would call the
/// Japanese and Chinese windows narrow when they are not.
///
/// The budget is the pane's width less the padding each segment carries. It is a count and not a
/// measurement because the alternative -- laying the control out and asking how wide it came
/// out -- can only be answered after the frame it would decide, and a control that changed shape
/// one frame late would flicker between the two on every language change.
///
/// Five options are a dropdown whatever they say: a row of five is a list, and a list wants the
/// shape a list has.
fn segments_fit(options: &[&str]) -> bool {
	let columns: usize = options
		.iter()
		.map(|option| option.chars().map(|c| if wide(c) { 2 } else { 1 }).sum::<usize>())
		.sum();
	options.len() <= 4 && columns <= 52
}

/// The ranges a terminal and a font both draw at two columns: the CJK blocks, the kana, Hangul
/// and the full-width forms. Enough for the three languages the window is read in.
fn wide(c: char) -> bool {
	matches!(c as u32,
		0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
		| 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
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

#[cfg(test)]
mod tests {
	use super::*;

	/// The two sets the rule was written between: the languages, which fit and read better as
	/// segments, and the disguises, whose fourth option was drawn off the edge of the pane.
	#[test]
	fn a_choice_is_segments_while_its_words_fit_and_a_dropdown_after_that() {
		assert!(segments_fit(&["System", "English", "简体中文", "日本語"]));
		assert!(!segments_fit(&[
			"This application",
			"Chrome on Windows",
			"Chrome on Linux",
			"Something else",
		]));
		// The widest set that still fits, and the one the budget was measured against.
		assert!(segments_fit(&["Ignore them", "Show what is inside", "Keep them as folders"]));
	}

	/// A CJK glyph draws in two columns, so the same sentence is half as many characters and the
	/// same width. Counting characters would have called this set narrow and drawn it off the pane.
	#[test]
	fn a_cjk_glyph_counts_as_the_two_columns_it_draws_in() {
		assert_eq!("简体中文".chars().count(), 4);
		assert!(wide('简') && wide('日') && wide('ア') && wide('한'));
		assert!(!wide('a') && !wide('/') && !wide('1'));
		// Twenty-seven Latin characters fit; twenty-seven CJK characters are fifty-four columns
		// and do not.
		let latin = ["aaaaaaaaa", "aaaaaaaaa", "aaaaaaaaa"];
		let cjk = ["简体中文简体中文简", "简体中文简体中文简", "简体中文简体中文简"];
		assert!(segments_fit(&latin));
		assert!(!segments_fit(&cjk));
	}

	/// However short they are: a row of five words is a list, and a list gets a list's shape.
	#[test]
	fn five_options_are_a_dropdown_whatever_they_say() {
		assert!(segments_fit(&["a", "b", "c", "d"]));
		assert!(!segments_fit(&["a", "b", "c", "d", "e"]));
	}
}
