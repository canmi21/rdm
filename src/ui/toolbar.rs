use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Status;
use crate::ui::button;
use crate::ui::icon::Icon;

/// The strip the traffic lights share; main.rs derives their offset from it.
pub const HEIGHT: f32 = 36.0;

/// Two labelled buttons: Add URL, and the one thing the selection can do next. Everything else
/// is an icon in the status bar's corner.
impl Rdm {
	pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let next = self.selected().map(|d| match d.status {
			Status::Downloading => (Icon::Pause, "Pause"),
			// A cross up here; the trash can is the icon row's, below.
			Status::Completed => (Icon::X, "Remove"),
			Status::Paused | Status::Queued | Status::Failed => (Icon::Play, "Resume"),
		});
		let (glyph, label) = next.unwrap_or((Icon::Pause, "Pause"));
		div()
			.flex()
			.items_center()
			.gap_0p5()
			.h(px(HEIGHT))
			// The traffic lights sit in this strip because the system titlebar is transparent.
			.pl(px(78.0))
			.pr_2()
			.border_b_1()
			.border_color(p.border)
			.bg(p.panel)
			.child(button(
				p,
				"add",
				Icon::Plus,
				"Add URL",
				true,
				cx.listener(|this, _, window, cx| this.open_add(window, cx)),
			))
			.child(button(
				p,
				"next",
				glyph,
				label,
				next.is_some(),
				cx.listener(|this, _, _, cx| this.act_on_selected(cx)),
			))
	}
}
