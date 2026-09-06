// Headless: the test platform draws the window into no screen, so this exercises what a click
// does without a window, a pointer or a display. See spec/workflow.md.
use gpui::{Entity, EntityInputHandler, Modifiers, TestAppContext, VisualTestContext};

use super::*;
use crate::testing::scratch;

/// Somewhere under the temp directory, so a test that really downloads writes there and not
/// into the repository -- which one did, and three commits carried its files.
fn scratch_paths(name: &str) -> Paths {
	let paths = Paths::under(&scratch(name));
	std::fs::create_dir_all(&paths.downloads).unwrap();
	paths
}

fn open(cx: &mut TestAppContext) -> (Entity<Rdm>, VisualTestContext) {
	open_in(cx, "open")
}

/// The same, with a download folder of the test's own, for one that reads the folder.
fn open_in(cx: &mut TestAppContext, name: &str) -> (Entity<Rdm>, VisualTestContext) {
	let window = cx.update(|cx| {
		cx.open_window(Default::default(), |window, cx| {
			cx.new(|cx| {
				let (engine, events) = Engine::new(engine::EngineSettings::default()).unwrap();
				let paths = scratch_paths(name);
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

/// The same, for the archive index: until no run is under way.
fn wait_for_index(rdm: &Entity<Rdm>, cx: &mut VisualTestContext) {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	while std::time::Instant::now() < deadline {
		rdm.update(cx, |rdm, cx| rdm.pump_events(cx));
		cx.run_until_parked();
		if rdm.read_with(cx, |rdm, _| rdm.indexing.is_none()) {
			return;
		}
		std::thread::sleep(std::time::Duration::from_millis(20));
	}
	panic!("the archives were not indexed in time");
}

/// Runs the window's ticks until the folder's rows have arrived from the background read, or
/// gives up after a few seconds: the read is a real thread, so the test clock cannot hurry it.
fn wait_for_folder(rdm: &Entity<Rdm>, cx: &mut VisualTestContext) {
	let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
	while std::time::Instant::now() < deadline {
		rdm.update(cx, |rdm, cx| rdm.pump_events(cx));
		cx.run_until_parked();
		if rdm.read_with(cx, |rdm, _| rdm.folder_scan.is_none()) {
			return;
		}
		std::thread::sleep(std::time::Duration::from_millis(20));
	}
	panic!("the folder was not read in time");
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
	rdm
		.read_with(&cx, |rdm, _| assert!(rdm.default_order(), "a third click returns to newest first"));
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
fn reset_under_appearance_puts_every_column_width_back(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| {
		rdm.widths = [200.0, 90.0, 60.0, 70.0, 80.0];
		rdm.open_settings(cx);
	});
	click(&mut cx, "section:Appearance");
	click(&mut cx, "button:Reset");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.widths, Column::DEFAULT_WIDTHS));
}

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
	click(&mut cx, "button:Folder files");
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
	let downloads = rdm.read_with(&cx, |rdm, _| rdm.shown().len());
	click(&mut cx, "button:Folder files");
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

/// The stop is a ceiling, not a place the boundary bounces off: it held one drag's starting width
/// against the widths as they stood, so once the column passed it the two disagreed, the ceiling
/// fell as the column rose, and the boundary alternated across the pointer -- stopping short,
/// twitching there, and letting a fresh press take half of what was left.
#[gpui::test]
fn a_drag_past_the_stop_holds_there_and_a_second_takes_no_more(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Point, Pixels, point, px};

	/// One press on the Size handle and twenty points of leftward travel per move, which widens
	/// the column, with the width after every move so the walk itself can be read.
	fn drag_left(rdm: &Entity<Rdm>, cx: &mut VisualTestContext, from: Point<Pixels>) -> Vec<f32> {
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: from,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse: false,
		});
		let widths = (1u8..=25)
			.map(|step| {
				cx.simulate_event(MouseMoveEvent {
					position: point(from.x - px(20.0 * f32::from(step)), from.y),
					pressed_button: Some(MouseButton::Left),
					modifiers: Modifiers::default(),
				});
				rdm.read_with(cx, |rdm, _| rdm.width(Column::Size))
			})
			.collect();
		cx.simulate_event(MouseUpEvent {
			button: MouseButton::Left,
			position: from,
			modifiers: Modifiers::default(),
			click_count: 1,
		});
		widths
	}

	let (rdm, mut cx) = open(cx);
	let start = cx.debug_bounds("resize:Size").unwrap().center();
	let first = drag_left(&rdm, &mut cx, start);
	assert!(
		first.windows(2).all(|w| w[1] >= w[0]),
		"the pointer only went left, so the column only widened: {first:?}"
	);
	let stop = *first.last().unwrap();
	let second = drag_left(&rdm, &mut cx, start);
	assert_eq!(
		second.last().copied(),
		Some(stop),
		"the same travel from the same place reaches the same stop, however often it is asked"
	);
}

#[gpui::test]
fn the_custom_form_adds_a_rule_and_advanced_exposes_the_pattern(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	assert!(cx.debug_bounds("preset:Videos").is_some(), "the sheet opens on the presets");
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
	click(&mut cx, "preset:eBooks");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.categories.len(), before - 1));
	click(&mut cx, "preset:eBooks");
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
	assert_eq!(&before[..2], ["Videos", "Audio"]);
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
	drag(&mut cx, "filter:Videos", "filter:Code");
	rdm.read_with(&cx, |rdm, _| {
		let after = names(rdm);
		assert_eq!(after[0], "Audio", "{after:?}");
		assert_eq!(after.iter().position(|n| n == "Videos"), before.iter().position(|n| n == "Code"));
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
	click(&mut cx, "preset:eBooks");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.categories.iter().any(|c| c.name == "eBooks")));
	click(&mut cx, "button:Edit");
	click(&mut cx, "preset:eBooks");
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			matches!(rdm.category_sheet, Some(CategorySheet::Presets { editing: true })),
			"a preset that is off has no list to open"
		)
	});
	click(&mut cx, "preset:Videos");
	let add = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
		form.add.clone()
	});
	cx.update(|window, cx| window.focus(&add.read(cx).focus(), cx));
	click(&mut cx, "extension:mkv");
	assert!(
		cx.update(|window, cx| add.read(cx).focus().is_focused(window)),
		"a chip is a switch: pressing it leaves the field's focus alone"
	);
	cx.update(|window, cx| {
		add.update(cx, |input, cx| input.replace_text_in_range(None, "xyz, zyx", window, cx))
	});
	// The tests bind no keys; main does. The action is what Enter is bound to.
	cx.dispatch_action(crate::ui::text_input::Confirm);
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let list = video.extensions();
		assert!(!list.contains(&"mkv".to_owned()));
		assert_eq!(&list[list.len() - 2..], ["xyz", "zyx"]);
	});
	assert_eq!(add.read_with(&cx, |input, _| input.content.to_string()), "", "the field clears");
	click(&mut cx, "extension:xyz");
	click(&mut cx, "extension:mkv");
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let list = video.extensions();
		assert!(list.contains(&"mkv".to_owned()) && !list.contains(&"xyz".to_owned()));
		assert_eq!(list.last().map(String::as_str), Some("zyx"));
	});
	click(&mut cx, "icon:film");
	click(&mut cx, "swatch:#bf616a");
	click(&mut cx, "button:Reset");
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let shipped = Category::preset("Videos").unwrap();
		assert_eq!(video.extensions(), shipped.extensions());
		assert_eq!((video.icon, video.color), (shipped.icon, shipped.color), "icon and color too");
		assert!(!video.differs_from_preset());
	});
	assert!(cx.debug_bounds("button:Reset").is_none(), "nothing left to reset");
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
		custom
			.update(cx, |input, cx| input.replace_text_in_range(None, "rgb(170, 187, 204)", window, cx));
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
	rdm.read_with(&cx, |rdm, _| assert_eq!(audio(rdm).color, 0xaabbcc, "and it can be chosen again"));
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
fn the_colorful_categories_switch_flips_the_preference(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_categories, "on to start with"));
	click(&mut cx, "button:Settings");
	click(&mut cx, "section:Appearance");
	click(&mut cx, "switch:Always use colorful categories");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.colorful_categories));
	click(&mut cx, "switch:Always use colorful categories");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_categories));
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
	rdm.read_with(&cx, |rdm, _| assert!(rdm.settings_open()));
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.settings_open()));
	click(&mut cx, "button:Add Task");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none(), "an empty Add Task goes too"));
}

