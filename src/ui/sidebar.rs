use gpui::{
	Context, Hsla, IntoElement, Pixels, Point, Render, Role, SharedString, Window, div, prelude::*,
	px,
};

use crate::app::{DraggedCategory, Rdm};
use crate::category::Category;
use crate::download::Filter;
use crate::ui::icon::{Icon, hover_icon, icon};
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
			// The categories scroll and the filters above them do not. There is no ceiling on how
			// many a user writes, and the fifteen presets alone are taller than a short window: the
			// list used to run off the bottom edge, where the rows below it could not be reached at
			// all. See spec/ui.md.
			.child(
				div()
					.relative()
					.flex()
					.flex_col()
					.flex_1()
					.min_h_0()
					.child(
						div()
							.id("categories")
							.flex()
							.flex_col()
							.gap_0p5()
							.flex_1()
							.min_h_0()
							.overflow_y_scroll()
							.track_scroll(&self.categories_scroll)
							.children(categories),
					)
					.child(fades(p, &self.categories_scroll)),
			)
	}

	fn filter_row(&self, filter: Filter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let active = self.filter == filter;
		let label = filter.label(&self.categories);
		let glyph = match filter {
			Filter::Category(id) => Category::find(&self.categories, id).map_or(Icon::File, |c| c.icon),
			other => Icon::for_filter(other),
		};
		let count = self.rows().filter(|d| self.passes(filter, d)).count();
		let selector = label.clone();
		// A category's icon wears its own hue always when the categories are set to be colorful;
		// otherwise, and for the state filters above them always, only while the row is chosen
		// or hovered. The svg cannot inherit a hover color, so it watches the row through a
		// group. The window's inactive grey is in the hue already.
		let tint = p.hue(filter.color(&self.categories));
		let colorful = self.preferences.colorful_categories && matches!(filter, Filter::Category(_));
		let lit = active || colorful;
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
				hover_icon(glyph, "filter-row", if lit { tint } else { p.muted }, (!lit).then_some(tint))
					.size_3p5(),
			)
			.child(div().flex_1().child(label))
			.child(div().text_xs().text_color(p.muted).child(count.to_string()))
	}

	/// A category while the order is being edited: a grip where the count was, dragged onto
	/// another row to take its place. The catch-all keeps its place at the end and shows no grip.
	fn reorder_row(&self, id: u64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(category) = Category::find(&self.categories, id) else {
			return div().id(("reorder", id));
		};
		let movable = !category.is_catch_all();
		let label = category.name.clone();
		let selector = label.clone();
		// The rows keep the hues they had a moment ago when the categories are set to be colorful;
		// reordering a legend should not first wipe it. Otherwise they are plain text, with Other
		// grey since it is neither dragged nor a target.
		let tint = if self.preferences.colorful_categories {
			p.hue(category.color)
		} else if movable {
			p.text
		} else {
			p.muted
		};
		let preview = (label.clone(), category.icon, tint, p);
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
			.child(icon(category.icon, tint).size_3p5())
			.child(div().flex_1().text_color(if movable { p.text } else { p.muted }).child(label))
			.when(movable, |s| {
				s.cursor_grab()
					.hover(move |s| s.bg(p.hover))
					.child(icon(Icon::GripVertical, p.muted).size_3p5())
					.on_drag(DraggedCategory(id), move |_, position, _, cx| {
						let (name, glyph, tint, p) = preview.clone();
						cx.new(|_| DragPreview { name, glyph, tint, palette: p, position })
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
	tint: Hsla,
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
				.child(icon(self.glyph, self.tint).size_3p5())
				.child(self.name.clone()),
		)
	}
}

/// Whether the categories run past the top of their scroller, and past the bottom of it. Read
/// where the two are read from: the scroll handle, which the scroller fills in as it is laid out.
pub(crate) fn cut_above(scroll: &gpui::ScrollHandle) -> bool {
	scroll.offset().y < px(-0.5)
}

pub(crate) fn cut_below(scroll: &gpui::ScrollHandle) -> bool {
	scroll.offset().y.abs() < scroll.max_offset().y - px(0.5)
}

/// The short wash over a cut row. A list longer than its window has to say so, and the row at the
/// fold says it best: it dissolves into the sidebar instead of ending in a straight line, which
/// reads as more list rather than as the end of one. Fourteen points against a row of twenty-two,
/// so it takes a row's lower half and never a whole one -- and where the fold happens to fall
/// between two rows there is nothing under the wash but the sidebar itself, which is the case
/// that wants no fade and gets none by drawing one nobody can see.
///
/// It is painted rather than laid out because of when it has to be decided. How far the list is
/// scrolled and how far it can scroll are known once it has been laid out, which is after
/// everything in the same frame has been built: an element that asked the question while being
/// built would answer it from the frame before, and be a fade that arrives late and lingers after
/// the scroll that earned it. Painting happens after layout, so the question is asked when it can
/// be answered.
///
/// The colour is the sidebar's own, fading to that colour at no opacity rather than to a
/// transparent black, since the two are mixed as they are and black would leave a dark bloom
/// through the middle of the gradient. See spec/ui.md.
fn fades(p: Palette, scroll: &gpui::ScrollHandle) -> impl IntoElement {
	const DEEP: f32 = 14.0;
	let scroll = scroll.clone();
	gpui::canvas(
		|_, _, _| (),
		move |bounds, (), window, _| {
			let wash = |angle: f32| {
				gpui::linear_gradient(
					angle,
					gpui::linear_color_stop(p.sidebar, 0.0),
					gpui::linear_color_stop(p.sidebar.opacity(0.0), 1.0),
				)
			};
			let strip = |top: Pixels| {
				gpui::Bounds::new(
					gpui::point(bounds.origin.x, top),
					gpui::size(bounds.size.width, px(DEEP)),
				)
			};
			if cut_above(&scroll) {
				window.paint_quad(gpui::fill(strip(bounds.origin.y), wash(180.0)));
			}
			if cut_below(&scroll) {
				window.paint_quad(gpui::fill(strip(bounds.bottom() - px(DEEP)), wash(0.0)));
			}
		},
	)
	.absolute()
	.top_0()
	.left_0()
	.size_full()
}
