use gpui::{Context, IntoElement, Role, SharedString, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Filter;
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button;

/// Shared with the status bar, whose left segment sits under this column.
pub const WIDTH: f32 = 176.0;

impl Rdm {
	pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let states: Vec<_> = Filter::STATES.iter().map(|f| self.filter_row(*f, cx)).collect();
		let categories: Vec<_> =
			self.categories.iter().map(|c| self.filter_row(Filter::Category(c.id), cx)).collect();
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
						true,
						cx.listener(|this, _, window, cx| this.open_category_form(window, cx)),
					)),
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
			.when(active, |s| s.bg(p.selection))
			.when(!active, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
			.child(icon(glyph, if active { p.text } else { p.muted }).size_3p5())
			.child(div().flex_1().child(label))
			.child(div().text_xs().text_color(p.muted).child(count.to_string()))
	}
}
