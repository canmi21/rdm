//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::Duration;

use gpui::{
	App, Bounds, Context, IntoElement, Render, Task, TitlebarOptions, Window, WindowBounds,
	WindowHandle, WindowOptions, div, prelude::*, px, size,
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

mod categories;
mod indexing;
mod notices;
#[cfg(test)]
mod tests;
mod transfers;
pub(crate) use transfers::Asked;
mod updates;

/// How the list is drawn. Detailed is the default because it is the one that shows progress,
/// speed and size at once; the others trade that for density or for a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum View {
	/// One line a row: what a long queue is read in.
	Compact,
	/// The system's own icon for the file, and its name. What a file manager shows.
	Thumbnails,
	/// The table: type, name, size, progress, speed, status, added.
	Detailed,
	/// A card with a large icon, or a picture of the file where one can be made.
	Grid,
}

impl View {
	/// In the order the switcher offers them, densest first: a line, a row with a picture in it,
	/// the whole table, and cards.
	pub const ALL: [View; 4] = [View::Compact, View::Thumbnails, View::Detailed, View::Grid];
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
	/// The width a column will not go below. It is a floor, not a taste: enough that the cell is
	/// still read -- "1.2G", a stub of a bar, a truncated word beside its mark -- and no wider,
	/// because a floor anyone would willingly stop at is one that a drag runs into. What every
	/// floor comes to, plus the name's and the chrome around them, is the window's own minimum
	/// width, so the table is never given less room than its floors need. See spec/ui.md.
	pub const MINS: [f32; 5] = [40.0, 40.0, 40.0, 40.0, 40.0];
	/// The widths the columns start with, and go back to on the header's reset.
	pub const DEFAULT_WIDTHS: [f32; 5] = [132.0, 150.0, 84.0, 112.0, 108.0];

	pub fn min(self) -> f32 {
		Self::MINS[self.index()]
	}

	fn index(self) -> usize {
		self as usize
	}
}

/// A drag on a column's edge in progress: which column, where the pointer started, and every
/// width as it stood then. A move recomputes the whole row from that snapshot rather than from
/// the row it last left, so a drag back the way it came gives back exactly what it took.
#[derive(Clone, Copy, Debug)]
pub struct Resize {
	pub column: Column,
	pub from_x: gpui::Pixels,
	/// The row as it was drawn when the press landed, which is the geometry the drag works in.
	pub from_widths: [f32; 5],
	/// The widths as they were asked for, which at a narrow window is not the row that was drawn.
	/// A drag that comes to move nothing puts these back, so taking hold of a handle at a window
	/// too narrow to give anything cannot quietly spend what the window is holding back.
	pub asked: [f32; 5],
}

/// Where the folder's files are numbered from: far above any download's id, which the store
/// hands out from one.
pub(crate) const FOLDER_ID: u64 = 1 << 62;

/// How long something may run behind the window before the status bar spins for it.
pub(crate) const SPINNER_AFTER: Duration = Duration::from_millis(300);

/// One of the folder's files as the scan found it, before it is a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FolderFile {
	pub name: String,
	pub size: u64,
	pub modified: Option<std::time::SystemTime>,
	pub path: std::path::PathBuf,
	/// How far inside the download folder it is: nothing at the top, one for a file in a folder
	/// there, and so on. Zero unless the folders are being kept as folders, since flattening is
	/// the whole point of not keeping them.
	pub depth: u8,
	/// Whether the row is the folder rather than a file in it.
	pub directory: bool,
}

/// The folder read for `scan_folder`, off the window's thread: every plain file that is not
/// hidden, not one of a download's two files meanwhile, and not named or placed by a download
/// in `taken`, in name order.
pub(crate) fn read_folder(
	directory: &std::path::Path,
	taken: &[(String, Option<std::path::PathBuf>)],
	folders: crate::download::Folders,
) -> Vec<FolderFile> {
	let mut files = Vec::new();
	read_into(&mut files, directory, taken, folders, 0);
	files
}

/// How deep the reader will go. A download folder is not a filesystem and a folder nested eight
/// deep in one is not what anybody came for; the limit is there so a symlink loop or a checked
/// out repository cannot hold the read open.
const DEEPEST: u8 = 8;
/// And how many rows it will make. A folder of a hundred thousand files is a folder to open in a
/// file manager, not a list to draw.
const AT_MOST: usize = 20_000;

