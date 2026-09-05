use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::{Filter, Kind};
use crate::ui::theme;

impl Rdm {
	pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let states: Vec<_> = Filter::STATES.iter().map(|f| self.filter_row(*f, cx)).collect();
		let kinds: Vec<_> = Kind::ALL.iter().map(|k| self.filter_row(Filter::Kind(*k), cx)).collect();
		div()
			.flex()
			.flex_col()
			.w(px(200.0))
			.h_full()
			.py_2()
			.px_2()
			.gap_px()
			.border_r_1()
			.border_color(theme::border())
			.bg(theme::sidebar())
			.children(states)
			.child(section("Categories"))
			.children(kinds)
	}

	fn filter_row(&self, filter: Filter, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let active = self.filter == filter;
		let count = self.downloads.iter().filter(|d| filter.matches(d)).count();
		div()
			.id(filter.label())
			.flex()
			.justify_between()
			.items_center()
			.px_2()
			.py_1()
			.rounded_md()
			.cursor_pointer()
			.when(active, |s| s.bg(theme::selection()))
			.when(!active, |s| s.hover(|s| s.bg(theme::hover())))
			.on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter, cx)))
			.child(filter.label())
			.child(div().text_xs().text_color(theme::muted()).child(count.to_string()))
	}
}

fn section(title: &'static str) -> impl IntoElement {
	div().pt_3().pb_1().px_2().text_xs().text_color(theme::muted()).child(title)
}
