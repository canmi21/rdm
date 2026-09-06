use gpui::{Context, IntoElement, Window, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Status;
use crate::ui::button;
use crate::ui::icon::Icon;

/// The strip the traffic lights share; main.rs derives their offset from it.
pub const HEIGHT: f32 = 36.0;

/// Two labelled buttons: Add Task, and the one thing the selection can do next. Everything else
/// is an icon in the status bar's corner. The strip is also the titlebar: on macOS the traffic
/// lights sit at its left, on Windows the three controls are drawn at its right and the rest of
/// it drags the window, on Linux the window manager keeps its own bar above. See spec/ui.md.
impl Rdm {
	pub(crate) fn render_toolbar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = self.palette;
		let next = self.selected().map(|d| match d.status {
			Status::Downloading => (Icon::Pause, "Pause"),
			// A cross up here; the trash can is the icon row's, below.
			Status::Completed => (Icon::X, "Remove"),
			Status::Paused | Status::Queued | Status::Failed => (Icon::Play, "Resume"),
		});
		let (glyph, label) = next.unwrap_or((Icon::Pause, "Pause"));
		div()
			.flex()
			.items_center()
			.gap_0p5()
			.h(px(HEIGHT))
			// The traffic lights sit in this strip because the system titlebar is transparent.
			.when(cfg!(target_os = "macos"), |s| s.pl(px(78.0)))
			.when(!cfg!(target_os = "macos"), |s| s.pl_1())
			.when(cfg!(target_os = "windows"), |s| s.window_control_area(gpui::WindowControlArea::Drag))
			.border_b_1()
			.border_color(p.border)
			.bg(p.panel)
			.child(button(
				p,
				"add",
				Icon::Plus,
				"Add Task",
				true,
				cx.listener(|this, _, window, cx| this.open_add(window, cx)),
			))
			.child(button(
				p,
				"next",
				glyph,
				label,
				next.is_some(),
				cx.listener(|this, _, _, cx| this.act_on_selected(cx)),
			))
			.child(div().flex_1())
			.when(cfg!(target_os = "windows"), |s| s.child(window_controls(p, window.is_maximized())))
	}
}

/// Windows draws no caption over a transparent titlebar, so the three controls are drawn here in
/// the system's own arrangement -- minimize, maximize or restore, close, each 46 wide, close
/// hovering red -- and marked as the window's control areas: the system does the hit-testing,
/// the pressing and the snap layouts, so they carry no click handlers of their own.
fn window_controls(p: crate::ui::theme::Palette, maximized: bool) -> impl IntoElement {
	use gpui::WindowControlArea;
	let control = move |id: &'static str, area: WindowControlArea, glyph: Icon, close: bool| {
		div()
			.id(id)
			.debug_selector(move || format!("window:{id}"))
			.flex()
			.items_center()
			.justify_center()
			.w(px(46.0))
			.h(px(HEIGHT))
			.window_control_area(area)
			.hover(move |s| if close { s.bg(p.failure).text_color(p.text) } else { s.bg(p.hover) })
			.child(crate::ui::icon::icon(glyph, p.muted).size_3p5())
	};
	div()
		.flex()
		.items_center()
		.h_full()
		.child(control("minimize", WindowControlArea::Min, Icon::Minus, false))
		.child(control(
			"maximize",
			WindowControlArea::Max,
			if maximized { Icon::Copy } else { Icon::Square },
			false,
		))
		.child(control("close", WindowControlArea::Close, Icon::X, true))
}
