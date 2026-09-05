//! A few lines of guidance laid over whatever sheet is up, with one button that takes them
//! away again. Over the sheet rather than inside it, so the form does not move under the
//! pointer; inside the window rather than a window of its own, so it is where the eye already
//! is and goes away with one press. See spec/ui.md.

use gpui::{Context, IntoElement, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::button;
use crate::ui::icon::Icon;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guide {
	pub title: &'static str,
	pub lines: &'static [&'static str],
}

impl Rdm {
	pub(crate) fn show_guide(&mut self, guide: Guide, cx: &mut Context<Self>) {
		self.guide = Some(guide);
		cx.notify();
	}

	pub(crate) fn close_guide(&mut self, cx: &mut Context<Self>) {
		self.guide = None;
		cx.notify();
	}

	/// Above every sheet, on a backdrop that takes every press so the form beneath cannot be
	/// touched through it; only OK closes it, since it was asked for.
	pub(crate) fn guide_sheet(
		&self,
		guide: Guide,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		deferred(
			div().absolute().inset_0().occlude().flex().items_center().justify_center().bg(p.dim).child(
				div()
					.id("guide")
					.debug_selector(|| "guide".to_owned())
					.flex()
					.flex_col()
					.gap_3()
					.w(px(440.0))
					.p_4()
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.bg(p.panel)
					.shadow_lg()
					.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(guide.title))
					.child(
						div()
							.flex()
							.flex_col()
							.gap_1()
							.text_xs()
							.text_color(p.muted)
							.children(guide.lines.iter().map(|line| div().child(*line))),
					)
					.child(div().flex().justify_end().child(button(
						p,
						"guide-ok",
						Icon::CircleCheck,
						"OK",
						true,
						cx.listener(|this, _, _, cx| this.close_guide(cx)),
					))),
			),
		)
		.priority(3)
	}
}
