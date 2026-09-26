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
	Context, Entity, IntoElement, Role, SharedString, anchored, canvas, deferred, div, point,
	prelude::*, px, relative,
};

use crate::app::Rdm;
use crate::identity;
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::text_input::TextInput;
use crate::ui::{LeavesFocus, backdrop, floating, icon_button};
use std::collections::HashMap;

use crate::download::Folders;
use crate::engine::{Bump, HttpVersion};
use crate::notify::{Occasion, Style};
use crate::update::Policy;

mod controls;
mod rows;
mod values;
mod view;

use controls::{group_title, section_title, segments_fit};

// TODO: every value row here is a label until there is a setting behind it and a store to keep it
// in; the folder is the one the engine writes to, the rest are the engine's defaults, read only.

/// The card's size. Fixed, so changing sections moves nothing.
///
/// The height is what six of the eight sections hold without scrolling, and not what the longest
/// one does: at 520 the card stood 87 out of every 100 points of a window opened at its default
/// height, which left the list a rim around it rather than something the card was laid over, and
/// every short section -- General has four rows, Folder three -- ended in a third of a card of
/// nothing. Transfers and Network are longer than this and scroll, which the pane has always
/// done.
const SHEET_W: f32 = 680.0;
const SHEET_H: f32 = 440.0;

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
	/// The row whose dropdown is open. Only which one: where it opens is the button's own
	/// business, and the panel hangs off it. See `choice_menu`. One menu at a time -- two down
	/// the same pane would each be answering for the other's row.
	pub menu: Option<&'static str>,
	/// The row whose menu a press outside it has just closed. A press on the button its menu
	/// belongs to is a press outside the panel, so it closes the menu on the way down and would
	/// open it again on the way up -- a menu that will not shut. This is what the second half of
	/// that press reads to know the first half already answered it, and it is cleared by the
	/// reading. Ordering the two handlers instead was tried: `default_prevented` is not set yet
	/// when the panel hears the press.
	pub dismissed: Option<&'static str>,
}

/// Every field there is: the key that is its setting's identity, its placeholder, and the note
/// saying what it takes. The note is a key like every other row's rather than the English it used
/// to be: it lands in `Row::note` and is read there, so a field's explanation is translated and
/// reaches the pointer by the same path as a switch's. It used to be a second note living inside
/// `Control::Field`, drawn between the label and the input where there was never room for it --
/// every row of Transfers showed a sentence cut off at four words. One note, one home.
const FIELDS: [(&str, &str, &str); 17] = [
	("settings.label.concurrent_downloads", "3", "settings.note.concurrent"),
	("settings.label.speed_limit", "Off", "settings.note.speed_limit"),
	("settings.label.connections", "Auto", "settings.note.connections"),
	("settings.label.limit_slider_from", "1", "settings.note.limit_slider_from"),
	("settings.label.limit_slider_to", "100", "settings.note.limit_slider_to"),
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
				dismissed: None,
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
			// The press that got here may have closed this very menu a moment ago, on its way
			// down; opening it again would make the button unable to shut what it opened.
			if sheet.dismissed.take() == Some(label) {
				cx.notify();
				return;
			}
			sheet.menu = match sheet.menu {
				Some(open) if open == label => None,
				_ => Some(label),
			};
			cx.notify();
		}
	}

	pub(crate) fn close_settings_menu(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings
			&& sheet.menu.take().is_some()
		{
			sheet.dismissed = None;
			cx.notify();
		}
	}

	/// The same, from a press outside the panel, which remembers what it closed so that a press
	/// on the button does not reopen it. See `SettingsSheet::dismissed`.
	pub(crate) fn dismiss_settings_menu(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings
			&& let Some(label) = sheet.menu.take()
		{
			sheet.dismissed = Some(label);
			cx.notify();
		}
	}

	pub(crate) fn settings_menu_open(&self) -> bool {
		self.settings.as_ref().is_some_and(|sheet| sheet.menu.is_some())
	}

	/// The labels of the rows whose control is a dropdown, for the control socket: a menu is
	/// opened by a press and the pointer is not ours to move. See spec/workflow.md.
	#[cfg(all(debug_assertions, unix))]
	pub(crate) fn settings_dropdowns(&self) -> Vec<&'static str> {
		self
			.settings_rows()
			.iter()
			.filter(|row| match &row.control {
				Control::Choice { options, .. } => !segments_fit(options),
				_ => false,
			})
			.map(|row| row.label)
			.collect()
	}

	pub(crate) fn set_settings_section(&mut self, section: Section, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.settings {
			sheet.section = section;
			sheet.menu = None;
			sheet.dismissed = None;
			cx.notify();
		}
	}
}
