use gpui::{
	ClickEvent, Context, Div, Hsla, IntoElement, MouseButton, MouseDownEvent, Role, SharedString,
	Stateful, div, prelude::*, px, relative,
};

use crate::app::{Column, Rdm, SortKey, View};
use crate::download::{Download, Status, format_added, format_bytes, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;
use crate::ui::tooltip::tooltip;

/// The name column never drops below this; the fixed columns share what is left of the table.
pub const NAME_MIN: f32 = 120.0;
/// The gap the drag handle takes before every fixed column, matched by the cells.
pub const HANDLE_W: f32 = 12.0;
/// The table's horizontal overhead: the type icon, and the margins and padding around the header.
pub const TABLE_CHROME: f32 = 14.0 + 2.0 * 6.0 + 2.0 * 8.0;

/// What each fixed column sorts by and is titled; widths live on the view, since they are dragged.
const COLUMNS: [(Column, SortKey, &str); 5] = [
	(Column::Size, SortKey::Size, "Size"),
	(Column::Progress, SortKey::Progress, "Progress"),
	(Column::Speed, SortKey::Speed, "Speed"),
	(Column::Status, SortKey::Status, "Status"),
	(Column::Added, SortKey::Added, "Added"),
];

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

	/// Column titles that sort, each preceded by a handle on its left edge: the table is anchored
	/// at its right, so the left edge is the one that can move, and dragging the boundary right
	/// narrows the column as the pointer expects. The ordered column carries a chevron in a slot
	/// every title reserves, so ordering by another does not shift it.
	fn header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let cells: Vec<_> = COLUMNS
			.iter()
			.flat_map(|(column, key, title)| {
				[
					self.resize_handle(*column, cx).into_any_element(),
					self
						.header_cell(*key, title, true, cx)
						.w(px(self.width(*column)))
						.justify_end()
						.into_any_element(),
				]
			})
			.collect();
		div()
			.flex()
			.items_center()
			.h(px(24.0))
			.mx_1p5()
			.mt_1()
			.px_2()
			.text_xs()
			.text_color(p.muted)
			.border_b_1()
			.border_color(p.border)
			.child(self.reset_widths_control(cx))
			.child(self.header_cell(SortKey::Name, "Name", false, cx).flex_1().min_w_0().pl(px(12.0)))
			.children(cells)
	}

	/// The corner over the type icons is empty until the pointer rests on it; then it shows a
	/// reset arrow, named in a tooltip, and a press puts every column back to its starting
	/// width. Hidden by opacity rather than left out, so the slot keeps the width the cells match.
	fn reset_widths_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.id("reset-widths")
			.role(Role::Button)
			.aria_label("Reset column widths")
			.debug_selector(|| "button:Reset to default".to_owned())
			.w(px(14.0))
			.h_full()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.group("reset-widths")
			.tooltip(tooltip("Reset to default"))
			.on_click(cx.listener(|this, _, _, cx| this.reset_widths(cx)))
			.child(
				icon(Icon::RotateCcw, p.text)
					.size_3()
					.opacity(0.0)
					.group_hover("reset-widths", |s| s.opacity(1.0)),
			)
	}

	/// A title sits over its cells' edge -- the name left, the numbers right -- and the chevron's
	/// slot sits on the side the text is not aligned to: after the name, before a number's title, so
	/// the title's edge is the column's edge and the mark hangs off it inward. The slot is always
	/// there, so ordering never shifts a title.
	fn header_cell(
		&self,
		key: SortKey,
		title: &'static str,
		end: bool,
		cx: &mut Context<Self>,
	) -> Stateful<Div> {
		let p = self.palette;
		let active = self.sort == key && !self.default_order();
		let chevron = if self.ascending { Icon::ChevronUp } else { Icon::ChevronDown };
		let slot = div().size_3().flex_none().when(active, |s| s.child(icon(chevron, p.text).size_3()));
		div()
			.id(SharedString::from(format!("sort:{title}")))
			.role(Role::ColumnHeader)
			.aria_label(format!("Sort by {title}"))
			.debug_selector(|| format!("sort:{title}"))
			.flex()
			.flex_none()
			.items_center()
			.gap_0p5()
			.cursor_pointer()
			.when(active, |s| s.text_color(p.text))
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx)))
			.map(|s| if end { s.child(slot).child(title) } else { s.child(title).child(slot) })
	}

	/// The boundary at a column's left edge, draggable: the column follows the pointer. The line is
	/// a pixel wide and the layout spends twelve on it, but the part that takes the pointer is
	/// twice that and the header's full height, laid over its neighbours: a hot zone the width of
	/// the line was missed more often than hit.
	fn resize_handle(&self, column: Column, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let dragging = self.resizing.is_some_and(|r| r.column == column);
		div()
			.relative()
			.w(px(HANDLE_W))
			.h_full()
			.flex()
			.flex_none()
			.justify_center()
			.child(div().w_px().h_full().bg(if dragging { p.accent } else { p.border }))
			.child(
				div()
					.id(SharedString::from(format!("resize:{column:?}")))
					.debug_selector(|| format!("resize:{column:?}"))
					.absolute()
					.top_0()
					.left(px(-HANDLE_W / 2.0))
					.w(px(HANDLE_W * 2.0))
					.h_full()
					.cursor_col_resize()
					.on_mouse_down(
						MouseButton::Left,
						cx.listener(move |this, event: &MouseDownEvent, _, cx| {
							this.begin_resize(column, event.position.x);
							cx.notify();
						}),
					),
			)
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
		let tint = p.status(download.status);
		// Every fixed cell is preceded by the same 12px the header spends on a drag handle, so the
		// columns line up under their titles.
		let cell = |column: Column| {
			div().w(px(self.width(column) + 12.0)).pl(px(12.0)).flex_none().flex().justify_end().text_xs()
		};
		self
			.item(download, cx)
			.flex()
			.items_center()
			.h(px(26.0))
			.px_2()
			.child(tinted_icon(self.category_icon(download)).size_3p5())
			.child(div().flex_1().min_w_0().pl(px(12.0)).truncate().child(download.name.clone()))
			.child(
				cell(Column::Size).text_color(p.muted).child(div().truncate().child(size_cell(download))),
			)
			.child(
				cell(Column::Progress).items_center().gap_2().child(progress_bar(p, download, tint)).child(
					div().w(px(32.0)).flex_none().text_right().text_color(p.muted).child(percent(download)),
				),
			)
			.child(cell(Column::Speed).text_color(p.muted).child(speed_cell(download)))
			.child(cell(Column::Status).whitespace_nowrap().child(status_label(download, tint)))
			.child(
				cell(Column::Added)
					.text_color(p.muted)
					.whitespace_nowrap()
					.child(format_added(download.added)),
			)
	}

	fn compact(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = p.status(download.status);
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_2()
			.h(px(22.0))
			.px_2()
			.text_xs()
			.child(tinted_icon(self.category_icon(download)).size_3())
			.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
			.child(div().w(px(96.0)).flex_none().child(progress_bar(p, download, tint)))
			.child(
				div().w(px(90.0)).flex_none().text_right().text_color(p.muted).child(size_cell(download)),
			)
			.child(icon(Icon::for_status(download.status), tint).size_3())
	}

	fn card(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = p.status(download.status);
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
					.child(tinted_icon(self.category_icon(download)).size_8()),
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

/// A row's type icon in its category's hue.
fn tinted_icon((glyph, color): (Icon, Hsla)) -> gpui::Svg {
	icon(glyph, color)
}

fn status_label(download: &Download, tint: Hsla) -> impl IntoElement {
	div()
		.flex()
		.items_center()
		.gap_1()
		.text_color(tint)
		.whitespace_nowrap()
		.child(download.status.label())
		.child(icon(Icon::for_status(download.status), tint).size_3())
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
