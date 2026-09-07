// A Windows executable is a console program unless told otherwise, and opens a console window
// beside its own; a release build says it is a windowed one. A debug build keeps the console,
// which is where its log goes. See spec/ui.md.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod agent;
mod app;
mod assets;
mod category;
mod config;
// A Unix socket, so a debug build on Windows has no control socket; the tests stand in for it there.
#[cfg(all(debug_assertions, unix))]
mod ctl;
mod dns;
mod download;
mod engine;
mod i18n;
mod identity;
mod index;
mod notify;
mod proxy;
mod quarantine;
mod reveal;
mod screens;
mod startup;
mod state;
mod thumbnail;
mod tls;
mod store;
#[cfg(test)]
mod testing;
mod tray;
mod ui;
mod update;
mod watch;

use gpui::{
	App, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point,
	prelude::*, px, size,
};
use gpui_platform::application;

use crate::app::Rdm;
use crate::assets::Assets;
use crate::state::Paths;

/// Measured from a capture of the window; see spec/ui.md.
const TRAFFIC_LIGHT: f32 = 14.0;

fn main() {
	// gpui reports what it cannot draw through `log` and nowhere else. See spec/framework.md.
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
	application().with_assets(Assets).run(|cx: &mut App| {
		cx.bind_keys(ui::text_input::key_bindings());
		let paths = Paths::resolve();
		let (engine, events) =
			engine::Engine::new(engine::EngineSettings::default()).expect("start the engine's runtime");
		let saved = paths.as_ref().map(|p| state::load(&p.state)).unwrap_or_default();
		let config =
			paths.as_ref().map(|p| config::load_or_seed(&p.config)).unwrap_or_else(config::Config::seed);
		// Every display there is now, by the name the system keeps for it, so the window can be put
		// back on the one it was left on. See src/screens.rs.
		let screens = screens::all(cx);
		// The size outlives the place: a window whose display is gone comes back centred on the
		// main one, but at the size the user made it, not at the size a first launch opens with.
		let extent = saved
			.window
			.map_or_else(|| size(px(960.0), px(600.0)), |f| size(px(f.width), px(f.height)));
		// The frame is a place on the display named beside it, so the display is handed to GPUI
		// with it; without one GPUI opens on the main display, which is where a centred window
		// belongs anyway. See src/screens.rs.
		let (display_id, bounds) = match saved.frame_on(&screens) {
			Some(f) => (
				saved.display.as_deref().and_then(|uuid| screens::id_of(cx, uuid)),
				Bounds::new(point(px(f.x), px(f.y)), size(px(f.width), px(f.height))),
			),
			None => (None, Bounds::centered(None, extent, cx)),
		};
		let window_bounds = if saved.maximized {
			WindowBounds::Maximized(bounds)
		} else {
			WindowBounds::Windowed(bounds)
		};
		// The language before the first frame: a window that came up in English and turned
		// Chinese a moment later would be a window that flickered.
		i18n::use_language(config.settings.language);
		let main = cx
			.open_window(
				WindowOptions {
					window_bounds: Some(window_bounds),
					display_id,
					// What a Linux desktop matches the window to its .desktop entry by; the same three
					// words as the bundle identifier. See spec/packaging.md.
					app_id: Some(identity::id()),
					// The desktop shows through the blur; the palette carries the alpha. See spec/ui.md.
					window_background: WindowBackgroundAppearance::Blurred,
					// Every column's floor and what sits around them: past this the table would have
					// less room than its own floors need. See spec/ui.md.
					window_min_size: Some(size(px(ui::MIN_WIDTH), px(ui::MIN_HEIGHT))),
					titlebar: Some(TitlebarOptions {
						title: Some(identity::DISPLAY_NAME.into()),
						appears_transparent: true,
						// y is padding on both sides of the buttons, so this centres them. See spec/ui.md.
						traffic_light_position: Some(point(
							px(12.0),
							px((ui::toolbar::HEIGHT - TRAFFIC_LIGHT) / 2.0),
						)),
					}),
					..Default::default()
				},
				|window, cx| cx.new(|cx| Rdm::new(saved, config, paths, engine, events, window, cx)),
			)
			.expect("open the main window");
		// The main window is the application: closing it quits, however many download or settings
		// windows are still open. Closing one of those closes only itself.
		#[cfg(all(debug_assertions, unix))]
		if let Ok(rdm) = main.update(cx, |_, _, cx| cx.entity()) {
			ctl::serve(rdm, cx);
		}
		tray::install(cx);
		let main_id = main.window_id();
		cx.on_window_closed(move |cx, id| {
			if id == main_id {
				cx.quit();
			}
		})
		.detach();
		cx.activate(true);
	});
}
