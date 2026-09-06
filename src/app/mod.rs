//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use gpui::{
	App, Bounds, Context, IntoElement, Render, Task, TitlebarOptions, Window, WindowBounds,
	WindowHandle, WindowOptions, div, prelude::*, px, size,
};

use serde::Serialize;

use crate::category::{Category, categories_of};
use crate::config::{Config, Preferences};
use crate::download::{Download, Filter, Status};
use crate::engine::{self, Engine, Event, TaskId};
use crate::state::{self, Frame, Paths, State};
use crate::store::Store;
use crate::ui::category_sheet::CategorySheet;
use crate::ui::download_window::DownloadWindow;
use crate::ui::icon::Icon;
use crate::ui::settings_sheet::SettingsSheet;
use crate::ui::theme::{self, Palette};

mod categories;
#[cfg(test)]
mod tests;
mod transfers;
pub(crate) use transfers::Asked;
mod updates;

/// How the list is drawn. Detailed is the default because it is the one that shows progress,
/// speed and size at once; the others trade that for density or for a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
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
	/// The widths the columns start with, and go back to on the header's reset.
	pub const DEFAULT_WIDTHS: [f32; 5] = [132.0, 150.0, 84.0, 112.0, 108.0];

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

/// What a sidebar row carries while it is dragged: the category's id.
#[derive(Clone, Copy, Debug)]
pub struct DraggedCategory(pub u64);

pub struct Rdm {
	/// The rows, as the engine last reported them. Ids are the engine's task ids.
	pub(crate) downloads: Vec<Download>,
	pub(crate) engine: Engine,
	/// The engine's events, drained a few times a second by the pump below.
	events: std::sync::mpsc::Receiver<Event>,
	/// The rows between launches; None when the platform gave no place to keep them, or the
	/// database could not be opened, in which case the list lives for the session.
	store: Option<Store>,
	/// Eyes on the download folder: a plan dropped in, or one removed, is picked up between
	/// launches as well as at them. None where the folder could not be watched.
	watcher: Option<crate::watch::Watcher>,
	/// From config.json, in its order; written back when one is added.
	pub(crate) categories: Vec<Category>,
	/// The switches from config.json, written back when one is flipped.
	pub(crate) preferences: Preferences,
	pub(crate) category_sheet: Option<CategorySheet>,
	/// A few lines of guidance laid over whatever sheet is up, until OK is pressed.
	pub(crate) guide: Option<crate::ui::guide::Guide>,
	/// Holds the keyboard whenever nothing else does, so Escape always has somewhere to land:
	/// a key goes along the focus path and nowhere at all when there is none.
	root_focus: gpui::FocusHandle,
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
	/// The Settings sheet while it is up.
	pub(crate) settings: Option<SettingsSheet>,
	/// The Add Task sheet while it is up.
	pub(crate) adding: Option<crate::ui::add_dialog::AddSheet>,
	/// Where state.json lives, if the platform gave us a place; the frame as last observed.
	pub(crate) paths: Option<Paths>,
	frame: Option<Frame>,
	maximized: bool,
	/// The pending write. Replacing it cancels the old one, which is the debounce.
	save: Option<Task<()>>,
	_tick: Task<()>,
	/// The update check: what it found, and the card and notification that follow.
	pub(crate) updates: updates::Updates,
	_checks: Option<Task<()>>,
}

