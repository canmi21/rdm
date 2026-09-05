use gpui::{Context, IntoElement, Role, div, prelude::*, px};

use crate::app::{Rdm, View};
use crate::download::{Status, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::{chip, icon_button, sidebar};

const CHIPS: [Status; 5] =
	[Status::Downloading, Status::Queued, Status::Paused, Status::Completed, Status::Failed];

/// One line across the whole window, the way an editor keeps its status. Under the sidebar it
/// holds the application's own controls; under the list, the list's: status chips, a summary,
/// the selected download as a link to its window, and the view switch.
impl Rdm {
	pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let active = self.downloads.iter().filter(|d| d.status == Status::Downloading).count();
		let speed: u64 = self.downloads.iter().map(|d| d.speed).sum();
		let summary = match (self.downloads.len(), active) {
			(0, _) => "No downloads".to_owned(),
			(n, 0) => format!("{n} downloads"),
			(n, a) => format!("{n} downloads, {a} active"),
		};
		let selected = self.selected().map(|d| d.name.clone());
		div()
			.flex()
			.items_center()
			.h(px(24.0))
			.text_xs()
			.text_color(p.muted)
			.border_t_1()
			.border_color(p.border)
			.bg(p.panel)
			.child(
				div()
					.flex()
					.items_center()
					.h_full()
					.w(px(sidebar::WIDTH))
					.px_1p5()
					.border_r_1()
					.border_color(p.border)
					.child(icon_button(
						p,
						"settings",
						Icon::Settings,
						"Settings",
						cx.listener(|this, _, _, cx| this.open_settings(cx)),
					)),
			)
			.child(
				div()
					.flex()
					.flex_1()
					.min_w_0()
					.items_center()
					.gap_1()
					.px_2()
					.children(self.chips(cx))
					.child(div().flex_1())
					.child(div().whitespace_nowrap().child(summary))
					.when(speed > 0, |s| s.child(format_speed(speed)))
					.when_some(selected, |s, name| {
						s.child(
							div()
								.id("open-selected")
								.role(Role::Button)
								.aria_label("Open selected")
								.debug_selector(|| "button:Open selected".to_owned())
								.flex()
								.items_center()
								.gap_1()
								.ml_2()
								.cursor_pointer()
								.hover(move |s| s.text_color(p.text))
								.on_click(cx.listener(|this, _, _, cx| this.open_selected(cx)))
								.child(div().max_w(px(240.0)).truncate().child(name))
								.child(icon(Icon::ExternalLink, p.muted).size_3()),
						)
					})
					.child(div().w(px(8.0)))
					.child(self.view_switch(cx)),
			)
	}

	/// Status chips: a second cut inside whatever the sidebar selected, one at a time.
	fn chips(&self, cx: &mut Context<Self>) -> Vec<impl IntoElement + use<>> {
		let p = self.palette;
		CHIPS
			.iter()
			.map(|status| {
				let status = *status;
				let count =
					self.downloads.iter().filter(|d| self.filter.matches(d) && d.status == status).count();
				chip(
					p,
					gpui::SharedString::from(format!("chip:{}", status.label())),
					format!("{} {count}", status.label()),
					self.status == Some(status),
					cx.listener(move |this, _, _, cx| this.toggle_status(status, cx)),
				)
				.debug_selector(|| format!("chip:{}", status.label()))
			})
			.collect()
	}

	/// One segment per view, the active one raised like a pressed key.
	fn view_switch(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let segments: Vec<_> = View::ALL
			.iter()
			.map(|view| {
				let view = *view;
				let active = self.view == view;
				let color = if active { p.text } else { p.muted };
				div()
					.id(view_id(view))
					.role(Role::RadioButton)
					.aria_label(format!("View: {view:?}"))
					.aria_selected(active)
					.debug_selector(|| format!("view:{view:?}"))
					.flex()
					.items_center()
					.justify_center()
					.size_5()
					.rounded_sm()
					.cursor_pointer()
					.when(active, |s| s.bg(p.selection))
					.when(!active, move |s| s.hover(move |s| s.bg(p.hover)))
					.on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx)))
					.child(icon(view_icon(view), color).size_3())
			})
			.collect();
		div().flex().items_center().gap_px().children(segments)
	}
}

fn view_id(view: View) -> &'static str {
	match view {
		View::Detailed => "view-detailed",
		View::Compact => "view-compact",
		View::Grid => "view-grid",
	}
}

fn view_icon(view: View) -> Icon {
	match view {
		View::Detailed => Icon::LayoutList,
		View::Compact => Icon::Rows,
		View::Grid => Icon::LayoutGrid,
	}
}
