//! The window, split into the pieces a reader would look for: toolbar, sidebar, list, status
//! bar -- and the windows it opens beside itself.

pub mod add_dialog;
pub mod category_sheet;
pub mod download_window;
pub mod first_mouse;
pub mod guide;
pub mod icon;
pub mod list;
pub mod settings_sheet;
pub mod sidebar;
pub mod status_bar;
pub mod text_input;
pub mod theme;
pub mod toolbar;
pub mod tooltip;

use gpui::{
	ClickEvent, Div, ElementId, MouseButton, Role, SharedString, Stateful, div, prelude::*,
};

use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::theme::Palette;
use crate::ui::tooltip::tooltip;

/// The wash under every sheet. It takes every mouse event, so nothing behind the sheet can be
/// pressed through it; and a press that lands on nothing focusable -- the card, a button, a
/// row -- takes the keyboard away from whatever field had it. GPUI moves focus only onto a
/// focusable element that is pressed and never off one on its own, and the sheet occludes the
/// window's root, which would otherwise be the focusable thing under the press; so the wash
/// blurs instead, and the root takes the keyboard back at the next frame. A field pressed
/// claims the press first and says so, which is what `default_prevented` reports.
pub fn backdrop(p: Palette) -> Div {
	div()
		.absolute()
		.inset_0()
		.occlude()
		.flex()
		.items_center()
		.justify_center()
		.bg(p.dim)
		.on_mouse_down(MouseButton::Left, |_, window, _| {
			if !window.default_prevented() {
				window.blur();
			}
		})
}

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
		.group("icon-button")
		.child(
			hover_icon(
				glyph,
				"icon-button",
				if enabled { p.muted } else { p.border },
				enabled.then_some(p.text),
			)
			.size_3p5(),
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
		.child(hover_icon(glyph, "menu-row", glyph_color, Some(tint)).size_3())
		.child(div().flex_1().child(label))
		.child(div().text_color(p.muted).child(count.to_string()))
}
