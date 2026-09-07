//! The small label that appears under the pointer after it has rested on an icon: what an icon
//! alone cannot say. GPUI shows it after half a second and places it itself.

use gpui::{AnyView, App, Context, IntoElement, Render, SharedString, Window, div, prelude::*};

use crate::ui::theme;

pub struct Tooltip {
	text: SharedString,
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
			.whitespace_nowrap()
			.child(self.text.clone())
	}
}

/// The builder an element's `.tooltip(...)` takes, for a fixed piece of text.
pub fn tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> AnyView {
	let text = text.into();
	move |_, cx| cx.new(|_| Tooltip { text: text.clone() }).into()
}
