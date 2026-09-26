//! The download folder read into rows, the archive index, and the rows a previous run left.

use super::*;

#[gpui::test]
fn the_status_bar_spins_while_something_runs_behind_the_window(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	assert!(cx.debug_bounds("activity").is_none(), "at rest the bar is the count alone");
	rdm.update(&mut cx, |rdm, cx| {
		rdm.updates.checking = true;
		cx.notify();
	});
	cx.run_until_parked();
	assert!(cx.debug_bounds("activity").is_some());
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.activities(), ["Checking for updates"]));
	rdm.update(&mut cx, |rdm, _| {
		rdm.updates.checking = false;
		// A read of the folder that has only just started is not yet worth a spinner.
		let (_, receiver) = std::sync::mpsc::channel();
		rdm.folder_scan = Some((std::time::Instant::now(), receiver));
		assert!(rdm.activities().is_empty());
		rdm.folder_scan = Some((
			std::time::Instant::now() - crate::app::SPINNER_AFTER * 2,
			rdm.folder_scan.take().unwrap().1,
		));
		assert_eq!(rdm.activities(), ["Reading the folder"]);
	});
}

#[gpui::test]
fn an_archive_in_the_folder_is_indexed_and_placed_by_what_it_holds(cx: &mut TestAppContext) {
	use std::io::Write;
	let (rdm, mut cx) = open_in(cx, "archive-index");
	let directory = rdm.read_with(&cx, |rdm, _| rdm.paths.as_ref().unwrap().downloads.clone());
	for stale in std::fs::read_dir(&directory).unwrap().flatten() {
		let _ = std::fs::remove_file(stale.path()).or_else(|_| std::fs::remove_dir_all(stale.path()));
	}
	let zip_path = directory.join("tool.zip");
	{
		let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
		zip.start_file("setup.exe", zip::write::SimpleFileOptions::default()).unwrap();
		zip.write_all(b"MZ").unwrap();
		zip.finish().unwrap();
	}
	rdm.update(&mut cx, |rdm, _| rdm.scan_folder());
	wait_for_folder(&rdm, &mut cx);
	wait_for_index(&rdm, &mut cx);
	rdm.read_with(&cx, |rdm, _| {
		let row = rdm.folder_files.iter().find(|d| d.name == "tool.zip").expect("the zip is a row");
		let names: Vec<&str> = rdm.categories_of(row).iter().map(|c| c.name.as_str()).collect();
		assert_eq!(names, ["Archives", "Programs"], "a zip of one program is a program too");
		assert_eq!(rdm.contents_of(row), ["setup.exe"]);
		let programs = rdm.categories.iter().find(|c| c.name == "Programs").unwrap().id;
		assert!(rdm.passes(Filter::Category(programs), row));
		let key = zip_path.to_string_lossy().into_owned();
		assert!(rdm.store.as_ref().unwrap().archives().unwrap().contains_key(&key), "and kept");
	});
	// The file gone, its index goes with it at the next look.
	std::fs::remove_file(&zip_path).unwrap();
	rdm.update(&mut cx, |rdm, _| rdm.queue_indexing());
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.archives.is_empty());
		assert!(rdm.store.as_ref().unwrap().archives().unwrap().is_empty());
	});
}

