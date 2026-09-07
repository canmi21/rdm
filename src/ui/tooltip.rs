//! The small label that appears under the pointer after it has rested on an icon: what an icon
//! alone cannot say. GPUI shows it after half a second and places it itself.

use gpui::{AnyView, App, Context, IntoElement, Render, SharedString, Window, div, prelude::*};

use crate::ui::theme;

pub struct Tooltip {
	text: SharedString,
	/// A label wider than a few words wraps inside a ceiling instead of running off the display.
	/// An icon's name never needs it; a setting's note is a whole sentence and always does.
	wrapped: bool,
}

impl Render for Tooltip {
	fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		div()
			.px_1p5()
			.py_0p5()
			.rounded_sm()
			.border_1()
			.border_color(p.border)
			.bg(p.panel)
			.text_xs()
			.text_color(p.text)
			.shadow_md()
			.when(!self.wrapped, |s| s.whitespace_nowrap())
			.when(self.wrapped, |s| s.max_w(gpui::px(260.0)))
			.child(self.text.clone())
	}
}

/// The builder an element's `.tooltip(...)` takes, for a fixed piece of text.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
	let text = text.into();
	move |_, cx| cx.new(|_| Tooltip { text: text.clone(), wrapped: false }).into()
}

/// The same for a sentence rather than a name: it wraps within a ceiling. This is what a
/// setting's row uses, its note having moved off the screen and under the pointer so that a row
/// is one line. See spec/ui.md.
pub fn tooltip_wrapped(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
	let text = text.into();
	move |_, cx| cx.new(|_| Tooltip { text: text.clone(), wrapped: true }).into()
}