impl Rdm {
	pub fn new(
		saved: State,
		config: Config,
		paths: Option<Paths>,
		engine: Engine,
		events: std::sync::mpsc::Receiver<Event>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		// Linux is asked for client-side decorations, so the toolbar is the frame there as it is
		// on Windows; a compositor that cannot give them says so and keeps its own bar. See
		// src/ui/frame.rs.
		#[cfg(target_os = "linux")]
		window.request_decorations(gpui::WindowDecorations::Client);
		// Every move or resize is remembered a moment later; there is no hook for a forced quit.
		cx.observe_window_bounds(window, |this, window, cx| {
			this.remember_frame(window);
			this.schedule_save(cx);
		})
		.detach();
		// The engine's events are drained on the window's own executor a few times a second: the
		// engine runs on tokio and the window on gpui, and a channel read by a timer is the whole
		// of what joins them. Nothing redraws unless an event arrived.
		let tick = cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(200)).await;
				if this.update(cx, |this, cx| this.pump_events(cx)).is_err() {
					break;
				}
			}
		});
		// The rows a previous run left. One that was moving or waiting when the window closed is
		// handed back to the engine, which continues from the plan beside its partial file; one
		// that was paused, failed or done is left as it was.
		let store = paths.as_ref().and_then(|p| match Store::open(&p.database) {
			Ok(store) => Some(store),
			Err(error) => {
				eprintln!("downloads will not be kept: {error:#}");
				None
			}
		});
		let mut downloads = store.as_ref().and_then(|s| s.load().ok()).unwrap_or_default();
		let directory =
			paths.as_ref().map(|p| p.downloads.clone()).unwrap_or_else(|| std::path::PathBuf::from("."));
		for download in &mut downloads {
			if matches!(download.status, Status::Queued | Status::Downloading)
				&& let Ok(url) = reqwest::Url::parse(&download.url)
			{
				download.status = Status::Queued;
				download.speed = 0;
				let mut request = engine::Request::new(url, directory.clone());
				request.file_name = Some(download.name.clone());
				engine.add_with_id(TaskId(download.id), request, None);
			}
		}
		let watcher = match crate::watch::Watcher::new(&directory) {
			Ok(watcher) => Some(watcher),
			Err(error) => {
				eprintln!("the download folder will not be watched: {error}");
				None
			}
		};
		let mut this = Self {
			downloads,
			engine,
			events,
			store,
			watcher,
			preferences: config.settings.clone(),
			categories: config.categories(),
			category_sheet: None,
			guide: None,
			root_focus: cx.focus_handle(),
			filter: Filter::All,
			status: None,
			filter_open: false,
			sort: SortKey::Added,
			ascending: false,
			view: saved.view.unwrap_or(View::Detailed),
			widths: saved.widths.unwrap_or(Column::DEFAULT_WIDTHS),
			resizing: None,
			selected: None,
			palette: theme::palette(true),
			viewport: gpui::Size::default(),
			open: HashMap::new(),
			settings: None,
			adding: None,
			paths,
			frame: saved.window,
			maximized: saved.maximized,
			save: None,
			_tick: tick,
			updates: updates::Updates::default(),
			_checks: None,
		};
		this.engine.set_speed_limit(this.preferences.speed_limit);
		this.engine.set_max_active(this.preferences.max_active);
		this.import_strays();
		// The headless tests have no network to ask and no build number to compare; a test
		// that wants a manifest hands one in.
		if !cfg!(test) {
			this._checks = Some(this.start_update_checks(window, cx));
		}
		// A numbered build that an older one left under the old name takes the new one, once,
		// and every numbered build records itself so the next knows what it came after.
		if crate::update::this_build().is_some() {
			if let Some(moved) = crate::update::install::fix_legacy_name(saved.last_build) {
				eprintln!("renamed to {}", moved.display());
			}
			if saved.last_build != crate::update::this_build() {
				this.schedule_save(cx);
			}
		}
		this
	}

	/// The rows the list shows, in the order it shows them.
	pub(crate) fn shown(&self) -> Vec<&Download> {
		let mut rows: Vec<&Download> = self
			.downloads
			.iter()
			.filter(|d| {
				self.filter.matches(d, &self.categories) && self.status.is_none_or(|s| d.status == s)
			})
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

	pub(crate) fn categories_of(&self, download: &Download) -> Vec<&Category> {
		categories_of(&self.categories, download)
	}

	/// The category a row is drawn as: the filtered one when one is filtered and matches, else
	/// the first that matches. None when nothing does and there is no catch-all.
	fn category_shown(&self, download: &Download) -> Option<&Category> {
		let matched = self.categories_of(download);
		if let Filter::Category(id) = self.filter
			&& let Some(c) = matched.iter().find(|c| c.id == id)
		{
			return Some(c);
		}
		matched.first().copied()
	}

	/// The icon a row shows, and the hue it is drawn in: its category's, so the list reads the
	/// way the sidebar does. A plain file, muted, when no category claims it.
	pub(crate) fn category_icon(&self, download: &Download) -> (Icon, gpui::Hsla) {
		match self.category_shown(download) {
			Some(c) => (c.icon, self.palette.hue(c.color)),
			None => (Icon::File, self.palette.muted),
		}
	}

	pub(crate) fn download(&self, id: u64) -> Option<&Download> {
		self.downloads.iter().find(|d| d.id == id)
	}

	pub(crate) fn selected(&self) -> Option<&Download> {
		self.selected.and_then(|id| self.download(id))
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

	/// The order nothing has been asked for: newest first.
	pub(crate) fn default_order(&self) -> bool {
		self.sort == SortKey::Added && !self.ascending
	}

	/// Three clicks on a title: ascending, descending, then back to the default order. A click on
	/// another title starts that one ascending.
	pub(crate) fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
		if self.sort != key || self.default_order() {
			self.sort = key;
			self.ascending = true;
		} else if self.ascending {
			self.ascending = false;
		} else {
			self.sort = SortKey::Added;
			self.ascending = false;
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
		// Widening one column narrows the name column, which keeps a floor: past it the row would
		// overflow the window.
		let others: f32 = self.widths.iter().sum::<f32>() - resize.from_width;
		let table =
			f32::from(self.viewport.width) - crate::ui::sidebar::WIDTH - crate::ui::list::TABLE_CHROME;
		let room = table - others - 5.0 * crate::ui::list::HANDLE_W - crate::ui::list::NAME_MIN;
		let width = resize.from_width - f32::from(at - resize.from_x);
		self.widths[resize.column.index()] = width.clamp(Column::MIN, room.max(Column::MIN));
		cx.notify();
	}

	pub(crate) fn end_resize(&mut self, cx: &mut Context<Self>) {
		if self.resizing.take().is_some() {
			self.schedule_save(cx);
			cx.notify();
		}
	}

	/// Every column back to the width it started with, from the control in the header's corner.
	pub(crate) fn reset_widths(&mut self, cx: &mut Context<Self>) {
		self.widths = Column::DEFAULT_WIDTHS;
		self.schedule_save(cx);
		cx.notify();
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
		self.schedule_save(cx);
		cx.notify();
	}

	fn remember_frame(&mut self, window: &Window) {
		let bounds = window.window_bounds();
		self.maximized = matches!(bounds, gpui::WindowBounds::Maximized(_));
		let b = bounds.get_bounds();
		self.frame = Some(Frame {
			x: b.origin.x.into(),
			y: b.origin.y.into(),
			width: b.size.width.into(),
			height: b.size.height.into(),
		});
	}

	fn snapshot(&self) -> State {
		State {
			window: self.frame,
			maximized: self.maximized,
			widths: Some(self.widths),
			view: Some(self.view),
			last_build: crate::update::this_build(),
			..State::default()
		}
	}

	/// Writes state.json a third of a second after the last change, off the main thread. A drag
	/// produces dozens of changes a second and one file at the end of it.
	pub(crate) fn schedule_save(&mut self, cx: &mut Context<Self>) {
		let Some(path) = self.paths.as_ref().map(|p| p.state.clone()) else { return };
		let state = self.snapshot();
		self.save = Some(cx.spawn(async move |_, cx| {
			cx.background_executor().timer(Duration::from_millis(300)).await;
			cx.background_executor()
				.spawn(async move {
					if let Err(error) = state::save(&path, &state) {
						eprintln!("could not write {}: {error:#}", path.display());
					}
				})
				.await;
		}));
	}

	pub(crate) fn select(&mut self, id: u64, cx: &mut Context<Self>) {
		self.selected = if self.selected == Some(id) { None } else { Some(id) };
		cx.notify();
	}

	/// The one rule every sheet keeps: Escape, or a press outside it, closes it while it has
	/// nothing unsaved -- nothing typed, nothing switched away from how it came -- and once it
	/// has, only its cross does. Escape is answered by the topmost sheet alone, since that is the
	/// one the press outside would reach.
	pub(crate) fn escape(&mut self, cx: &mut Context<Self>) {
		if self.guide.is_some() {
			self.close_guide(cx);
		} else if self.adding.is_some() {
			self.dismiss_add(cx);
		} else if self.category_sheet.is_some() {
			self.dismiss_category_sheet(cx);
		} else if self.settings_open() {
			self.close_settings(cx);
		} else if self.filter_open {
			self.toggle_filter_menu(false, cx);
		}
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
			let options = child_window(cx, "Download", size(px(480.0), px(360.0)));
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
}

impl Render for Rdm {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.palette = theme::palette(window.is_window_active());
		self.viewport = window.viewport_size();
		// A field that closed took the focus with it; the root takes it back so keys still land.
		if window.focused(cx).is_none() {
			window.focus(&self.root_focus, cx);
		}
		let p = self.palette;
		div()
			.flex()
			.flex_col()
			.size_full()
			// Zed's density: a 13px UI face, and everything else in rems of it.
			.text_size(px(13.0))
			.bg(p.window)
			// The window's corners, the system's radius, on the systems that draw none.
			.rounded(crate::ui::frame::radius(window))
			.overflow_hidden()
			.text_color(p.text)
			.relative()
			.track_focus(&self.root_focus)
			.on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
				this.resize_to(event.position.x, event.pressed_button == Some(gpui::MouseButton::Left), cx)
			}))
			.on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| this.end_resize(cx)))
			// A press on the window's own edge, where the system draws no frame to take it.
			.on_mouse_down(gpui::MouseButton::Left, |event, window, _| {
				crate::ui::frame::on_root_mouse_down(event, window)
			})
			// Escape reaches here when no field took it: the topmost sheet is asked to go.
			.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
				if event.keystroke.key == "escape" {
					this.escape(cx);
				}
			}))
			// First, so its listener is the first of the frame; see first_mouse.rs.
			.child(crate::ui::first_mouse::FirstMouseGuard)
			.child(self.render_toolbar(window, cx))
			.child(
				div()
					.flex()
					.flex_1()
					.min_h_0()
					.child(self.render_sidebar(cx))
					.child(div().flex().flex_col().flex_1().min_w_0().child(self.render_list(cx))),
			)
			.child(self.render_status_bar(cx))
			.when_some(self.update_toast(cx), |s, toast| s.child(toast))
			.when(self.filter_open, |s| s.child(self.filter_popover(cx)))
			.when(self.adding.is_some(), |s| s.child(self.add_dialog(cx)))
			.when(self.settings_open(), |s| s.child(self.settings_sheet(cx)))
			.when(self.category_sheet.is_some(), |s| s.child(self.render_category_sheet(cx)))
			.when_some(self.guide, |s, guide| s.child(self.guide_sheet(guide, cx)))
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
