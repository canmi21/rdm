//! One download in a window of its own, titled with its name and how far it has come, opened by
//! double-clicking its row. See spec/ui.md, "One thing, one window".

use gpui::{
	ClipboardItem, Context, Entity, IntoElement, Render, Subscription, Window, div, prelude::*, px,
	relative, text,
};

use crate::app::Rdm;
use crate::download::{Download, Status, format_bytes, format_duration, format_speed};
use crate::engine::Connections;
use crate::engine::segments::Segment;
use crate::ui::add_dialog::{
	format_limit, limit_at, limit_position, limit_snap, parse_connections, parse_limit,
};
use crate::ui::icon::{Icon, icon};
use crate::ui::slider::Slider;
use crate::ui::text_input::TextInput;
use crate::ui::{frame, icon_button, theme, toolbar};

/// A megabyte, as the limit's field counts it.
const MB: f64 = 1_048_576.0;

/// How far along the connections slider a count sits: on a log scale from one to the most, two
/// to a step at the coarsest, with Auto a place of its own past the scale's high end, as no limit
/// is on the limit's.
const COUNT_SCALE: f32 = 0.9;

fn count_position(count: Option<u16>) -> f32 {
	match count {
		None => 1.0,
		Some(n) => {
			let top = f32::from(Connections::MAX).log2();
			(f32::from(n.max(1)).log2() / top).clamp(0.0, 1.0) * COUNT_SCALE
		}
	}
}

/// The count a place on the slider stands for, or None for Auto past the middle of the gap after
/// the scale.
fn count_at(position: f32) -> Option<u16> {
	if position > (1.0 + COUNT_SCALE) / 2.0 {
		return None;
	}
	let top = f32::from(Connections::MAX).log2();
	let n = 2f32.powf((position / COUNT_SCALE).clamp(0.0, 1.0) * top).round();
	Some((n as u16).clamp(1, Connections::MAX))
}

/// Where a drag lands: on the powers of two at the coarsest -- 1, 2, 4 and on to the most, even on
/// the scale -- and on whole counts at every level finer, or on Auto.
fn count_snap(level: usize, position: f32) -> f32 {
	if position > (1.0 + COUNT_SCALE) / 2.0 {
		return 1.0;
	}
	let top = f32::from(Connections::MAX).log2();
	if level == 0 {
		let step = COUNT_SCALE / top;
		return ((position / step).round() * step).min(COUNT_SCALE);
	}
	count_position(count_at(position))
}

pub struct DownloadWindow {
	rdm: Entity<Rdm>,
	id: u64,
	/// What about a download can be changed here, each a slider for setting it roughly beside a
	/// field that sets it exactly, as New Task's limit is: its own limit, in MB/s, none at the far
	/// right; and its connections, Auto at the far right. Each applies as it is moved or typed, to
	/// the engine and the row. See spec/ui.md.
	limit: Entity<TextInput>,
	limit_slider: Entity<Slider>,
	connections: Entity<TextInput>,
	connections_slider: Entity<Slider>,
	/// The parts as the control file on disk last had them, read once while the engine does not
	/// hold the download -- paused since a restart, say -- and dropped as soon as it does.
	saved: Option<Option<Vec<Segment>>>,
	/// When the address was last copied: the copy button shows a check for a moment after, so a
	/// press that changes nothing on screen still says it worked.
	copied: Option<std::time::Instant>,
	_follow: Subscription,
}

/// How long the copy button shows its check.
const COPIED: std::time::Duration = std::time::Duration::from_millis(1500);

