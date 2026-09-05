//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, detail.

mod detail;
mod list;
mod sidebar;
pub mod theme;
mod toolbar;

use gpui::{ClickEvent, Div, ElementId, Stateful, div, prelude::*};

/// A flat text button. Disabled ones stay in the layout but neither react nor invite a click.
pub fn button(
	id: impl Into<ElementId>,
	label: &'static str,
	enabled: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let base = div().id(id).px_3().py_1().rounded_md().text_sm().child(label);
	if enabled {
		base
			.cursor_pointer()
			.text_color(theme::text())
			.hover(|s| s.bg(theme::hover()))
			.on_click(on_click)
	} else {
		base.text_color(theme::muted())
	}
}
