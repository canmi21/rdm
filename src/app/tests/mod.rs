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
	// The tests read English, whatever the machine this runs on is set to: a test that asserted
	// on what the window says would otherwise pass or fail by where it was run.
	crate::i18n::use_language(crate::i18n::Language::En);
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

mod table;
mod folder;
mod categories;
mod settings;
mod windows;
mod tasks;