impl DownloadWindow {
	pub fn new(rdm: Entity<Rdm>, id: u64, cx: &mut Context<Self>) -> Self {
		// The main view owns the downloads; this window only looks at one of them, so it redraws
		// whenever that view changes and holds no copy of its own.
		let follow = cx.observe(&rdm, |_, _, cx| cx.notify());
		let this = cx.weak_entity();
		let download = rdm.read(cx).download(id).cloned();
		let scale = rdm.read(cx).limit_scale();

		let current = download.as_ref().and_then(|d| d.speed_limit).map(|bytes| bytes as f64 / MB);
		let (owner, window) = (rdm.clone(), this.clone());
		let limit = cx.new(|cx| {
			let mut field =
				TextInput::new("Unlimited", cx).with_trailing("MB/s").on_confirm(move |text, _, cx| {
					let Ok(megabytes) = parse_limit(text) else { return };
					owner.update(cx, |rdm, cx| {
						rdm.set_task_speed_limit(id, megabytes.map(|m| (m * MB) as u64), cx)
					});
					let _ = window.update(cx, |this, cx| {
						let at = limit_position(megabytes, scale);
						this.limit_slider.update(cx, |slider, cx| slider.set(vec![at], cx));
					});
				});
			if let Some(megabytes) = current {
				field.set_content(&format_limit(megabytes), cx);
			}
			field
		});
		let (owner, field, scaled) = (rdm.clone(), limit.clone(), rdm.clone());
		let limit_slider = cx.new(|_| {
			Slider::new("Speed limit", vec![limit_position(current, scale)])
				.snap(move |level, at, cx| limit_snap(level, at, scaled.read(cx).limit_scale()))
				.on_change(move |_, at, _, _, cx| {
					let megabytes = limit_at(at, owner.read(cx).limit_scale());
					field.update(cx, |input, cx| {
						input.set_content(&megabytes.map(format_limit).unwrap_or_default(), cx)
					});
					owner.update(cx, |rdm, cx| {
						rdm.set_task_speed_limit(id, megabytes.map(|m| (m * MB) as u64), cx)
					});
				})
		});

		let asked = download.as_ref().and_then(|d| d.connections);
		let (owner, window) = (rdm.clone(), this.clone());
		let connections = cx.new(|cx| {
			let mut field = TextInput::new("Auto", cx).on_confirm(move |text, _, cx| {
				// A count out of range is left unapplied, and the field keeps what was typed.
				let Ok(count) = parse_connections(text) else { return };
				owner.update(cx, |rdm, cx| rdm.set_task_connections(id, count, cx));
				let _ = window.update(cx, |this, cx| {
					this
						.connections_slider
						.update(cx, |slider, cx| slider.set(vec![count_position(count)], cx));
				});
			});
			if let Some(count) = asked {
				field.set_content(&count.to_string(), cx);
			}
			field
		});
		let (owner, field) = (rdm.clone(), connections.clone());
		let connections_slider = cx.new(|_| {
			Slider::new("Connections", vec![count_position(asked)])
				.snap(|level, at, _| count_snap(level, at))
				.on_change(move |_, at, _, _, cx| {
					let count = count_at(at);
					field.update(cx, |input, cx| {
						input.set_content(&count.map(|n| n.to_string()).unwrap_or_default(), cx)
					});
					owner.update(cx, |rdm, cx| rdm.set_task_connections(id, count, cx));
				})
		});
		Self {
			rdm,
			id,
			limit,
			limit_slider,
			connections,
			connections_slider,
			saved: None,
			copied: None,
			_follow: follow,
		}
	}
}
impl Render for DownloadWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let id = self.id;
		let body = div().flex().flex_col().flex_1().min_h_0().gap_2p5().p_4().text_xs();
		let Some(download) = self.rdm.read(cx).download(id).cloned() else {
			// Removed from the list while this window was open: nothing left to show.
			window.remove_window();
			return chrome(p, window, Title::default(), body);
		};
		let title = Title::of(&download);
		window.set_window_title(&title.plain());
		// The engine's parts while it holds the download, with the connections open on them; else
		// the parts the control file kept, with none open.
		let parts = match self.rdm.read(cx).parts_of(id) {
			Some((open, parts)) => {
				self.saved = None;
				Some((Some(open), parts))
			}
			None if download.status == Status::Completed => None,
			None => {
				let saved = self.saved.get_or_insert_with(|| self.rdm.read(cx).saved_parts_of(&download));
				saved.clone().map(|parts| (None, parts))
			}
		};
		let rdm = self.rdm.read(cx);
		let resumable = rdm.resumable(&download);
		let download_folder = rdm.paths.as_ref().map(|paths| paths.downloads.display().to_string());
		let tint = p.status(download.status);

		// The address on a line of its own, cut short, with a button that copies it whole: nothing
		// here is typed, so it is not a field.
		let copied = self.copied.is_some_and(|at| at.elapsed() < COPIED);
		let address = (!download.url.is_empty()).then(|| {
			let url = download.url.clone();
			div()
				.flex()
				.items_center()
				.gap_2()
				.child(div().flex_1().min_w_0().truncate().text_color(p.muted).child(download.url.clone()))
				.child(
					div()
						.id("copy-address")
						.role(gpui::Role::Button)
						.aria_label("Copy address")
						.debug_selector(|| "button:Copy address".to_owned())
						.flex_none()
						.p_1()
						.rounded_sm()
						.cursor_pointer()
						.hover(move |s| s.bg(p.hover))
						.on_click(cx.listener(move |this, _, _, cx| {
							cx.write_to_clipboard(ClipboardItem::new_string(url.clone()));
							this.copied = Some(std::time::Instant::now());
							cx.notify();
							// Drawn again once the check's moment is over, which nothing else would.
							cx.spawn(async move |this, cx| {
								cx.background_executor().timer(COPIED).await;
								let _ = this.update(cx, |_, cx| cx.notify());
							})
							.detach();
						}))
						.child(if copied {
							icon(Icon::Check, p.success).size_3p5()
						} else {
							icon(Icon::Copy, p.muted).size_3p5()
						}),
				)
		});

		// What the transfer is doing, in a card as New Task shows what it found: how much has
		// landed, how fast, how long is left; whether it can be resumed, how it is divided, and
		// where it goes.
		let landed = if download.size > 0 {
			format!("{} of {}", format_bytes(download.received), format_bytes(download.size))
		} else {
			format_bytes(download.received)
		};
		let moving = download.status == Status::Downloading;
		let speed = if moving && download.speed > 0 {
			format_speed(download.speed)
		} else {
			"\u{2014}".to_owned()
		};
		let left = download
			.remaining()
			.filter(|_| moving)
			.map(format_duration)
			.unwrap_or_else(|| "\u{2014}".to_owned());
		let resume = match resumable {
			_ if download.status == Status::Completed => "\u{2014}",
			Some(true) => "Supported",
			Some(false) => "Not supported",
			None => "Not known yet",
		};
		let divided = match &parts {
			Some((open, parts)) if parts.len() > 1 => match open.filter(|_| moving) {
				Some(open) => format!("{}, {open} open", parts.len()),
				None => parts.len().to_string(),
			},
			_ => "One".to_owned(),
		};
		let folder = download.directory.clone().or(download_folder).unwrap_or_default();
		let folder = std::path::Path::new(&folder)
			.file_name()
			.map(|name| name.to_string_lossy().into_owned())
			.unwrap_or(folder);
		let fact = move |label: &'static str, value: String| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.min_w_0()
				.child(div().w(px(FACT)).flex_none().text_color(p.muted).child(text!(id = label, label)))
				.child(div().min_w_0().truncate().child(text!(id = (label, 1usize), value)))
		};
		let column = || div().flex().flex_col().flex_1().min_w_0().gap_1();
		let facts = div()
			.flex()
			.gap_4()
			.p_3()
			.rounded_md()
			.bg(p.hover)
			.child(
				column()
					.child(fact("Downloaded", landed))
					.child(fact("Speed", speed))
					.child(fact("Time left", left)),
			)
			.child(
				column()
					.child(fact("Resume", resume.to_owned()))
					.child(fact("Parts", divided))
					.child(fact("Folder", folder)),
			);

		let control = |name: &'static str, slider: Entity<Slider>, field: Entity<TextInput>| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.h(px(LINE))
				.child(
					div()
						.w(px(LABEL))
						.flex_none()
						.text_color(p.muted)
						.child(text!(id = (name, 0usize), name)),
				)
				.child(div().flex_1().min_w_0().child(slider))
				.child(div().w(px(FIELD)).flex_none().child(field))
		};

		// The actions, icons at the right in a fixed order, each dimmed where it does not apply rather
		// than gone, so none moves under the pointer as the download changes state. See spec/ui.md.
		let status = download.status;
		let rdm = self.rdm.read(cx);
		let waiting = rdm.others_waiting(id);
		let on_disk = rdm.file_of(id).is_some();
		let show_in = match crate::reveal::manager_name() {
			"Finder" => "Show in Finder",
			"File Explorer" => "Show in File Explorer",
			_ => "Show in the file manager",
		};
		let action = |f: fn(&mut Rdm, u64, &mut Context<Rdm>)| {
			let rdm = self.rdm.clone();
			move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| {
				rdm.update(cx, |rdm, cx| f(rdm, id, cx))
			}
		};
		let run = match status {
			Status::Downloading | Status::Queued => {
				icon_button(p, "pause", Icon::Pause, "Pause", true, action(Rdm::pause))
			}
			Status::Paused | Status::Failed => {
				icon_button(p, "resume", Icon::Play, "Resume", true, action(Rdm::resume))
			}
			Status::Completed => {
				icon_button(p, "resume", Icon::Play, "Resume", false, action(Rdm::resume))
			}
		};
		let turn = match status {
			Status::Downloading => icon_button(
				p,
				"yield",
				Icon::ListEnd,
				if waiting {
					"Let the next one go first"
				} else {
					"Let the next one go first (none is waiting)"
				},
				waiting,
				action(Rdm::yield_place),
			),
			Status::Queued | Status::Paused | Status::Failed => icon_button(
				p,
				"now",
				Icon::ListStart,
				"Start now, ahead of the queue",
				true,
				action(Rdm::start_now),
			),
			Status::Completed => icon_button(
				p,
				"now",
				Icon::ListStart,
				"Start now, ahead of the queue",
				false,
				action(Rdm::start_now),
			),
		};
		let again = icon_button(
			p,
			"again",
			Icon::RotateCcw,
			if status == Status::Completed {
				"Download again (the file goes to the Trash)"
			} else {
				"Download again from the start"
			},
			matches!(status, Status::Completed | Status::Paused | Status::Failed),
			action(Rdm::redownload),
		);
		let open = {
			let rdm = self.rdm.clone();
			icon_button(
				p,
				"open",
				Icon::SquareArrowOutUpRight,
				"Open",
				status == Status::Completed && on_disk,
				move |_, _, cx| rdm.read(cx).open_file(id),
			)
		};
		let reveal = {
			let rdm = self.rdm.clone();
			icon_button(p, "reveal", Icon::FolderSearch, show_in, on_disk, move |_, _, cx| {
				rdm.read(cx).reveal_file(id)
			})
		};
		let remove = icon_button(
			p,
			"remove",
			Icon::Trash,
			if status == Status::Downloading { "Remove (pause it first)" } else { "Remove" },
			status != Status::Downloading,
			action(Rdm::remove),
		);
		// The state at the left, the reason with it while it has failed.
		let state = match &download.error {
			Some(error) if download.status == Status::Failed => {
				format!("{}: {error}", download.status.label())
			}
			_ => download.status.label().to_owned(),
		};
		let body = body
			.when_some(address, |s, address| s.child(address))
			.child(facts)
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.child(control("Speed limit", self.limit_slider.clone(), self.limit.clone()))
					.child(control("Connections", self.connections_slider.clone(), self.connections.clone())),
			)
			.child(div().flex_1())
			.child(transfer(p, &download, parts.as_ref(), tint))
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap_3()
					.child(div().min_w_0().truncate().text_color(tint).child(state))
					.child(
						div()
							.flex()
							.flex_none()
							.items_center()
							.gap_0p5()
							.child(run)
							.child(turn)
							.child(again)
							.child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(p.border))
							.child(open)
							.child(reveal)
							.child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(p.border))
							.child(remove),
					),
			);
		chrome(p, window, title, body)
	}
}

