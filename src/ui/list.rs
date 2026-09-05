use gpui::{
	ClickEvent, Context, Div, Hsla, IntoElement, Role, SharedString, Stateful, div, prelude::*, px,
	relative,
};

use crate::app::{Rdm, SortKey, View};
use crate::download::{Download, Status, format_bytes, format_speed};
use crate::ui::chip;
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;

/// The table's fixed columns; the name takes what is left.
const SIZE_W: f32 = 104.0;
const PROGRESS_W: f32 = 150.0;
const SPEED_W: f32 = 84.0;
const STATUS_W: f32 = 104.0;

const COLUMNS: [(SortKey, &str, f32); 4] = [
	(SortKey::Size, "Size", SIZE_W),
	(SortKey::Progress, "Progress", PROGRESS_W),
	(SortKey::Speed, "Speed", SPEED_W),
	(SortKey::Status, "Status", STATUS_W),
];

const CHIPS: [Status; 5] =
	[Status::Downloading, Status::Queued, Status::Paused, Status::Completed, Status::Failed];

impl Rdm {
	pub(crate) fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let shown = self.shown();
		let empty = shown.is_empty();
		let items: Vec<_> = shown
			.iter()
			.map(|d| match self.view {
				View::Detailed => self.table_row(d, cx).into_any_element(),
				View::Compact => self.compact(d, cx).into_any_element(),
				View::Grid => self.card(d, cx).into_any_element(),
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.child(self.filter_strip(cx))
			.when(self.view == View::Detailed, |s| s.child(self.header(cx)))
			.child(
				div()
					.id("downloads")
					.flex()
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.map(|s| match self.view {
						View::Grid => s.flex_row().flex_wrap().gap_1p5().content_start().p_2(),
						View::Detailed | View::Compact => s.flex_col().px_1p5().py_1(),
					})
					.children(items)
					.when(empty, |s| {
						s.child(
							div()
								.flex()
								.size_full()
								.justify_center()
								.items_center()
								.text_color(p.muted)
								.child("Nothing here"),
						)
					}),
			)
	}

	/// Status chips: a second cut inside whatever the sidebar selected, one at a time.
	fn filter_strip(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let chips: Vec<_> = CHIPS
			.iter()
			.map(|status| {
				let status = *status;
				let count =
					self.downloads.iter().filter(|d| self.filter.matches(d) && d.status == status).count();
				chip(
					p,
					SharedString::from(format!("chip:{}", status.label())),
					format!("{} {count}", status.label()),
					self.status == Some(status),
					cx.listener(move |this, _, _, cx| this.toggle_status(status, cx)),
				)
				.debug_selector(|| format!("chip:{}", status.label()))
			})
			.collect();
		div().flex().items_center().gap_1().px_3().pt_2().pb_1().children(chips)
	}

	/// Column titles that sort. The ordered column carries a chevron for the direction.
	fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let cells: Vec<_> = COLUMNS
			.iter()
			.map(|(key, title, width)| self.header_cell(*key, title, cx).w(px(*width)).justify_end())
			.collect();
		div()
			.flex()
			.items_center()
			.gap_3()
			.mx_1p5()
			.px_2()
			.py_0p5()
			.text_xs()
			.text_color(p.muted)
			.border_b_1()
			.border_color(p.border)
			.child(div().w(px(14.0)).flex_none())
			.child(self.header_cell(SortKey::Name, "Name", cx).flex_1().min_w_0())
			.children(cells)
	}

