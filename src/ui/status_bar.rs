use gpui::{
	Anchor, AnchoredPositionMode, Context, IntoElement, Role, SharedString, anchored, deferred, div,
	point, prelude::*, px,
};

use crate::app::{Rdm, View};
use crate::download::{Status, format_speed};
use crate::ui::icon::{Icon, icon};
use crate::ui::{icon_button, menu_row, sidebar};

/// The status bar's height, which the filter menu sits just above.
const HEIGHT: f32 = 24.0;

/// One line across the whole window, the way an editor keeps its status. Under the sidebar, the
/// four actions as icons, always drawn and enabled by the selection. Under the list, from left
/// to right: a summary of the collection, and at the corner the controls about the list itself -- the status filter, the view switch, Settings.
impl Rdm {
	pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let active = self.downloads.iter().filter(|d| d.status == Status::Downloading).count();
		let speed: u64 = self.downloads.iter().map(|d| d.speed).sum();
		let summary = match (self.downloads.len(), active) {
			(0, _) => "No downloads".to_owned(),
			(n, 0) => format!("{n} downloads"),
			(n, a) => format!("{a} of {n} active"),
		};
		let selected = self.selected();
		let can_pause = selected.is_some_and(|d| d.status == Status::Downloading);
		let can_resume = selected
			.is_some_and(|d| matches!(d.status, Status::Paused | Status::Failed | Status::Queued));
		div()
			.flex()
			.items_center()
			.h(px(HEIGHT))
			.text_xs()
			.text_color(p.muted)
			.border_t_1()
			.border_color(p.border)
			.bg(p.panel)
			.child(
				div()
					.flex()
					.items_center()
					.gap_1()
					.h_full()
					.w(px(sidebar::WIDTH))
					.px_1p5()
					.border_r_1()
					.border_color(p.border)
					.child(icon_button(
						p,
						"bar-add",
						Icon::Plus,
						"Add URL",
						true,
						cx.listener(|this, _, window, cx| this.open_add(window, cx)),
					))
					.child(icon_button(
						p,
						"bar-pause",
						Icon::Pause,
						"Pause",
						can_pause,
						cx.listener(|this, _, _, cx| this.pause_selected(cx)),
					))
					.child(icon_button(
						p,
						"bar-resume",
						Icon::Play,
						"Resume",
						can_resume,
						cx.listener(|this, _, _, cx| this.resume_selected(cx)),
					))
					.child(icon_button(
						p,
						"bar-remove",
						Icon::Trash,
						"Remove",
						selected.is_some(),
						cx.listener(|this, _, _, cx| this.remove_selected(cx)),
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
					.child(div().whitespace_nowrap().child(summary))
					.when(speed > 0, |s| s.child(div().whitespace_nowrap().child(format_speed(speed))))
					.child(div().flex_1())
					.child(self.funnel(cx))
					.children(self.view_switch(cx))
					.child(icon_button(
						p,
						"settings",
						Icon::Settings,
						"Settings",
						true,
						cx.listener(|this, _, _, cx| this.toggle_settings(true, cx)),
					)),
			)
	}

	/// The funnel opens the menu; it keeps a background only while a status is chosen or the
	/// menu is open, since those are states, and brightens on hover like any other icon.
	fn funnel(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let lit = self.status.is_some();
		let open = self.filter_open;
		div()
			.id("filter")
			.role(Role::Button)
			.aria_label("Filter by status")
			.debug_selector(|| "button:Filter by status".to_owned())
			.flex()
			.items_center()
			.gap_1()
			.h_5()
			.px_1()
			.rounded_sm()
			.cursor_pointer()
			.text_color(if lit { p.text } else { p.muted })
			.when(open || lit, |s| s.bg(p.selection))
			.group("funnel")
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(move |this, _, _, cx| this.toggle_filter_menu(!open, cx)))
			.child(
				icon(Icon::Funnel, if lit { p.accent } else { p.muted })
					.size_3p5()
					.when(!lit, move |s| s.group_hover("funnel", move |s| s.text_color(p.text))),
			)
			.when_some(self.status, |s, status| s.child(status.label()))
	}

	/// The menu of statuses, a second cut inside whatever the sidebar selected, one at a time.
	/// It hangs off the window root, not the funnel: an anchored element inside a centred flex
	/// row is laid out off its own origin and lands that far from where it was told. Positioned
	/// in window space at the corner just above the status bar.
	pub(crate) fn filter_popover(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let rows: Vec<_> = std::iter::once(None)
			.chain(Status::ALL.into_iter().map(Some))
			.map(|status| {
				let label = status.map_or("All", Status::label);
				let count = self
					.downloads
					.iter()
					.filter(|d| {
						self.filter.matches(d, &self.categories) && status.is_none_or(|s| d.status == s)
					})
					.count();
				menu_row(
					p,
					SharedString::from(format!("chip:{label}")),
					status.map_or(Icon::List, Icon::for_status),
					label,
					count,
					self.status == status,
					cx.listener(move |this, _, _, cx| this.set_status(status, cx)),
				)
				.debug_selector(|| format!("chip:{label}"))
			})
			.collect();
		deferred(
			anchored()
				.position_mode(AnchoredPositionMode::Window)
				.anchor(Anchor::BottomRight)
				.position(point(self.viewport.width - px(8.0), self.viewport.height - px(HEIGHT + 4.0)))
				.snap_to_window_with_margin(px(8.0))
				.child(
					div()
						.id("filter-menu")
						.debug_selector(|| "menu".to_owned())
						.flex()
						.flex_col()
						.gap_px()
						.w(px(168.0))
						.p_1()
						.rounded_md()
						.border_1()
						.border_color(p.border)
						.bg(p.panel)
						.shadow_md()
						.on_mouse_down_out(cx.listener(|this, _, _, cx| this.toggle_filter_menu(false, cx)))
						.children(rows),
				),
		)
		.priority(1)
	}

	/// One segment per view, the active one lit because it stays chosen.
	fn view_switch(&self, cx: &mut Context<Self>) -> Vec<impl IntoElement + use<>> {
		let p = self.palette;
		View::ALL
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
					.group("view")
					.when(active, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx)))
					.child(
						icon(view_icon(view), color)
							.size_3p5()
							.when(!active, move |s| s.group_hover("view", move |s| s.text_color(p.text))),
					)
			})
			.collect()
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
