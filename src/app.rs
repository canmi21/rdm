//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use gpui::{
	App, Bounds, Context, IntoElement, Render, Task, TitlebarOptions, Window, WindowBounds,
	WindowHandle, WindowOptions, div, prelude::*, px, size,
};

use serde::Serialize;

use crate::download::{self, Download, Filter, Status};
use crate::ui::download_window::DownloadWindow;
use crate::ui::settings_window::SettingsWindow;
use crate::ui::theme::{self, Palette};

/// How the list is drawn. Detailed is the default because it is the one that shows progress,
/// speed and size at once; the others trade that for density or for a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum View {
	Detailed,
	Compact,
	Grid,
}

impl View {
	pub const ALL: [View; 3] = [View::Detailed, View::Compact, View::Grid];
}

/// A column the table can be ordered by. `Added` is the default: the order downloads arrived in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SortKey {
	Added,
	Name,
	Size,
	Progress,
	Speed,
	Status,
}

/// A fixed-width column of the table; the name takes whatever is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
	Size,
	Progress,
	Speed,
	Status,
	Added,
}

impl Column {
	pub const MIN: f32 = 56.0;

	fn index(self) -> usize {
		self as usize
	}
}

/// A drag on a column's edge in progress: which column, where the pointer started, how wide it was.
#[derive(Clone, Copy, Debug)]
pub struct Resize {
	pub column: Column,
	pub from_x: gpui::Pixels,
	pub from_width: f32,
}

pub struct Rdm {
	pub(crate) downloads: Vec<Download>,
	pub(crate) filter: Filter,
	/// A second cut within the sidebar's filter, from the chips above the list.
	pub(crate) status: Option<Status>,
	/// The status menu under the funnel is open.
	pub(crate) filter_open: bool,
	pub(crate) sort: SortKey,
	pub(crate) ascending: bool,
	pub(crate) view: View,
	pub(crate) widths: [f32; 5],
	pub(crate) resizing: Option<Resize>,
	pub(crate) selected: Option<u64>,
	/// Set at the top of every render from the window's state, read by everything below it.
	pub(crate) palette: Palette,
	pub(crate) viewport: gpui::Size<gpui::Pixels>,
	/// The windows opened beside this one. A handle stays here after its window closes and is
	/// found dead on the next use, which is cheaper than being told.
	pub(crate) open: HashMap<u64, WindowHandle<DownloadWindow>>,
	pub(crate) settings: Option<WindowHandle<SettingsWindow>>,
	_tick: Task<()>,
}

