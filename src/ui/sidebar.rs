use gpui::{
	Context, IntoElement, Pixels, Point, Render, Role, SharedString, Window, div, prelude::*, px,
};

use crate::app::{DraggedCategory, Rdm};
use crate::download::Filter;
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button;
use crate::ui::theme::Palette;

/// Shared with the status bar, whose left segment sits under this column.
pub const WIDTH: f32 = 176.0;

impl Rdm {
	pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let reordering = self.reordering();
		let states: Vec<_> = Filter::STATES.iter().map(|f| self.filter_row(*f, cx)).collect();
		let categories: Vec<_> = self
			.categories
			.iter()
			.map(|c| {
				if reordering {
					self.reorder_row(c.id, cx).into_any_element()
				} else {
					self.filter_row(Filter::Category(c.id), cx).into_any_element()
				}
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.w(px(WIDTH))
			.h_full()
			.py_1p5()
			.px_1p5()
			// Adjacent rows must not touch when one is lit and the next is hovered.
			.gap_0p5()
			.border_r_1()
			.border_color(p.border)
			.bg(p.sidebar)
			.child(
				// While the categories are being reordered they are the one lit thing in the window,
				// so the filters above them are dimmed here, with the same wash the sheet's backdrop
				// lays over everything else. Anchored so the wash covers them and nothing more.
				div()
					.relative()
					.flex()
					.flex_col()
					.gap_0p5()
					.children(states)
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.pt_4()
							.pb_1()
							.pl_1p5()
							.text_xs()
							.text_color(p.muted)
							.child("Categories")
							.child(icon_button(
								p,
								"add-category",
								Icon::Plus,
								"New category",
								!reordering,
								cx.listener(|this, _, window, cx| this.open_category_sheet(window, cx)),
							)),
					)
					// A press on this wash is a press outside the work, and finishes the reorder.
					.when(reordering, |s| {
						s.child(
							div().id("sidebar-wash").absolute().inset_0().occlude().bg(p.dim).on_mouse_down(
								gpui::MouseButton::Left,
								cx.listener(|this, _, _, cx| this.close_category_sheet(cx)),
							),
						)
					}),
			)
			.children(categories)
	}

	fn filter_row(&self, filter: Filter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let active = self.filter == filter;
		let label = filter.label(&self.categories);
		let glyph = match filter {
			Filter::Category(id) => {
				self.categories.iter().find(|c| c.id == id).map_or(Icon::File, |c| c.icon)
			}
			other => Icon::for_filter(other),
		};
		let count = self.downloads.iter().filter(|d| filter.matches(d, &self.categories)).count();
		let selector = label.clone();
		// The icon wears its own hue always when the sidebar is set to be colorful, else only
		// while the row is chosen or hovered; the svg cannot inherit a hover color, so it watches
		// the row through a group. The window's inactive grey is in the hue already.
		let tint = p.hue(filter.color(&self.categories));
		let lit = active || self.preferences.colorful_sidebar;
		div()
			.id(SharedString::from(format!("filter:{label}")))
			.role(Role::Tab)
			.aria_label(format!("Filter: {label}"))
			.aria_selected(active)
			.debug_selector(move || format!("filter:{selector}"))
			.flex()
			.items_center()
			.gap_2()
			.px_1p5()
			.py_0p5()
			.rounded_sm()
			.cursor_pointer()
			.group("filter-row")
			.when(active, |s| s.bg(p.selection))
			.when(!active, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
			.child(
				icon(glyph, if lit { tint } else { p.muted })
					.size_3p5()
					.when(!lit, move |s| s.group_hover("filter-row", move |s| s.text_color(tint))),
			)
			.child(div().flex_1().child(label))
			.child(div().text_xs().text_color(p.muted).child(count.to_string()))
	}

	/// A category while the order is being edited: a grip where the count was, dragged onto
	/// another row to take its place. The catch-all keeps its place at the end and shows no grip.
	fn reorder_row(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(category) = self.categories.iter().find(|c| c.id == id) else {
			return div().id(("reorder", id));
		};
		let movable = !category.is_catch_all();
		let label = category.name.clone();
		let selector = label.clone();
		let preview = (label.clone(), category.icon, p);
		div()
			.id(("reorder", id))
			.role(Role::ListItem)
			.aria_label(format!("Category: {label}"))
			.debug_selector(move || format!("filter:{selector}"))
			.flex()
			.items_center()
			.gap_2()
			.px_1p5()
			.py_0p5()
			.rounded_sm()
			.child(icon(category.icon, if movable { p.text } else { p.muted }).size_3p5())
			.child(div().flex_1().text_color(if movable { p.text } else { p.muted }).child(label))
			.when(movable, |s| {
				s.cursor_grab()
					.hover(move |s| s.bg(p.hover))
					.child(icon(Icon::GripVertical, p.muted).size_3p5())
					.on_drag(DraggedCategory(id), move |_, position, _, cx| {
						let (name, glyph, p) = preview.clone();
						cx.new(|_| DragPreview { name, glyph, palette: p, position })
					})
					// The row the pointer is over is where the drop will land, so it lights up.
					.drag_over::<DraggedCategory>(move |s, _, _, _| s.bg(p.selection))
					.on_drop(cx.listener(move |this, dragged: &DraggedCategory, _, cx| {
						this.move_category(dragged.0, id, cx)
					}))
			})
	}
}

/// The row as it travels under the pointer.
struct DragPreview {
	name: String,
	glyph: Icon,
	palette: Palette,
	position: Point<Pixels>,
}

impl Render for DragPreview {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		div().pl(self.position.x - px(12.0)).pt(self.position.y - px(11.0)).child(
			div()
				.flex()
				.items_center()
				.gap_2()
				.px_1p5()
				.py_0p5()
				.rounded_sm()
				.bg(p.selection)
				.text_size(px(13.0))
				.text_color(p.text)
				.shadow_md()
				.child(icon(self.glyph, p.text).size_3p5())
				.child(self.name.clone()),
		)
	}
}
