//! Settings are a sheet inside the main window, like Add Task; only a download gets a window of
//! its own, because a download is a thing to keep beside the list while it moves.

use gpui::{Context, IntoElement, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::icon::Icon;
use crate::ui::icon_button;

// TODO: every row here is a label until there is a setting behind it and a store to keep it in;
// the folder is the one the engine writes to, the rest are the engine's defaults, read only.
const ROWS: [(&str, &str); 3] =
	[("Concurrent downloads", "3"), ("Speed limit", "Off"), ("On completion", "Do nothing")];

impl Rdm {
	pub(crate) fn toggle_settings(&mut self, open: bool, cx: &mut Context<Self>) {
		self.settings_open = open;
		cx.notify();
	}

	pub(crate) fn settings_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let folder = self
			.paths
			.as_ref()
			.map(|p| p.downloads.display().to_string())
			.unwrap_or_else(|| "the working directory".to_owned());
		let rows = std::iter::once(("Download folder", folder))
			.chain(ROWS.iter().map(|(name, value)| (*name, (*value).to_owned())))
			.map(|(name, value)| {
				div()
					.flex()
					.justify_between()
					.items_center()
					.py_1p5()
					.border_b_1()
					.border_color(p.border)
					.child(name)
					.child(div().text_color(p.muted).child(value))
			});
		// The one switch with something behind it: a track with a knob, lit while on.
		let colorful = self.preferences.colorful_categories;
		let switch = div()
			.flex()
			.justify_between()
			.items_center()
			.py_1p5()
			.border_b_1()
			.border_color(p.border)
			.child("Always use colorful categories")
			.child(
				div()
					.id("colorful-categories")
					.role(gpui::Role::CheckBox)
					.aria_label("Always use colorful categories")
					.aria_toggled(if colorful { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(|| "setting:Always use colorful categories".to_owned())
					.flex()
					.items_center()
					.w(px(30.0))
					.h(px(18.0))
					.p_px()
					.rounded_full()
					.cursor_pointer()
					.bg(if colorful { p.accent } else { p.track })
					.when(!colorful, |s| s.justify_start())
					.when(colorful, |s| s.justify_end())
					.on_click(cx.listener(move |this, _, _, cx| this.set_colorful_categories(!colorful, cx)))
					.child(div().size(px(14.0)).rounded_full().bg(p.text)),
			);
		deferred(
			div().absolute().inset_0().occlude().flex().items_center().justify_center().bg(p.dim).child(
				div()
					.id("settings-sheet")
					.flex()
					.flex_col()
					.gap_2()
					.w(px(440.0))
					.p_4()
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.bg(p.panel)
					.shadow_lg()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.toggle_settings(false, cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Settings"))
							.child(icon_button(
								p,
								"settings-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.toggle_settings(false, cx)),
							)),
					)
					.child(switch)
					.children(rows),
			),
		)
		.priority(2)
	}
}