#[gpui::test]
fn a_press_on_a_sheet_that_lands_on_no_field_drops_the_fields_focus(cx: &mut TestAppContext) {
	use gpui::{point, px};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let name = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		form.name.clone()
	});
	let focused =
		|cx: &mut VisualTestContext| cx.update(|window, cx| name.read(cx).focus().is_focused(window));
	assert!(focused(&mut cx), "a form opens with its first field ready to type into");
	// The card's padding: inside the sheet, on nothing.
	let card = cx.debug_bounds("category-sheet").expect("the form's card");
	cx.simulate_click(point(card.origin.x + px(6.0), card.origin.y + px(6.0)), Modifiers::default());
	assert!(!focused(&mut cx), "a press on the card takes the keyboard away from the field");
	cx.update(|window, cx| window.focus(&name.read(cx).focus(), cx));
	click(&mut cx, "toggle:Match case");
	assert!(focused(&mut cx), "a switch leaves it: the typing carries on after the press");
	click(&mut cx, "icon:film");
	assert!(!focused(&mut cx), "a press on a picker takes it, like any other control");
	rdm.read_with(&cx, |rdm, _| {
		assert!(matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))), "the form stays");
	});
	// The switch made the form dirty, so only its cross leaves it, back to the presets, which
	// hold nothing and close from Escape.
	click(&mut cx, "button:Close");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none()));
	click(&mut cx, "button:Settings");
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	assert!(
		!cx.update(|window, cx| search.read(cx).focus().is_focused(window)),
		"Settings is a place, not a form: nothing in it takes the keyboard on opening"
	);
}

