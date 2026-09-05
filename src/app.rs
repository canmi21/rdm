//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use gpui::{
	App, Bounds, Context, Entity, IntoElement, Render, Task, TitlebarOptions, Window, WindowBounds,
	WindowHandle, WindowOptions, div, prelude::*, px, size,
};

use serde::Serialize;

use crate::config::{self, Config, Preferences};
use crate::download::{self, Category, Combine, Download, Filter, Status, categories_of};
use crate::engine::{self, Engine, Event, TaskId};
use crate::state::{self, Frame, Paths, State};
use crate::store::Store;
use crate::ui::download_window::DownloadWindow;
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;
use crate::ui::theme::{self, Palette};

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

/// The custom category form while it is up. The pattern field is what runs; until Advanced is
/// opened it is derived from the basic fields and never seen.
pub struct CategoryForm {
	pub name: Entity<TextInput>,
	pub extensions: Entity<TextInput>,
	pub contains: Entity<TextInput>,
	/// How the two basic fields combine when both are filled.
	pub combine: Combine,
	/// The two switches after the contains field. Off, the text is matched loosely: case is
	/// ignored; on, it must match as typed. Spaces are the other way: kept unless switched off.
	pub match_case: bool,
	pub ignore_space: bool,
	pub pattern: Entity<TextInput>,
	pub icon: Icon,
	/// The color the icon will be lit in, `0xrrggbb`; the swatch beside the name opens the
	/// picker, whose field takes a color written any way the stack reads.
	pub color: u32,
	pub color_open: bool,
	pub custom: Entity<TextInput>,
	pub advanced: bool,
}

/// A preset being edited: which category, the field that adds to its list, and the field for
/// a color of the user's own, which follows the category.
pub struct PresetForm {
	pub id: u64,
	pub add: Entity<TextInput>,
	pub custom: Entity<TextInput>,
}

