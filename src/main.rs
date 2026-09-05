mod app;
mod download;
mod ui;

use gpui::{
	App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, prelude::*, px, size,
};
use gpui_platform::application;

use crate::app::Rdm;

fn main() {
	// gpui reports a font it cannot load through `log` and nowhere else, so without a logger a
	// window with no text is silent about why.
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
	application().run(|cx: &mut App| {
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
					traffic_light_position: Some(point(px(12.0), px(13.0))),
				}),
				..Default::default()
			},
			|_, cx| cx.new(Rdm::new),
		)
		.expect("open the main window");
		cx.activate(true);
	});
}
