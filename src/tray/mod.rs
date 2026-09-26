//! The icon in the system's tray -- the menu bar's right on macOS, the notification area on
//! Windows, the panel's indicator area on Linux -- with the system's own menu under it. The icon
//! says what the application is doing behind its window, moving while something downloads or the
//! rules sync; the menu says it in words and reaches the windows and the common actions. The window
//! closing still quits the application. See spec/ui.md, "An icon in the tray".
//!
//! macOS and Windows are `tray-icon`, which creates the icon on the main thread, whose loop is
//! gpui's, and reports presses through its crates' global channels. Linux is `ksni`, which puts
//! a StatusNotifierItem on the session bus and runs the service on a thread of its own; its menu
//! answers on that thread, so the presses queue here instead. Either way the window's tick drains
//! them and hands back a `Summary` of what to show. See spec/framework.md for why Linux is not
//! `tray-icon` too.
//!
//! The frames are drawn as they are needed, from the glyph's own geometry; see art.rs.

mod art;
#[cfg(target_os = "linux")]
mod indicator;
#[cfg(not(target_os = "linux"))]
mod menu_bar;

use std::time::Duration;

use gpui::BorrowAppContext as _;

pub use art::{Look, Motion};

#[cfg(target_os = "linux")]
use indicator as system;
#[cfg(not(target_os = "linux"))]
use menu_bar as system;

/// What a press in the tray asks of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
	Show,
	NewTask,
	/// One download's own window.
	Open(u64),
	PauseAll,
	ResumeAll,
	Rules,
	SyncRules,
	Settings,
	CheckUpdates,
	Quit,
}

/// A download the menu names, with where it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
	pub id: u64,
	pub label: String,
}

/// Everything the tray shows, worked out by the application from its rows; the tray only draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
	pub look: Look,
	pub tooltip: String,
	/// The menu's first line, which is not a button.
	pub status: String,
	pub rows: Vec<Row>,
	pub can_pause: bool,
	pub can_resume: bool,
	pub syncing: bool,
	/// Under the sync item, when the last sync failed.
	pub sync_note: Option<String>,
}

impl Default for Summary {
	fn default() -> Summary {
		Summary {
			look: Look::IDLE,
			tooltip: crate::identity::DISPLAY_NAME.to_owned(),
			status: "Nothing downloading".to_owned(),
			rows: Vec::new(),
			can_pause: false,
			can_resume: false,
			syncing: false,
			sync_note: None,
		}
	}
}

/// How long a frame of a moving look stays up, and how often a still one is looked at again.
const FRAME: Duration = Duration::from_millis(83);
const STILL: Duration = Duration::from_millis(500);

/// What holds the icon up, and what it last showed: dropping it takes the icon out of the tray,
/// so it is kept for as long as the application runs.
pub struct Tray {
	system: system::System,
	summary: Summary,
	phase: u32,
	/// The look and frame on screen, so a still look is drawn once.
	drawn: Option<(Look, u32)>,
}

impl gpui::Global for Tray {}

impl Tray {
	/// The next frame of the look, drawn when it differs from the one up.
	fn advance(&mut self) {
		let look = self.summary.look;
		let frames = look.frames();
		if frames > 1 {
			self.phase = self.phase.wrapping_add(1);
		}
		let key = (look, self.phase % frames);
		if self.drawn == Some(key) {
			return;
		}
		match art::frame(look, key.1, art::Style::native()) {
			Ok(art) => self.system.set_icon(art),
			Err(error) => eprintln!("could not draw the tray icon: {error:#}"),
		}
		self.drawn = Some(key);
	}
}

/// Puts the icon in the tray and starts its frames. A tray that cannot be made -- no session bus,
/// no host for the item on this desktop -- is reported and done without; the window is whole
/// without it.
pub fn install(cx: &mut gpui::App) {
	let summary = Summary::default();
	let made = art::frame(summary.look, 0, art::Style::native())
		.and_then(|art| system::build(art, &summary));
	match made {
		Ok(system) => {
			cx.set_global(Tray { system, summary, phase: 0, drawn: Some((Look::IDLE, 0)) });
		}
		Err(error) => {
			eprintln!("no tray icon: {error:#}");
			return;
		}
	}
	cx.spawn(async move |cx| {
		loop {
			let moving = cx.update(|cx| {
				cx.update_global::<Tray, _>(|tray, _| {
					tray.advance();
					tray.summary.look.frames() > 1
				})
			});
			cx.background_executor().timer(if moving { FRAME } else { STILL }).await;
		}
	})
	.detach();
}

/// Shows what the application is doing, when it differs from what is up.
pub fn show(cx: &mut gpui::App, summary: Summary) {
	if !cx.has_global::<Tray>() {
		return;
	}
	cx.update_global::<Tray, _>(|tray, _| {
		if tray.summary == summary {
			return;
		}
		tray.system.set_summary(&summary);
		let look_changed = tray.summary.look != summary.look;
		tray.summary = summary;
		if look_changed {
			tray.advance();
		}
	});
}

/// Every press since the last look.
pub fn poll() -> Vec<Action> {
	system::poll()
}
