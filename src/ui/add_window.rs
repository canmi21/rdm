//! Adding a download is a small window with one field, as a native application asks for a URL.

use gpui::{Context, Entity, Focusable, IntoElement, Render, Window, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::button;
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;
use crate::ui::theme;

pub struct AddWindow {
	rdm: Entity<Rdm>,
	url: Entity<TextInput>,
}

impl AddWindow {
	pub fn new(rdm: Entity<Rdm>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let target = rdm.clone();
		let url = cx.new(|cx| {
			TextInput::new("https://", cx).on_confirm(move |text, window, cx| {
				if !text.is_empty() {
					target.update(cx, |rdm, cx| rdm.add_url(text, cx));
					window.remove_window();
				}
			})
		});
		window.focus(&url.read(cx).focus_handle(cx), cx);
		Self { rdm, url }
	}

	fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let text = self.url.read(cx).content.trim().to_owned();
		if text.is_empty() {
			return;
		}
		self.rdm.update(cx, |rdm, cx| rdm.add_url(&text, cx));
		window.remove_window();
	}
}

impl Render for AddWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let ready = !self.url.read(cx).content.trim().is_empty();
		div()
			.flex()
			.flex_col()
			.size_full()
			.gap_3()
			.p_4()
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.child(div().text_xs().text_color(p.muted).child("Address"))
			.child(self.url.clone())
			.child(div().flex_1())
			.child(div().flex().justify_end().child(button(
				p,
				"confirm",
				Icon::Plus,
				"Add",
				ready,
				cx.listener(|this, _, window, cx| this.submit(window, cx)),
			)))
	}
}
