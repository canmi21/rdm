use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Status;
use crate::ui::{button, theme};

impl Rdm {
	pub(crate) fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let selected = self.selected();
		let can_pause = selected.is_some_and(|d| d.status == Status::Downloading);
		let can_resume = selected
			.is_some_and(|d| matches!(d.status, Status::Paused | Status::Failed | Status::Queued));
		div()
			.flex()
			.items_center()
			.gap_1()
			.h(px(52.0))
			// The traffic lights sit in this strip because the system titlebar is transparent.
			.pl(px(80.0))
			.pr_3()
			.border_b_1()
			.border_color(theme::border())
			.bg(theme::panel())
			.child(button("add", "Add URL", true, cx.listener(|this, _, _, cx| this.add(cx))))
			.child(button(
				"pause",
				"Pause",
				can_pause,
				cx.listener(|this, _, _, cx| this.pause_selected(cx)),
			))
			.child(button(
				"resume",
				"Resume",
				can_resume,
				cx.listener(|this, _, _, cx| this.resume_selected(cx)),
			))
			.child(button(
				"remove",
				"Remove",
				selected.is_some(),
				cx.listener(|this, _, _, cx| this.remove_selected(cx)),
			))
	}
}
