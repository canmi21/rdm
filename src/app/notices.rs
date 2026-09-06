//! Where a notice goes once something has decided it is worth saying: the system's notification
//! centre, a card in the window's corner, or nowhere. What is worth saying is each caller's to
//! judge -- a finished download, a failed one, an emptied queue, a newer build -- and where it is
//! said is the user's, one choice a moment under Notifications. See src/notify.rs and spec/ui.md.

use std::time::{Duration, Instant};

use gpui::{Context, IntoElement, Role, div, prelude::*, px};

use crate::app::Rdm;
use crate::identity;
use crate::notify::{Occasion, Style};
use crate::ui::status_bar;

/// A notice on show in the window's corner.
pub struct Shown {
	pub title: String,
	pub body: String,
	at: Instant,
}

/// How long a card stays before it goes on its own: long enough to read a file's name, short
/// enough that a queue finishing does not leave a wall of them.
const KEEP: Duration = Duration::from_secs(6);

/// How many cards the corner holds at once. Past this the oldest goes, since a corner stacked
/// past the window's height would cover the list it is meant to sit over.
const AT_ONCE: usize = 4;

impl Rdm {
	/// Says something, in the place this occasion's setting names. Nothing is said anywhere else
	/// if that place cannot take it: a notice arriving somewhere it was not asked for is worse
	/// than one that does not arrive.
	pub(crate) fn tell_of(
		&mut self,
		occasion: Occasion,
		title: String,
		body: String,
		cx: &mut Context<Self>,
	) {
		match self.preferences.notice(occasion) {
			Style::Silent => {}
			Style::System => system(&title, &body, cx),
			// A newer build has a card of its own, with a button on it, which the corner is already
			// drawing; a second card saying the same thing would be one too many.
			Style::InApp | Style::Window if occasion == Occasion::Update => {}
			Style::InApp | Style::Window => {
				self.notices.push(Shown { title, body, at: Instant::now() });
				if self.notices.len() > AT_ONCE {
					self.notices.remove(0);
				}
				cx.notify();
			}
		}
	}

	/// Settings' Notifications rows: where this occasion is said from now on, kept in config.json.
	pub(crate) fn set_notice(&mut self, occasion: Occasion, style: Style, cx: &mut Context<Self>) {
		self.preferences.set_notice(occasion, style);
		self.save_config();
		cx.notify();
	}

	/// Drops the cards that have had their time. Called from the window's tick, which is also
	/// what makes them go without a press.
	pub(crate) fn expire_notices(&mut self, cx: &mut Context<Self>) {
		let before = self.notices.len();
		self.notices.retain(|notice| notice.at.elapsed() < KEEP);
		if self.notices.len() != before {
			cx.notify();
		}
	}

	pub(crate) fn dismiss_notice(&mut self, at: usize, cx: &mut Context<Self>) {
		if at < self.notices.len() {
			self.notices.remove(at);
			cx.notify();
		}
	}

	/// The window's corner: the notices, oldest at the top, with the update's card under them --
	/// one column, so a notice arriving does not land on top of the card or the card on top of
	/// it. Nothing is drawn at all when there is nothing to say.
	pub(crate) fn corner(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
		let toast = self.update_toast(cx);
		if self.notices.is_empty() && toast.is_none() {
			return None;
		}
		let p = self.palette;
		let cards: Vec<_> = self
			.notices
			.iter()
			.enumerate()
			.map(|(at, notice)| {
				div()
					.id(("notice", at))
					.role(Role::Alert)
					.debug_selector(move || format!("notice:{at}"))
					.max_w(px(420.0))
					.flex()
					.flex_col()
					.px_2p5()
					.py_1p5()
					.rounded_md()
					.border_1()
					.border_color(p.border)
					.bg(p.panel)
					.shadow_md()
					.cursor_pointer()
					.hover(move |s| s.bg(p.hover))
					.on_click(cx.listener(move |this, _, _, cx| this.dismiss_notice(at, cx)))
					.child(div().text_xs().child(notice.title.clone()))
					.when(!notice.body.is_empty(), |s| {
						s.child(div().text_xs().text_color(p.muted).truncate().child(notice.body.clone()))
					})
					.into_any_element()
			})
			.collect();
		Some(
			div()
				.absolute()
				.bottom(px(status_bar::HEIGHT + 12.0))
				.right(px(12.0))
				.flex()
				.flex_col()
				.items_end()
				.gap_1p5()
				.children(cards)
				.when_some(toast, |s, toast| s.child(toast)),
		)
	}
}

/// The system's own notification centre. macOS delivers one only on behalf of an installed
/// bundle; a binary run from the build tree has none and the call fails quietly, which is why a
/// development build is not the place to check that this works.
fn system(title: &str, body: &str, cx: &mut Context<Rdm>) {
	let summary = title.to_owned();
	let body = body.to_owned();
	cx.spawn(async move |this, cx| {
		let pressed = cx
			.background_executor()
			.spawn(async move {
				#[cfg(target_os = "macos")]
				let _ = notify_rust::set_application(&identity::id());
				let mut notification = notify_rust::Notification::new();
				notification.summary(&summary).body(&body);
				#[cfg(target_os = "linux")]
				{
					// Linux hands the press back to the application, which the others do by
					// activating it; the action has to be declared for the press to exist.
					notification.action("default", "Open");
					match notification.show() {
						Ok(handle) => {
							let mut pressed = false;
							handle.wait_for_action(|action| pressed = action == "default");
							pressed
						}
						Err(_) => false,
					}
				}
				#[cfg(not(target_os = "linux"))]
				{
					let _ = notification.show();
					false
				}
			})
			.await;
		if pressed {
			let _ = this.update(cx, |_, cx| cx.activate(true));
			cx.update(|cx| {
				for handle in cx.windows() {
					let _ = handle.update(cx, |_, window, _| window.activate_window());
				}
			});
		}
	})
	.detach();
}
