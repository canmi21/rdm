//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, status
//! bar -- and the windows it opens beside itself.

pub mod download_window;
pub mod icon;
mod list;
pub mod settings_window;
mod sidebar;
mod status_bar;
pub mod theme;
pub mod toolbar;

use gpui::{ClickEvent, Div, ElementId, Role, Stateful, div, prelude::*};

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
		.role(Role::Button)
		.aria_label(label)
		.debug_selector(|| format!("button:{label}"))
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

/// An icon alone, for the toolbar's corners; the label is what assistive technology reads.
pub fn icon_button(
	p: Palette,
	id: impl Into<ElementId>,
	glyph: Icon,
	label: &'static str,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.debug_selector(|| format!("button:{label}"))
		.flex()
		.items_center()
		.justify_center()
		.size_6()
		.rounded_sm()
		.cursor_pointer()
		.hover(move |s| s.bg(p.hover))
		.on_click(on_click)
		.child(icon(glyph, p.muted).size_3p5())
}

/// A small toggle, lit when `on`, for a filter or a mode.
pub fn chip(
	p: Palette,
	id: impl Into<ElementId>,
	label: impl Into<gpui::SharedString>,
	on: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let label = label.into();
	div()
		.id(id)
		.role(Role::CheckBox)
		.aria_label(label.clone())
		.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
		.px_1p5()
		.py_px()
		.rounded_sm()
		.text_xs()
		.cursor_pointer()
		.text_color(if on { p.text } else { p.muted })
		.when(on, |s| s.bg(p.selection))
		.when(!on, move |s| s.hover(move |s| s.bg(p.hover)))
		.on_click(on_click)
		.child(label)
}