/// New Task's measures, which this window is as wide as: the facts' label column, the grid's label
/// column, and the column at the right that a slider's field sits in; a line two points under New
/// Task's thirty, which is what fits three by two.
const FACT: f32 = 72.0;
const LABEL: f32 = 72.0;
const LINE: f32 = 28.0;
const FIELD: f32 = 128.0;

/// How the file is coming in: one bar made of two. Above, the file as the engine cut it, each part
/// a slot of its own filled from its start as far as it has landed, with a hairline where one part
/// ends and the next begins; under it, half as high and in green, the whole download's progress.
/// Complete, both are green, and the hairlines stay, so how many parts it took is still there to
/// see. A download the engine does not hold and left no plan, or never split, is one part. See
/// spec/ui.md.
fn transfer(
	p: theme::Palette,
	download: &Download,
	parts: Option<&(Option<u64>, Vec<Segment>)>,
	tint: gpui::Hsla,
) -> impl IntoElement {
	let parts = parts.map(|(_, parts)| parts.clone()).unwrap_or_default();
	let done = download.status == Status::Completed;
	let fill = if done { p.success } else { tint };
	let (from, to) = match (parts.first(), parts.last()) {
		(Some(first), Some(last)) => (first.span.start, last.span.end),
		_ => (0, download.size),
	};
	let span = to.saturating_sub(from).max(1) as f32;
	let at = |byte: u64| (byte.saturating_sub(from)) as f32 / span;
	let split = div().relative().h(px(8.0)).w_full().rounded_full().overflow_hidden().bg(p.track);
	let split = if parts.is_empty() {
		split.child(div().h_full().w(relative(if done { 1.0 } else { download.progress() })).bg(fill))
	} else {
		split
			.children(parts.iter().map(|part| {
				let landed = if done { part.span.len() } else { part.done.min(part.span.len()) };
				div()
					.absolute()
					.top_0()
					.bottom_0()
					.left(relative(at(part.span.start)))
					.w(relative(landed as f32 / span))
					.bg(fill)
			}))
			.children(parts.iter().skip(1).map(|part| {
				div()
					.absolute()
					.top_0()
					.bottom_0()
					.left(relative(at(part.span.start)))
					.w(px(1.0))
					.bg(p.window)
			}))
	};
	let whole =
		div().h(px(4.0)).w_full().rounded_full().overflow_hidden().bg(p.track).child(
			div().h_full().w(relative(if done { 1.0 } else { download.progress() })).bg(p.success),
		);
	div().flex().flex_col().gap_0p5().child(split).child(whole)
}

