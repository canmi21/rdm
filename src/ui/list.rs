use gpui::{Context, IntoElement, div, prelude::*, px, relative};

use crate::app::Rdm;
use crate::download::{Download, Status, format_bytes, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme;

impl Rdm {
	pub(crate) fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let rows: Vec<_> =
			self.downloads.iter().filter(|d| self.filter.matches(d)).map(|d| self.row(d, cx)).collect();
		let empty = rows.is_empty();
		div()
			.id("downloads")
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.py_1()
			.children(rows)
			.when(empty, |s| {
				s.child(
					div()
						.flex()
						.size_full()
						.justify_center()
						.items_center()
						.text_color(theme::muted())
						.child("Nothing here"),
				)
			})
	}

	fn row(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let id = download.id;
		let selected = self.selected == Some(id);
		let tint = status_color(download.status);
		let trailing = match download.status {
			Status::Downloading => format_speed(download.speed),
			other => other.label().to_owned(),
		};
		div()
			.id(("download", id))
			.flex()
			.items_center()
			.gap_3()
			.mx_2()
			.px_2()
			.py_2()
			.rounded_md()
			.cursor_pointer()
			.when(selected, |s| s.bg(theme::selection()))
			.when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
			.on_click(cx.listener(move |this, _, _, cx| this.select(id, cx)))
			.child(
				div()
					.flex()
					.flex_none()
					.size_9()
					.justify_center()
					.items_center()
					.rounded_md()
					.bg(theme::panel())
					.text_color(theme::muted())
					.child(icon(Icon::for_kind(download.kind()), theme::muted()).size_5()),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.flex_1()
					.min_w_0()
					.gap_1()
					.child(
						div()
							.flex()
							.justify_between()
							.items_center()
							.gap_4()
							.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
							.child(
								div()
									.flex()
									.items_center()
									.gap_1()
									.text_xs()
									.text_color(tint)
									.whitespace_nowrap()
									.child(icon(Icon::for_status(download.status), tint).size_3p5())
									.child(trailing),
							),
					)
					.child(progress_bar(download, tint))
					.child(
						div()
							.flex()
							.justify_between()
							.text_xs()
							.text_color(theme::muted())
							.child(size_line(download))
							.child(download.kind().label()),
					),
			)
	}
}

fn status_color(status: Status) -> gpui::Rgba {
	match status {
		Status::Completed => theme::success(),
		Status::Failed => theme::failure(),
		Status::Paused => theme::warning(),
		Status::Downloading => theme::accent(),
		Status::Queued => theme::muted(),
	}
}

fn progress_bar(download: &Download, fill: gpui::Rgba) -> impl IntoElement {
	div()
		.h(px(3.0))
		.w_full()
		.rounded_full()
		.bg(theme::track())
		.child(div().h_full().rounded_full().w(relative(download.progress())).bg(fill))
}

fn size_line(download: &Download) -> String {
	match (download.status, download.size) {
		(Status::Completed, size) => format_bytes(size),
		(_, 0) => format_bytes(download.received),
		(_, size) => format!("{} of {}", format_bytes(download.received), format_bytes(size)),
	}
}
