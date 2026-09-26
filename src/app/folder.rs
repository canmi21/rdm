//! The download folder's own files: read in the background, turned into rows, and folded into
//! folders or flattened as the preference says.

use super::*;

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
pub(super) const DEEPEST: u8 = 8;
/// And how many rows it will make. A folder of a hundred thousand files is a folder to open in a
/// file manager, not a list to draw.
pub(super) const AT_MOST: usize = 20_000;

pub(super) fn read_into(
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
pub(super) fn is_bundle(path: &std::path::Path) -> bool {
	const BUNDLES: [&str; 8] =
		["app", "bundle", "framework", "kext", "plugin", "prefpane", "qlgenerator", "appdir"];
	path
		.extension()
		.and_then(|e| e.to_str())
		.map(str::to_ascii_lowercase)
		.is_some_and(|e| BUNDLES.contains(&e.as_str()))
}

impl Rdm {
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
	/// How many cards the grid fits across the window it has, and the gap that goes between them.
	/// The arithmetic is in `crate::ui::list`, beside the width it is about; this is the room to
	/// do it in -- the window less the sidebar and the padding the grid keeps around its cards.
	pub(crate) fn grid_columns(&self) -> (usize, f32) {
		crate::ui::list::grid_columns(f32::from(self.viewport.width) - crate::ui::sidebar::WIDTH)
	}

	pub(crate) fn folder_shape(&self, id: u64) -> Option<(u8, bool)> {
		self.folder_shape.get(&id).copied()
	}

	/// Whether a row inside a folder is drawn: only where every folder between it and the
	/// download folder has been opened. Nothing else can be hidden this way -- flattening and
	/// ignoring make no folder rows, so nothing has a folder to be inside of.
	pub(super) fn under_an_open_folder(&self, download: &Download) -> bool {
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
}
