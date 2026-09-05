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
	/// What the guide is about, in one sentence under the title -- one line of the card, so it
	/// is read whole before the examples.
	pub about: &'static str,
	/// One line per shape: its name, muted, and the examples in the text color. Two colors on
	/// a line because a column of lines in one color reads as a block, not as a list.
	pub lines: &'static [(&'static str, &'static str)],
	/// The caveat at the end, muted: what to know, after what to type.
	pub note: &'static str,
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
					.child(div().flex().flex_col().gap_1().text_xs().children(guide.lines.iter().map(
						|(name, examples)| {
							div()
								.flex()
								.gap_2()
								.child(div().w(px(32.0)).flex_none().text_color(p.muted).child(*name))
								.child(*examples)
						},
					)))
					.child(div().text_xs().text_color(p.muted).child(guide.note)),
			),
		)
		.priority(3)
	}
}
