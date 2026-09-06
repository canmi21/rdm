use gpui::{Context, IntoElement, Window, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::Status;
use crate::ui::button;
use crate::ui::frame;
use crate::ui::icon::Icon;

/// The strip the traffic lights share; main.rs derives their offset from it.
pub const HEIGHT: f32 = 36.0;

/// Two labelled buttons: Add Task, and the one thing the selection can do next. Everything else
/// is an icon in the status bar's corner. The strip is also the titlebar: on macOS the traffic
/// lights sit at its left, until the window is full screen and there are none; where the
/// system draws no frame, the frame's controls sit at its right and its middle drags the
/// window. See src/ui/frame.rs and spec/ui.md.
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
		let lights = cfg!(target_os = "macos") && !window.is_fullscreen();
		let framed = frame::draws_frame(window);
		div()
			.flex()
			.items_center()
			.gap_0p5()
			.h(px(HEIGHT))
			// The traffic lights sit in this strip because the system titlebar is transparent.
			.when(lights, |s| s.pl(px(78.0)))
			.when(!lights, |s| s.pl_1())
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
			.child(frame::drag_area())
			.when(framed, |s| s.child(frame::controls(p, window)))
	}
}
