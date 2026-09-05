//! Adding a download is a sheet inside the main window: one field over a dimmed list. The other
//! windows stay windows; this one is tried as a dialog to see how it reads.

use gpui::{Context, Entity, IntoElement, Window, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::ui::button;
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;

impl Rdm {
	/// Opens the sheet with the field focused; a second press just refocuses it.
	pub(crate) fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let input = match &self.adding {
			Some(input) => input.clone(),
			None => {
				let rdm = cx.entity();
				let cancel = rdm.clone();
				let input = cx.new(|cx| {
					TextInput::new("https://", cx)
						.on_confirm(move |text, _, cx| {
							if !text.is_empty() {
								rdm.update(cx, |this, cx| {
									this.add_url(text, cx);
									this.close_add(cx);
								});
							}
						})
						.on_cancel(move |_, cx| cancel.update(cx, |this, cx| this.close_add(cx)))
				});
				self.adding = Some(input.clone());
				input
			}
		};
		window.focus(&input.read(cx).focus(), cx);
		cx.notify();
	}

	/// A click outside closes the sheet only while nothing has been typed; typed text is kept until
	/// the cross is pressed. See spec/ui.md.
	pub(crate) fn dismiss_add(&mut self, cx: &mut Context<Self>) {
		let clean = self.adding.as_ref().is_none_or(|input| input.read(cx).content.trim().is_empty());
		if clean {
			self.close_add(cx);
		}
	}

	pub(crate) fn close_add(&mut self, cx: &mut Context<Self>) {
		self.adding = None;
		cx.notify();
	}

	fn submit_add(&mut self, cx: &mut Context<Self>) {
		let Some(input) = &self.adding else { return };
		let text = input.read(cx).content.trim().to_owned();
		if text.is_empty() {
			return;
		}
		self.add_url(&text, cx);
		self.close_add(cx);
	}

	/// Drawn over everything from the window root; a click outside the sheet closes it.
	pub(crate) fn add_dialog(
		&self,
		input: Entity<TextInput>,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let ready = !input.read(cx).content.trim().is_empty();
		deferred(
			// The backdrop takes every mouse event, so nothing behind the sheet can be pressed through it.
			div().absolute().inset_0().occlude().flex().items_center().justify_center().bg(p.dim).child(
				div()
					.id("add-dialog")
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
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_add(cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Add URL"))
							.child(crate::ui::icon_button(
								p,
								"add-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_add(cx)),
							)),
					)
					.child(input)
					.child(div().flex().justify_end().child(button(
						p,
						"add-confirm",
						Icon::Plus,
						"Add",
						ready,
						cx.listener(|this, _, _, cx| this.submit_add(cx)),
					))),
			),
		)
		.priority(2)
	}
}
