mod app;
mod assets;
mod download;
mod ui;

use gpui::{
	App, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point,
	prelude::*, px, size,
};
use gpui_platform::application;

use crate::app::Rdm;
use crate::assets::Assets;

/// Measured from a capture of the window; see spec/ui.md.
const TRAFFIC_LIGHT: f32 = 14.0;

fn main() {
	// gpui reports what it cannot draw through `log` and nowhere else. See spec/framework.md.
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
	application().with_assets(Assets).run(|cx: &mut App| {
		let bounds = Bounds::centered(None, size(px(960.0), px(600.0)), cx);
		let main = cx
			.open_window(
				WindowOptions {
					window_bounds: Some(WindowBounds::Windowed(bounds)),
					// The desktop shows through the blur; the palette carries the alpha. See spec/ui.md.
					window_background: WindowBackgroundAppearance::Blurred,
					titlebar: Some(TitlebarOptions {
						title: Some("rdm".into()),
						appears_transparent: true,
						// y is padding on both sides of the buttons, so this centres them. See spec/ui.md.
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
		// The main window is the application: closing it quits, however many download or settings
		// windows are still open. Closing one of those closes only itself.
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
