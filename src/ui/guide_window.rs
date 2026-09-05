//! A few lines of guidance in a window of their own, with one button that closes it. Opened by
//! the question mark beside a field; a window rather than a fold-out because the form must not
//! move under the pointer when help is asked for. See spec/ui.md.

use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px};

use crate::ui::button;
use crate::ui::icon::Icon;
use crate::ui::theme;

pub struct GuideWindow {
	title: &'static str,
	lines: &'static [&'static str],
}

impl GuideWindow {
	pub fn new(title: &'static str, lines: &'static [&'static str]) -> GuideWindow {
		GuideWindow { title, lines }
	}

	/// The window's extent, from the lines it holds: wide enough for the longest example, tall
	/// enough for them all and the button.
	pub fn extent(lines: &[&str]) -> gpui::Size<gpui::Pixels> {
		let widest = lines.iter().map(|l| l.len()).max().unwrap_or(0) as f32;
		gpui::size(px((widest * 7.2 + 48.0).clamp(320.0, 640.0)), px(lines.len() as f32 * 20.0 + 96.0))
	}
}

impl Render for GuideWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		window.set_window_title(self.title);
		div()
			.flex()
			.flex_col()
			.size_full()
			.gap_3()
			.p_4()
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.text_xs()
					.children(self.lines.iter().map(|line| div().text_color(p.muted).child(*line))),
			)
			.child(div().flex_1())
			.child(div().flex().justify_end().child(button(
				p,
				"guide-ok",
				Icon::CircleCheck,
				"OK",
				true,
				cx.listener(|_, _, window, _| window.remove_window()),
			)))
	}
}
