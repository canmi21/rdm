//! Settings live in a window of their own, as a native application's do.

use gpui::{Context, IntoElement, Render, Window, div, prelude::*, px};

use crate::ui::theme;

pub struct SettingsWindow;

// TODO: every row here is a label until there is a setting behind it and a store to keep it in.
const ROWS: [(&str, &str); 4] = [
	("Download folder", "~/Downloads"),
	("Concurrent downloads", "3"),
	("Speed limit", "Off"),
	("On completion", "Do nothing"),
];

impl Render for SettingsWindow {
	fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let rows = ROWS.iter().map(|(name, value)| {
			div()
				.flex()
				.justify_between()
				.items_center()
				.py_1p5()
				.border_b_1()
				.border_color(p.border)
				.child(*name)
				.child(div().text_color(p.muted).child(*value))
		});
		div()
			.flex()
			.flex_col()
			.size_full()
			.px_4()
			.py_2()
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.children(rows)
	}
}
