//! One preset a level down: its color, its extension list as switches with a field to add
//! more, and its icon.

use gpui::{Context, IntoElement, Role, SharedString, deferred, div, prelude::*};

use crate::app::Rdm;
use crate::category::Category;
use crate::ui::category_sheet::{PresetForm, Shading, section, word};
use crate::ui::tooltip::tooltip;
use crate::ui::{LeavesFocus, backdrop};

impl Rdm {
	/// The Extensions heading, and what it turns into. `Colors` puts the list into the state
	/// where a chip is a door to its own colour rather than a switch; while one is open the
	/// heading names it and offers the way back to the category's colour, which is what leaving
	/// a shade empty means. The same turn the presets face makes under Edit.
	fn shade_row(
		&self,
		form: &PresetForm,
		category: &Category,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let id = form.id;
		let shading = form.shading.clone();
		let heading = match &shading {
			Shading::One(extension) => format!("Color for .{extension}"),
			Shading::Picking => "Pick an extension".to_owned(),
			Shading::Off => "Extensions".to_owned(),
		};
		let inherits = match &shading {
			Shading::One(extension) => !category.shades.contains_key(extension),
			_ => false,
		};
		div()
			.flex()
			.items_center()
			.justify_between()
			.child(section(p.muted, heading))
			.child(match shading {
				// Nothing open: the word that turns the chips into doors, lit while they are.
				Shading::Off | Shading::Picking => word(
					p,
					"shade",
					"Colors",
					matches!(form.shading, Shading::Picking),
					cx.listener(|this, _, _, cx| this.shade_extension(Shading::Picking, cx)),
				)
				.tooltip(tooltip("Give an extension a color of its own"))
				.into_any_element(),
				// One open: the way back to the category's own colour, lit while it is there.
				Shading::One(extension) => word(
					p,
					"inherit",
					"Inherit",
					inherits,
					cx.listener(move |this, _, _, cx| {
						this.set_extension_shade(id, &extension, None, cx);
					}),
				)
				.tooltip(tooltip("Use the category's color"))
				.into_any_element(),
			})
	}

	/// A preset's list: the built-in extensions and the added ones as chips that switch, a field
	/// that adds more, and Reset while anything has been changed. Every change applies and is
	/// written as it is made, like the preset switches themselves.
	pub(super) fn preset_face(&self, form: &PresetForm, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let id = form.id;
		let Some(category) = Category::find(&self.categories, id) else {
			return deferred(div()).priority(2);
		};
		let Some((preset, overrides)) = category.preset.as_ref() else {
			return deferred(div()).priority(2);
		};
		// A built-in extension switches off and back; an added one is simply dropped. While a
		// colour is being set the same chips are doors instead, which is the turn the presets
		// face makes under Edit: one list, two things to do with it, never both at once.
		let shading = form.shading != Shading::Off;
		let chips: Vec<_> = preset
			.base()
			.into_iter()
			.map(|e| (!overrides.removed.contains(&e), e))
			.chain(overrides.added.iter().map(|e| (true, e.clone())))
			.map(|(on, extension)| {
				let label: SharedString = extension.clone().into();
				let selector = extension.clone();
				// The chip wears the colour a file of this extension would: the feature says what
				// it is by being what it does, without a word of explanation.
				let shade = category.shade(&format!("f.{extension}"));
				let chosen = form.shading == Shading::One(extension.clone());
				let opening = extension.clone();
				div()
					.id(SharedString::from(format!("extension:{extension}")))
					.role(Role::CheckBox)
					.aria_label(format!("Extension: {extension}"))
					.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(move || format!("extension:{selector}"))
					.px_2()
					.py_1()
					.rounded_md()
					.cursor_pointer()
					.leaves_focus()
					.text_xs()
					.text_color(if on { p.hue(shade) } else { p.muted })
					.when(on && !shading, |s| s.bg(p.selection))
					.when(chosen, |s| s.bg(p.selection))
					.when(!on, |s| s.line_through())
					// One hover, whichever reason there is for it: gpui takes only one.
					.when(!on || (shading && !chosen), move |s| s.hover(move |s| s.bg(p.hover)))
					.on_click(cx.listener(move |this, _, _, cx| {
						if shading {
							this.shade_extension(Shading::One(opening.clone()), cx);
						} else {
							this.set_preset_extension(id, &extension, !on, cx);
						}
					}))
					.child(label)
			})
			.collect();
		let changed = category.differs_from_preset();
		let (icon_now, color_now) = (category.icon, category.color);
		// Reset stands alone in the corner, a word rather than a button, and only while there is
		// something to undo: a preset left as shipped has nothing to go back to.
		let reset = div().flex().justify_end().child(if changed {
			word(p, "reset", "Reset", false, cx.listener(move |this, _, _, cx| this.reset_preset(id, cx)))
				.tooltip(tooltip("Reset to default"))
				.into_any_element()
		} else {
			div().text_xs().text_color(p.border).child("Reset").into_any_element()
		});
		deferred(
			backdrop(p).child(
				self
					.sheet_card("category-sheet", 480.0, true, cx)
					.child(self.title_row(preset.name.into(), true, cx))
					// The name alone above; every color it could wear on the line under it. While an
					// extension is chosen the swatches set that one's colour and the heading says
					// so, so there is never a doubt about what the next press paints.
					.child(self.color_row(
						match &form.shading {
							Shading::One(extension) => category.shade(&format!("f.{extension}")),
							_ => color_now,
						},
						form.custom.clone(),
						cx,
					))
					.child(self.shade_row(form, category, cx))
					.child(div().flex().flex_wrap().gap_1().children(chips))
					.child(form.add.clone())
					.child(section(p.muted, "Icon"))
					.child(self.icon_picker(
						icon_now,
						move |this, choice, cx| this.set_category_icon(id, choice, cx),
						cx,
					))
					.child(reset),
			),
		)
		.priority(2)
	}
}
