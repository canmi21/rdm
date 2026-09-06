//! The window's frame where the system draws none. On macOS the titlebar is transparent and
//! the traffic lights sit in the toolbar's left, drawn and run by the system. On Windows the
//! transparent titlebar leaves the whole strip to the application; on Linux the window may
//! carry client-side decorations, in which case there is no bar above and no buttons at all.
//! In both, the toolbar draws minimize, maximize and close at its right with the
//! application's own glyphs and answers their presses itself, its empty middle drags the
//! window, and the window's edges resize it. See spec/ui.md.

use gpui::{
	Decorations, IntoElement, MouseButton, MouseDownEvent, Pixels, ResizeEdge, SharedString, Tiling,
	Window, div, prelude::*, px,
};

use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;
use crate::ui::toolbar;

/// How wide the strip's edge is that resizes rather than drags, in points.
const EDGE: f32 = 6.0;

/// Whether the toolbar is the window's frame here: always on Windows, whose transparent
/// titlebar is otherwise empty; on Linux when the window was given client-side decorations;
/// never on macOS, whose system draws the lights.
pub fn draws_frame(window: &Window) -> bool {
	match std::env::consts::OS {
		"windows" => true,
		"linux" => matches!(window.window_decorations(), Decorations::Client { .. }),
		_ => false,
	}
}

/// Which edge of the window the point is on, if any, for a window that is not tiled there.
pub fn edge_at(
	position: gpui::Point<Pixels>,
	size: gpui::Size<Pixels>,
	tiling: Tiling,
) -> Option<ResizeEdge> {
	let edge = px(EDGE);
	let top = !tiling.top && position.y < edge;
	let bottom = !tiling.bottom && position.y > size.height - edge;
	let left = !tiling.left && position.x < edge;
	let right = !tiling.right && position.x > size.width - edge;
	Some(match (top, bottom, left, right) {
		(true, _, true, _) => ResizeEdge::TopLeft,
		(true, _, _, true) => ResizeEdge::TopRight,
		(_, true, true, _) => ResizeEdge::BottomLeft,
		(_, true, _, true) => ResizeEdge::BottomRight,
		(true, _, _, _) => ResizeEdge::Top,
		(_, true, _, _) => ResizeEdge::Bottom,
		(_, _, true, _) => ResizeEdge::Left,
		(_, _, _, true) => ResizeEdge::Right,
		_ => return None,
	})
}

/// A press on the window's edge starts a resize, on Linux with client-side decorations, where
/// nothing else would. Called first from the root, before anything under the pointer.
pub fn on_root_mouse_down(event: &MouseDownEvent, window: &mut Window) {
	if event.button != MouseButton::Left || !cfg!(target_os = "linux") {
		return;
	}
	let Decorations::Client { tiling } = window.window_decorations() else { return };
	if window.is_maximized() || window.is_fullscreen() {
		return;
	}
	if let Some(edge) = edge_at(event.position, window.viewport_size(), tiling) {
		window.start_window_resize(edge);
	}
}

/// The toolbar's empty middle: on Windows the system's own caption area, which drags and
/// double-clicks to maximize by itself; on Linux a press starts a move through the compositor,
/// a double press zooms, and the right button opens the window's own menu.
pub fn drag_area() -> impl IntoElement {
	div()
		.flex_1()
		.h_full()
		.when(cfg!(target_os = "windows"), |s| s.window_control_area(gpui::WindowControlArea::Drag))
		.when(cfg!(target_os = "linux"), |s| {
			s.on_mouse_down(MouseButton::Left, |event, window, _| {
				if event.click_count == 2 {
					window.zoom_window();
				} else {
					window.start_window_move();
				}
			})
			.on_mouse_down(MouseButton::Right, |event, window, _| {
				window.show_window_menu(event.position);
			})
		})
}

/// Minimize, maximize or restore, close: the system's arrangement and width, the
/// application's glyphs on a ten point grid, close hovering red. Each answers its own press.
pub fn controls(p: Palette, window: &Window) -> impl IntoElement {
	let maximized = window.is_maximized();
	let control = move |label: &'static str, glyph: Icon, close: bool, press: fn(&mut Window)| {
		div()
			.id(SharedString::from(format!("frame-{label}")))
			.role(gpui::Role::Button)
			.aria_label(label)
			.debug_selector(move || format!("frame:{label}"))
			.flex()
			.items_center()
			.justify_center()
			.w(px(46.0))
			.h(px(toolbar::HEIGHT))
			.cursor_default()
			.hover(move |s| if close { s.bg(p.failure).text_color(p.text) } else { s.bg(p.hover) })
			.on_click(move |_, window, _| press(window))
			.child(icon(glyph, p.muted).size(px(10.0)))
	};
	div()
		.flex()
		.items_center()
		.h_full()
		.child(control("Minimize", Icon::Minimize, false, |window| window.minimize_window()))
		.child(control(
			if maximized { "Restore" } else { "Maximize" },
			if maximized { Icon::Restore } else { Icon::Maximize },
			false,
			|window| window.zoom_window(),
		))
		.child(control("Close", Icon::Close, true, |window| window.remove_window()))
}

#[cfg(test)]
mod tests {
	use super::*;
	use gpui::{point, size};

	#[test]
	fn the_edge_under_the_pointer_and_none_where_the_window_is_tiled() {
		let extent = size(px(800.0), px(600.0));
		let free = Tiling::default();
		assert_eq!(edge_at(point(px(2.0), px(300.0)), extent, free), Some(ResizeEdge::Left));
		assert_eq!(edge_at(point(px(797.0), px(3.0)), extent, free), Some(ResizeEdge::TopRight));
		assert_eq!(edge_at(point(px(400.0), px(598.0)), extent, free), Some(ResizeEdge::Bottom));
		assert_eq!(edge_at(point(px(400.0), px(300.0)), extent, free), None);
		let tiled = Tiling { left: true, top: true, ..Tiling::default() };
		assert_eq!(edge_at(point(px(2.0), px(300.0)), extent, tiled), None);
		assert_eq!(edge_at(point(px(2.0), px(598.0)), extent, tiled), Some(ResizeEdge::Bottom));
	}
}
