//! The displays, as a window needs to know them: a name each one keeps across a replug, and
//! where each one sits on the desktop.
//!
//! GPUI has both and gives back only one of them here. Its macOS display reads `CGDisplayBounds`,
//! whose rectangle is in the desktop's own coordinates -- the same ones a window's frame is in,
//! with the primary display's top left at zero -- and then returns it with the origin thrown
//! away. Every display comes back at `(0, 0)`, which says how big each screen is and nothing at
//! all about where it is or which one a window is over. So the origin is asked for again, of the
//! system this time, and only the size is taken from GPUI.
//!
//! Elsewhere GPUI's own bounds are used as they are. See spec/state.md.

use gpui::App;

use crate::state::{Frame, Screen};

/// Every display there is, in no particular order. A display whose name the system will not give
/// is left out: it cannot be recognised at the next launch, which is the only thing the name is
/// for, and the frame alone still reaches the fallback in `State::frame_on`.
pub fn all(cx: &App) -> Vec<Screen> {
	cx.displays()
		.into_iter()
		.filter_map(|display| {
			let uuid = display.uuid().ok()?.to_string();
			Some(Screen { uuid, frame: where_it_sits(&display) })
		})
		.collect()
}

#[cfg(target_os = "macos")]
fn where_it_sits(display: &std::rc::Rc<dyn gpui::PlatformDisplay>) -> Frame {
	// The same call GPUI makes, kept whole. A `DisplayId` on macOS is the `CGDirectDisplayID` it
	// was made from, which is what this takes.
	let id: u64 = display.id().into();
	let bounds = objc2_core_graphics::CGDisplayBounds(id as u32);
	Frame {
		x: bounds.origin.x as f32,
		y: bounds.origin.y as f32,
		width: bounds.size.width as f32,
		height: bounds.size.height as f32,
	}
}

#[cfg(not(target_os = "macos"))]
fn where_it_sits(display: &std::rc::Rc<dyn gpui::PlatformDisplay>) -> Frame {
	let bounds = display.bounds();
	Frame {
		x: bounds.origin.x.into(),
		y: bounds.origin.y.into(),
		width: bounds.size.width.into(),
		height: bounds.size.height.into(),
	}
}
