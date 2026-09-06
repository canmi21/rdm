//! One download in a window of its own, opened by double-clicking its row.

use gpui::{
	Context, Entity, IntoElement, Render, Subscription, Window, div, prelude::*, px, relative,
};

use crate::app::Rdm;
use crate::download::{Status, format_bytes, format_duration, format_speed};
use crate::ui::button;
use crate::ui::icon::Icon;
use crate::ui::theme;

pub struct DownloadWindow {
	rdm: Entity<Rdm>,
	id: u64,
	/// The one thing about a running download that can be changed here: its own limit,
	/// applied on Enter to the engine and kept on the row.
	limit: Entity<crate::ui::text_input::TextInput>,
	_follow: Subscription,
}

impl DownloadWindow {
	pub fn new(rdm: Entity<Rdm>, id: u64, cx: &mut Context<Self>) -> Self {
		// The main view owns the downloads; this window only looks at one of them, so it redraws
		// whenever that view changes and holds no copy of its own.
		let follow = cx.observe(&rdm, |_, _, cx| cx.notify());
		let current = rdm.read(cx).download(id).and_then(|d| d.speed_limit);
		let owner = rdm.clone();
		let limit = cx.new(|cx| {
			let mut field =
				crate::ui::text_input::TextInput::new("Off", cx).on_confirm(move |text, _, cx| {
					let limit = crate::download::parse_rate(text).unwrap_or(None);
					owner.update(cx, |rdm, cx| rdm.set_task_speed_limit(id, limit, cx));
				});
			if let Some(limit) = current {
				field.set_content(&crate::download::format_rate(Some(limit)), cx);
			}
			field
		});
		Self { rdm, id, limit, _follow: follow }
	}
}

impl Render for DownloadWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let id = self.id;
		let rdm = self.rdm.clone();
		let frame = div()
			.flex()
			.flex_col()
			.size_full()
			.gap_3()
			.p_4()
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text);
		let Some(download) = self.rdm.read(cx).download(id).cloned() else {
			// Removed from the list while this window was open: nothing left to show.
			window.remove_window();
			return frame;
		};
		window.set_window_title(&download.name);
		let tint = p.status(download.status);
		let mut state = download.status.label().to_owned();
		if download.speed > 0 {
			state.push_str(&format!(", {}", format_speed(download.speed)));
		}
		if let Some(left) = download.remaining() {
			state.push_str(&format!(", {} left", format_duration(left)));
		}
		let can_pause = download.status == Status::Downloading;
		let can_resume = matches!(download.status, Status::Paused | Status::Failed | Status::Queued);
		let resume = rdm.clone();
		let remove = rdm.clone();
		// The name is the window's title; the body starts with what the title cannot hold.
		frame
			.child(field(p.muted, "URL", download.url.clone()))
			.child(field(p.muted, "Size", format_bytes(download.size)))
			.child(field(
				p.muted,
				"Folder",
				download.directory.clone().unwrap_or_else(|| "Download folder".to_owned()),
			))
			.child(field(
				p.muted,
				"Connections",
				download.connections.map(|n| n.to_string()).unwrap_or_else(|| "Auto".to_owned()),
			))
			.when(!download.mirrors.is_empty(), |s| {
				s.child(field(p.muted, "Mirrors", download.mirrors.join(" ")))
			})
			.when_some(download.checksum.clone(), |s, sum| s.child(field(p.muted, "Checksum", sum)))
			.when_some(download.range.clone(), |s, range| s.child(field(p.muted, "Range", range)))
			.when_some(download.error.clone(), |s, error| s.child(field(p.failure, "Error", error)))
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.text_xs()
					.child(div().w(px(36.0)).flex_none().text_color(p.muted).child("Limit"))
					.child(div().w(px(112.0)).child(self.limit.clone()))
					.child(div().text_color(p.muted).child("KB/s, or with m or g; Enter applies")),
			)
			.child(field(p.muted, "Contents", {
				let contents = self.rdm.read(cx).contents_of(&download);
				match contents.len() {
					0 => String::new(),
					n if n <= 6 => contents.join(", "),
					n => format!("{}, and {} more", contents[..6].join(", "), n - 6),
				}
			}))
			.child(field(
				p.muted,
				"Category",
				self
					.rdm
					.read(cx)
					.categories_of(&download)
					.iter()
					.map(|c| c.name.as_str())
					.collect::<Vec<_>>()
					.join(", "),
			))
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.h(px(6.0))
							.w_full()
							.rounded_full()
							.bg(p.track)
							.child(div().h_full().rounded_full().w(relative(download.progress())).bg(tint)),
					)
					.child(
						div()
							.flex()
							.justify_between()
							.text_xs()
							.text_color(p.muted)
							.child(format!(
								"{} of {}",
								format_bytes(download.received),
								format_bytes(download.size)
							))
							.child(div().text_color(tint).child(state)),
					),
			)
			.child(div().flex_1())
			.child(
				div()
					.flex()
					.gap_1()
					.child(button(p, "pause", Icon::Pause, "Pause", can_pause, move |_, _, cx| {
						rdm.update(cx, |rdm, cx| rdm.pause(id, cx));
					}))
					.child(button(p, "resume", Icon::Play, "Resume", can_resume, move |_, _, cx| {
						resume.update(cx, |rdm, cx| rdm.resume(id, cx));
					}))
					.child(button(p, "remove", Icon::Trash, "Remove", true, move |_, _, cx| {
						remove.update(cx, |rdm, cx| rdm.remove(id, cx));
					})),
			)
	}
}

fn field(label: gpui::Hsla, name: &'static str, value: String) -> impl IntoElement {
	div()
		.flex()
		.gap_2()
		.text_xs()
		.child(div().w(px(36.0)).flex_none().text_color(label).child(name))
		.child(div().flex_1().min_w_0().truncate().child(value))
}