#[gpui::test]
fn settings_has_sections_and_a_search_that_cuts_across_them(cx: &mut TestAppContext) {
	use crate::ui::settings_sheet::Section;
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Settings");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.settings.as_ref().map(|s| s.section), Some(Section::General))
	});
	assert!(cx.debug_bounds("setting:Download folder").is_some());
	assert!(
		cx.debug_bounds("setting:Always use colorful categories").is_none(),
		"another section's rows are not shown"
	);
	click(&mut cx, "section:Appearance");
	assert!(cx.debug_bounds("setting:Always use colorful categories").is_some());
	assert!(cx.debug_bounds("setting:Download folder").is_none());
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	cx.update(|_, cx| search.update(cx, |i, cx| i.set_content("speed", cx)));
	cx.run_until_parked();
	assert!(
		cx.debug_bounds("setting:Speed limit").is_some(),
		"a search shows every section's matches"
	);
	assert!(cx.debug_bounds("setting:Always use colorful categories").is_none());
	cx.update(|_, cx| search.update(cx, |i, cx| i.set_content("nothing like this", cx)));
	cx.run_until_parked();
	assert!(cx.debug_bounds("setting:Speed limit").is_none(), "no match, no rows");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.settings_open(), "escape in the field closes"));
}

#[gpui::test]
fn opening_a_download_adds_one_window_and_removing_it_closes_it(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 2);
	rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 2, "a second request raises the window, it does not open another");
	rdm.update(&mut cx, |rdm, cx| rdm.remove(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 1);
}

#[gpui::test]
fn a_newer_build_puts_a_card_in_the_corner_until_waved_away(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let manifest = crate::update::Manifest::parse(
		r#"{ "channel": "nightly", "version": "2026.9.5", "build": 99, "sha": "abc", "assets": [
			{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 1, "sha256": "aa" },
			{ "target": "windows-x64", "kind": "zip", "file": "rdm-nightly-windows-x64.zip", "size": 1, "sha256": "bb" },
			{ "target": "linux-x64", "kind": "AppImage", "file": "rdm-nightly-linux-x64.AppImage", "size": 1, "sha256": "cc" },
			{ "target": "linux-arm64", "kind": "AppImage", "file": "rdm-nightly-linux-arm64.AppImage", "size": 1, "sha256": "dd" }
		] }"#,
	)
	.unwrap();
	assert!(cx.debug_bounds("toast:update").is_none(), "nothing known, nothing shown");
	// A hand build is shown a newer build only when it asked. The test binary made in CI
	// carries the run's number, so the test says which build it is.
	rdm.update(&mut cx, |rdm, _| rdm.updates.this = None);
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest.clone(), false, cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_none(), "a hand build was not asked");
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest, true, cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_some(), "build 99 is newer than no number");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.updates.available.as_ref().map(|a| a.build), Some(99));
		assert!(
			rdm.update_status().starts_with("2026.9.5 (99) is the latest"),
			"{}",
			rdm.update_status()
		);
	});
	assert!(cx.debug_bounds("button:Install").is_some(), "the card offers the install");
	click(&mut cx, "button:Later");
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_none(), "waved away for this build");
	// The settings row names the check and its outcome.
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("setting:Update channel").is_some());
	assert!(cx.debug_bounds("setting:Check for updates").is_some());
	assert!(cx.debug_bounds("button:Check now").is_some());
}

