//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, detail.

mod detail;
pub mod icon;
mod list;
mod sidebar;
pub mod theme;
pub mod toolbar;

use gpui::{ClickEvent, Div, ElementId, Stateful, div, prelude::*};

use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;

/// An icon with its label. Disabled ones stay in the layout but neither react nor invite a click.
pub fn button(
	p: Palette,
	id: impl Into<ElementId>,
	glyph: Icon,
	label: &'static str,
	enabled: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let color = if enabled { p.text } else { p.muted };
	let base = div()
		.id(id)
		.flex()
		.items_center()
		.gap_1()
		.px_2()
		.py_0p5()
		.rounded_sm()
		.text_color(color)
		.child(icon(glyph, color).size_3p5())
		.child(label);
	if enabled {
		base.cursor_pointer().hover(move |s| s.bg(p.hover)).on_click(on_click)
	} else {
		base
	}
}

/// A small toggle, lit when `on`, for a filter or a mode.
pub fn chip(
	p: Palette,
	id: impl Into<ElementId>,
	label: impl Into<gpui::SharedString>,
	on: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	div()
		.id(id)
		.px_1p5()
		.py_px()
		.rounded_sm()
		.text_xs()
		.cursor_pointer()
		.text_color(if on { p.text } else { p.muted })
		.when(on, |s| s.bg(p.selection))
		.when(!on, move |s| s.hover(move |s| s.bg(p.hover)))
		.on_click(on_click)
		.child(label.into())
}
