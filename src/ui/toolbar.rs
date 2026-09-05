use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Status;
use crate::ui::button;
use crate::ui::icon::Icon;

/// The strip the traffic lights share; main.rs derives their offset from it.
pub const HEIGHT: f32 = 36.0;

/// Actions on the selection and nothing else; what is about the window lives in the status bar.
impl Rdm {
	pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let selected = self.selected();
		let can_pause = selected.is_some_and(|d| d.status == Status::Downloading);
		let can_resume = selected
			.is_some_and(|d| matches!(d.status, Status::Paused | Status::Failed | Status::Queued));
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
				cx.listener(|this, _, _, cx| this.add(cx)),
			))
			.child(button(
				p,
				"pause",
				Icon::Pause,
				"Pause",
				can_pause,
				cx.listener(|this, _, _, cx| this.pause_selected(cx)),
			))
			.child(button(
				p,
				"resume",
				Icon::Play,
				"Resume",
				can_resume,
				cx.listener(|this, _, _, cx| this.resume_selected(cx)),
			))
			.child(button(
				p,
				"remove",
				Icon::Trash,
				"Remove",
				selected.is_some(),
				cx.listener(|this, _, _, cx| this.remove_selected(cx)),
			))
	}
}