/// The window's title: how far the download has come, then the file's name -- the percentage while
/// it moves, its state while it waits or has stopped, nothing once it is complete. See spec/ui.md.
#[derive(Default)]
struct Title {
	before: Option<String>,
	name: String,
}

impl Title {
	fn of(download: &Download) -> Title {
		let before = match download.status {
			Status::Completed => None,
			Status::Downloading if download.size > 0 => {
				Some(format!("{}%", (download.progress() * 100.0).floor() as u32))
			}
			status => Some(status.label().to_owned()),
		};
		Title { before, name: download.name.clone() }
	}

	/// As the system shows it, in the window list and the Window menu.
	fn plain(&self) -> String {
		match &self.before {
			Some(before) => format!("{before} {}", self.name),
			None => self.name.clone(),
		}
	}
}

/// The window around the body: its own title strip across the top, the toolbar's height, with the
/// traffic lights in it on macOS and the application's window buttons at its right where the system
/// draws no frame; the system's radius on the systems that draw none; and a press on the edge
/// resizing on Linux. The system titlebar is transparent, as the main window's is. See spec/ui.md.
fn chrome(p: theme::Palette, window: &Window, title: Title, body: gpui::Div) -> gpui::Div {
	let inset = frame::lights_inset(window);
	let strip = div()
		.relative()
		.flex()
		.flex_none()
		.items_center()
		.h(px(toolbar::HEIGHT))
		.pl(inset)
		.border_b_1()
		.border_color(p.border)
		.bg(p.panel)
		.child(frame::drag_area())
		.when(frame::draws_frame(window), |s| s.child(frame::controls(p, window)))
		// The title across the middle of the whole strip, as the system centres its own, clear of the
		// lights on both sides so it stays centred; it takes no press, so the strip under it still
		// drags. The name gives way before the state in front of it.
		.child(
			div()
				.absolute()
				.inset_0()
				.px(inset)
				.flex()
				.items_center()
				.justify_center()
				.font_weight(gpui::FontWeight::MEDIUM)
				.child(
					div()
						.flex()
						.min_w_0()
						.gap_1p5()
						.when_some(title.before, |s, before| {
							s.child(div().flex_none().text_color(p.muted).child(before))
						})
						.child(div().min_w_0().truncate().child(title.name)),
				),
		);
	div()
		.flex()
		.flex_col()
		.size_full()
		.text_size(px(13.0))
		.bg(p.window)
		.text_color(p.text)
		.rounded(frame::radius(window))
		.overflow_hidden()
		.on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
			frame::on_root_mouse_down(event, window)
		})
		.child(strip)
		.child(body)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_connections_slider_is_powers_of_two_then_counts_with_auto_past_its_end() {
		assert_eq!(count_position(None), 1.0, "Auto is the far right");
		assert_eq!(count_at(1.0), None);
		assert_eq!(count_at(count_position(Some(1))), Some(1));
		assert_eq!(count_at(count_position(Some(32))), Some(32));
		assert_eq!(count_at(count_position(Some(23))), Some(23), "a count typed comes back as itself");
		let coarse: Vec<Option<u16>> =
			(0..=5).map(|k| count_at(count_snap(0, k as f32 * COUNT_SCALE / 5.0))).collect();
		assert_eq!(coarse, [1, 2, 4, 8, 16, 32].map(Some).to_vec(), "the coarsest is powers of two");
		assert_eq!(count_at(count_snap(0, count_position(Some(20)))), Some(16));
		assert_eq!(count_at(count_snap(1, count_position(Some(20)))), Some(20), "then whole counts");
		assert_eq!(count_snap(0, 0.97), 1.0, "and Auto is a place of its own");
	}
}