/// A download folder's own folders are ignored, flattened, or kept as folders. Flattening is the
/// default: what somebody wants from a download folder is usually the files, and a folder of
/// folders hides them.
#[gpui::test]
fn the_folders_own_folders_are_ignored_flattened_or_kept(cx: &mut TestAppContext) {
	use crate::download::Folders;
	let (rdm, mut cx) = open_in(cx, "folders");
	let directory = rdm.read_with(&cx, |rdm, _| rdm.paths.as_ref().unwrap().downloads.clone());
	for stale in std::fs::read_dir(&directory).unwrap().flatten() {
		let _ = std::fs::remove_file(stale.path()).or_else(|_| std::fs::remove_dir_all(stale.path()));
	}
	std::fs::write(directory.join("loose.txt"), b"loose").unwrap();
	std::fs::create_dir(directory.join("papers")).unwrap();
	std::fs::write(directory.join("papers/inside.pdf"), b"paper").unwrap();
	std::fs::create_dir(directory.join("papers/deeper")).unwrap();
	std::fs::write(directory.join("papers/deeper/buried.txt"), b"deep").unwrap();
	// A bundle is a directory the system draws as one file, and is left as one.
	std::fs::create_dir(directory.join("Thing.app")).unwrap();
	std::fs::write(directory.join("Thing.app/binary"), b"mach-o").unwrap();

	let names = |rdm: &Rdm| -> Vec<String> { rdm.shown().iter().map(|d| d.name.clone()).collect() };
	rdm.update(&mut cx, |rdm, cx| rdm.set_folders(Folders::Flatten, cx));
	wait_for_folder(&rdm, &mut cx);
	rdm.read_with(&cx, |rdm, _| {
		let shown = names(rdm);
		assert!(shown.contains(&"loose.txt".to_owned()));
		assert!(shown.contains(&"inside.pdf".to_owned()), "flattened out: {shown:?}");
		assert!(shown.contains(&"buried.txt".to_owned()), "however deep");
		assert!(!shown.contains(&"papers".to_owned()), "and no row for the folder itself");
		assert!(!shown.contains(&"binary".to_owned()), "a bundle is one file, not its contents");
	});

	rdm.update(&mut cx, |rdm, cx| rdm.set_folders(Folders::Ignore, cx));
	wait_for_folder(&rdm, &mut cx);
	rdm.read_with(&cx, |rdm, _| {
		let shown = names(rdm);
		assert_eq!(shown.iter().filter(|n| *n == "loose.txt").count(), 1);
		assert!(!shown.contains(&"inside.pdf".to_owned()), "nothing from inside: {shown:?}");
	});

	rdm.update(&mut cx, |rdm, cx| rdm.set_folders(Folders::Tree, cx));
	wait_for_folder(&rdm, &mut cx);
	let papers = rdm.read_with(&cx, |rdm, _| {
		let shown = names(rdm);
		assert!(shown.contains(&"papers".to_owned()), "the folder is a row: {shown:?}");
		assert!(!shown.contains(&"inside.pdf".to_owned()), "and what is inside waits to be opened");
		rdm.shown().iter().find(|d| d.name == "papers").map(|d| d.id).expect("the folder's row")
	});
	rdm.update(&mut cx, |rdm, cx| rdm.toggle_folder(papers, cx));
	rdm.read_with(&cx, |rdm, _| {
		let shown = names(rdm);
		assert!(shown.contains(&"inside.pdf".to_owned()), "opened: {shown:?}");
		assert!(!shown.contains(&"buried.txt".to_owned()), "but not what is inside the next one");
	});
	rdm.update(&mut cx, |rdm, cx| rdm.toggle_folder(papers, cx));
	rdm.read_with(&cx, |rdm, _| {
		assert!(!names(rdm).contains(&"inside.pdf".to_owned()), "and closed again");
	});
}

/// The junk a download folder collects is kept out of the lists, and a torrent is the one kind
/// that is filed rather than dropped: no row among the downloads, a row under Torrents.
#[gpui::test]
fn the_folders_junk_is_hidden_and_a_torrent_shows_under_its_own_category(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open_in(cx, "junk");
	let directory = rdm.read_with(&cx, |rdm, _| rdm.paths.as_ref().unwrap().downloads.clone());
	for stale in std::fs::read_dir(&directory).unwrap().flatten() {
		let _ = std::fs::remove_file(stale.path()).or_else(|_| std::fs::remove_dir_all(stale.path()));
	}
	std::fs::write(directory.join("Thumbs.db"), b"junk").unwrap();
	std::fs::write(directory.join("~$Report.docx"), b"junk").unwrap();
	std::fs::write(directory.join("ubuntu-24.04.torrent"), b"d8:announce").unwrap();
	std::fs::write(directory.join("notes.txt"), b"a real file").unwrap();
	rdm.update(&mut cx, |rdm, _| rdm.scan_folder());
	wait_for_folder(&rdm, &mut cx);
	rdm.read_with(&cx, |rdm, _| {
		let names: Vec<&str> = rdm.shown().iter().map(|d| d.name.as_str()).collect();
		assert!(names.contains(&"notes.txt"), "a real file is a row: {names:?}");
		assert!(!names.contains(&"Thumbs.db"), "the system's leavings are not");
		assert!(!names.contains(&"~$Report.docx"), "nor an editor's scratch");
		assert!(!names.contains(&"ubuntu-24.04.torrent"), "nor a torrent, among the downloads");
	});
	// Torrents is a preset the seed leaves in the sheet, so it is taken first, the way a user
	// who downloads torrents would take it. See `Category::COMMON`.
	click(&mut cx, "button:New category");
	click(&mut cx, "preset:Torrents");
	rdm.update(&mut cx, |rdm, cx| rdm.close_category_sheet(cx));
	cx.run_until_parked();
	let torrents =
		rdm.read_with(&cx, |rdm, _| rdm.categories.iter().find(|c| c.name == "Torrents").map(|c| c.id));
	let torrents = torrents.expect("the preset was taken");
	rdm.update(&mut cx, |rdm, cx| rdm.set_filter(Filter::Category(torrents), cx));
	rdm.read_with(&cx, |rdm, _| {
		let names: Vec<&str> = rdm.shown().iter().map(|d| d.name.as_str()).collect();
		assert!(names.contains(&"ubuntu-24.04.torrent"), "but a row under Torrents: {names:?}");
		assert!(!names.contains(&"Thumbs.db"), "and the dropped kind stays dropped");
	});
	// Told to show it all, it shows it all.
	rdm.update(&mut cx, |rdm, cx| {
		rdm.set_filter(Filter::All, cx);
		rdm.set_hide_junk(false, cx);
	});
	rdm.read_with(&cx, |rdm, _| {
		let names: Vec<&str> = rdm.shown().iter().map(|d| d.name.as_str()).collect();
		assert!(names.contains(&"Thumbs.db") && names.contains(&"ubuntu-24.04.torrent"), "{names:?}");
	});
}