fn read_into(
	files: &mut Vec<FolderFile>,
	directory: &std::path::Path,
	taken: &[(String, Option<std::path::PathBuf>)],
	folders: crate::download::Folders,
	depth: u8,
) {
	use crate::download::Folders;
	let Ok(entries) = std::fs::read_dir(directory) else { return };
	// How deep a row says it is. Only the tree draws anything from it: flattening puts what it
	// finds at the top level, and a row that remembered being one down would then be hidden by
	// the folder it is no longer shown inside.
	let shown = if matches!(folders, Folders::Tree) { depth } else { 0 };
	let mut here: Vec<FolderFile> = entries
		.flatten()
		.filter_map(|entry| {
			let path = entry.path();
			let name = path.file_name()?.to_str()?.to_owned();
			let metadata = entry.metadata().ok()?;
			if name.starts_with('.') {
				return None;
			}
			if metadata.is_dir() {
				// A bundle is a directory the system draws as one file, and it is one: reading
				// inside a `.app` would list its whole contents where the application belongs.
				let bundled = matches!(folders, Folders::Ignore) || is_bundle(&path);
				return (!bundled).then(|| FolderFile {
					name,
					size: 0,
					modified: metadata.modified().ok(),
					path,
					depth: shown,
					directory: true,
				});
			}
			(metadata.is_file()
				&& engine::control::target_of(&path).is_none()
				&& !taken.iter().any(|(n, p)| *n == name || p.as_deref() == Some(path.as_path())))
			.then(|| FolderFile {
				name,
				size: metadata.len(),
				modified: metadata.modified().ok(),
				path,
				depth: shown,
				directory: false,
			})
		})
		.collect();
	// Folders first and then files, each in name order, which is what a file manager does.
	here.sort_by(|a, b| b.directory.cmp(&a.directory).then_with(|| a.name.cmp(&b.name)));
	for entry in here {
		if files.len() >= AT_MOST {
			return;
		}
		let inside = entry.directory.then(|| entry.path.clone());
		// Flattening keeps no row for the folder itself; keeping them as folders keeps the row
		// and puts what is inside under it.
		let keep = !entry.directory || matches!(folders, Folders::Tree);
		if keep {
			files.push(entry);
		}
		if let Some(inside) = inside
			&& depth < DEEPEST
		{
			read_into(files, &inside, taken, folders, depth + 1);
		}
	}
}

