//! The category sheet, in four faces. It opens on the presets, each a switch, with Edit, Reorder
//! and Add under them. Edit turns the chips into doors to each preset's extension list. Reorder
//! shrinks the sheet to one line pointing at the sidebar, where the rows are dragged into order.
//! Add opens the custom form: a name, an icon, extensions and text the name contains -- or,
//! under Advanced, the regular expression written out. The faces are drawn in files of their
//! own, the moves between them in `actions`, and the pickers two faces share in `pickers`;
//! this file holds the sheet's state, the card every face sits in, and the small controls.

use gpui::{
	Context, Entity, IntoElement, Role, SharedString, Window, deferred, div, prelude::*, px,
};

use crate::app::Rdm;
use crate::category::Combine;
use crate::ui::icon::{Icon, hover_icon};
use crate::ui::icon_button;
use crate::ui::text_input::TextInput;
use crate::ui::theme::Palette;
use crate::ui::tooltip::tooltip;

mod actions;
mod custom;
mod pickers;
mod preset_editor;
mod presets;

/// The custom category form while it is up. The pattern field is what runs; until Advanced is
/// opened it is derived from the basic fields and never seen.
pub struct CategoryForm {
	pub name: Entity<TextInput>,
	pub extensions: Entity<TextInput>,
	pub contains: Entity<TextInput>,
	/// How the two basic fields combine when both are filled.
	pub combine: Combine,
	/// The two switches after the contains field. Off, the text is matched loosely: case is
	/// ignored; on, it must match as typed. Spaces are the other way: kept unless switched off.
	pub match_case: bool,
	pub ignore_space: bool,
	pub pattern: Entity<TextInput>,
	pub icon: Icon,
	/// The color the icon will be lit in, `0xrrggbb`; the swatch beside the name opens the
	/// picker, whose field takes a color written any way the stack reads.
	pub color: u32,
	pub color_open: bool,
	pub custom: Entity<TextInput>,
	pub advanced: bool,
}

/// A preset being edited: which category, the field that adds to its list, and the field for
/// a color of the user's own, which follows the category.
pub struct PresetForm {
	pub id: u64,
	pub add: Entity<TextInput>,
	pub custom: Entity<TextInput>,
}

/// The category sheet's faces: the presets with Edit, Reorder and Add under them; the one-line
/// hint while the sidebar's categories are being dragged into order; one preset's extension
/// list; and the custom form.
pub enum CategorySheet {
	/// `editing` turns the preset chips from switches into doors to their lists.
	Presets {
		editing: bool,
	},
	Reorder,
	Preset(PresetForm),
	Custom(CategoryForm),
}

impl Rdm {
	pub(crate) fn render_category_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		match &self.category_sheet {
			None => deferred(div()).priority(2),
			Some(CategorySheet::Presets { editing }) => self.presets_face(*editing, cx),
			Some(CategorySheet::Reorder) => self.reorder_face(cx),
			Some(CategorySheet::Preset(form)) => self.preset_face(form, cx),
			Some(CategorySheet::Custom(form)) => self.custom_face(form, cx),
		}
	}

	/// The card every face sits in, over a backdrop that takes every mouse event so nothing
	/// behind it can be pressed through. A press outside the card asks to dismiss, except on the
	/// reorder face, where outside the card is the sidebar and a press there is the start of a
	/// drag.
	fn sheet_card(
		&self,
		id: &'static str,
		width: f32,
		dismiss_outside: bool,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let p = self.palette;
		div()
			.id(id)
			.debug_selector(move || id.to_owned())
			.flex()
			.flex_col()
			.gap_3()
			.w(px(width))
			.p_4()
			.rounded_lg()
			.border_1()
			.border_color(p.border)
			.bg(p.panel)
			.shadow_lg()
			.when(dismiss_outside, |s| {
				s.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_category_sheet(cx)))
			})
	}

	/// The title and the cross. On the presets the cross closes; a level down it steps back.
	fn title_row(
		&self,
		title: SharedString,
		back: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.flex()
			.items_center()
			.justify_between()
			.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(title))
			.child(icon_button(
				p,
				"category-close",
				Icon::X,
				"Close",
				true,
				cx.listener(
					move |this, _, _, cx| {
						if back { this.back_to_presets(cx) } else { this.close_category_sheet(cx) }
					},
				),
			))
	}
}

/// The wash over the whole window that the presets, preset and custom faces sit on.
fn backdrop(p: Palette) -> gpui::Div {
	div().absolute().inset_0().occlude().flex().items_center().justify_center().bg(p.dim)
}

fn section(color: gpui::Hsla, title: &'static str) -> impl IntoElement {
	div().text_xs().text_color(color).child(title)
}

/// A word that acts: Edit, Reorder, Reset. It brightens on hover and stays bright while it names
/// a mode that is on.
fn word(
	p: Palette,
	id: &'static str,
	label: &'static str,
	on: bool,
	on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.debug_selector(move || format!("button:{label}"))
		.text_xs()
		.text_color(if on { p.text } else { p.muted })
		.cursor_pointer()
		.hover(move |s| s.text_color(p.text))
		.on_click(on_click)
		.child(label)
}

/// An icon that is a switch: lit and backed while on, muted while off.
fn toggle(
	p: Palette,
	id: &'static str,
	glyph: Icon,
	label: &'static str,
	on: bool,
	on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
	div()
		.id(id)
		.role(Role::CheckBox)
		.aria_label(label)
		.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
		.debug_selector(move || format!("toggle:{label}"))
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size_6()
		.rounded_sm()
		.cursor_pointer()
		.group("toggle")
		.tooltip(tooltip(label))
		.when(on, |s| s.bg(p.selection))
		.on_click(on_click)
		.child(
			hover_icon(glyph, "toggle", if on { p.text } else { p.muted }, (!on).then_some(p.text))
				.size_4(),
		)
}