#[gpui::test]
fn the_update_settings_are_switches_and_a_choice_that_follows_the_switch(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("setting:Check for updates").is_some());
	assert!(cx.debug_bounds("setting:Automatic updates").is_some());
	assert!(cx.debug_bounds("choice:Download and install").is_some(), "the policy, while automatic");
	click(&mut cx, "choice:Download only");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.update_policy, crate::update::Policy::Download);
	});
	click(&mut cx, "switch:Automatic updates");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.auto_update));
	assert!(cx.debug_bounds("choice:Download only").is_none(), "no policy without the switch");
	click(&mut cx, "switch:Check for updates");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.check_updates));
}

#[gpui::test]
fn a_file_is_shown_before_it_is_added_and_the_connections_are_chosen_there(
	cx: &mut TestAppContext,
) {
	use crate::engine::testing::{Options, TestServer, body};
	let server = TestServer::start(body(300_000), Options::default());
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Add Task");
	let input = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().input.clone());
	let address = server.url("/tool.bin").to_string();
	cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&address, cx)));
	click(&mut cx, "button:Add");
	let mut seen = false;
	for _ in 0..200 {
		std::thread::sleep(Duration::from_millis(10));
		rdm.update(&mut cx, |rdm, cx| rdm.poll_add(cx));
		cx.run_until_parked();
		if rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().is_some_and(|s| s.found.is_some())) {
			seen = true;
			break;
		}
	}
	assert!(seen, "the address was looked at and found to be a file");
	assert!(cx.debug_bounds("add-found").is_some(), "the file is shown before it is added");
	assert!(cx.debug_bounds("connections:Auto").is_some(), "ranges, so connections are a choice");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.downloads.iter().all(|d| d.url != address), "not added yet")
	});
	click(&mut cx, "connections:Fixed");
	let count = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().count.clone());
	cx.update(|_, cx| count.update(cx, |i, cx| i.set_content("999", cx)));
	click(&mut cx, "button:Add");
	assert!(cx.debug_bounds("add-error").is_some(), "999 is more than the most");
	cx.update(|_, cx| count.update(cx, |i, cx| i.set_content("8", cx)));
	// More: the rest of what can be asked, each checked before anything is added.
	click(&mut cx, "button:More");
	assert!(cx.debug_bounds("add-more").is_some(), "the fields are shown");
	let (name, mirrors, checksum, range, limit) = rdm.read_with(&cx, |rdm, _| {
		let s = rdm.adding.as_ref().unwrap();
		(s.name.clone(), s.mirrors.clone(), s.checksum.clone(), s.range.clone(), s.limit.clone())
	});
	cx.update(|_, cx| checksum.update(cx, |i, cx| i.set_content("not-a-hash", cx)));
	click(&mut cx, "button:Add");
	assert!(cx.debug_bounds("add-error").is_some(), "a checksum that is none is refused");
	let mirror = server.url("/mirror.bin").to_string();
	cx.update(|_, cx| {
		name.update(cx, |i, cx| i.set_content("renamed.bin", cx));
		mirrors.update(cx, |i, cx| i.set_content(&mirror, cx));
		checksum.update(cx, |i, cx| i.set_content("d41d8cd98f00b204e9800998ecf8427e", cx));
		range.update(cx, |i, cx| i.set_content("0-1000", cx));
		limit.update(cx, |i, cx| i.set_content("500", cx));
	});
	click(&mut cx, "button:Add");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_none(), "added and closed");
		let row = rdm.downloads.iter().find(|d| d.url == address).expect("the download");
		assert_eq!(row.connections, Some(8));
		assert_eq!(row.name, "renamed.bin");
		assert_eq!(row.mirrors, vec![mirror.clone()]);
		assert_eq!(row.checksum.as_deref(), Some("d41d8cd98f00b204e9800998ecf8427e"));
		assert_eq!(row.range.as_deref(), Some("0-1000"));
		assert_eq!(row.speed_limit, Some(500 * 1024));
	});
}

#[gpui::test]
fn the_transfer_fields_apply_on_enter_and_say_no_to_nonsense(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	rdm.update(&mut cx, |rdm, cx| {
		rdm.apply_setting("Speed limit", "2m", cx);
		rdm.apply_setting("Connections", "32", cx);
	});
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.speed_limit, Some(2 * 1024 * 1024));
		assert_eq!(rdm.preferences.connections, Some(32));
	});
	rdm.update(&mut cx, |rdm, cx| rdm.apply_setting("Connections", "lots", cx));
	cx.run_until_parked();
	click(&mut cx, "section:Transfers");
	assert!(cx.debug_bounds("settings-complaint").is_some(), "nonsense is said no to under its row");
	rdm.update(&mut cx, |rdm, cx| {
		rdm.apply_setting("Connections", "auto", cx);
		rdm.apply_setting("Speed limit", "", cx);
	});
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!((rdm.preferences.connections, rdm.preferences.speed_limit), (None, None));
	});
}