#[gpui::test]
fn the_funnel_in_the_corner_lets_the_folder_files_into_every_list_they_fit(
	cx: &mut TestAppContext,
) {
	let (rdm, mut cx) = open_in(cx, "folder-files");
	let directory = rdm.read_with(&cx, |rdm, _| rdm.paths.as_ref().unwrap().downloads.clone());
	for stale in std::fs::read_dir(&directory).unwrap().flatten() {
		let _ = std::fs::remove_file(stale.path()).or_else(|_| std::fs::remove_dir_all(stale.path()));
	}
	std::fs::write(directory.join("photo.jpg"), b"jpeg").unwrap();
	std::fs::write(directory.join(".DS_Store"), b"").unwrap();
	std::fs::write(directory.join("movie.mkv.rdm"), b"plan").unwrap();
	std::fs::create_dir(directory.join("folder")).unwrap();
	let sample = rdm.read_with(&cx, |rdm, _| rdm.downloads[0].name.clone());
	std::fs::write(directory.join(&sample), b"already a row").unwrap();
	let downloads = rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.folder_shown, "a first launch shows the folder's files without being asked");
		rdm.shown().len()
	});
	// The files were written after the window opened, so the folder is read again, as the watcher
	// has it read whenever it changes while the funnel is lit.
	rdm.update(&mut cx, |rdm, _| rdm.scan_folder());
	wait_for_folder(&rdm, &mut cx);
	rdm.read_with(&cx, |rdm, _| {
		let shown = rdm.shown();
		let extra: Vec<&Download> =
			shown.iter().copied().filter(|d| Rdm::is_folder_file(d.id)).collect();
		assert_eq!(shown.len(), downloads + extra.len(), "the downloads are all still there");
		let names: Vec<&str> = extra.iter().map(|d| d.name.as_str()).collect();
		assert_eq!(names, ["photo.jpg"], "one plain file: no hidden, no plan, no directory, no row");
		assert_eq!((extra[0].status, extra[0].size), (Status::Completed, 4));
	});
	click(&mut cx, "filter:Completed");
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			rdm.shown().iter().any(|d| Rdm::is_folder_file(d.id)),
			"a folder file is complete, so Completed lists it too"
		);
	});
	click(&mut cx, "filter:Images");
	rdm.read_with(&cx, |rdm, _| {
		let names: Vec<&str> = rdm.shown().iter().map(|d| d.name.as_str()).collect();
		assert!(names.contains(&"photo.jpg"), "and the category its name fits: {names:?}");
	});
	click(&mut cx, "filter:All Tasks");
	click(&mut cx, "button:Folder files");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.shown().len(), downloads, "pressed again, the downloads alone");
		assert!(rdm.folder_files.is_empty());
	});
}

#[gpui::test]
fn the_rows_come_back_from_the_store_and_the_unfinished_are_queued_again(cx: &mut TestAppContext) {
	let dir = scratch("app-store");
	let paths = || Paths::under(&dir);
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
	let dir = scratch("app-stray");
	let paths = || Paths::under(&dir);
	let downloads = paths().downloads;
	std::fs::create_dir_all(&downloads).unwrap();
	let mut plan = Plan::whole(Span::new(0, 1000));
	plan.segments[0].done = 300;
	control::save(
		&downloads.join("left.bin"),
		&Control::new("https://h/left.bin", Some(1000), None, plan),
	)
	.unwrap();
	std::fs::write(control::part_path(&downloads.join("left.bin")), vec![0; 1000]).unwrap();
	// A plan that cannot be read stays untouched and unlisted.
	std::fs::write(control::control_path(&downloads.join("odd.bin")), "{ \"version\": 42 }").unwrap();
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
