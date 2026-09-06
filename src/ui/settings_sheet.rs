//! Settings are a sheet inside the main window, like Add Task; only a download gets a window of
//! its own, because a download is a thing to keep beside the list while it moves. The sheet is
//! shaped for the many settings to come: a rail of sections on the left with a search field
//! over it, and the chosen section's rows on the right. A search cuts across every section and
//! shows what matches under its section's name, so a setting is found without knowing where
//! it was filed. See spec/ui.md.

use gpui::{Context, Entity, IntoElement, Role, SharedString, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::icon::{Icon, hover_icon};
use crate::ui::text_input::TextInput;
use crate::ui::{LeavesFocus, backdrop, icon_button};

// TODO: every value row here is a label until there is a setting behind it and a store to keep it
// in; the folder is the one the engine writes to, the rest are the engine's defaults, read only.

/// The sheet while it is up: which section is open, and the field that searches all of them.
pub struct SettingsSheet {
	pub section: Section,
	pub search: Entity<TextInput>,
}

/// The sections down the rail, in their order. A setting belongs to exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
	General,
	Transfers,
	Appearance,
}

impl Section {
	pub const ALL: [Section; 3] = [Section::General, Section::Transfers, Section::Appearance];

	pub fn name(self) -> &'static str {
		match self {
			Section::General => "General",
			Section::Transfers => "Transfers",
			Section::Appearance => "Appearance",
		}
	}

	fn icon(self) -> Icon {
		match self {
			Section::General => Icon::SlidersHorizontal,
			Section::Transfers => Icon::Download,
			Section::Appearance => Icon::Palette,
		}
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
}

struct Row {
	section: Section,
	label: &'static str,
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
			self.settings = Some(SettingsSheet { section: Section::General, search });
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

	/// Every setting there is, in the rail's order, with what it shows now.
	fn settings_rows(&self) -> Vec<Row> {
		let folder = self
			.paths
			.as_ref()
			.map(|p| p.downloads.display().to_string())
			.unwrap_or_else(|| "the working directory".to_owned());
		vec![
			Row { section: Section::General, label: "Download folder", control: Control::Value(folder) },
			Row {
				section: Section::General,
				label: "On completion",
				control: Control::Value("Do nothing".to_owned()),
			},
			// TODO: a picker once there is a second channel to pick.
			Row {
				section: Section::General,
				label: "Update channel",
				control: Control::Value(self.preferences.update_channel.name().to_owned()),
			},
			Row {
				section: Section::General,
				label: "Check for updates",
				control: Control::Action {
					word: "Check now",
					note: self.update_status(),
					run: |this, cx| this.check_for_updates(true, cx),
				},
			},
			Row {
				section: Section::Transfers,
				label: "Concurrent downloads",
				control: Control::Value("3".to_owned()),
			},
			Row {
				section: Section::Transfers,
				label: "Speed limit",
				control: Control::Value("Off".to_owned()),
			},
			Row {
				section: Section::Appearance,
				label: "Always use colorful categories",
				control: Control::Switch {
					on: self.preferences.colorful_categories,
					set: Rdm::set_colorful_categories,
				},
			},
		]
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
		let shown: Vec<&Row> = rows
			.iter()
			.filter(|row| {
				if searching {
					row.label.to_lowercase().contains(&query)
				} else {
					row.section == sheet.section
				}
			})
			.collect();
		let mut pane = div().flex().flex_col().flex_1().min_w_0().p_4().gap_1();
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
			for row in shown {
				pane = pane.child(self.setting_row(row, cx));
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
					.h(px(420.0))
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
			Control::Value(value) => div().text_color(p.muted).child(value.clone()).into_any_element(),
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
		div()
			.debug_selector(move || format!("setting:{label}"))
			.flex()
			.justify_between()
			.items_center()
			.gap_4()
			.py_1p5()
			.border_b_1()
			.border_color(p.border)
			.child(div().flex_none().child(label))
			.child(div().min_w_0().truncate().child(right))
	}
}

fn section_title(p: crate::ui::theme::Palette, name: &'static str) -> gpui::Div {
	div().text_xs().text_color(p.muted).pb_1().child(name)
}