	fn header_cell(
		&self,
		key: SortKey,
		title: &'static str,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let p = self.palette;
		let active = self.sort == key;
		let chevron = if self.ascending { Icon::ChevronUp } else { Icon::ChevronDown };
		div()
			.id(SharedString::from(format!("sort:{title}")))
			.role(Role::ColumnHeader)
			.aria_label(format!("Sort by {title}"))
			.debug_selector(|| format!("sort:{title}"))
			.flex()
			.items_center()
			.gap_0p5()
			.cursor_pointer()
			.when(active, |s| s.text_color(p.text))
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx)))
			.child(title)
			.when(active, |s| s.child(icon(chevron, p.text).size_3()))
	}

	/// The selectable shell every view shares: one id, one click, one highlight.
	fn item(&self, download: &Download, cx: &mut Context<Self>) -> Stateful<Div> {
		let p = self.palette;
		let id = download.id;
		let selected = self.selected == Some(id);
		div()
			.id(("download", id))
			.role(Role::ListItem)
			.aria_label(download.name.clone())
			.aria_selected(selected)
			.debug_selector(|| format!("row:{id}"))
			.rounded_sm()
			.cursor_pointer()
			.when(selected, |s| s.bg(p.selection))
			.when(!selected, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
				if event.click_count() == 2 {
					this.open_download(id, cx);
				} else {
					this.select(id, cx);
				}
			}))
	}

	fn table_row(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = status_color(p, download.status);
		let cell = |width: f32| div().w(px(width)).flex_none().flex().justify_end().text_xs();
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_3()
			.h(px(26.0))
			.px_2()
			.child(icon(Icon::for_kind(download.kind()), p.muted).size_3p5())
			.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
			.child(cell(SIZE_W).text_color(p.muted).child(size_cell(download)))
			.child(cell(PROGRESS_W).items_center().gap_2().child(progress_bar(p, download, tint)).child(
				div().w(px(32.0)).flex_none().text_right().text_color(p.muted).child(percent(download)),
			))
			.child(cell(SPEED_W).text_color(p.muted).child(speed_cell(download)))
			.child(cell(STATUS_W).child(status_label(download, tint)))
	}

	fn compact(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = status_color(p, download.status);
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_2()
			.h(px(22.0))
			.px_2()
			.text_xs()
			.child(icon(Icon::for_kind(download.kind()), p.muted).size_3())
			.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
			.child(div().w(px(96.0)).flex_none().child(progress_bar(p, download, tint)))
			.child(
				div().w(px(90.0)).flex_none().text_right().text_color(p.muted).child(size_cell(download)),
			)
			.child(icon(Icon::for_status(download.status), tint).size_3())
	}

	fn card(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = status_color(p, download.status);
		self
			.item(download, cx)
			.flex()
			.flex_col()
			.gap_1p5()
			.w(px(156.0))
			.p_2()
			.child(
				div()
					.flex()
					.h(px(72.0))
					.justify_center()
					.items_center()
					.rounded_sm()
					.bg(p.panel)
					.child(icon(Icon::for_kind(download.kind()), p.muted).size_8()),
			)
			.child(div().truncate().text_xs().child(download.name.clone()))
			.child(progress_bar(p, download, tint))
			.child(
				div()
					.flex()
					.justify_between()
					.items_center()
					.text_xs()
					.text_color(p.muted)
					.child(format_bytes(download.size.max(download.received)))
					.child(icon(Icon::for_status(download.status), tint).size_3()),
			)
	}
}

fn status_label(download: &Download, tint: Hsla) -> impl IntoElement {
	div()
		.flex()
		.items_center()
		.gap_1()
		.text_color(tint)
		.whitespace_nowrap()
		.child(icon(Icon::for_status(download.status), tint).size_3())
		.child(download.status.label())
}

fn status_color(p: Palette, status: Status) -> Hsla {
	match status {
		Status::Completed => p.success,
		Status::Failed => p.failure,
		Status::Paused => p.warning,
		Status::Downloading => p.accent,
		Status::Queued => p.muted,
	}
}

fn progress_bar(p: Palette, download: &Download, fill: Hsla) -> impl IntoElement {
	div()
		.h(px(3.0))
		.w_full()
		.rounded_full()
		.bg(p.track)
		.child(div().h_full().rounded_full().w(relative(download.progress())).bg(fill))
}

fn percent(download: &Download) -> SharedString {
	format!("{:.0}%", download.progress() * 100.0).into()
}

fn size_cell(download: &Download) -> String {
	match (download.status, download.size) {
		(Status::Completed, size) => format_bytes(size),
		(_, 0) => format_bytes(download.received),
		(_, size) => format!("{} / {}", format_bytes(download.received), format_bytes(size)),
	}
}

fn speed_cell(download: &Download) -> SharedString {
	if download.speed > 0 { format_speed(download.speed).into() } else { "\u{2013}".into() }
}
