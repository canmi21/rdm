//! The displays, as a window needs to know them: a name each one keeps across a replug, and how
//! big it is now.
//!
//! Where a display sits on the desktop is deliberately not among them. A window's frame, as GPUI
//! reports it and as GPUI takes it back, is in the coordinates of the display the window is on --
//! that display's top left is its zero, whichever display it is -- so the desktop's own
//! coordinates never come into it, and the same numbers mean a different place on every screen.
//! What has to go beside them is which display they belong to, and that is asked of the window
//! rather than worked out from where it is. See spec/state.md.

use gpui::{App, DisplayId};

use crate::state::Screen;

/// Every display there is, in no particular order. A display whose name the system will not give
/// is left out: it cannot be recognised at the next launch, which is the only thing the name is
/// for.
pub fn all(cx: &App) -> Vec<Screen> {
	cx.displays()
		.into_iter()
		.filter_map(|display| {
			let uuid = display.uuid().ok()?.to_string();
			let size = display.bounds().size;
			Some(Screen { uuid, width: size.width.into(), height: size.height.into() })
		})
		.collect()
}

/// The display GPUI knows by that name, which is what a window is opened on. Read at launch, from
/// the name in state.json.
pub fn id_of(cx: &App, uuid: &str) -> Option<DisplayId> {
	cx.displays()
		.into_iter()
		.find(|display| display.uuid().is_ok_and(|found| found.to_string() == uuid))
		.map(|display| display.id())
}
