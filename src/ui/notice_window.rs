//! A notice in a window of its own: a small panel at the screen's top right, above the other
//! windows, that needs no main window open to be seen. It is the third place a notice can go --
//! the system's centre reaches past the application, the card in the corner reaches only somebody
//! looking at the window, and this reaches somebody whose main window is closed or buried without
//! asking the system for anything. See src/notify.rs and spec/ui.md.
//!
//! It takes no focus. `WindowKind::PopUp` puts it above the rest and `focus: false` leaves the
//! keyboard where it was, so a notice arriving while somebody is typing does not take the next
//! keystroke; a press closes it and brings the application forward, which is what a press on the
//! system's own notification does.

use gpui::{Context, IntoElement, Render, Role, Window, div, prelude::*, px};

use crate::ui::theme::{self, Palette};

/// The panel's extent. Wide enough for a file's name at the density the window uses, and no
/// taller than the two lines it holds.
pub const WIDTH: f32 = 320.0;
pub const HEIGHT: f32 = 64.0;

/// The gap from the screen's edges, and between one panel and the next below it.
pub const MARGIN: f32 = 16.0;
pub const GAP: f32 = 8.0;

pub struct NoticeWindow {
	title: String,
	body: String,
	palette: Palette,
}

impl NoticeWindow {
	pub fn new(title: String, body: String) -> NoticeWindow {
		// A panel of its own is never the active window -- it takes no focus -- so it is painted
		// as the window's active palette rather than the grey an inactive one would get.
		NoticeWindow { title, body, palette: theme::palette(true) }
	}
}

impl Render for NoticeWindow {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		div()
			.id("notice-window")
			.role(Role::Alert)
			.debug_selector(|| "notice-window".to_owned())
			.size_full()
			.flex()
			.flex_col()
			.justify_center()
			.px_3()
			.py_2()
			.gap_0p5()
			.rounded(px(10.0))
			.border_1()
			.border_color(p.border)
			.bg(p.panel)
			.text_size(px(13.0))
			.text_color(p.text)
			.cursor_pointer()
			// A press does what a press on the system's own notification does: takes the notice
			// away and brings the application forward.
			.on_click(cx.listener(|_, _, window, cx| {
				cx.activate(true);
				window.remove_window();
			}))
			.child(div().text_xs().truncate().child(self.title.clone()))
			.when(!self.body.is_empty(), |s| {
				s.child(div().text_xs().text_color(p.muted).truncate().child(self.body.clone()))
			})
	}
}