/// The category sheet's faces: the presets with Edit, Reorder and Add under them; the one-line
/// hint while the sidebar's categories are being dragged into order; one preset's extension
/// list; and the custom form.
pub enum CategorySheet {
	/// `editing` turns the preset chips from switches into doors to their lists.
	Presets {
		editing: bool,
	},
	Reorder,
	Preset(PresetForm),
	Custom(CategoryForm),
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
	pub(crate) settings_open: bool,
	/// The Add Task sheet while it is up.
	pub(crate) adding: Option<crate::ui::add_dialog::AddSheet>,
	/// Where state.json lives, if the platform gave us a place; the frame as last observed.
	pub(crate) paths: Option<Paths>,
	frame: Option<Frame>,
	maximized: bool,
	/// The pending write. Replacing it cancels the old one, which is the debounce.
	save: Option<Task<()>>,
	_tick: Task<()>,
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
			widths: saved.widths.unwrap_or([132.0, 150.0, 84.0, 112.0, 108.0]),
			resizing: None,
			selected: None,
			palette: theme::palette(true),
			viewport: gpui::Size::default(),
			open: HashMap::new(),
			settings_open: false,
			adding: None,
			paths,
			frame: saved.window,
			maximized: saved.maximized,
			save: None,
			_tick: tick,
		};
		this.import_strays();
		this
	}

	/// Downloads the folder holds that the list does not: a plan and a partial file left by a
	/// run whose rows were lost, or copied in from elsewhere. Each that can be continued comes
	/// in as a paused row, to be resumed by hand; what cannot be read is left where it is. Run
	/// at launch and whenever the folder changes; true when a row was added.
	pub(crate) fn import_strays(&mut self) -> bool {
		let Some(directory) = self.paths.as_ref().map(|p| p.downloads.clone()) else { return false };
		let found = engine::control::find(&directory);
		self.import_found(found)
	}

	/// The same, for the files the watcher named: each that is one of a download's two files is
	/// looked at on its own, so a change to one file costs one look and not a read of the folder.
	pub(crate) fn import_paths(&mut self, paths: &[std::path::PathBuf]) -> bool {
		let mut targets: Vec<std::path::PathBuf> =
			paths.iter().filter_map(|p| engine::control::target_of(p)).collect();
		targets.sort();
		targets.dedup();
		let found = targets.iter().filter_map(|t| engine::control::find_one(t)).collect();
		self.import_found(found)
	}

	fn import_found(&mut self, found: Vec<engine::Found>) -> bool {
		let mut added = false;
		for found in found {
			let name = found.target.file_name().map(|n| n.to_string_lossy().into_owned());
			let Some(name) = name else { continue };
			if self.downloads.iter().any(|d| d.name == name || d.url == found.control.url) {
				continue;
			}
			let id = self
				.store
				.as_ref()
				.and_then(|s| s.next_id().ok())
				.unwrap_or(0)
				.max(self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1);
			self.downloads.push(Download {
				id,
				name,
				url: found.control.url.clone(),
				size: found.control.size.unwrap_or(0),
				received: found.control.plan.done(),
				speed: 0,
				status: Status::Paused,
				added: found.modified.map_or_else(chrono::Local::now, chrono::DateTime::from),
				source: None,
				path: None,
				error: None,
			});
			self.persist(id);
			added = true;
		}
		added
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

	/// A new category goes before the catch-all, which stays last so it reads as the remainder.
	pub(crate) fn add_category(
		&mut self,
		name: &str,
		icon: Icon,
		color: Option<u32>,
		custom_color: Option<String>,
		pattern: &str,
		cx: &mut Context<Self>,
	) -> Result<(), String> {
		let name = name.trim();
		if name.is_empty() {
			return Err("a category needs a name".to_owned());
		}
		if pattern.trim().is_empty() {
			return Err("a category needs a pattern".to_owned());
		}
		if self.categories.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
			return Err(format!("there is already a category called {name}"));
		}
		let mut category = Category::new(self.next_category_id(), name, icon, pattern.trim())?;
		if let Some(color) = color {
			category.color = color;
		}
		category.custom_color = custom_color.filter(|t| crate::ui::theme::parse_color(t).is_some());
		self.insert_category(category, cx);
		Ok(())
	}

	fn next_category_id(&self) -> u64 {
		self.categories.iter().map(|c| c.id).max().unwrap_or(0) + 1
	}

	fn insert_category(&mut self, category: Category, cx: &mut Context<Self>) {
		let at =
			self.categories.iter().position(Category::is_catch_all).unwrap_or(self.categories.len());
		self.categories.insert(at, category);
		self.save_config();
		cx.notify();
	}

	/// A preset is added when absent and removed when present, so the sheet's preset row is a
	/// switch for what the sidebar shows. Removing one drops the user's changes to its list with
	/// it; the list is the preset's and goes where it goes.
	pub(crate) fn toggle_preset(&mut self, name: &str, cx: &mut Context<Self>) {
		if let Some(at) = self.categories.iter().position(|c| c.name == name) {
			let removed = self.categories.remove(at);
			if self.filter == Filter::Category(removed.id) {
				self.filter = Filter::All;
			}
			self.save_config();
			cx.notify();
		} else if let Some(preset) =
			Category::from_preset(self.next_category_id(), name, download::Overrides::default())
		{
			self.insert_category(preset, cx);
		}
	}

	/// One extension of a preset's list switched on or off; see `Category::set_extension`.
	pub(crate) fn set_preset_extension(
		&mut self,
		id: u64,
		extension: &str,
		on: bool,
		cx: &mut Context<Self>,
	) {
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			category.set_extension(extension, on);
			self.save_config();
			cx.notify();
		}
	}

	/// `rs, py` typed into a preset's editor: each switched on, whether built in or new.
	pub(crate) fn add_preset_extensions(&mut self, id: u64, text: &str, cx: &mut Context<Self>) {
		for extension in download::split_extensions(text) {
			self.set_preset_extension(id, &extension, true, cx);
		}
	}

	pub(crate) fn set_category_icon(&mut self, id: u64, icon: Icon, cx: &mut Context<Self>) {
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			category.icon = icon;
			self.save_config();
			cx.notify();
		}
	}

	pub(crate) fn set_category_color(&mut self, id: u64, color: u32, cx: &mut Context<Self>) {
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			category.color = color;
			self.save_config();
			cx.notify();
		}
	}

	/// A color the user wrote for a category: kept as written beside the named ones, and made
	/// the one in use. Text that is not a color is ignored.
	pub(crate) fn set_category_custom_color(&mut self, id: u64, text: &str, cx: &mut Context<Self>) {
		let Some(color) = crate::ui::theme::parse_color(text) else { return };
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			category.custom_color = Some(text.trim().to_owned());
			category.color = color;
			self.save_config();
			cx.notify();
		}
	}

	pub(crate) fn reset_preset(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			category.reset_preset();
			self.save_config();
			cx.notify();
		}
	}

	/// The sidebar's categories are being dragged into order; the rows drag instead of filtering.
	pub(crate) fn reordering(&self) -> bool {
		matches!(self.category_sheet, Some(CategorySheet::Reorder))
	}

	/// Drops the dragged category at the target's position, the rest shifting to make room. The
	/// catch-all is neither dragged nor a target, so it stays last. Written at once, like an add.
	pub(crate) fn move_category(&mut self, dragged: u64, onto: u64, cx: &mut Context<Self>) {
		let position = |id: u64| self.categories.iter().position(|c| c.id == id);
		let (Some(from), Some(to)) = (position(dragged), position(onto)) else { return };
		if from == to || self.categories[from].is_catch_all() || self.categories[to].is_catch_all() {
			return;
		}
		let category = self.categories.remove(from);
		self.categories.insert(to, category);
		self.save_config();
		cx.notify();
	}

	/// Written at once, not debounced: a category is added once, and the file is the user's.
	fn save_config(&self) {
		if let Some(paths) = &self.paths
			&& let Err(error) =
				config::save(&paths.config, &Config::from_parts(&self.categories, &self.preferences))
		{
			eprintln!("could not write {}: {error:#}", paths.config.display());
		}
	}

	pub(crate) fn set_colorful_sidebar(&mut self, on: bool, cx: &mut Context<Self>) {
		self.preferences.colorful_sidebar = on;
		self.save_config();
		cx.notify();
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
		} else if self.settings_open {
			self.toggle_settings(false, cx);
		} else if self.filter_open {
			self.toggle_filter_menu(false, cx);
		}
	}

	/// Every event the engine has sent since the last look, applied to the rows.
	fn pump_events(&mut self, cx: &mut Context<Self>) {
		let mut changed = false;
		while let Ok(event) = self.events.try_recv() {
			self.apply(event);
			changed = true;
		}
		// The folder changed and has been quiet since: look at the files that changed, and only
		// those, for a plan that arrived.
		if let Some(paths) = self.watcher.as_ref().and_then(|w| w.try_signal())
			&& self.import_paths(&paths)
		{
			changed = true;
		}
		if changed {
			cx.notify();
		}
		self.poll_add(cx);
	}

	/// The row as it now is, written to the store.
	fn persist(&self, id: u64) {
		if let (Some(store), Some(download)) = (&self.store, self.downloads.iter().find(|d| d.id == id))
			&& let Err(error) = store.save(download)
		{
			eprintln!("could not keep download {id}: {error:#}");
		}
	}

	fn apply(&mut self, event: Event) {
		let touched = match &event {
			Event::Started(id)
			| Event::Completed(id, _)
			| Event::Failed(id, _)
			| Event::Paused(id)
			| Event::Removed(id) => id.0,
			Event::Progress(s) => s.id.0,
		};
		self.apply_event(event);
		self.persist(touched);
	}

	fn apply_event(&mut self, event: Event) {
		fn row(downloads: &mut [Download], id: TaskId) -> Option<&mut Download> {
			downloads.iter_mut().find(|d| d.id == id.0)
		}
		match event {
			Event::Started(id) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Downloading;
					d.error = None;
				}
			}
			Event::Progress(s) => {
				if let Some(d) = row(&mut self.downloads, s.id) {
					d.received = s.done;
					if s.total > 0 {
						d.size = s.total;
					}
					d.speed = s.speed;
					if let Some(name) = s.file_name {
						d.name = name;
					}
					d.status = Status::from_engine(&s.status);
				}
			}
			Event::Completed(id, finished) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Completed;
					d.size = finished.size;
					d.received = finished.size;
					d.speed = 0;
					if let Some(name) = finished.path.file_name() {
						d.name = name.to_string_lossy().into_owned();
					}
					d.path = Some(finished.path.to_string_lossy().into_owned());
				}
			}
			Event::Failed(id, message) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Failed;
					d.speed = 0;
					d.error = Some(message);
				}
			}
			Event::Paused(id) => {
				if let Some(d) = row(&mut self.downloads, id) {
					d.status = Status::Paused;
					d.speed = 0;
				}
			}
			Event::Removed(_) => {}
		}
	}

	/// A new download from an address as typed; the sheet has already looked at it, so this is
	/// for the control socket. What is not an address is dropped.
	pub(crate) fn add_url(&mut self, url: &str, cx: &mut Context<Self>) {
		if let Some(parsed) = crate::ui::add_dialog::parse_address(url) {
			self.add_request(parsed, None, None, cx);
		}
	}

	/// A new download, handed to the engine and shown at once under `name` or the address's
	/// last path segment; the probe's name replaces it as soon as it is known. `source` is the
	/// page it was found on, if any. The id is the store's next, so it is never reused while a
	/// partial file might still carry it.
	pub(crate) fn add_request(
		&mut self,
		url: reqwest::Url,
		name: Option<String>,
		source: Option<String>,
		cx: &mut Context<Self>,
	) {
		let directory = self
			.paths
			.as_ref()
			.map(|p| p.downloads.clone())
			.unwrap_or_else(|| std::path::PathBuf::from("."));
		let id = self
			.store
			.as_ref()
			.and_then(|s| s.next_id().ok())
			.unwrap_or(0)
			.max(self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1);
		self.engine.add_with_id(TaskId(id), engine::Request::new(url.clone(), directory), None);
		let name = name.unwrap_or_else(|| {
			url
				.path_segments()
				.and_then(|mut s| s.next_back())
				.filter(|n| !n.is_empty())
				.unwrap_or("download")
				.to_owned()
		});
		self.downloads.push(Download {
			id,
			name,
			url: url.to_string(),
			size: 0,
			received: 0,
			speed: 0,
			status: Status::Queued,
			added: chrono::Local::now(),
			source,
			path: None,
			error: None,
		});
		self.persist(id);
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

	/// The row changes at once and the engine confirms by event; a click should not wait on a
	/// connection to close.
	pub(crate) fn pause(&mut self, id: u64, cx: &mut Context<Self>) {
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Paused;
			download.speed = 0;
			self.engine.pause(TaskId(id));
			self.persist(id);
			cx.notify();
		}
	}

	pub(crate) fn resume(&mut self, id: u64, cx: &mut Context<Self>) {
		let directory = self
			.paths
			.as_ref()
			.map(|p| p.downloads.clone())
			.unwrap_or_else(|| std::path::PathBuf::from("."));
		if let Some(download) = self.downloads.iter_mut().find(|d| d.id == id) {
			download.status = Status::Queued;
			download.error = None;
			// A row from an earlier run is not in the engine yet; it is queued afresh and the
			// engine picks up the plan beside its partial file.
			if self.engine.contains(TaskId(id)) {
				self.engine.resume(TaskId(id));
			} else if let Ok(url) = reqwest::Url::parse(&download.url) {
				let mut request = engine::Request::new(url, directory);
				request.file_name = Some(download.name.clone());
				self.engine.add_with_id(TaskId(id), request, None);
			}
			self.persist(id);
			cx.notify();
		}
	}

	/// The row goes, and with it a partial file and its plan; a finished file stays where it
	/// landed, since it is the user's now.
	pub(crate) fn remove(&mut self, id: u64, cx: &mut Context<Self>) {
		self.downloads.retain(|d| d.id != id);
		if self.selected == Some(id) {
			self.selected = None;
		}
		self.open.remove(&id);
		self.engine.remove(TaskId(id), true);
		if let Some(store) = &self.store
			&& let Err(error) = store.remove(id)
		{
			eprintln!("could not forget download {id}: {error:#}");
		}
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
			.text_color(p.text)
			.track_focus(&self.root_focus)
			.on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
				this.resize_to(event.position.x, event.pressed_button == Some(gpui::MouseButton::Left), cx)
			}))
			.on_mouse_up(gpui::MouseButton::Left, cx.listener(|this, _, _, cx| this.end_resize(cx)))
			// Escape reaches here when no field took it: the topmost sheet is asked to go.
			.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
				if event.keystroke.key == "escape" {
					this.escape(cx);
				}
			}))
			// First, so its listener is the first of the frame; see first_mouse.rs.
			.child(crate::ui::first_mouse::FirstMouseGuard)
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
			.when(self.adding.is_some(), |s| s.child(self.add_dialog(cx)))
			.when(self.settings_open, |s| s.child(self.settings_sheet(cx)))
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