/// A directory the system draws as one thing: a macOS bundle, and the two Linux directories that
/// are handed about as if they were files.
fn is_bundle(path: &std::path::Path) -> bool {
	const BUNDLES: [&str; 8] =
		["app", "bundle", "framework", "kext", "plugin", "prefpane", "qlgenerator", "appdir"];
	path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase)
		.is_some_and(|e| BUNDLES.contains(&e.as_str()))
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
	pub(crate) notices: Vec<notices::Shown>,
	/// The notices that are windows of their own, while they are up. A handle stays here after
	/// its window closes and is found dead on the next one, as the download windows' do.
	pub(crate) notice_windows: Vec<WindowHandle<crate::ui::notice_window::NoticeWindow>>,
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
			settings: None,
			adding: None,
			paths,
			frame: saved.window,
			maximized: saved.maximized,
			save: None,
			_tick: tick,
			folder_shape: HashMap::new(),
			opened: std::collections::HashSet::new(),
			notices: Vec::new(),
			notice_windows: Vec::new(),
			updates: updates::Updates::default(),
			_checks: None,
		};
		this.engine.set_speed_limit(this.preferences.speed_limit);
		this.engine.set_max_active(this.preferences.max_active);
		this.import_strays();
		this.load_archives();
		// The funnel left lit last time reads the folder now, as a press would.
		if this.folder_shown {
			this.scan_folder();
		}
		this.queue_indexing();
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

	/// Every row the lists are cut from: the downloads, and with the funnel lit, the folder's
	/// other files. A filter or a count reads these, so a file the funnel let in is under All
	/// Tasks, under Completed, and under whichever category its name fits, like a download
	/// that finished.
	pub(crate) fn rows(&self) -> impl Iterator<Item = &Download> {
		let folder = if self.folder_shown { &self.folder_files[..] } else { &[] };
		self.downloads.iter().chain(folder)
	}

	/// The rows the list shows, in the order it shows them: what the sidebar's filter and the
	/// status menu let through.
	pub(crate) fn shown(&self) -> Vec<&Download> {
		let mut rows: Vec<&Download> = self
			.rows()
			.filter(|d| {
				self.passes(self.filter, d)
					&& self.status.is_none_or(|s| d.status == s)
					&& self.worth_a_row(d)
					&& self.under_an_open_folder(d)
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

	/// Whether a row is worth showing at all in the list as it stands. Junk is not, while the
	/// preference says so -- except a kind that is filed rather than dropped, which is worth a
	/// row under the category it belongs to, since that is where somebody looking for one looks.
	/// See `download::junk`.
	fn worth_a_row(&self, download: &Download) -> bool {
		if !self.preferences.hide_junk {
			return true;
		}
		match crate::download::junk(&download.name) {
			None => true,
			Some(crate::download::Junk::Noise) => false,
			Some(crate::download::Junk::Filed) => matches!(self.filter, Filter::Category(_)),
		}
	}

	/// The categories a row is in: by its name, and for an archive that has been read, by what
	/// it holds. See src/app/indexing.rs.
	pub(crate) fn categories_of(&self, download: &Download) -> Vec<&Category> {
		categories_with_contents(&self.categories, download, &self.contents_of(download))
	}

	/// Whether the sidebar's filter lets a row through, judging a category by `categories_of`.
	pub(crate) fn passes(&self, filter: Filter, download: &Download) -> bool {
		match filter {
			Filter::Category(id) => self.categories_of(download).iter().any(|c| c.id == id),
			other => other.matches(download, &self.categories),
		}
	}

	/// The category a row is drawn as: the filtered one when one is filtered and matches, else
	/// the first that matches. None when nothing does and there is no catch-all.
	fn category_shown(&self, download: &Download) -> Option<&Category> {
		// An archive that is a program, or an album, by what it holds wears that icon in every
		// list, the Archives list included: the contents are what it is.
		if let Some(nature) = category::nature(&self.categories, download, &self.contents_of(download))
		{
			return Some(nature);
		}
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
			// The icon is the category's; the colour may be the extension's, where the category
			// draws one of its own apart. See `Category::shade` and spec/ui.md.
			Some(c) => (c.icon, self.palette.hue(c.shade(&download.name))),
			None => (Icon::File, self.palette.muted),
		}
	}

	pub(crate) fn download(&self, id: u64) -> Option<&Download> {
		self.downloads.iter().chain(&self.folder_files).find(|d| d.id == id)
	}

	/// Whether the row is one of the folder's files rather than a download: nothing to pause,
	/// resume or forget, and nothing the engine or the store knows.
	pub(crate) fn is_folder_file(id: u64) -> bool {
		id >= FOLDER_ID
	}

	/// The funnel in the header's corner: lit, the lists also hold the folder's other files;
	/// pressed again, the downloads alone. The state is kept for the next launch.
	pub(crate) fn toggle_folder_files(&mut self, cx: &mut Context<Self>) {
		self.folder_shown = !self.folder_shown;
		if self.folder_shown {
			self.scan_folder();
		} else {
			self.folder_files.clear();
			self.folder_scan = None;
			if self.selected.is_some_and(Self::is_folder_file) {
				self.selected = None;
			}
		}
		self.schedule_save(cx);
		cx.notify();
	}

	/// The download folder's files that are not a download's: not hidden, not one of the two
	/// files a download keeps meanwhile, and not named by any row. Read on the engine's
	/// runtime, since a folder of thousands takes longer than a frame; the rows arrive through
	/// `pump_events`, and the status bar shows a spinner if they take more than a moment.
	pub(crate) fn scan_folder(&mut self) {
		let Some(directory) = self.paths.as_ref().map(|p| p.downloads.clone()) else { return };
		let taken: Vec<(String, Option<std::path::PathBuf>)> = self
			.downloads
			.iter()
			.map(|d| (d.name.clone(), d.path.as_deref().map(std::path::PathBuf::from)))
			.collect();
		let folders = self.preferences.folders;
		let receiver = self.engine.run(async move {
			tokio::task::spawn_blocking(move || read_folder(&directory, &taken, folders))
				.await
				.unwrap_or_default()
		});
		self.folder_scan = Some((std::time::Instant::now(), receiver));
	}

	/// The rows a scan produced, in place of the last: each is a completed row with the file's
	/// size and time and no address, in name order, numbered from `FOLDER_ID` so a press on one
	/// finds it and nothing mistakes it for a download.
	/// How deep a folder row sits and whether it is a folder, by its id. A `Download` is the
	/// engine's row and has no room for either, and the two lists are built together and
	/// numbered together, so a second list beside it is the smaller of the two lies.
	pub(crate) fn folder_shape(&self, id: u64) -> Option<(u8, bool)> {
		self.folder_shape.get(&id).copied()
	}

	/// Whether a row inside a folder is drawn: only where every folder between it and the
	/// download folder has been opened. Nothing else can be hidden this way -- flattening and
	/// ignoring make no folder rows, so nothing has a folder to be inside of.
	fn under_an_open_folder(&self, download: &Download) -> bool {
		let Some((depth, _)) = self.folder_shape(download.id) else { return true };
		if depth == 0 {
			return true;
		}
		let (Some(root), Some(path)) = (
			self.paths.as_ref().map(|p| p.downloads.clone()),
			download.path.as_deref().map(std::path::PathBuf::from),
		) else {
			return true;
		};
		let mut here = path.parent().map(std::path::Path::to_path_buf);
		while let Some(folder) = here {
			if folder == root {
				return true;
			}
			if !self.opened.contains(&folder) {
				return false;
			}
			here = folder.parent().map(std::path::Path::to_path_buf);
		}
		true
	}

	/// Opens a folder row, or closes it. Closing it closes what is under it by the same rule
	/// that hid it, so nothing has to be walked.
	pub(crate) fn toggle_folder(&mut self, id: u64, cx: &mut Context<Self>) {
		let Some(path) = self.download(id).and_then(|d| d.path.clone()) else { return };
		let path = std::path::PathBuf::from(path);
		if !self.opened.remove(&path) {
			self.opened.insert(path);
		}
		cx.notify();
	}

	pub(crate) fn adopt_folder_files(&mut self, files: Vec<FolderFile>) {
		let selected = self
			.selected
			.filter(|&id| Self::is_folder_file(id))
			.and_then(|id| self.folder_files.iter().find(|d| d.id == id).map(|d| d.name.clone()));
		self.folder_shape = files
			.iter()
			.enumerate()
			.filter(|(_, file)| file.depth > 0 || file.directory)
			.map(|(index, file)| (FOLDER_ID + index as u64, (file.depth, file.directory)))
			.collect();
		self.folder_files = files
			.into_iter()
			.enumerate()
			.map(|(index, file)| Download {
				id: FOLDER_ID + index as u64,
				name: file.name,
				url: String::new(),
				size: file.size,
				received: file.size,
				speed: 0,
				status: Status::Completed,
				added: file.modified.map_or_else(chrono::Local::now, chrono::DateTime::from),
				source: None,
				path: Some(file.path.to_string_lossy().into_owned()),
				error: None,
				connections: None,
				directory: None,
				mirrors: Vec::new(),
				checksum: None,
				range: None,
				speed_limit: None,
			})
			.collect();
		// The rows were renumbered; the selection follows its file by name, or lets go.
		if let Some(name) = selected {
			self.selected = self.folder_files.iter().find(|d| d.name == name).map(|d| d.id);
		} else if self.selected.is_some_and(Self::is_folder_file) {
			self.selected = None;
		}
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

	/// What a column is drawn at. The stored width is what was asked for; this is what the table
	/// has room for, which is less whenever the window is too narrow to hold them all. The
	/// shortfall is shared out in proportion to what each column has to spare above its floor, so
	/// narrowing the window compresses the table evenly rather than crushing one column, and every
	/// column lands exactly on its floor at the window's own minimum width -- which is where that
	/// minimum comes from. The stored widths are untouched, so widening gives back what narrowing
	/// took, to the pixel.
	pub(crate) fn width(&self, column: Column) -> f32 {
		self.drawn()[column.index()]
	}

	pub(crate) fn drawn(&self) -> [f32; 5] {
		let mut widths = self.widths;
		let short = crate::ui::list::NAME_MIN - self.name_width(&widths);
		let spare: f32 = widths.iter().zip(Column::MINS).map(|(w, min)| (w - min).max(0.0)).sum();
		if short <= 0.0 || spare <= 0.0 {
			return widths;
		}
		let taken = short.min(spare);
		for (width, min) in widths.iter_mut().zip(Column::MINS) {
			*width -= (*width - min).max(0.0) / spare * taken;
		}
		widths
	}

	/// The drag starts from what is on screen, not from what was asked for: at a narrow window the
	/// two differ, and the boundary has to leave from under the pointer.
	pub(crate) fn begin_resize(&mut self, column: Column, at: gpui::Pixels) {
		self.resizing = Some(Resize { column, from_x: at, from_widths: self.drawn(), asked: self.widths });
	}

	/// What the name column is left once the fixed columns and their handles have taken theirs.
	pub(crate) fn name_width(&self, widths: &[f32; 5]) -> f32 {
		let table =
			f32::from(self.viewport.width) - crate::ui::sidebar::WIDTH - crate::ui::list::TABLE_CHROME;
		table - 5.0 * crate::ui::list::HANDLE_W - widths.iter().sum::<f32>()
	}

	/// Called for every pointer move over the window. The handle is a column's left edge and the
	/// table is anchored at its right, so moving the boundary left widens the column. A move with
	/// the button up ends the drag: the release happened outside the window, unseen.
	///
	/// What widening takes has to come from the left of the handle, and it is taken in the order
	/// the eye expects the squeeze to travel: the name column first, since it is the one holding
	/// the slack, then each fixed column between the name and the handle, nearest first, each down
	/// to its own floor and no further. The boundary only stops once everything left of it is on
	/// its floor -- there is no ceiling derived from any one column, so nothing to snap to when a
	/// press lands, and a floor small enough that the stop is rarely reached at all.
	pub(crate) fn resize_to(&mut self, at: gpui::Pixels, pressed: bool, cx: &mut Context<Self>) {
		let Some(resize) = self.resizing else { return };
		if !pressed {
			self.end_resize(cx);
			return;
		}
		let column = resize.column.index();
		let mut widths = resize.from_widths;
		widths[column] =
			(resize.from_widths[column] - f32::from(at - resize.from_x)).max(resize.column.min());
		// Narrowing owes nothing: the name column takes back what is given up. Widening owes the
		// difference, and asks for it leftwards until it is met or nobody has any left.
		let mut owed = widths[column] - resize.from_widths[column];
		owed -= (self.name_width(&resize.from_widths) - crate::ui::list::NAME_MIN).max(0.0);
		for other in (0..column).rev() {
			if owed <= 0.0 {
				break;
			}
			let spare = (resize.from_widths[other] - Column::MINS[other]).max(0.0).min(owed);
			widths[other] = resize.from_widths[other] - spare;
			owed -= spare;
		}
		// Asked for more than the row had: the boundary stops where the last of it was found.
		if owed > 0.0 {
			widths[column] = (widths[column] - owed).max(resize.column.min());
		}
		// A drag that has come to move nothing leaves the asked-for widths as they were, squeezed
		// or not, so that letting go where the press landed is the same as never having pressed.
		self.widths = if widths == resize.from_widths { resize.asked } else { widths };
		cx.notify();
	}

	pub(crate) fn end_resize(&mut self, cx: &mut Context<Self>) {
		if self.resizing.take().is_some() {
			self.schedule_save(cx);
			cx.notify();
		}
	}

	/// Every column back to the width it started with, from Reset under Appearance in Settings.
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
			folder_shown: Some(self.folder_shown),
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
		self.palette = theme::palette(window.is_window_active() || !self.preferences.dim_inactive);
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
			.when_some(self.corner(cx), |s, corner| s.child(corner))
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
