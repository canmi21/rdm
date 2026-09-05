//! A few lines of guidance laid over whatever sheet is up. Over the sheet rather than inside
//! it, so the form does not move under the pointer; inside the window rather than a window of
//! its own, so it is where the eye already is. It has nothing to save, so it goes away like any
//! clean sheet: the cross, Escape, or a press outside it. See spec/ui.md.

use gpui::{Context, IntoElement, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::icon::Icon;
use crate::ui::{backdrop, icon_button};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guide {
	pub title: &'static str,
	/// What the guide is about, in one sentence under the title.
	pub about: &'static str,
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
	/// touched through it. A press on the backdrop closes the guide and nothing else.
	pub(crate) fn guide_sheet(
		&self,
		guide: Guide,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		deferred(
			backdrop(p).child(
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
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_guide(cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(guide.title))
							.child(icon_button(
								p,
								"guide-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_guide(cx)),
							)),
					)
					.child(div().text_xs().child(guide.about))
					.child(
						div()
							.flex()
							.flex_col()
							.gap_1()
							.text_xs()
							.text_color(p.muted)
							.children(guide.lines.iter().map(|line| div().child(*line))),
					),
			),
		)
		.priority(3)
	}
}