/// A press at a column's handle, the pointer walked to each offset from it in turn, then the
/// release. What every step left the row at, so a drag reads as the rows it went through.
fn drag(
	rdm: &Entity<Rdm>,
	cx: &mut VisualTestContext,
	handle: &'static str,
	steps: &[f32],
) -> Vec<[f32; 5]> {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};
	let from = cx.debug_bounds(handle).expect("a handle to take hold of").center();
	cx.simulate_event(MouseDownEvent {
		button: MouseButton::Left,
		position: from,
		modifiers: Modifiers::default(),
		click_count: 1,
		first_mouse: false,
	});
	let rows = steps
		.iter()
		.map(|dx| {
			cx.simulate_event(MouseMoveEvent {
				position: point(from.x + px(*dx), from.y),
				pressed_button: Some(MouseButton::Left),
				modifiers: Modifiers::default(),
			});
			rdm.read_with(cx, |rdm, _| rdm.drawn())
		})
		.collect();
	cx.simulate_event(MouseUpEvent {
		button: MouseButton::Left,
		position: from,
		modifiers: Modifiers::default(),
		click_count: 1,
	});
	rows
}

/// The size the application opens at with nothing remembered. The default widths leave the name
/// column under its floor here, which is what made the old stop unreachable: it was derived from
/// that floor, so every column's ceiling sat below the width it already had, and the first move
/// of any press snapped it there -- twenty-four points for a one-point nudge, in both directions.
#[gpui::test]
fn a_nudge_at_a_handle_moves_the_boundary_by_the_nudge(cx: &mut TestAppContext) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	let before = rdm.read_with(&cx, |rdm, _| rdm.drawn());
	let narrower = drag(&rdm, &mut cx, "resize:Size", &[1.0]);
	assert_eq!(narrower[0][0], before[0] - 1.0, "a point of travel is a point of width");
	let wider = drag(&rdm, &mut cx, "resize:Size", &[-1.0]);
	assert_eq!(wider[0][0], before[0], "and the same the other way");
}

/// Widening takes from the left of the handle, and only from what is above a floor: the name
/// column first, since it holds the slack, then each fixed column between it and the handle,
/// nearest first.
#[gpui::test]
fn widening_squeezes_leftwards_until_everything_left_of_the_handle_is_on_its_floor(
	cx: &mut TestAppContext,
) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	let before = rdm.read_with(&cx, |rdm, _| rdm.drawn());
	let rows = drag(&rdm, &mut cx, "resize:Added", &[-100.0, -800.0, 0.0]);
	assert_eq!(rows[0][..3], before[..3], "the columns further left are not asked yet");
	assert!(rows[0][3] < before[3], "Status is nearest the handle, so it gives what the name could not");
	assert_eq!(rows[1][..4], Column::MINS[..4], "far enough, and everything left of it is on its floor");
	let name = rdm.read_with(&cx, |rdm, _| rdm.name_width(&rows[1]));
	assert_eq!(name, crate::ui::list::NAME_MIN, "the name column on its own floor with them");
	assert_eq!(rows[2], before, "and the way back gives back exactly what the way out took");
}

/// The window will not go narrower than every floor together, and at that width the table is
/// exactly its floors: what the window took from the widths on the way down, it gives back.
#[gpui::test]
fn at_the_windows_least_width_the_columns_are_their_floors_and_the_row_still_fits(
	cx: &mut TestAppContext,
) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(crate::ui::MIN_WIDTH), px(crate::ui::MIN_HEIGHT)));
	cx.run_until_parked();
	let (drawn, name) = rdm.read_with(&cx, |rdm, _| (rdm.drawn(), rdm.name_width(&rdm.drawn())));
	assert_eq!(drawn, Column::MINS, "every column on its floor");
	assert_eq!(name, crate::ui::list::NAME_MIN, "the name column included");
	let added = cx.debug_bounds("sort:Added").expect("the last title is drawn");
	assert!(
		added.right() <= px(crate::ui::MIN_WIDTH),
		"the row is inside the window, not overflowing it: {:?}",
		added.right()
	);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.drawn(), Column::DEFAULT_WIDTHS, "widened again, and back to what was asked for");
	});
}
