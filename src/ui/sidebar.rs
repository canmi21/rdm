use gpui::{Context, IntoElement, Role, SharedString, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::{Filter, Kind};
use crate::ui::icon::{Icon, icon};

/// Shared with the status bar, whose left segment sits under this column.
pub const WIDTH: f32 = 176.0;

impl Rdm {
	pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let states: Vec<_> = Filter::STATES.iter().map(|f| self.filter_row(*f, cx)).collect();
		let kinds: Vec<_> = Kind::ALL.iter().map(|k| self.filter_row(Filter::Kind(*k), cx)).collect();
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
			.child(div().pt_4().pb_1().px_1p5().text_xs().text_color(p.muted).child("Categories"))
			.children(kinds)
	}

	fn filter_row(&self, filter: Filter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let active = self.filter == filter;
		let count = self.downloads.iter().filter(|d| filter.matches(d)).count();
		div()
			.id(SharedString::from(format!("filter:{}", filter.label())))
			.role(Role::Tab)
			.aria_label(format!("Filter: {}", filter.label()))
			.aria_selected(active)
			.debug_selector(|| format!("filter:{}", filter.label()))
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
			.child(icon(Icon::for_filter(filter), if active { p.text } else { p.muted }).size_3p5())
			.child(div().flex_1().child(filter.label()))
			.child(div().text_xs().text_color(p.muted).child(count.to_string()))
	}
}