// Headless: the test platform draws the window into no screen, so this exercises what a click
// does without a window, a pointer or a display. See spec/workflow.md.
#[cfg(test)]
mod tests {
	use gpui::{Entity, EntityInputHandler, Modifiers, TestAppContext, VisualTestContext};

	use super::*;

	/// Somewhere under the temp directory, so a test that really downloads writes there and not
	/// into the repository -- which one did, and three commits carried its files.
	fn scratch_paths(name: &str) -> Paths {
		// Numbered, since tests run at once and two clearing the same directory collide.
		static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
		let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
		let dir = std::env::temp_dir().join(format!("rdm-app-{}-{name}-{n}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(dir.join("downloads")).unwrap();
		Paths {
			state: dir.join("state.json"),
			config: dir.join("config.json"),
			database: dir.join("internal.sqlite"),
			downloads: dir.join("downloads"),
		}
	}

	fn open(cx: &mut TestAppContext) -> (Entity<Rdm>, VisualTestContext) {
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |window, cx| {
				cx.new(|cx| {
					let (engine, events) = Engine::new(engine::EngineSettings::default()).unwrap();
					let paths = scratch_paths("open");
					let mut rdm =
						Rdm::new(State::default(), Config::seed(), Some(paths), engine, events, window, cx);
					rdm.downloads = crate::download::sample();
					rdm
				})
			})
			.unwrap()
		});
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let rdm = window.root(&mut cx).unwrap();
		(rdm, cx)
	}

	fn click(cx: &mut VisualTestContext, selector: &'static str) {
		let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("nothing drawn as {selector}"));
		cx.simulate_click(bounds.center(), Modifiers::default());
	}

	#[gpui::test]
	fn a_title_cycles_ascending_descending_default(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "sort:Size");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!((rdm.sort, rdm.ascending), (SortKey::Size, true));
			let sizes: Vec<u64> = rdm.shown().iter().map(|d| d.size).collect();
			assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{sizes:?}");
		});
		click(&mut cx, "sort:Size");
		rdm.read_with(&cx, |rdm, _| assert_eq!((rdm.sort, rdm.ascending), (SortKey::Size, false)));
		click(&mut cx, "sort:Size");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.default_order(), "a third click returns to newest first")
		});
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
	fn a_drag_stops_where_the_name_column_would_vanish(cx: &mut TestAppContext) {
		use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, point, px};
		let (rdm, mut cx) = open(cx);
		let handle = cx.debug_bounds("resize:Size").unwrap();
		let start = handle.center();
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: start,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse: false,
		});
		cx.simulate_event(MouseMoveEvent {
			position: point(px(0.0), start.y),
			pressed_button: Some(MouseButton::Left),
			modifiers: Modifiers::default(),
		});
		let name = cx.debug_bounds("sort:Name").expect("the name title is still drawn");
		assert!(
			f32::from(name.size.width) >= crate::ui::list::NAME_MIN - 12.0,
			"name column kept {:?}",
			name.size.width
		);
		let added = cx.debug_bounds("sort:Added").unwrap();
		let viewport = cx.update(|w, _| w.viewport_size());
		assert!(added.right() <= viewport.width, "the last column stays inside the window");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.resizing.is_some()));
	}

	#[gpui::test]
	fn the_custom_form_adds_a_rule_and_advanced_exposes_the_pattern(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		assert!(cx.debug_bounds("preset:Video").is_some(), "the sheet opens on the presets");
		assert!(cx.debug_bounds("button:Advanced").is_none(), "the form is a level down");
		click(&mut cx, "button:Add");
		let (name, extensions, pattern) = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
			(form.name.clone(), form.extensions.clone(), form.pattern.clone())
		});
		cx.update(|window, cx| {
			name.update(cx, |input, cx| input.replace_text_in_range(None, "Rust", window, cx));
			extensions.update(cx, |input, cx| input.replace_text_in_range(None, "rs, rlib", window, cx));
		});
		assert!(cx.debug_bounds("category-sheet").is_some());
		click(&mut cx, "button:Advanced");
		let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
		assert_eq!(
			derived, r"(?i)\.(rs|rlib)$",
			"opening Advanced fills the pattern from the basic fields"
		);
		cx.update(|window, cx| {
			pattern.update(cx, |input, cx| input.replace_text_in_range(None, "(", window, cx));
		});
		let card = cx.debug_bounds("category-sheet").unwrap();
		let create = cx.debug_bounds("button:Create").unwrap();
		assert!(
			card.contains(&create.center()),
			"the Create button stays inside the card however long the report"
		);
		click(&mut cx, "button:Create");
		rdm.read_with(&cx, |rdm, _| {
			assert!(
				matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))),
				"a pattern that does not compile is not added"
			)
		});
		cx.update(|window, cx| {
			pattern.update(cx, |input, cx| {
				let end = input.content.len();
				input.replace_text_in_range(Some(end - 1..end), "", window, cx)
			});
		});
		click(&mut cx, "icon:globe");
		click(&mut cx, "button:Create");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.category_sheet.is_none());
			let rust = rdm.categories.iter().find(|c| c.name == "Rust").expect("added");
			assert_eq!(rust.icon, Icon::Globe);
			assert!(rdm.categories.last().unwrap().is_catch_all(), "Other stays last");
		});
		assert!(cx.debug_bounds("filter:Rust").is_some(), "the sidebar lists the new category");
	}

	#[gpui::test]
	fn a_preset_row_toggles_the_category_in_and_out(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		let before = rdm.read_with(&cx, |rdm, _| rdm.categories.len());
		click(&mut cx, "preset:Ebooks");
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.categories.len(), before - 1));
		click(&mut cx, "preset:Ebooks");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(rdm.categories.len(), before);
			assert!(rdm.categories.last().unwrap().is_catch_all());
		});
	}

	#[gpui::test]
	fn a_sheet_swallows_clicks_and_only_a_clean_one_closes_from_outside(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		let row = cx.debug_bounds("row:3").unwrap().center();
		click(&mut cx, "button:New category");
		// The row is under the backdrop now: a click there reaches nothing behind, and the presets
		// have nothing to lose, so the sheet takes it as a request to close.
		cx.simulate_click(row, Modifiers::default());
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(rdm.selected, None, "the row behind the sheet was not pressed");
			assert!(rdm.category_sheet.is_none(), "an untouched sheet closes from a click outside");
		});
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Add");
		let name = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
			form.name.clone()
		});
		cx.update(|window, cx| {
			name.update(cx, |input, cx| input.replace_text_in_range(None, "Rust", window, cx))
		});
		cx.simulate_click(row, Modifiers::default());
		rdm.read_with(&cx, |rdm, _| {
			assert!(
				matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))),
				"typed text is not thrown away by a click outside"
			);
			assert_eq!(rdm.selected, None);
		});
		click(&mut cx, "button:Close");
		rdm.read_with(&cx, |rdm, _| {
			assert!(
				matches!(rdm.category_sheet, Some(CategorySheet::Presets { .. })),
				"the form's cross steps back to the presets"
			)
		});
		click(&mut cx, "button:Close");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "the presets' cross closes"));
	}

	#[gpui::test]
	fn reorder_drags_a_sidebar_row_onto_another_and_other_stays_last(cx: &mut TestAppContext) {
		use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Reorder");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.reordering()));
		let names = |rdm: &Rdm| rdm.categories.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
		let before = rdm.read_with(&cx, |rdm, _| names(rdm));
		assert_eq!(&before[..2], ["Video", "Audio"]);
		let drag = |cx: &mut VisualTestContext, from: &'static str, onto: &'static str| {
			let start = cx.debug_bounds(from).unwrap().center();
			let end = cx.debug_bounds(onto).unwrap().center();
			cx.simulate_event(MouseDownEvent {
				button: MouseButton::Left,
				position: start,
				modifiers: Modifiers::default(),
				click_count: 1,
				first_mouse: false,
			});
			for position in [start + gpui::point(px(0.0), px(6.0)), end] {
				cx.simulate_event(MouseMoveEvent {
					position,
					pressed_button: Some(MouseButton::Left),
					modifiers: Modifiers::default(),
				});
			}
			cx.simulate_event(MouseUpEvent {
				button: MouseButton::Left,
				position: end,
				modifiers: Modifiers::default(),
				click_count: 1,
			});
		};
		drag(&mut cx, "filter:Video", "filter:Code");
		rdm.read_with(&cx, |rdm, _| {
			let after = names(rdm);
			assert_eq!(after[0], "Audio", "{after:?}");
			assert_eq!(after.iter().position(|n| n == "Video"), before.iter().position(|n| n == "Code"));
			assert_eq!(rdm.filter, Filter::All, "a row in reorder mode does not filter");
		});
		drag(&mut cx, "filter:Audio", "filter:Other");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.categories.last().unwrap().is_catch_all(), "Other is not a drop target");
			assert_eq!(names(rdm)[0], "Audio");
		});
		// Every drop is already written, so a press anywhere but the categories finishes.
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "Escape finishes"));
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Reorder");
		let card = cx.debug_bounds("category-sheet").unwrap().center();
		cx.simulate_click(card, Modifiers::default());
		rdm.read_with(&cx, |rdm, _| assert!(rdm.reordering(), "the hint itself is not outside"));
		let row = cx.debug_bounds("row:3").unwrap().center();
		cx.simulate_click(row, Modifiers::default());
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.category_sheet.is_none(), "a press on the list finishes");
			assert_eq!(rdm.selected, None, "and reaches nothing behind the wash");
		});
	}

	#[gpui::test]
	fn edit_opens_a_presets_list_where_extensions_switch_and_are_added(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "preset:Ebooks");
		rdm.read_with(&cx, |rdm, _| assert!(!rdm.categories.iter().any(|c| c.name == "Ebooks")));
		click(&mut cx, "button:Edit");
		click(&mut cx, "preset:Ebooks");
		rdm.read_with(&cx, |rdm, _| {
			assert!(
				matches!(rdm.category_sheet, Some(CategorySheet::Presets { editing: true })),
				"a preset that is off has no list to open"
			)
		});
		click(&mut cx, "preset:Video");
		let add = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
			form.add.clone()
		});
		click(&mut cx, "extension:mkv");
		cx.update(|window, cx| {
			add.update(cx, |input, cx| input.replace_text_in_range(None, "xyz, zyx", window, cx))
		});
		// The tests bind no keys; main does. The action is what Enter is bound to.
		cx.dispatch_action(crate::ui::text_input::Confirm);
		cx.run_until_parked();
		rdm.read_with(&cx, |rdm, _| {
			let video = rdm.categories.iter().find(|c| c.name == "Video").unwrap();
			let list = video.extensions();
			assert!(!list.contains(&"mkv".to_owned()));
			assert_eq!(&list[list.len() - 2..], ["xyz", "zyx"]);
		});
		assert_eq!(add.read_with(&cx, |input, _| input.content.to_string()), "", "the field clears");
		click(&mut cx, "extension:xyz");
		click(&mut cx, "extension:mkv");
		rdm.read_with(&cx, |rdm, _| {
			let video = rdm.categories.iter().find(|c| c.name == "Video").unwrap();
			let list = video.extensions();
			assert!(list.contains(&"mkv".to_owned()) && !list.contains(&"xyz".to_owned()));
			assert_eq!(list.last().map(String::as_str), Some("zyx"));
		});
		click(&mut cx, "button:Reset");
		rdm.read_with(&cx, |rdm, _| {
			let video = rdm.categories.iter().find(|c| c.name == "Video").unwrap();
			assert_eq!(video.extensions(), Category::preset("Video").unwrap().extensions());
		});
		click(&mut cx, "button:Close");
		rdm.read_with(&cx, |rdm, _| {
			assert!(matches!(rdm.category_sheet, Some(CategorySheet::Presets { editing: false })))
		});
	}

	#[gpui::test]
	fn a_color_is_picked_from_a_swatch_or_written_and_kept(cx: &mut TestAppContext) {
		use crate::ui::theme::Tint;
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Add");
		assert!(cx.debug_bounds("swatch:#b48ead").is_none(), "the picker waits behind the swatch");
		click(&mut cx, "button:Color");
		click(&mut cx, "swatch:#b48ead");
		let (name, extensions) = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
			assert_eq!(form.color, Tint::Purple.rgb());
			(form.name.clone(), form.extensions.clone())
		});
		cx.update(|window, cx| {
			name.update(cx, |input, cx| input.replace_text_in_range(None, "Plum", window, cx));
			extensions.update(cx, |input, cx| input.replace_text_in_range(None, "plum", window, cx));
		});
		click(&mut cx, "button:Create");
		rdm.read_with(&cx, |rdm, _| {
			let plum = rdm.categories.iter().find(|c| c.name == "Plum").expect("added");
			assert_eq!(plum.color, Tint::Purple.rgb());
		});
		// A preset's color, written; the writing stays with the category beside the named hues.
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Edit");
		click(&mut cx, "preset:Audio");
		let custom = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
			form.custom.clone()
		});
		cx.update(|window, cx| {
			window.focus(&custom.read(cx).focus(), cx);
			custom.update(cx, |input, cx| {
				input.replace_text_in_range(None, "rgb(170, 187, 204)", window, cx)
			});
		});
		cx.dispatch_action(crate::ui::text_input::Confirm);
		cx.run_until_parked();
		click(&mut cx, "icon:globe");
		let audio = |rdm: &Rdm| rdm.categories.iter().find(|c| c.name == "Audio").unwrap().clone();
		rdm.read_with(&cx, |rdm, _| {
			let audio = audio(rdm);
			assert_eq!((audio.color, audio.icon), (0xaabbcc, Icon::Globe));
			assert_eq!(audio.custom_color.as_deref(), Some("rgb(170, 187, 204)"), "as written");
		});
		click(&mut cx, "swatch:#8fbcbb");
		rdm.read_with(&cx, |rdm, _| {
			let audio = audio(rdm);
			assert_eq!(audio.color, Tint::Teal.rgb());
			assert!(audio.custom_color.is_some(), "a named hue does not erase the written one");
		});
		click(&mut cx, "swatch:custom");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(audio(rdm).color, 0xaabbcc, "and it can be chosen again")
		});
		assert_eq!(
			custom.read_with(&cx, |input, _| input.content.to_string()),
			"rgb(170, 187, 204)",
			"the field keeps the user's spelling"
		);
	}

	#[gpui::test]
	fn advanced_shares_a_line_with_create_until_it_opens(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Add");
		let advanced = cx.debug_bounds("button:Advanced").unwrap();
		let create = cx.debug_bounds("button:Create").unwrap();
		assert!(
			(f32::from(advanced.center().y) - f32::from(create.center().y)).abs() < 1.0,
			"one line while closed"
		);
		let card = cx.debug_bounds("category-sheet").unwrap();
		assert!(advanced.size.width < card.size.width / 3.0, "Advanced is only as wide as its words");
		click(&mut cx, "button:Advanced");
		let create_open = cx.debug_bounds("button:Create").unwrap();
		assert!(create_open.top() > cx.debug_bounds("button:Advanced").unwrap().bottom());
		rdm.read_with(&cx, |rdm, _| {
			assert!(matches!(&rdm.category_sheet, Some(CategorySheet::Custom(f)) if f.advanced))
		});
	}

	#[gpui::test]
	fn the_custom_form_combines_its_fields_by_the_switch(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Add");
		let (extensions, contains, pattern) = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
			(form.extensions.clone(), form.contains.clone(), form.pattern.clone())
		});
		cx.update(|window, cx| {
			extensions.update(cx, |input, cx| input.replace_text_in_range(None, "pdf", window, cx));
			contains.update(cx, |input, cx| input.replace_text_in_range(None, "rust book", window, cx));
		});
		click(&mut cx, "combine:OR");
		click(&mut cx, "toggle:Ignore spaces");
		click(&mut cx, "button:Advanced");
		let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
		assert_eq!(derived, r"(?:(?i:rust\s*book)|(?i:\.(pdf))$)", "case is ignored until Match case");
		click(&mut cx, "button:Advanced");
		click(&mut cx, "toggle:Match case");
		cx.update(|window, cx| {
			pattern.update(cx, |input, cx| input.set_content("", cx));
			let _ = window;
		});
		click(&mut cx, "button:Advanced");
		let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
		assert_eq!(derived, r"(?:rust\s*book|(?i:\.(pdf))$)");
	}

	#[gpui::test]
	fn add_task_reads_the_clipboard_names_junk_and_offers_a_pages_files(cx: &mut TestAppContext) {
		use crate::engine::testing::{Options, TestServer};
		let page = TestServer::start(
			b"<a href=\"tool.zip\">tool</a> <a href=\"notes.pdf\">notes</a>".to_vec(),
			Options { content_type: Some("text/html".into()), ..Options::default() },
		);
		let (rdm, mut cx) = open(cx);
		cx.write_to_clipboard(gpui::ClipboardItem::new_string("example.org/a.zip".into()));
		click(&mut cx, "button:Add Task");
		let input = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().input.clone());
		assert_eq!(
			input.read_with(&cx, |i, _| i.content.to_string()),
			"https://example.org/a.zip",
			"the clipboard is read as an address, scheme supplied"
		);
		cx.update(|window, cx| {
			input.update(cx, |i, cx| i.set_content("not an address at all", cx));
			let _ = window;
		});
		click(&mut cx, "button:Add");
		assert!(cx.debug_bounds("add-error").is_some(), "junk is named as such");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_some(), "the sheet stays"));

		let address = page.url("/downloads/").to_string();
		cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&address, cx)));
		click(&mut cx, "button:Add");
		// The engine looks at the address on its own threads; the pump collects the answer.
		let mut seen = false;
		for _ in 0..200 {
			std::thread::sleep(Duration::from_millis(10));
			rdm.update(&mut cx, |rdm, cx| rdm.poll_add(cx));
			cx.run_until_parked();
			if rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().is_some_and(|s| s.page.is_some())) {
				seen = true;
				break;
			}
		}
		assert!(seen, "the address was recognised as a page");
		assert!(cx.debug_bounds("add-page").is_some());
		click(&mut cx, "link:tool.zip");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.adding.is_some(), "the sheet stays up for more");
			let added = rdm.downloads.iter().find(|d| d.name == "tool.zip").expect("queued");
			assert!(added.url.ends_with("/downloads/tool.zip"), "{}", added.url);
		});
		click(&mut cx, "link:tool.zip");
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(rdm.downloads.iter().filter(|d| d.name == "tool.zip").count(), 1, "once");
		});
		click(&mut cx, "button:Close");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none()));
	}

	#[gpui::test]
	fn the_rows_come_back_from_the_store_and_the_unfinished_are_queued_again(
		cx: &mut TestAppContext,
	) {
		let dir = std::env::temp_dir().join(format!("rdm-app-store-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let paths = || Paths {
			state: dir.join("state.json"),
			config: dir.join("config.json"),
			database: dir.join("internal.sqlite"),
			downloads: dir.join("downloads"),
		};
		{
			let store = Store::open(&paths().database).unwrap();
			let mut rows = crate::download::sample();
			rows.truncate(4);
			// Downloading, Completed, Paused, Queued in the sample's first four.
			for row in &rows {
				store.save(row).unwrap();
			}
		}
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |window, cx| {
				cx.new(|cx| {
					let (engine, events) = Engine::new(engine::EngineSettings::default()).unwrap();
					Rdm::new(State::default(), Config::seed(), Some(paths()), engine, events, window, cx)
				})
			})
			.unwrap()
		});
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let rdm = window.root(&mut cx).unwrap();
		rdm.read_with(&cx, |rdm, _| {
			let status: Vec<Status> = rdm.downloads.iter().map(|d| d.status).collect();
			assert_eq!(
				status,
				[Status::Queued, Status::Completed, Status::Paused, Status::Queued],
				"the one that was moving is queued again; the rest are as they were"
			);
			assert!(rdm.engine.contains(TaskId(1)) && rdm.engine.contains(TaskId(4)));
			assert!(!rdm.engine.contains(TaskId(2)) && !rdm.engine.contains(TaskId(3)));
		});
		// Resuming a paused row from before hands it to the engine afresh.
		rdm.update(&mut cx, |rdm, cx| rdm.resume(3, cx));
		rdm.read_with(&cx, |rdm, _| assert!(rdm.engine.contains(TaskId(3))));
		// A new row takes an id above every id the store has seen.
		rdm.update(&mut cx, |rdm, cx| rdm.add_url("https://example.org/new.bin", cx));
		let store = Store::open(&paths().database).unwrap();
		let rows = store.load().unwrap();
		assert_eq!(rows.len(), 5);
		assert_eq!(rows[4].id, 5);
		assert_eq!(rows[2].status, Status::Queued, "the resume was written");
		rdm.update(&mut cx, |rdm, cx| rdm.remove(2, cx));
		assert_eq!(store.load().unwrap().len(), 4, "a removed row is gone from the store");
	}

	#[gpui::test]
	fn a_plan_left_in_the_folder_comes_in_as_a_paused_row(cx: &mut TestAppContext) {
		use crate::engine::control::{self, Control};
		use crate::engine::{Plan, Span};
		let dir = std::env::temp_dir().join(format!("rdm-app-stray-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		let downloads = dir.join("downloads");
		std::fs::create_dir_all(&downloads).unwrap();
		let paths = || Paths {
			state: dir.join("state.json"),
			config: dir.join("config.json"),
			database: dir.join("internal.sqlite"),
			downloads: downloads.clone(),
		};
		let mut plan = Plan::whole(Span::new(0, 1000));
		plan.segments[0].done = 300;
		control::save(
			&downloads.join("left.bin"),
			&Control::new("https://h/left.bin", Some(1000), None, plan),
		)
		.unwrap();
		std::fs::write(control::part_path(&downloads.join("left.bin")), vec![0; 1000]).unwrap();
		// A plan that cannot be read stays untouched and unlisted.
		std::fs::write(control::control_path(&downloads.join("odd.bin")), "{ \"version\": 42 }")
			.unwrap();
		std::fs::write(control::part_path(&downloads.join("odd.bin")), vec![0; 10]).unwrap();
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |window, cx| {
				cx.new(|cx| {
					let (engine, events) = Engine::new(engine::EngineSettings::default()).unwrap();
					Rdm::new(State::default(), Config::seed(), Some(paths()), engine, events, window, cx)
				})
			})
			.unwrap()
		});
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let rdm = window.root(&mut cx).unwrap();
		rdm.read_with(&cx, |rdm, _| {
			assert_eq!(rdm.downloads.len(), 1, "the readable one, and only it");
			let row = &rdm.downloads[0];
			assert_eq!(
				(row.name.as_str(), row.status, row.received, row.size),
				("left.bin", Status::Paused, 300, 1000)
			);
			assert_eq!(row.url, "https://h/left.bin");
			assert!(!rdm.engine.contains(TaskId(row.id)), "paused, not running, until resumed by hand");
		});
		assert!(control::control_path(&downloads.join("odd.bin")).exists(), "left where it was");
		assert_eq!(Store::open(&paths().database).unwrap().load().unwrap().len(), 1, "and kept");
	}

	#[gpui::test]
	fn the_guide_lies_over_the_form_and_leaves_it_alone(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		click(&mut cx, "button:Add");
		let name = rdm.read_with(&cx, |rdm, _| {
			let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
			form.name.clone()
		});
		cx.update(|window, cx| {
			name.update(cx, |input, cx| input.replace_text_in_range(None, "Kept", window, cx));
		});
		click(&mut cx, "button:Color");
		click(&mut cx, "button:Color formats");
		assert_eq!(cx.windows().len(), 1, "no window of its own");
		assert!(cx.debug_bounds("guide").is_some());
		// A press outside the guide closes the guide, and does not reach the form under it.
		let row = cx.debug_bounds("row:3").unwrap().center();
		cx.simulate_click(row, Modifiers::default());
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.guide.is_none(), "the guide has nothing to keep");
			assert!(matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))), "the form stays");
			assert_eq!(rdm.selected, None, "and the row behind was not pressed");
		});
		click(&mut cx, "button:Color formats");
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.guide.is_none(), "Escape closes the guide");
			assert!(rdm.category_sheet.is_some(), "and only the guide: the form has text in it");
		});
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.category_sheet.is_some(), "a form with text is closed by its cross alone")
		});
		click(&mut cx, "button:Close");
		click(&mut cx, "button:Close");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none()));
	}

	#[gpui::test]
	fn the_press_that_brings_the_window_back_does_nothing_else(cx: &mut TestAppContext) {
		use gpui::{MouseButton, MouseDownEvent, MouseUpEvent};
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		let row = cx.debug_bounds("row:3").unwrap().center();
		let press = |cx: &mut VisualTestContext, first_mouse: bool| {
			cx.simulate_event(MouseDownEvent {
				button: MouseButton::Left,
				position: row,
				modifiers: Modifiers::default(),
				click_count: 1,
				first_mouse,
			});
			cx.simulate_event(MouseUpEvent {
				button: MouseButton::Left,
				position: row,
				modifiers: Modifiers::default(),
				click_count: 1,
			});
		};
		press(&mut cx, true);
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.category_sheet.is_some(), "the first press only brought the window back");
		});
		press(&mut cx, false);
		rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "the next press counts"));
		press(&mut cx, true);
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, None, "nor does a row take it"));
		press(&mut cx, false);
		rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, Some(3)));
	}

	#[gpui::test]
	fn the_colorful_sidebar_switch_flips_the_preference(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_sidebar, "on to start with"));
		click(&mut cx, "button:Settings");
		click(&mut cx, "setting:Always use colorful sidebar");
		rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.colorful_sidebar));
		click(&mut cx, "setting:Always use colorful sidebar");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_sidebar));
	}

	#[gpui::test]
	fn escape_closes_whatever_clean_sheet_is_on_top(cx: &mut TestAppContext) {
		let (rdm, mut cx) = open(cx);
		click(&mut cx, "button:New category");
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| {
			assert!(rdm.category_sheet.is_none(), "the presets have nothing to keep")
		});
		click(&mut cx, "button:Settings");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.settings_open));
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| assert!(!rdm.settings_open));
		click(&mut cx, "button:Add Task");
		cx.simulate_keystrokes("escape");
		rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none(), "an empty Add Task goes too"));
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
