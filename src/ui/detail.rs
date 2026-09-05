use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::{Download, format_bytes, format_duration, format_speed};
use crate::ui::theme;

impl Rdm {
	pub(crate) fn render_detail(&self, _cx: &mut Context<Self>) -> impl IntoElement {
		let pane = div()
			.flex()
			.flex_col()
			.gap_1()
			.h(px(120.0))
			.px_4()
			.py_3()
			.text_xs()
			.border_t_1()
			.border_color(theme::border())
			.bg(theme::panel());
		match self.selected() {
			None => {
				pane.justify_center().items_center().text_color(theme::muted()).child("Select a download")
			}
			Some(download) => pane
				.child(div().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).child(download.name.clone()))
				.child(field("URL", download.url.clone()))
				.child(field("Size", format_bytes(download.size)))
				.child(field("Status", status_line(download))),
		}
	}
}

fn field(name: &'static str, value: String) -> impl IntoElement {
	div()
		.flex()
		.gap_2()
		.child(div().w(px(48.0)).text_color(theme::muted()).child(name))
		.child(div().flex_1().min_w_0().truncate().child(value))
}

fn status_line(download: &Download) -> String {
	let mut line = download.status.label().to_owned();
	if download.speed > 0 {
		line.push_str(&format!(", {}", format_speed(download.speed)));
	}
	if let Some(left) = download.remaining() {
		line.push_str(&format!(", {} left", format_duration(left)));
	}
	line
}
