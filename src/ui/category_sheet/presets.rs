//! The first face: the presets as chips, with Edit, Reorder and Add; and the one-line hint
//! the sheet shrinks to while the sidebar's categories are dragged into order.

use gpui::{Context, Role, SharedString, deferred, div, prelude::*, px};

use crate::app::Rdm;
use crate::category::Category;
use crate::ui::backdrop;
use crate::ui::category_sheet::{section, word};
use crate::ui::icon::{Icon, icon};
use crate::ui::{button, sidebar, status_bar, toolbar};

impl Rdm {
	pub(super) fn presets_face(&self, editing: bool, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let presets: Vec<_> = Category::PRESETS
			.iter()
			.map(|preset| {
				let name = preset.name;
				let on = self.categories.iter().find(|c| c.name == name).map(|c| c.id);
				let lit = on.is_some();
				// While editing, a chip that is on opens its list and shows a pencil to say so; one
				// that is off has no list and goes quiet.
				let quiet = editing && !lit;
				let color = if lit { p.text } else { p.muted };
				div()
					.id(SharedString::from(format!("preset:{name}")))
					.role(if editing { Role::Button } else { Role::CheckBox })
					.aria_label(format!("Preset: {name}"))
					.when(!editing, |s| {
						s.aria_toggled(if lit { gpui::Toggled::True } else { gpui::Toggled::False })
					})
					.debug_selector(|| format!("preset:{name}"))
					.flex()
					.items_center()
					.gap_1p5()
					.px_2()
					.py_1()
					.rounded_md()
					.text_xs()
					.text_color(if quiet { p.border } else { color })
					.when(lit, |s| s.bg(p.selection))
					.when(!lit && !editing, move |s| s.hover(move |s| s.bg(p.hover)))
					.when(!quiet, |s| s.cursor_pointer())
					.when(!editing, |s| {
						s.on_click(cx.listener(move |this, _, _, cx| this.toggle_preset(name, cx)))
					})
					.when_some(on.filter(|_| editing), |s, id| {
						s.on_click(
							cx.listener(move |this, _, window, cx| this.open_preset_editor(id, Some(window), cx)),
						)
					})
					.child(icon(if editing && lit { Icon::Pencil } else { preset.icon }, color).size_3p5())
					.child(name)
			})
			.collect();
		deferred(
			backdrop(p).child(
				self
					.sheet_card("category-sheet", 480.0, true, cx)
					.child(self.title_row("Categories".into(), false, cx))
					.child(section(p.muted, "Presets"))
					.child(div().flex().flex_wrap().gap_1().children(presets))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.gap_3()
									.child(word(
										p,
										"edit",
										"Edit",
										editing,
										cx.listener(|this, _, _, cx| this.toggle_preset_editing(cx)),
									))
									.child(word(
										p,
										"reorder",
										"Reorder",
										false,
										cx.listener(|this, _, _, cx| this.start_reorder(cx)),
									)),
							)
							.child(button(
								p,
								"category-add",
								Icon::Plus,
								"Add",
								true,
								cx.listener(|this, _, window, cx| this.open_custom_form(Some(window), cx)),
							)),
					),
			),
		)
		.priority(2)
	}

	/// One line pointing at the sidebar; Escape finishes, and so does a press anywhere that is
	/// neither this line nor the categories, since every drop is written as it lands and there
	/// is nothing to lose. A drag never counts: it begins on a row, not on a wash. The backdrop
	/// leaves the sidebar's column alone, since that is where the work is; the sidebar dims its
	/// own filters, above the categories, so the categories are the one lit thing. See spec/ui.md.
	pub(super) fn reorder_face(&self, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let side = px(sidebar::WIDTH);
		let wash = |id: &'static str, cx: &mut Context<Self>| {
			div().id(id).absolute().occlude().bg(p.dim).on_mouse_down(
				gpui::MouseButton::Left,
				cx.listener(|this, _, _, cx| this.close_category_sheet(cx)),
			)
		};
		deferred(
			div()
				.absolute()
				.inset_0()
				.child(wash("wash-top", cx).top_0().left_0().w(side).h(px(toolbar::HEIGHT)))
				.child(wash("wash-bottom", cx).bottom_0().left_0().w(side).h(px(status_bar::HEIGHT)))
				.child(
					wash("wash-right", cx)
						.top_0()
						.bottom_0()
						.left(side)
						.right_0()
						.flex()
						.items_center()
						.justify_center()
						.child(
							self
								.sheet_card("category-sheet", 400.0, false, cx)
								// The card is not outside: a press on it stays on it.
								.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
								.flex_row()
								.items_center()
								.gap_3()
								.child(icon(Icon::ArrowLeft, p.accent).size_4())
								.child(
									div()
										.flex_1()
										.text_xs()
										.child("Drag the categories in the sidebar into the order you want."),
								),
						),
				),
		)
		.priority(2)
	}
}
