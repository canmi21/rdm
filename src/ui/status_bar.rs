use std::time::Duration;

use gpui::{
	Anchor, AnchoredPositionMode, Animation, AnimationExt, Context, IntoElement, Role, SharedString,
	Transformation, anchored, deferred, div, percentage, point, prelude::*, px,
};

use crate::app::{Rdm, View};
use crate::download::{Status, format_speed};
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::theme::Tint;
use crate::ui::tooltip::tooltip;
use crate::ui::{icon_button, menu_row, sidebar};

/// The status bar's height, which the filter menu sits just above.
pub const HEIGHT: f32 = 24.0;

impl Rdm {
	/// What runs behind the window, after the summary: a spinner, and the first thing it is
	/// for, with every one of them in the tooltip. Nothing while nothing runs, so at rest the
	/// bar is the count alone. The spinner turns once a second for as long as it is drawn.
	fn activity(&self, _cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
		let p = self.palette;
		let activities = self.activities();
		let first = activities.first()?.clone();
		let all = activities.join(", ");
		Some(
			div()
				.id("activity")
				.debug_selector(|| "activity".to_owned())
				.flex()
				.items_center()
				.gap_1()
				.whitespace_nowrap()
				.tooltip(tooltip(all))
				.child(icon(Icon::Loader, p.muted).size_3().with_animation(
					"activity-spin",
					Animation::new(Duration::from_secs(1)).repeat(),
					|svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
				))
				.child(first),
		)
	}
}

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
						"Add Task",
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
					.children(self.activity(cx))
					.child(div().flex_1())
					.child(self.funnel(cx))
					.children(self.view_switch(cx))
					.child(icon_button(
						p,
						"settings",
						Icon::Settings,
						"Settings",
						true,
						cx.listener(|this, _, _, cx| this.open_settings(cx)),
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
			.tooltip(tooltip("Filter"))
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(move |this, _, _, cx| this.toggle_filter_menu(!open, cx)))
			.child(
				hover_icon(
					Icon::Funnel,
					"funnel",
					if lit { p.accent } else { p.muted },
					(!lit).then_some(p.text),
				)
				.size_3p5(),
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
					(
						status.map_or(Icon::List, Icon::for_status),
						status.map_or_else(|| p.hue(Tint::Snow.rgb()), |s| p.status(s)),
					),
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
					.tooltip(tooltip(format!("{view:?}")))
					.when(active, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| this.set_view(view, cx)))
					.child(hover_icon(view_icon(view), "view", color, (!active).then_some(p.text)).size_3p5())
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
