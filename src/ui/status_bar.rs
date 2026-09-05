use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::{Status, format_speed};
use crate::ui::icon::{Icon, icon};

/// One thin line under the list, the way an editor keeps its status: what is happening, not
/// what is selected. Anything about one download opens in that download's own window.
impl Rdm {
	pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let active = self.downloads.iter().filter(|d| d.status == Status::Downloading).count();
		let speed: u64 = self.downloads.iter().map(|d| d.speed).sum();
		let summary = match (self.downloads.len(), active) {
			(0, _) => "No downloads".to_owned(),
			(n, 0) => format!("{n} downloads, none active"),
			(n, a) => format!("{n} downloads, {a} active"),
		};
		let selected = self.selected().map(|d| d.name.clone());
		div()
			.flex()
			.items_center()
			.gap_3()
			.h(px(22.0))
			.px_2()
			.text_xs()
			.text_color(p.muted)
			.border_t_1()
			.border_color(p.border)
			.bg(p.panel)
			.child(summary)
			.when(speed > 0, |s| s.child(format_speed(speed)))
			.child(div().flex_1())
			.when_some(selected, |s, name| {
				s.child(
					div()
						.id("open-selected")
						.flex()
						.items_center()
						.gap_1()
						.cursor_pointer()
						.hover(move |s| s.text_color(p.text))
						.on_click(cx.listener(|this, _, _, cx| this.open_selected(cx)))
						.child(div().max_w(px(320.0)).truncate().child(name))
						.child(icon(Icon::ExternalLink, p.muted).size_3()),
				)
			})
	}
}
