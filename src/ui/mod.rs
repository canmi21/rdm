//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, detail.

mod detail;
pub mod icon;
mod list;
mod sidebar;
pub mod theme;
pub mod toolbar;

use gpui::{ClickEvent, Div, ElementId, Stateful, div, prelude::*};

use crate::ui::icon::{Icon, icon};

/// An icon with its label. Disabled ones stay in the layout but neither react nor invite a click.
pub fn button(
	id: impl Into<ElementId>,
	glyph: Icon,
	label: &'static str,
	enabled: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let color = if enabled { theme::text() } else { theme::muted() };
	let base = div()
		.id(id)
		.flex()
		.items_center()
		.gap_1p5()
		.px_2p5()
		.py_1()
		.rounded_md()
		.text_sm()
		.text_color(color)
		.child(icon(glyph, color))
		.child(label);
	if enabled {
		base.cursor_pointer().hover(|s| s.bg(theme::hover())).on_click(on_click)
	} else {
		base
	}
}
