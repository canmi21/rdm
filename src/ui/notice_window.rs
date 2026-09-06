//! A notice in a window of its own: a dialog in the middle of the screen, above the other
//! windows, that needs no main window open to be seen. It is the third place a notice can go --
//! the system's centre reaches past the application, the card in the corner reaches only somebody
//! looking at the window, and this reaches somebody whose main window is closed or buried without
//! asking the system for anything. See src/notify.rs and spec/ui.md.
//!
//! It wears no frame of the system's. A rounded rectangle, a cross at the top right, what
//! happened and what it happened to, and three things to do about it. The cross does nothing but
//! close; each of the three does its thing and closes, since a dialog that stayed open after
//! being acted on would be a dialog to close twice.
//!
//! It takes no focus. `WindowKind::PopUp` puts it above the rest and `focus: false` leaves the
//! keyboard where it was, so a notice arriving while somebody is typing does not take the next
//! keystroke.

use gpui::{Context, IntoElement, Render, Role, SharedString, Window, div, prelude::*, px};

use crate::download::format_bytes;
use crate::notify::Notice;
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::{self, Palette};

/// The dialog's extent. Wide enough for a file's name at the density the window uses and for
/// three words side by side under it; no taller than what it holds.
pub const WIDTH: f32 = 380.0;
pub const HEIGHT: f32 = 168.0;

pub struct NoticeWindow {
	notice: Notice,
	/// What the user named to show a file where it lives, which only Linux has to ask about.
	manager: String,
	palette: Palette,
}

impl NoticeWindow {
	pub fn new(notice: Notice, manager: String) -> NoticeWindow {
		// A dialog of its own is never the active window -- it takes no focus -- so it is painted
		// in the window's active palette rather than the grey an inactive one would get.
		NoticeWindow { notice, manager, palette: theme::palette(true) }
	}

	/// One of the three: a word in a row of three, each doing its thing and closing.
	fn action(
		&self,
		id: &'static str,
		word: SharedString,
		run: impl Fn(&mut NoticeWindow, &mut Window, &mut Context<NoticeWindow>) + 'static,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let p = self.palette;
		div()
			.id(id)
			.role(Role::Button)
			.aria_label(word.to_string())
			.debug_selector(move || format!("notice:{id}"))
			.flex_1()
			.flex()
			.items_center()
			.justify_center()
			.py_1p5()
			.rounded_md()
			.border_1()
			.border_color(p.border)
			.text_xs()
			.cursor_pointer()
			.hover(move |s| s.bg(p.hover))
			.on_click(cx.listener(move |this, _, window, cx| {
				run(this, window, cx);
				window.remove_window();
			}))
			.child(word)
	}
}

impl Render for NoticeWindow {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let file = self.notice.file.clone();
		// Size and time, on one muted line, and only what there is: a notice about nothing in
		// particular has neither, and says neither rather than saying zero.
		let detail = file.as_ref().map(|f| {
			let took = f.took.as_secs();
			let spent = if took < 60 {
				format!("{took}s")
			} else if took < 3600 {
				format!("{}m {}s", took / 60, took % 60)
			} else {
				format!("{}h {}m", took / 3600, (took % 3600) / 60)
			};
			format!("{} in {spent}", format_bytes(f.size))
		});
		div()
			.id("notice-window")
			.role(Role::Alert)
			.debug_selector(|| "notice-window".to_owned())
			.size_full()
			.flex()
			.flex_col()
			.p_4()
			.gap_1()
			.rounded(px(12.0))
			.border_1()
			.border_color(p.border)
			.bg(p.panel)
			.text_size(px(13.0))
			.text_color(p.text)
			.child(
				div()
					.flex()
					.items_start()
					.gap_2()
					.child(div().flex_1().min_w_0().child(self.notice.title.clone()))
					// The cross does nothing but close, which is the whole of what it promises.
					.child(
						div()
							.id("notice-close")
							.role(Role::Button)
							.aria_label(crate::i18n::t("dialog.close"))
							.debug_selector(|| "notice:close".to_owned())
							.flex()
							.flex_none()
							.items_center()
							.justify_center()
							.size_5()
							.rounded_sm()
							.cursor_pointer()
							.hover(move |s| s.bg(p.hover))
							.on_click(cx.listener(|_, _, window, _| window.remove_window()))
							.child(icon(Icon::Close, p.muted).size_3()),
					),
			)
			.child(div().text_xs().text_color(p.muted).truncate().child(self.notice.body.clone()))
			.when_some(detail, |s, detail| {
				s.child(div().text_xs().text_color(p.muted).child(detail))
			})
			.child(div().flex_1())
			.child(
				div()
					.flex()
					.gap_2()
					.child(self.action(
						"open",
						crate::i18n::t("dialog.open").into(),
						move |this, _, _| {
							if let Some(file) = &this.notice.file {
								crate::reveal::open(&file.path);
							}
						},
						cx,
					))
					.child(self.action(
						"show",
						crate::i18n::t("dialog.show_in").replace("{}", crate::reveal::manager_name()).into(),
						move |this, _, _| {
							if let Some(file) = &this.notice.file {
								crate::reveal::show(&file.path, &this.manager);
							}
						},
						cx,
					))
					.child(self.action(
						"window",
						crate::i18n::t("dialog.window").into(),
						|_, _, cx| {
							cx.activate(true);
							for handle in cx.windows() {
								let _ = handle.update(cx, |_, window, _| window.activate_window());
							}
						},
						cx,
					)),
			)
	}
}
