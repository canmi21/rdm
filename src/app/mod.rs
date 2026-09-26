//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use gpui::{
	App, Bounds, Context, IntoElement, Render, Task, Window, WindowBounds, WindowHandle,
	WindowOptions, div, prelude::*, px, size,
};

use serde::Serialize;

use crate::category::{self, Category, categories_with_contents};
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

mod background;
mod categories;
mod folder;
mod indexing;
mod network;
mod notices;
pub(crate) mod quarantine;
mod rules;
mod table;
#[cfg(test)]
mod tests;
mod transfers;
mod tray;
pub(crate) use folder::FolderFile;
pub use table::{Column, Resize, SortKey, View};
pub(crate) use transfers::Asked;
mod updates;

/// Where the folder's files are numbered from: far above any download's id, which the store
/// hands out from one.
pub(crate) const FOLDER_ID: u64 = 1 << 62;

/// How long something may run behind the window before the status bar spins for it.
pub(crate) const SPINNER_AFTER: Duration = Duration::from_millis(300);

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
	/// The header's funnel is lit: the lists also hold what else the download folder holds.
	/// Remembered in state.json.
	pub(crate) folder_shown: bool,
	/// The folder's other files as rows, read when the funnel is lit and whenever the folder
	/// changes while it is; empty otherwise. Their ids start at `FOLDER_ID`.
	pub(crate) folder_files: Vec<Download>,
	/// A read of the folder under way: when it started, and where its rows will arrive.
	pub(crate) folder_scan: Option<(std::time::Instant, std::sync::mpsc::Receiver<Vec<FolderFile>>)>,
	/// What each archive among the rows holds, by path, from the store and the indexer.
	pub(crate) archives: HashMap<String, crate::index::Indexed>,
	pub(crate) indexing: Option<indexing::Indexing>,
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
	/// The rules window, found dead on the next use once it is closed, as a download's is.
	pub(crate) rules_window: Option<WindowHandle<crate::ui::rules_window::RulesWindow>>,
	/// The Settings sheet while it is up.
	pub(crate) settings: Option<SettingsSheet>,
	/// The Add Task sheet while it is up.
	pub(crate) adding: Option<crate::ui::add_dialog::AddSheet>,
	/// Where state.json lives, if the platform gave us a place; the frame as last observed.
	pub(crate) paths: Option<Paths>,
	/// Every layer of the rules merged, read at start and again when a choice is written to the
	/// custom layer. See spec/rules.md.
	pub(crate) rules: std::sync::Arc<crate::rules::Compiled>,
	frame: Option<Frame>,
	/// And the display that frame is a frame on, by name, since the frame is a place on that
	/// display and says nothing on its own about which one it is.
	display: Option<String>,
	maximized: bool,
	/// The pending write. Replacing it cancels the old one, which is the debounce.
	save: Option<Task<()>>,
	_tick: Task<()>,
	/// The cards in the window's corner, oldest first: what has been said in the window and not
	/// yet gone. See src/app/notices.rs.
	/// How deep each folder row sits and whether it is a folder, by its id; empty unless the
	/// folders are being kept as folders. A `Download` is the engine's row and has room for
	/// neither, and the two are built and numbered together, so a table beside it is the
	/// smaller lie. See `read_folder`.
	pub(crate) folder_shape: HashMap<u64, (u8, bool)>,
	/// The folder rows that have been opened, by path. Kept across a rescan, since a scan that
	/// closed everything somebody had opened would be a scan nobody wanted.
	pub(crate) opened: std::collections::HashSet<std::path::PathBuf>,
	/// The system's picture for each file it has been asked about. Interior mutability because
	/// drawing is the only thing that asks and drawing has the window by shared reference; the
	/// alternative is asking the window server once a row a frame. See src/thumbnail/.
	pub(crate) thumbnails: std::cell::RefCell<crate::thumbnail::Thumbnails>,
	/// Where the sidebar's categories are scrolled to. Read while drawing, to know whether there
	/// is anything above or below the fold worth telling the reader about. See src/ui/sidebar.rs.
	pub(crate) categories_scroll: gpui::ScrollHandle,
	/// Which files the system has marked as having come from the internet, by path. Read once a
	/// file and kept: the answer is one attribute lookup, and the list draws every row it has.
	/// Interior mutability for the reason the pictures have it -- drawing is what asks.
	pub(crate) marked: std::cell::RefCell<crate::app::quarantine::Marks>,
	/// The proxy the last look found, None until it has looked or when it found none. Not kept
	/// in the config: it is a fact about the machine now rather than a choice. See src/proxy.rs.
	pub(crate) found_proxy: Option<String>,
	pub(crate) looking_for_proxy: bool,
	proxy_look: Option<std::sync::mpsc::Receiver<Option<String>>>,
	pub(crate) notices: Vec<notices::Shown>,
	/// The notices that are windows of their own, while they are up. A handle stays here after
	/// its window closes and is found dead on the next one, as the download windows' do.
	pub(crate) notice_windows: Vec<WindowHandle<crate::ui::notice_window::NoticeWindow>>,
	/// The update check: what it found, and the card and notification that follow.
	pub(crate) updates: updates::Updates,
	/// When the update check and the rules sync are next due, and where the last sync stands. See
	/// src/app/background.rs.
	pub(crate) schedule: background::Schedule,
	pub(crate) rules_sync: background::RulesSync,
	/// A download failed since the main window was last in front: the tray's dot.
	pub(crate) unseen_failure: bool,
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
			this.remember_frame(window, cx);
			this.schedule_save(cx);
		})
		.detach();
		// The engine's events are drained on the window's own executor a few times a second: the
		// engine runs on tokio and the window on gpui, and a channel read by a timer is the whole
		// of what joins them. Nothing redraws unless an event arrived.
		let tick = cx.spawn_in(window, async move |this, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(200)).await;
				if this.update(cx, |this, cx| this.pump_events(cx)).is_err() {
					break;
				}
				let _ = this.update_in(cx, |this, window, cx| this.pump_tray(window, cx));
			}
		});
		// The rows a previous run left. One that was moving or waiting when the window closed is
		// handed back to the engine, which continues from the plan beside its partial file; one
		// that was paused, failed or done is left as it was.
		let paths_for_thumbnails = paths.as_ref().map(|p| p.thumbnails.clone());
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
		// What earlier runs learnt of how many connections each host takes.
		engine.learn_hosts(saved.hosts.clone());
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
			// A first launch shows the folder's files; a state file that names the funnel has been
			// chosen for, either way, and is left alone. See spec/ui.md.
			folder_shown: saved.folder_shown.unwrap_or(true),
			folder_files: Vec::new(),
			folder_scan: None,
			archives: HashMap::new(),
			indexing: None,
			sort: SortKey::Added,
			ascending: false,
			view: saved.view.unwrap_or(View::Detailed),
			widths: saved.widths.unwrap_or(Column::DEFAULT_WIDTHS),
			resizing: None,
			selected: None,
			palette: theme::palette(true),
			viewport: gpui::Size::default(),
			open: HashMap::new(),
			rules_window: None,
			rules: std::sync::Arc::new(match &paths {
				Some(paths) => crate::rules::load(&paths.rule_places()),
				None => crate::rules::compile(&[(crate::rules::Layer::BuiltIn, crate::rules::built_in())]),
			}),
			settings: None,
			adding: None,
			paths,
			frame: saved.window,
			display: saved.display.clone(),
			maximized: saved.maximized,
			save: None,
			_tick: tick,
			folder_shape: HashMap::new(),
			opened: std::collections::HashSet::new(),
			thumbnails: std::cell::RefCell::new(crate::thumbnail::Thumbnails::keeping_pictures_in(
				paths_for_thumbnails,
			)),
			categories_scroll: gpui::ScrollHandle::new(),
			marked: std::cell::RefCell::default(),
			found_proxy: None,
			looking_for_proxy: false,
			proxy_look: None,
			notices: Vec::new(),
			notice_windows: Vec::new(),
			updates: updates::Updates::default(),
			schedule: background::Schedule::new(std::time::Instant::now()),
			rules_sync: background::RulesSync::default(),
			unseen_failure: false,
			_checks: None,
		};
		this.engine.set_speed_limit(this.preferences.speed_limit);
		this.engine.set_max_active(this.preferences.max_active);
		this.engine.set_bump(this.preferences.bump);
		// The machine is asked what proxy it is running, once, off this thread.
		if this.preferences.proxy_source == crate::proxy::Source::Found {
			this.look_for_proxy(cx);
		}
		// The kept pictures are counted and the oldest dropped, once, where nothing waits for it.
		if let Some(folder) = this.paths.as_ref().map(|p| p.thumbnails.clone()) {
			cx.background_executor().spawn(async move { crate::thumbnail::trim(&folder) }).detach();
		}
		this.import_strays();
		// Where the window opened, read now rather than waited for. The observer above only fires
		// on a move or a resize, so a launch that touched neither would have left the file saying
		// nothing about the display -- and on a first launch, nothing at all. The system may have
		// put the window somewhere other than where it was asked to, which is worth recording too.
		this.remember_frame(window, cx);
		this.load_archives();
		// The funnel left lit last time reads the folder now, as a press would.
		if this.folder_shown {
			this.scan_folder();
		}
		this.queue_indexing();
		// The headless tests have no network to ask and no build number to compare; a test
		// that wants a manifest hands one in.
		if !cfg!(test) {
			this._checks = Some(this.start_background(window, cx));
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

	pub(crate) fn download(&self, id: u64) -> Option<&Download> {
		self.downloads.iter().chain(&self.folder_files).find(|d| d.id == id)
	}

	/// What is going on behind the window, for the status bar's spinner: the update check or
	/// a build on its way, and a read of the folder that has taken more than a moment -- one
	/// that finishes within it is not worth a spinner that would only flash.
	pub(crate) fn activities(&self) -> Vec<String> {
		let mut list = Vec::new();
		let build = self.updates.available.as_ref().map(|a| a.build).unwrap_or_default();
		if self.updates.checking {
			list.push("Checking for updates".to_owned());
		}
		if self.rules_sync.running {
			list.push("Syncing the rules".to_owned());
		}
		match self.updates.stage {
			updates::Stage::Downloading { .. } => list.push(format!("Getting build {build}")),
			updates::Stage::Installing => list.push(format!("Installing build {build}")),
			_ => {}
		}
		if self.folder_scan.as_ref().is_some_and(|(since, _)| since.elapsed() >= SPINNER_AFTER) {
			list.push("Reading the folder".to_owned());
		}
		if let Some(run) = &self.indexing
			&& run.since.elapsed() >= SPINNER_AFTER
		{
			list.push(match run.pending {
				1 => "Indexing an archive".to_owned(),
				n => format!("Indexing {n} archives"),
			});
		}
		list
	}

	pub(crate) fn selected(&self) -> Option<&Download> {
		self.selected.and_then(|id| self.download(id))
	}

	/// The frame, and the display it is a frame on. Both are read on every move and resize, so
	/// the file is right whatever ends the process -- a quit, a crash, or a kill, none of which
	/// gets a hook. The display is written down by the name the system keeps across a restart and
	/// a replug, since GPUI reports the frame in the coordinates of whichever display the window
	/// is on: the same numbers are a different place on each of them, and mean nothing at all
	/// without the name. A system with no such name for a screen simply records none. See
	/// src/screens.rs, src/state.rs and spec/state.md.
	fn remember_frame(&mut self, window: &Window, cx: &App) {
		let bounds = window.window_bounds();
		self.maximized = matches!(bounds, gpui::WindowBounds::Maximized(_));
		let b = bounds.get_bounds();
		self.frame = Some(Frame {
			x: b.origin.x.into(),
			y: b.origin.y.into(),
			width: b.size.width.into(),
			height: b.size.height.into(),
		});
		// Which display, asked of the window: GPUI reads it from the window itself and refreshes it
		// on every move, so it is the one answer that is right for a window that has been dragged.
		// It is nothing only before the window is on screen, which is where this is called from
		// once at launch -- and nothing is kept as no news rather than written down over a name
		// that is still good. See src/screens.rs and spec/state.md.
		if let Some(uuid) = window.display(cx).and_then(|display| display.uuid().ok()) {
			self.display = Some(uuid.to_string());
		}
	}

	fn snapshot(&self) -> State {
		State {
			window: self.frame,
			display: self.display.clone(),
			maximized: self.maximized,
			widths: Some(self.widths),
			view: Some(self.view),
			folder_shown: Some(self.folder_shown),
			last_build: crate::update::this_build(),
			hosts: self.engine.learned_hosts(),
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
		} else if self.settings_menu_open() {
			// A dropdown is the topmost thing while one is open, so Escape answers it first and
			// leaves the sheet where it is; a second Escape then closes the sheet.
			self.close_settings_menu(cx);
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
			// Three by two, title strip included, and as wide as the New Task card, whose measures
			// the body takes. See spec/ui.md.
			let extent = size(px(480.0), px(320.0));
			let options = child_window(cx, "Edit Task", extent);
			let view = rdm.clone();
			if let Ok(handle) = cx.open_window(options, |window, cx| {
				// Linux is asked for client-side decorations, so the title strip is the frame
				// there too; see src/ui/frame.rs.
				#[cfg(target_os = "linux")]
				window.request_decorations(gpui::WindowDecorations::Client);
				#[cfg(not(target_os = "linux"))]
				let _ = window;
				cx.new(|cx| DownloadWindow::new(view, id, cx))
			}) {
				rdm.update(cx, |this, _| {
					this.open.insert(id, handle);
				});
			}
		});
	}
}

impl Render for Rdm {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.palette = theme::palette(window.is_window_active() || !self.preferences.dim_inactive);
		self.viewport = window.viewport_size();
		// This frame's allowance of system pictures. See src/thumbnail/.
		self.thumbnails.borrow_mut().begin_frame();
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
			.when_some(self.corner(cx), |s, corner| s.child(corner))
			.when(self.filter_open, |s| s.child(self.filter_popover(cx)))
			.when(self.adding.is_some(), |s| s.child(self.add_dialog(cx)))
			.when(self.settings_open(), |s| s.child(self.settings_sheet(cx)))
			.when(self.category_sheet.is_some(), |s| s.child(self.render_category_sheet(cx)))
			.when_some(self.guide, |s, guide| s.child(self.guide_sheet(guide, cx)))
	}
}

/// A secondary window draws its own title strip, as the main window draws its toolbar, with the
/// traffic lights in it on macOS. See spec/ui.md.
fn child_window(cx: &App, title: &str, extent: gpui::Size<gpui::Pixels>) -> WindowOptions {
	WindowOptions {
		window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, extent, cx))),
		titlebar: Some(crate::ui::frame::titlebar(title.to_owned())),
		..Default::default()
	}
}
