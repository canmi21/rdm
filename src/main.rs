mod app;
mod assets;
mod download;
mod ui;

use gpui::{
	App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px, size,
};
use gpui_platform::application;

use crate::app::Rdm;
use crate::assets::Assets;

/// The diameter macOS draws the traffic lights at, measured from a capture of the window.
const TRAFFIC_LIGHT: f32 = 14.0;

fn main() {
	// gpui reports a font it cannot load through `log` and nowhere else, so without a logger a
	// window with no text is silent about why.
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
	application().with_assets(Assets).run(|cx: &mut App| {
		// One window is the application: closing it quits rather than leaving a bare menu bar.
		cx.on_window_closed(|cx, _| {
			if cx.windows().is_empty() {
				cx.quit();
			}
		})
		.detach();
		let bounds = Bounds::centered(None, size(px(960.0), px(600.0)), cx);
		cx.open_window(
			WindowOptions {
				window_bounds: Some(WindowBounds::Windowed(bounds)),
				titlebar: Some(TitlebarOptions {
					title: Some("rdm".into()),
					appears_transparent: true,
					// gpui_macos makes the button strip `button height + 2 * y` tall and hangs it from the top, so
					// y is the padding on both sides: the strip is centred when it is as tall as the toolbar.
					traffic_light_position: Some(point(
						px(12.0),
						px((ui::toolbar::HEIGHT - TRAFFIC_LIGHT) / 2.0),
					)),
				}),
				..Default::default()
			},
			|_, cx| cx.new(Rdm::new),
		)
		.expect("open the main window");
		cx.activate(true);
	});
}