impl Rdm {
	pub fn new(cx: &mut Context<Self>) -> Self {
		// Drives the mock rows forward so the list moves while there is no transfer engine
		// behind it. The real engine will push state changes instead of being polled.
		let tick = cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(500)).await;
				let alive = this.update(cx, |this, cx| {
					this.advance();
					cx.notify();
				});
				if alive.is_err() {
					break;
				}
			}
		});
		Self {
			downloads: download::sample(),
			filter: Filter::All,
			status: None,
			filter_open: false,
			sort: SortKey::Added,
			ascending: true,
			view: View::Detailed,
			widths: [104.0, 150.0, 84.0, 104.0, 108.0],
			resizing: None,
			selected: None,
			palette: theme::palette(true),
			viewport: gpui::Size::default(),
			open: HashMap::new(),
			settings: None,
			_tick: tick,
		}
	}

	/// The rows the list shows, in the order it shows them.
	pub(crate) fn shown(&self) -> Vec<&Download> {
		let mut rows: Vec<&Download> = self
			.downloads
			.iter()
			.filter(|d| self.filter.matches(d) && self.status.is_none_or(|s| d.status == s))
			.collect();
		rows.sort_by(|a, b| {
			let order = match self.sort {
				SortKey::Added => a.added.cmp(&b.added),
				SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
				SortKey::Size => a.size.cmp(&b.size),
				SortKey::Progress => a.progress().partial_cmp(&b.progress()).unwrap_or(Ordering::Equal),
				SortKey::Speed => a.speed.cmp(&b.speed),
				SortKey::Status => (a.status as u8).cmp(&(b.status as u8)),
			};
			if self.ascending { order } else { order.reverse() }
		});
		rows
	}

	pub(crate) fn selected(&self) -> Option<&Download> {
		self.selected.and_then(|id| self.downloads.iter().find(|d| d.id == id))
	}

	pub(crate) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
		self.filter = filter;
		cx.notify();
	}

	/// From the funnel's menu, which closes on a choice; `None` is its "All".
	pub(crate) fn set_status(&mut self, status: Option<Status>, cx: &mut Context<Self>) {
		self.status = status;
		self.filter_open = false;
		cx.notify();
	}

	pub(crate) fn toggle_filter_menu(&mut self, open: bool, cx: &mut Context<Self>) {
		self.filter_open = open;
		cx.notify();
	}

	/// A second click on the same column flips the direction; a click on another starts ascending.
	pub(crate) fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
		if self.sort == key {
			self.ascending = !self.ascending;
		} else {
			self.sort = key;
			self.ascending = true;
		}
		cx.notify();
	}

	pub(crate) fn width(&self, column: Column) -> f32 {
		self.widths[column.index()]
	}

	pub(crate) fn begin_resize(&mut self, column: Column, at: gpui::Pixels) {
		self.resizing = Some(Resize { column, from_x: at, from_width: self.width(column) });
	}

	/// Called for every pointer move over the window. The handle is a column's left edge and the
	/// table is anchored at its right, so moving the boundary right narrows the column. A move
	/// with the button up ends the drag: the release happened outside the window, unseen.
	pub(crate) fn resize_to(&mut self, at: gpui::Pixels, pressed: bool, cx: &mut Context<Self>) {
		let Some(resize) = self.resizing else { return };
		if !pressed {
			self.end_resize(cx);
			return;
		}
		let width = resize.from_width - f32::from(at - resize.from_x);
		self.widths[resize.column.index()] = width.max(Column::MIN);
		cx.notify();
	}

	pub(crate) fn end_resize(&mut self, cx: &mut Context<Self>) {
		if self.resizing.take().is_some() {
			cx.notify();
		}
	}

	/// The toolbar's second button: what the selection can do next, by its state.
	pub(crate) fn act_on_selected(&mut self, cx: &mut Context<Self>) {
		let Some(download) = self.selected() else { return };
		match download.status {
			Status::Downloading => self.pause_selected(cx),
			Status::Completed => self.remove_selected(cx),
			Status::Paused | Status::Queued | Status::Failed => self.resume_selected(cx),
		}
	}

	pub(crate) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
		self.view = view;
		cx.notify();
	}

	pub(crate) fn select(&mut self, id: u64, cx: &mut Context<Self>) {
		self.selected = if self.selected == Some(id) { None } else { Some(id) };
		cx.notify();
	}

	// TODO: opens a dialog asking for a URL once there is an input to type it into.
	pub(crate) fn add(&mut self, cx: &mut Context<Self>) {
		let id = self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
		self.downloads.push(Download {
			id,
			name: format!("download-{id}.bin"),
			url: format!("https://example.org/files/download-{id}.bin"),
			size: 250_000_000,
			received: 0,
			speed: 0,
			status: Status::Queued,
			added: chrono::Local::now(),
		});
		self.selected = Some(id);
		cx.notify();
	}

	pub(crate) fn pause_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.pause(id, cx);
		}
	}

	pub(crate) fn resume_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.resume(id, cx);
		}
	}

	pub(crate) fn remove_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.remove(id, cx);
		}
	}

	pub(crate) fn pause(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Paused;
			download.speed = 0;
			cx.notify();
		}
	}

	pub(crate) fn resume(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Downloading;
			download.speed = 12_000_000;
			cx.notify();
		}
	}

	pub(crate) fn remove(&mut self, id: u64, cx: &mut Context<Self>) {
		self.downloads.retain(|d| d.id != id);
		if self.selected == Some(id) {
			self.selected = None;
		}
		self.open.remove(&id);
		cx.notify();
	}

	/// Double-clicking a row, or the name in the status bar, opens that download in its own
	/// window; a second time brings the window forward instead of opening another.
	pub(crate) fn open_download(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(handle) = self.open.get(&id)
			&& handle.update(cx, |_, window, _| window.activate_window()).is_ok()
		{
			return;
		}
		// Deferred because a new window draws its first frame inside `open_window`, and that frame
		// reads this entity, which is still being updated by the click that got us here.
		let rdm = cx.entity();
		cx.defer(move |cx| {
			let options = child_window(cx, "Download", size(px(440.0), px(230.0)));
			let view = rdm.clone();
			if let Ok(handle) =
				cx.open_window(options, |_, cx| cx.new(|cx| DownloadWindow::new(view, id, cx)))
			{
				rdm.update(cx, |this, _| {
					this.open.insert(id, handle);
				});
			}
		});
	}

	pub(crate) fn open_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected {
			self.open_download(id, cx);
		}
	}

	pub(crate) fn open_settings(&mut self, cx: &mut Context<Self>) {
		if let Some(handle) = &self.settings
			&& handle.update(cx, |_, window, _| window.activate_window()).is_ok()
		{
			return;
		}
		let rdm = cx.entity();
		cx.defer(move |cx| {
			let options = child_window(cx, "Settings", size(px(420.0), px(200.0)));
			let handle = cx.open_window(options, |_, cx| cx.new(|_| SettingsWindow)).ok();
			rdm.update(cx, |this, _| this.settings = handle);
		});
	}

	fn advance(&mut self) {
		for download in self.downloads.iter_mut().filter(|d| d.status == Status::Downloading) {
			download.received = (download.received + download.speed / 2).min(download.size);
			if download.received == download.size {
				download.status = Status::Completed;
				download.speed = 0;
			}
		}
	}
}

