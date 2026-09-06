use gpui::{
	ClickEvent, Context, Div, Hsla, IntoElement, MouseButton, MouseDownEvent, Role, SharedString,
	Stateful, div, prelude::*, px, relative,
};

use crate::app::{Column, Rdm, SortKey, View};
use crate::download::{Download, Status, format_added, format_bytes, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::{Palette, Tint};
use crate::ui::tooltip::tooltip;

/// The name column's floor, the same kind of floor the fixed columns keep: a word and an
/// ellipsis, not a width anyone would choose. See `Column::MINS`.
pub const NAME_MIN: f32 = 48.0;
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
				View::Thumbnails => self.thumbnail_row(d, cx).into_any_element(),
				View::Grid => self.card(d, cx).into_any_element(),
			})
			.collect();
		// A frame that ran out of its allowance of system pictures is owed another: the rest of
		// them are waiting in it.
		if self.thumbnails.borrow().starved() {
			cx.notify();
		}
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
						View::Detailed | View::Thumbnails => s.flex_col().px_1p5().py_1(),
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
			.child(self.folder_control(cx))
			.child(self.header_cell(SortKey::Name, "Name", false, cx).flex_1().min_w_0().pl(px(12.0)))
			.children(cells)
	}

	/// The corner over the type icons holds a funnel that stays: lit, the lists also hold what
	/// else the download folder holds, wherever a file fits; pressed again, the downloads
	/// alone. The slot keeps the width the cells match.
	fn folder_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let everything = self.folder_shown;
		// A funnel is lit when it is filtering, and this one filters by leaving the folder's other
		// files out: the whole list is the funnel doing nothing, and downloads alone is the funnel
		// at work. It was lit the other way round, which read as "the folder's files are on" -- a
		// switch rather than a filter, and the one arrangement where the window's plainest state
		// carried a lit control. White, the same white All Tasks carries at the top of the sidebar,
		// because both mean the list as this window keeps it rather than a hue of its own.
		let filtering = !everything;
		div()
			.id("folder-files")
			.role(Role::Button)
			.aria_label("Folder files")
			.debug_selector(|| "button:Folder files".to_owned())
			.w(px(14.0))
			.h_full()
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.cursor_pointer()
			.tooltip(tooltip(if everything { "Downloads only" } else { "Include folder files" }))
			.on_click(cx.listener(|this, _, _, cx| this.toggle_folder_files(cx)))
			.child(
				icon(Icon::Funnel, if filtering { p.hue(Tint::Snow.rgb()) } else { p.muted }).size_3(),
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
		// The title truncates rather than running over its neighbour, for the same reason the cells
		// under it do: at a column's floor the word is wider than the column.
		let label = div().min_w_0().truncate().child(title);
		div()
			.id(SharedString::from(format!("sort:{title}")))
			.role(Role::ColumnHeader)
			.aria_label(format!("Sort by {title}"))
			.debug_selector(|| format!("sort:{title}"))
			.flex()
			.flex_none()
			.items_center()
			.gap_0p5()
			.overflow_hidden()
			.cursor_pointer()
			.when(active, |s| s.text_color(p.text))
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(move |this, _, _, cx| this.sort_by(key, cx)))
			.map(|s| if end { s.child(slot).child(label) } else { s.child(label).child(slot) })
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
			// The zone lies over the titles on either side of the boundary and has to take the press
			// from both: a press on the line is a press on the line, and the title under the zone's
			// edge must not also read it as a click and re-sort the column, which it did.
			//
			// Not by occluding. A drag is driven by the window root's own move and up handlers, and
			// those run only while the root is the thing under the pointer; a zone that blocked the
			// root would take the press and then never hear the drag it began. So the zone is
			// deferred instead -- painted after the whole header, which puts its listener first,
			// since mouse events are offered to the frontmost listener first -- and it stops the
			// event there. The title behind it never learns a button went down, so no click of its
			// own comes back up. Priority zero, so a sheet or the funnel's menu still covers it.
			.child(gpui::deferred(
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
							cx.stop_propagation();
							this.begin_resize(column, event.position.x);
							cx.notify();
						}),
					),
			))
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
				// A folder row is a door and nothing else: it has no window of its own to open
				// and nothing to act on, so one press opens it and a second closes it.
				if this.folder_shape(id).is_some_and(|(_, directory)| directory) {
					this.toggle_folder(id, cx);
				} else if event.click_count() == 2 {
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
		// A cell never draws outside its column: at a column's floor the content is wider than the
		// column is, and without this the status ran across the date beside it.
		let cell = |column: Column| {
			div()
				.w(px(self.width(column) + 12.0))
				.pl(px(12.0))
				.flex_none()
				.flex()
				.justify_end()
				.overflow_hidden()
				.text_xs()
		};
		self
			.item(download, cx)
			.flex()
			.items_center()
			.h(px(26.0))
			.px_2()
			.child(self.folder_indent(download))
			.child(tinted_icon(self.category_icon(download)).size_3p5())
			.child(div().flex_1().min_w_0().pl(px(12.0)).truncate().child(download.name.clone()))
			.child(self.quarantine_flag(download, cx))
			.child(
				cell(Column::Size).text_color(p.muted).child(div().truncate().child(size_cell(download))),
			)
			.child(
				cell(Column::Progress).items_center().gap_2().child(progress_bar(p, download, tint)).child(
					div().w(px(32.0)).flex_none().text_right().text_color(p.muted).child(percent(download)),
				),
			)
			.child(
				cell(Column::Speed)
					.text_color(p.muted)
					.child(div().truncate().child(speed_cell(download))),
			)
			.child(cell(Column::Status).child(status_label(download, tint)))
			.child(
				cell(Column::Added)
					.text_color(p.muted)
					.child(div().truncate().child(format_added(download.added))),
			)
	}

	/// The small flag on a row whose file the system has marked as having come from the internet.
	/// Only on the kinds where the mark does anything -- it is on nearly every downloaded file
	/// and matters when one is opened as a program -- so a flag means something rather than
	/// being on everything. The pointer over it turns it into the same flag struck through,
	/// which is what pressing it does; pressing takes the mark off, and asks nobody for anything.
	fn quarantine_flag(&self, download: &Download, cx: &mut Context<Self>) -> gpui::AnyElement {
		let p = self.palette;
		let Some(path) = download.path.clone() else { return div().into_any_element() };
		if !crate::quarantine::worth_flagging(&download.name)
			|| !self.marked.borrow_mut().of(std::path::Path::new(&path))
		{
			return div().into_any_element();
		}
		let id = download.id;
		div()
			.id(("quarantine", id))
			.role(Role::Button)
			.aria_label("From the internet")
			.debug_selector(move || format!("quarantine:{id}"))
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size_4()
			.ml_1()
			.relative()
			.rounded_sm()
			.cursor_pointer()
			.group("quarantine")
			.tooltip(tooltip("From the internet; press to clear"))
			.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
				// The row's own click would select it; this press is about the flag.
				let _ = event;
				this.clear_quarantine(id, cx);
			}))
			// The two flags sit on top of each other and take turns: the plain one until the
			// pointer is over it, the struck-through one while it is, which is a picture of what
			// pressing does.
			.child(
				div()
					.absolute()
					.group_hover("quarantine", |s| s.invisible())
					.child(icon(Icon::Flag, p.muted).size_3p5()),
			)
			.child(
				div()
					.absolute()
					.invisible()
					.group_hover("quarantine", |s| s.visible())
					.child(icon(Icon::FlagOff, p.accent).size_3p5()),
			)
			.into_any_element()
	}

	/// What sits before a row's icon while the folders are kept as folders: a step of space for
	/// every folder it is inside, and for a folder row a chevron that opens it. Nothing at all in
	/// the other two modes, where no row is inside anything.
	fn folder_indent(&self, download: &Download) -> impl IntoElement + use<> {
		let p = self.palette;
		let (depth, directory) = self.folder_shape(download.id).unwrap_or((0, false));
		div()
			.flex()
			.flex_none()
			.items_center()
			.w(px(f32::from(depth) * 14.0 + if directory { 14.0 } else { 0.0 }))
			.when(directory, |s| {
				s.justify_end().child(
					icon(
						if self.opened.contains(std::path::Path::new(download.path.as_deref().unwrap_or(""))) {
							Icon::ChevronDown
						} else {
							Icon::ChevronRight
						},
						p.muted,
					)
					.size_3(),
				)
			})
	}

	/// A row with a picture on it: what the system draws for a file of this kind, at the size a
	/// file manager draws it, and the name beside it. Nothing else -- somebody in this view is
	/// looking for a file by eye, and columns would be in the way. The system's own icon is not
	/// always there to be had, and the category's glyph stands in when it is not.
	fn thumbnail_row(&self, download: &Download, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_2p5()
			.h(px(36.0))
			.px_2()
			.child(self.folder_indent(download))
			.child(self.thumbnail(download, 20.0))
			.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
			.child(
				div()
					.flex_none()
					.text_xs()
					.text_color(p.muted)
					.child(format_bytes(download.size)),
			)
	}

	/// The picture for a row: the system's own where there is one, the category's glyph where
	/// there is not. See src/thumbnail.rs.
	fn thumbnail(&self, download: &Download, size: f32) -> gpui::AnyElement {
		let picture = download
			.path
			.as_deref()
			.map(std::path::Path::new)
			.and_then(|path| self.thumbnails.borrow_mut().of(path));
		match picture {
			Some(picture) => gpui::img(picture).size(px(size)).into_any_element(),
			None => tinted_icon(self.category_icon(download)).size(px(size)).into_any_element(),
		}
	}

	/// What fills a card's picture: the file itself where one can be made of it, the first lines
	/// where it is text, the system's icon where there is one, and the category's glyph where
	/// there is not. A file with nothing to show is drawn the way every file used to be.
	fn card_face(&self, download: &Download) -> gpui::AnyElement {
		let p = self.palette;
		let preview = download
			.path
			.as_deref()
			.map(std::path::Path::new)
			.and_then(|path| self.thumbnails.borrow_mut().preview(path));
		match preview {
			Some(crate::thumbnail::Preview::Picture(picture)) => {
				// Filling the card rather than fitting inside it: a picture with bars around it
				// reads as a picture of a picture, and the card is a glance rather than a viewer.
				gpui::img(picture).size_full().object_fit(gpui::ObjectFit::Cover).into_any_element()
			}
			Some(crate::thumbnail::Preview::Lines(lines)) => div()
				.size_full()
				.flex()
				.flex_col()
				.px_1p5()
				.py_1()
				.overflow_hidden()
				.text_color(p.muted)
				.text_size(px(6.0))
				.children(lines.into_iter().map(|line| div().truncate().child(line)))
				.into_any_element(),
			Some(crate::thumbnail::Preview::Icon(icon)) => {
				gpui::img(icon).size_10().into_any_element()
			}
			None => tinted_icon(self.category_icon(download)).size_8().into_any_element(),
		}
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
					.overflow_hidden()
					.bg(p.panel)
					.child(self.card_face(download)),
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

/// Text then mark, so the marks line up down the column's right edge. The word gives way first
/// when the column is at its floor: the mark is what the eye reads down the edge, so it is the
/// half that stays whole.
fn status_label(download: &Download, tint: Hsla) -> impl IntoElement {
	div()
		.flex()
		.min_w_0()
		.items_center()
		.gap_1()
		.text_color(tint)
		.child(div().min_w_0().truncate().child(download.status.label()))
		.child(icon(Icon::for_status(download.status), tint).size_3().flex_none())
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
