//! The pickers two faces share: the grid of icons, the swatch that opens the colors, and the
//! row of named hues with the field for one written by hand.

use gpui::{Context, IntoElement, Role, SharedString, div, prelude::*};

use crate::app::Rdm;
use crate::ui::category_sheet::CategorySheet;
use crate::ui::icon::{Icon, hover_icon};
use crate::ui::text_input::TextInput;
use crate::ui::theme::{Tint, format_hex, parse_color};
use crate::ui::tooltip::tooltip;

/// The question mark's tooltip: an invitation, like every other icon's name, since the rule
/// itself is in the guide it opens.
const COLOR_RULE: &str = "Read guidelines";

impl Rdm {
	/// The fourteen glyphs a category may be drawn with, the chosen one lit.
	pub(super) fn icon_picker(
		&self,
		chosen: Icon,
		on_pick: impl Fn(&mut Rdm, Icon, &mut Context<Rdm>) + Clone + 'static,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let p = self.palette;
		let icons: Vec<_> = Icon::CATEGORY_CHOICES
			.into_iter()
			.map(|choice| {
				let on = choice == chosen;
				let on_pick = on_pick.clone();
				div()
					.id(SharedString::from(format!("icon:{}", choice.name())))
					.role(Role::RadioButton)
					.aria_label(format!("Icon: {}", choice.name()))
					.aria_selected(on)
					.debug_selector(|| format!("icon:{}", choice.name()))
					.flex()
					.items_center()
					.justify_center()
					.size_7()
					.rounded_sm()
					.cursor_pointer()
					.group("icon-choice")
					.tooltip(tooltip(choice.name()))
					.when(on, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| on_pick(this, choice, cx)))
					.child(
						hover_icon(
							choice,
							"icon-choice",
							if on { p.text } else { p.muted },
							(!on).then_some(p.text),
						)
						.size_4(),
					)
			})
			.collect();
		div().flex().flex_wrap().gap_1().children(icons)
	}

	/// The current color as a dot that opens the picker.
	pub(super) fn swatch(
		&self,
		id: &'static str,
		color: u32,
		open: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.id(id)
			.role(Role::Button)
			.aria_label("Color")
			.debug_selector(|| "button:Color".to_owned())
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size_6()
			.rounded_sm()
			.cursor_pointer()
			.tooltip(tooltip("Color"))
			.when(open, |s| s.bg(p.selection))
			.when(!open, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(|this, _, _, cx| this.toggle_color_picker(cx)))
			.child(div().size_3p5().rounded_full().bg(p.hue(color)))
	}

	/// One line of every color a category could wear: the nine named hues, then a field for one
	/// of the user's own -- hex, rgb() or hsl(), the placeholder showing each -- with a dot after
	/// it that previews what is typed and, once it reads as a color, is a swatch like the others.
	/// The field fills what the hues leave, so the line is the card's width.
	pub(super) fn color_row(
		&self,
		current: u32,
		custom: gpui::Entity<TextInput>,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let swatches: Vec<_> = Tint::CYCLE
			.into_iter()
			.map(|tint| {
				let color = tint.rgb();
				let on = color == current;
				let name = format_hex(color);
				let selector = name.clone();
				div()
					.id(SharedString::from(format!("swatch:{name}")))
					.role(Role::RadioButton)
					.aria_label(format!("Color {name}"))
					.aria_selected(on)
					.debug_selector(move || format!("swatch:{selector}"))
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size_6()
					.rounded_sm()
					.cursor_pointer()
					.when(on, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| this.choose_color(color, cx)))
					.child(div().size_3p5().rounded_full().bg(p.hue(color)))
			})
			.collect();
		let typed = custom.read(cx).content.trim().to_owned();
		let parsed = parse_color(&typed);
		let on = parsed == Some(current);
		let preview = div()
			.id("swatch-custom")
			.role(Role::RadioButton)
			.aria_label("Your color")
			.aria_selected(on)
			.debug_selector(|| "swatch:custom".to_owned())
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size_6()
			.rounded_sm()
			.tooltip(tooltip(if parsed.is_some() { "Your color" } else { "Not a color yet" }))
			.when(on, |s| s.bg(p.selection))
			.when_some(parsed, |s, color| {
				s.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
					// Chosen from the dot, the text is kept with the category as if entered.
					match &this.category_sheet {
						Some(CategorySheet::Preset(form)) => {
							let (id, text) = (form.id, typed.clone());
							this.set_category_custom_color(id, &text, cx);
						}
						_ => this.choose_color(color, cx),
					}
				}))
			})
			.child(match parsed {
				Some(color) => div().size_3p5().rounded_full().bg(p.hue(color)),
				None => div().size_3p5().rounded_full().border_1().border_color(p.border),
			});
		div()
			.flex()
			.items_center()
			.gap_1()
			.children(swatches)
			.child(div().flex_1().min_w_0().ml_1().child(custom))
			.child(preview)
			// A question mark after the field: the rule on hover, the whole guide laid over the
			// form on a press, so the form does not move under the pointer.
			.child(
				div()
					.id("color-help")
					.role(Role::Button)
					.aria_label("Color formats")
					.debug_selector(|| "button:Color formats".to_owned())
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size_6()
					.rounded_sm()
					.cursor_pointer()
					.group("color-help")
					.tooltip(tooltip(COLOR_RULE))
					.on_click(cx.listener(|this, _, _, cx| this.show_color_guide(cx)))
					.child(hover_icon(Icon::CircleQuestion, "color-help", p.muted, Some(p.text)).size_4()),
			)
	}
}