impl Render for Rdm {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.palette = theme::palette(window.is_window_active());
		self.viewport = window.viewport_size();
		let p = self.palette;
		div()
			.flex()
			.flex_col()
			.size_full()
			// Zed's density: a 13px UI face, and everything else in rems of it.
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
				this.resize_to(event.position.x, event.pressed_button == Some(gpui::MouseButton::Left), cx)
			}))
			.on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| this.end_resize(cx)))
			.child(self.render_toolbar(cx))
			.child(
				div()
					.flex()
					.flex_1()
					.min_h_0()
					.child(self.render_sidebar(cx))
					.child(div().flex().flex_col().flex_1().min_w_0().child(self.render_list(cx))),
			)
			.child(self.render_status_bar(cx))
			.when(self.filter_open, |s| s.child(self.filter_popover(cx)))
	}
}

/// A secondary window keeps the system titlebar: it is a document, not the application.
fn child_window(cx: &App, title: &str, extent: gpui::Size<gpui::Pixels>) -> WindowOptions {
	WindowOptions {
		window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, extent, cx))),
		titlebar: Some(TitlebarOptions { title: Some(title.to_owned().into()), ..Default::default() }),
		..Default::default()
	}
}

// Headless: the test platform draws the window into no screen, so this exercises what a click
// does without a window, a pointer or a display. See spec/workflow.md.
#[cfg(test)]
mod tests {
	use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

	use super::*;

	fn open(cx: &mut TestAppContext) -> (Entity<Rdm>, VisualTestContext) {
		let window =
			cx.update(|cx| cx.open_window(Default::default(), |_, cx| cx.new(Rdm::new)).unwrap());
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let rdm = window.root(&mut cx).unwrap();
		(rdm, cx)
	}

	fn click(cx: &mut VisualTestContext, selector: &'static str) {
		let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("nothing drawn as {selector}"));
		cx.simulate_click(bounds.center(), Modifiers::default());
	}

	#[gpui::test]
	fn a_header_click_sorts_and_a_second_flips(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "sort:Size");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!((rdm.sort, rdm.ascending), (SortKey::Size, true));
			let sizes: Vec<u64> = rdm.shown().iter().map(|d| d.size).collect();
			assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{sizes:?}");
		});
		click(&mut cx, "sort:Size");
		rdm.read_with(&cx, |rdm, _| assert!(!rdm.ascending));
	}

	#[gpui::test]
	fn the_funnel_menu_narrows_within_the_sidebar_and_all_clears_it(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:Filter by status");
		click(&mut cx, "chip:Completed");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(rdm.status, Some(Status::Completed));
			assert!(!rdm.filter_open, "choosing closes the menu");
			assert!(rdm.shown().iter().all(|d| d.status == Status::Completed));
		});
		click(&mut cx, "button:Filter by status");
		click(&mut cx, "chip:All");
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.status, None));
	}

	#[gpui::test]
	fn a_row_selects_and_the_view_switch_redraws_it(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "row:3");
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, Some(3)));
		click(&mut cx, "view:Grid");
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.view, View::Grid));
		click(&mut cx, "row:3");
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, None));
	}

	#[gpui::test]
	fn dragging_a_header_edge_resizes_that_column(cx: &mut TestAppContext) {
		use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};
		let (rdm, mut cx) = open(cx);
		let before = rdm.read_with(&cx, |rdm, _| rdm.width(Column::Size));
		let handle = cx.debug_bounds("resize:Size").expect("a handle after the Size title");
		let start = handle.center();
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: start,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse: false,
		});
		let moved = point(start.x + px(40.0), start.y);
		cx.simulate_event(MouseMoveEvent {
			position: moved,
			pressed_button: Some(MouseButton::Left),
			modifiers: Modifiers::default(),
		});
		cx.simulate_event(MouseUpEvent {
			button: MouseButton::Left,
			position: moved,
			modifiers: Modifiers::default(),
			click_count: 1,
		});
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(
				rdm.width(Column::Size),
				before - 40.0,
				"the boundary followed the pointer right, so the column narrowed"
			);
			assert!(rdm.resizing.is_none(), "the drag ends with the button");
		});
	}

	#[gpui::test]
	fn opening_a_download_adds_one_window_and_removing_it_closes_it(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
		cx.run_until_parked();
		assert_eq!(cx.windows().len(), 2);
		rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
		cx.run_until_parked();
		assert_eq!(
			cx.windows().len(),
			2,
			"a second request raises the window, it does not open another"
		);
		rdm.update(&mut cx, |rdm, cx| rdm.remove(2, cx));
		cx.run_until_parked();
		assert_eq!(cx.windows().len(), 1);
	}
}
