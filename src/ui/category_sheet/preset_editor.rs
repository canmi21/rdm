//! One preset a level down: its color, its extension list as switches with a field to add
//! more, and its icon.

use gpui::{Context, Role, SharedString, deferred, div, prelude::*};

use crate::app::Rdm;
use crate::category::Category;
use crate::ui::category_sheet::{PresetForm, section, word};
use crate::ui::tooltip::tooltip;
use crate::ui::{LeavesFocus, backdrop};

impl Rdm {
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
		// A built-in extension switches off and back; an added one is simply dropped.
		let chips: Vec<_> = preset
			.base()
			.into_iter()
			.map(|e| (!overrides.removed.contains(&e), e))
			.chain(overrides.added.iter().map(|e| (true, e.clone())))
			.map(|(on, extension)| {
				let label: SharedString = extension.clone().into();
				let selector = extension.clone();
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
					.text_color(if on { p.text } else { p.muted })
					.when(on, |s| s.bg(p.selection))
					.when(!on, move |s| s.line_through().hover(move |s| s.bg(p.hover)))
					.on_click(
						cx.listener(move |this, _, _, cx| this.set_preset_extension(id, &extension, !on, cx)),
					)
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
					// The name alone above; every color it could wear on the line under it.
					.child(self.color_row(color_now, form.custom.clone(), cx))
					.child(section(p.muted, "Extensions"))
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
