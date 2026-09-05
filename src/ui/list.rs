use gpui::{Context, IntoElement, Rgba, div, prelude::*, px, relative};

use crate::app::{Rdm, View};
use crate::download::{Download, Status, format_bytes, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme;

impl Rdm {
	pub(crate) fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let shown: Vec<&Download> = self.downloads.iter().filter(|d| self.filter.matches(d)).collect();
		let empty = shown.is_empty();
		let items: Vec<_> = shown
			.iter()
			.map(|d| match self.view {
				View::Detailed => self.detailed(d, cx).into_any_element(),
				View::Compact => self.compact(d, cx).into_any_element(),
				View::Grid => self.card(d, cx).into_any_element(),
			})
			.collect();
		div()
			.id("downloads")
			.flex()
			.flex_1()
			.min_h_0()
			.overflow_y_scroll()
			.p_2()
			.map(|s| match self.view {
				View::Grid => s.flex_row().flex_wrap().gap_2().content_start(),
				View::Detailed | View::Compact => s.flex_col(),
			})
			.children(items)
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

	/// The selectable shell every view shares: one id, one click, one highlight.
	fn item(&self, download: &Download, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
		let id = download.id;
		let selected = self.selected == Some(id);
		div()
			.id(("download", id))
			.rounded_md()
			.cursor_pointer()
			.when(selected, |s| s.bg(theme::selection()))
			.when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
			.on_click(cx.listener(move |this, _, _, cx| this.select(id, cx)))
	}

	fn detailed(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let tint = status_color(download.status);
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_3()
			.px_2()
			.py_2()
			.child(kind_badge(download))
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
							.child(status_label(download, tint)),
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

	fn compact(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let tint = status_color(download.status);
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_3()
			.px_2()
			.py_1()
			.text_color(theme::muted())
			.child(icon(Icon::for_kind(download.kind()), theme::muted()))
			.child(
				div().flex_1().min_w_0().truncate().text_color(theme::text()).child(download.name.clone()),
			)
			.child(div().w(px(120.0)).flex_none().child(progress_bar(download, tint)))
			.child(div().w(px(140.0)).flex_none().text_xs().text_right().child(size_line(download)))
			.child(
				div().w(px(120.0)).flex_none().flex().justify_end().child(status_label(download, tint)),
			)
	}

	fn card(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let tint = status_color(download.status);
		self
			.item(download, cx)
			.flex()
			.flex_col()
			.gap_2()
			.w(px(180.0))
			.p_3()
			.child(
				div()
					.flex()
					.h(px(88.0))
					.justify_center()
					.items_center()
					.rounded_md()
					.bg(theme::panel())
					.text_color(theme::muted())
					.child(icon(Icon::for_kind(download.kind()), theme::muted()).size_10()),
			)
			.child(div().truncate().text_xs().child(download.name.clone()))
			.child(progress_bar(download, tint))
			.child(
				div()
					.flex()
					.justify_between()
					.items_center()
					.text_xs()
					.text_color(theme::muted())
					.child(format_bytes(download.size.max(download.received)))
					.child(icon(Icon::for_status(download.status), tint).size_3p5()),
			)
	}
}

fn kind_badge(download: &Download) -> impl IntoElement {
	div()
		.flex()
		.flex_none()
		.size_9()
		.justify_center()
		.items_center()
		.rounded_md()
		.bg(theme::panel())
		.child(icon(Icon::for_kind(download.kind()), theme::muted()).size_5())
}

fn status_label(download: &Download, tint: Rgba) -> impl IntoElement {
	let text = match download.status {
		Status::Downloading => format_speed(download.speed),
		other => other.label().to_owned(),
	};
	div()
		.flex()
		.items_center()
		.gap_1()
		.text_xs()
		.text_color(tint)
		.whitespace_nowrap()
		.child(icon(Icon::for_status(download.status), tint).size_3p5())
		.child(text)
}

fn status_color(status: Status) -> Rgba {
	match status {
		Status::Completed => theme::success(),
		Status::Failed => theme::failure(),
		Status::Paused => theme::warning(),
		Status::Downloading => theme::accent(),
		Status::Queued => theme::muted(),
	}
}

fn progress_bar(download: &Download, fill: Rgba) -> impl IntoElement {
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
