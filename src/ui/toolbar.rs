use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::{Rdm, View};
use crate::download::Status;
use crate::ui::button;
use crate::ui::icon::{Icon, icon};

/// The strip the traffic lights share; main.rs derives their offset from it.
pub const HEIGHT: f32 = 36.0;

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
			.child(div().flex_1())
			.child(self.view_switch(cx))
	}

	/// One segment per view, the active one raised on the panel like a pressed key.
	fn view_switch(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let segments: Vec<_> = View::ALL
			.iter()
			.map(|view| {
				let view = *view;
				let active = self.view == view;
				let color = if active { p.text } else { p.muted };
				div()
					.id(view_id(view))
					.flex()
					.items_center()
					.justify_center()
					.size_6()
					.rounded_sm()
					.cursor_pointer()
					.when(active, |s| s.bg(p.selection))
					.when(!active, move |s| s.hover(move |s| s.bg(p.hover)))
					.on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx)))
					.child(icon(view_icon(view), color).size_3p5())
			})
			.collect();
		div().flex().items_center().gap_px().p_px().rounded_md().bg(p.window).children(segments)
	}
}

fn view_id(view: View) -> &'static str {
	match view {
		View::Detailed => "view-detailed",
		View::Compact => "view-compact",
		View::Grid => "view-grid",
	}
}

fn view_icon(view: View) -> Icon {
	match view {
		View::Detailed => Icon::LayoutList,
		View::Compact => Icon::Rows,
		View::Grid => Icon::LayoutGrid,
	}
}
