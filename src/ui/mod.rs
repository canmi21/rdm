//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, status
//! bar -- and the windows it opens beside itself.

pub mod add_dialog;
mod category_sheet;
pub mod download_window;
pub mod first_mouse;
pub mod guide;
pub mod icon;
pub mod list;
mod settings_sheet;
pub mod sidebar;
pub mod status_bar;
pub mod text_input;
pub mod theme;
pub mod toolbar;
pub mod tooltip;

use gpui::{ClickEvent, Div, ElementId, Role, SharedString, Stateful, div, prelude::*};

use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;
use crate::ui::tooltip::tooltip;

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

/// An icon alone that does one thing when pressed. It has no pressed state to show, so hovering
/// brightens it and nothing else; a background is for controls that stay selected. The label
/// the icon cannot show appears as a tooltip once the pointer has rested on it.
pub fn icon_button(
	p: Palette,
	id: impl Into<ElementId>,
	glyph: Icon,
	label: &'static str,
	enabled: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let base = div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.debug_selector(|| format!("button:{label}"))
		.flex()
		.items_center()
		.justify_center()
		.size_5()
		// The svg carries its own color, so hovering the button cannot recolor it by inheritance;
		// a group lets the icon watch its parent instead.
		.group("icon-button")
		.child(
			icon(glyph, if enabled { p.muted } else { p.border })
				.size_3p5()
				.when(enabled, move |s| s.group_hover("icon-button", move |s| s.text_color(p.text))),
		);
	if enabled { base.cursor_pointer().tooltip(tooltip(label)).on_click(on_click) } else { base }
}

/// A row in a menu: icon, label, and a count at the end; lit while it is the choice.
pub fn menu_row(
	p: Palette,
	id: impl Into<ElementId>,
	glyph: (Icon, gpui::Hsla),
	label: impl Into<SharedString>,
	count: usize,
	on: bool,
	on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
	let label = label.into();
	let color = if on { p.text } else { p.muted };
	// The icon follows the sidebar's rule: grey at rest, its own hue while the row is chosen or
	// hovered, so the menu reads as the same legend as the rows it filters.
	let (glyph, tint) = glyph;
	let glyph_color = if on { tint } else { p.muted };
	div()
		.id(id)
		.role(Role::CheckBox)
		.aria_label(label.clone())
		.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
		.flex()
		.items_center()
		.gap_2()
		.w_full()
		.px_1p5()
		.py_0p5()
		.rounded_sm()
		.text_xs()
		.cursor_pointer()
		.text_color(color)
		.when(on, |s| s.bg(p.selection))
		.when(!on, move |s| s.hover(move |s| s.bg(p.hover)))
		.group("menu-row")
		.on_click(on_click)
		.child(icon(glyph, glyph_color).size_3().group_hover("menu-row", move |s| s.text_color(tint)))
		.child(div().flex_1().child(label))
		.child(div().text_color(p.muted).child(count.to_string()))
}
