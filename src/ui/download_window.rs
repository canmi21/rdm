//! One download in a window of its own, opened by double-clicking its row.

use gpui::{
	Context, Entity, IntoElement, Render, Subscription, Window, div, prelude::*, px, relative, text,
};

use crate::app::Rdm;
use crate::download::{Status, format_bytes, format_duration, format_speed};
use crate::ui::button;
use crate::ui::icon::Icon;
use crate::ui::theme;

pub struct DownloadWindow {
	rdm: Entity<Rdm>,
	id: u64,
	/// What about a download can be changed here, each applied on Enter to the engine and kept on
	/// the row: its own limit, and its connections, Auto or a count. How a download runs is
	/// settled here, while it runs, rather than asked at Add Task. See spec/ui.md.
	limit: Entity<crate::ui::text_input::TextInput>,
	connections: Entity<crate::ui::text_input::TextInput>,
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
		let asked = rdm.read(cx).download(id).and_then(|d| d.connections);
		let owner = rdm.clone();
		let connections = cx.new(|cx| {
			let mut field =
				crate::ui::text_input::TextInput::new("Auto", cx).on_confirm(move |text, _, cx| {
					// A count out of range is left unapplied, and the field keeps what was typed.
					let Ok(count) = crate::ui::add_dialog::parse_connections(text) else { return };
					owner.update(cx, |rdm, cx| rdm.set_task_connections(id, count, cx));
				});
			if let Some(count) = asked {
				field.set_content(&count.to_string(), cx);
			}
			field
		});
		Self { rdm, id, limit, connections, _follow: follow }
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
		let most = crate::engine::Connections::MAX;
		let connections_hint = match self.rdm.read(cx).open_connections(id) {
			Some(open) if download.status == Status::Downloading => {
				format!("Auto, or 1 to {most}; {open} open; Enter applies")
			}
			_ => format!("Auto, or 1 to {most}; Enter applies"),
		};
		let can_resume = matches!(download.status, Status::Paused | Status::Failed | Status::Queued);
		let resume = rdm.clone();
		let remove = rdm.clone();
		// The name is the window's title; the body starts with what the title cannot hold.
		// Where it came from, which is the first thing anybody opening this window wants. A row
		// with no address was not downloaded here: it is a file the folder already held, and
		// saying so is better than showing an empty line.
		let came_from = if download.url.is_empty() {
			"Already in the download folder".to_owned()
		} else {
			download.url.clone()
		};
		frame
			.child(field(p.muted, "From", came_from))
			.when_some(download.source.clone(), |s, page| {
				// The page it was found on, where it was found on one rather than typed in.
				s.child(field(p.muted, "Found on", page))
			})
			.child(field(p.muted, "Size", format_bytes(download.size)))
			.child(field(
				p.muted,
				"Folder",
				download.directory.clone().unwrap_or_else(|| "Download folder".to_owned()),
			))
			.when(!download.mirrors.is_empty(), |s| {
				s.child(field(p.muted, "Mirrors", download.mirrors.join(" ")))
			})
			.when_some(download.checksum.clone(), |s, sum| s.child(field(p.muted, "Checksum", sum)))
			.when_some(download.range.clone(), |s, range| s.child(field(p.muted, "Range", range)))
			.when_some(download.error.clone(), |s, error| s.child(field(p.failure, "Error", error)))
			.child(control(
				p.muted,
				"Limit",
				self.limit.clone(),
				"KB/s, or with m or g; Enter applies".to_owned(),
			))
			.child(control(p.muted, "Connections", self.connections.clone(), connections_hint))
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

/// A setting of this download's own: its name, a field applied on Enter, and what the field takes.
fn control(
	muted: gpui::Hsla,
	name: &'static str,
	input: Entity<crate::ui::text_input::TextInput>,
	hint: String,
) -> impl IntoElement {
	div()
		.flex()
		.items_center()
		.gap_2()
		.text_xs()
		.child(div().w(px(72.0)).flex_none().text_color(muted).child(text!(id = (name, 0usize), name)))
		.child(div().w(px(112.0)).flex_none().child(input))
		.child(div().min_w_0().truncate().text_color(muted).child(text!(id = (name, 1usize), hint)))
}
