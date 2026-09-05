use gpui::{Context, IntoElement, div, prelude::*, px, relative};

use crate::app::Rdm;
use crate::download::{Download, Status, format_bytes, format_speed};
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
		let trailing = match download.status {
			Status::Downloading => format_speed(download.speed),
			other => other.label().to_owned(),
		};
		div()
			.id(("download", id))
			.flex()
			.flex_col()
			.gap_1()
			.px_4()
			.py_2()
			.border_b_1()
			.border_color(theme::border())
			.cursor_pointer()
			.when(selected, |s| s.bg(theme::selection()))
			.when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
			.on_click(cx.listener(move |this, _, _, cx| this.select(id, cx)))
			.child(
				div()
					.flex()
					.justify_between()
					.gap_4()
					.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
					.child(div().text_color(theme::muted()).whitespace_nowrap().child(trailing)),
			)
			.child(progress_bar(download))
			.child(
				div()
					.flex()
					.justify_between()
					.text_xs()
					.text_color(theme::muted())
					.child(size_line(download))
					.child(download.kind().label()),
			)
	}
}

fn progress_bar(download: &Download) -> impl IntoElement {
	let fill = match download.status {
		Status::Completed => theme::success(),
		Status::Failed => theme::failure(),
		Status::Paused => theme::warning(),
		Status::Downloading | Status::Queued => theme::accent(),
	};
	div()
		.h(px(4.0))
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
