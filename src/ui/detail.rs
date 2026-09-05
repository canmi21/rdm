use gpui::{Context, IntoElement, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::{Download, format_bytes, format_duration, format_speed};

impl Rdm {
	pub(crate) fn render_detail(&self, _cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let pane = div()
			.flex()
			.flex_col()
			.gap_0p5()
			.h(px(84.0))
			.px_3()
			.py_2()
			.text_xs()
			.border_t_1()
			.border_color(p.border)
			.bg(p.panel);
		match self.selected() {
			None => pane.justify_center().items_center().text_color(p.muted).child("Select a download"),
			Some(download) => pane
				.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(download.name.clone()))
				.child(field(p.muted, "URL", download.url.clone()))
				.child(field(p.muted, "Size", format_bytes(download.size)))
				.child(field(p.muted, "Status", status_line(download))),
		}
	}
}

fn field(label: gpui::Hsla, name: &'static str, value: String) -> impl IntoElement {
	div()
		.flex()
		.gap_2()
		.child(div().w(px(40.0)).text_color(label).child(name))
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
