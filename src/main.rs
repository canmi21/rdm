use gpui::{
	App, Bounds, Context, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use gpui_platform::application;

struct Rdm;

impl Render for Rdm {
	fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.flex()
			.size_full()
			.justify_center()
			.items_center()
			.bg(rgb(0x1e1e1e))
			.text_color(rgb(0xffffff))
			.child("rdm")
	}
}

fn main() {
	application().run(|cx: &mut App| {
		let bounds = Bounds::centered(None, size(px(800.0), px(500.0)), cx);
		cx.open_window(
			WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
			|_, cx| cx.new(|_| Rdm),
		)
		.expect("open the main window");
		cx.activate(true);
	});
}
