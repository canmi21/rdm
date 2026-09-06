//! The icon in the system's tray -- the menu bar's right on macOS, the notification area on
//! Windows, the panel's indicator area on Linux -- with the system's own menu under it: show
//! the window, quit. The window closing still quits the application; the tray is a way back to
//! the window and a way out, and nothing more yet. See spec/ui.md.
//!
//! macOS and Windows are `tray-icon`, which creates the icon on the main thread, whose loop is
//! gpui's, and reports presses through its crates' global channels. Linux is `ksni`, which puts
//! a StatusNotifierItem on the session bus and runs the service on a thread of its own; its menu
//! answers on that thread, so the presses queue here instead. Either way the window's tick drains
//! them. See spec/framework.md for why Linux is not `tray-icon` too.
//!
//! The icon is a PNG rendered from the artwork by `mise run icon`: on macOS the bare glyph as a
//! template image, which the system tints for the menu bar's light or dark; elsewhere the full
//! icon, since those trays draw an icon as it is.

use anyhow::{Context as _, Result};

// The systems' modules take what they need; only the Linux item names the application up here.
#[cfg(target_os = "linux")]
use crate::identity;

/// What a press in the tray asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
	Show,
	Quit,
}

/// The tray's artwork decoded: straight RGBA bytes and the square they make.
struct Artwork {
	width: u32,
	height: u32,
	rgba: Vec<u8>,
}

impl Artwork {
	/// The same pixels as ARGB32 in network byte order, which is what a StatusNotifierItem
	/// carries. Only Linux draws from it, but the test runs everywhere on purpose: a byte order
	/// put back to front is invisible until somebody opens the one desktop that reads it.
	#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
	fn argb32(&self) -> Vec<u8> {
		let mut data = self.rgba.clone();
		for pixel in data.as_chunks_mut::<4>().0 {
			pixel.rotate_right(1);
		}
		data
	}
}

/// The artwork this system's tray wants. macOS takes the bare glyph, which it tints itself.
fn artwork() -> Result<Artwork> {
	let bytes: &[u8] = if cfg!(target_os = "macos") {
		include_bytes!("../assets/tray/glyph-44.png")
	} else {
		include_bytes!("../assets/tray/icon-64.png")
	};
	decode(bytes)
}

/// A PNG as every tray wants it underneath: straight RGBA bytes.
fn decode(bytes: &[u8]) -> Result<Artwork> {
	let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
	decoder.set_transformations(png::Transformations::normalize_to_color8());
	let mut reader = decoder.read_info().context("read the png")?;
	let mut buffer = vec![0; reader.output_buffer_size().context("a png with a size")?];
	let info = reader.next_frame(&mut buffer).context("decode the png")?;
	anyhow::ensure!(info.color_type == png::ColorType::Rgba, "the tray icon must be RGBA");
	buffer.truncate(info.buffer_size());
	Ok(Artwork { width: info.width, height: info.height, rgba: buffer })
}

/// What holds the icon up: dropping it takes the icon out of the tray, so it is kept for as
/// long as the application runs.
#[cfg(not(target_os = "linux"))]
pub struct Tray(#[allow(dead_code)] tray_icon::TrayIcon);

#[cfg(target_os = "linux")]
pub struct Tray(#[allow(dead_code)] ksni::blocking::Handle<Indicator>);

impl gpui::Global for Tray {}

/// Puts the icon in the tray. A tray that cannot be made -- no session bus, no host for the
/// item on this desktop -- is reported and done without; the window is whole without it.
pub fn install(cx: &mut gpui::App) {
	match build() {
		Ok(tray) => cx.set_global(tray),
		Err(error) => eprintln!("no tray icon: {error:#}"),
	}
}

#[cfg(not(target_os = "linux"))]
mod system {
	use anyhow::{Context as _, Result};
	use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
	use tray_icon::{TrayIconBuilder, TrayIconEvent};

	use super::{Action, Tray, artwork};
	use crate::identity;

	const SHOW: &str = "tray-show";
	const QUIT: &str = "tray-quit";

	pub(super) fn build() -> Result<Tray> {
		let menu = Menu::new();
		menu.append_items(&[
			&MenuItem::with_id(SHOW, format!("Show {}", identity::DISPLAY_NAME), true, None),
			&PredefinedMenuItem::separator(),
			&MenuItem::with_id(QUIT, format!("Quit {}", identity::DISPLAY_NAME), true, None),
		])?;
		let art = artwork()?;
		let icon = tray_icon::Icon::from_rgba(art.rgba, art.width, art.height)?;
		let tray = TrayIconBuilder::new()
			.with_id(identity::NAME)
			.with_menu(Box::new(menu))
			.with_tooltip(identity::DISPLAY_NAME)
			.with_icon(icon)
			.with_icon_as_template(cfg!(target_os = "macos"))
			.build()
			.context("put the icon in the tray")?;
		Ok(Tray(tray))
	}

	/// Every press since the last look: a menu item, or on Windows a left click on the icon
	/// itself, which opens the window as the menu's first item would. On macOS a click opens the
	/// menu, which is how the menu bar works.
	pub(super) fn poll() -> Vec<Action> {
		let mut actions = Vec::new();
		while let Ok(event) = MenuEvent::receiver().try_recv() {
			match event.id.0.as_str() {
				SHOW => actions.push(Action::Show),
				QUIT => actions.push(Action::Quit),
				_ => {}
			}
		}
		while let Ok(event) = TrayIconEvent::receiver().try_recv() {
			if let TrayIconEvent::Click {
				button: tray_icon::MouseButton::Left,
				button_state: tray_icon::MouseButtonState::Up,
				..
			} = event
				&& !cfg!(target_os = "macos")
			{
				actions.push(Action::Show);
			}
		}
		actions
	}
}

/// The item on the session bus. StatusNotifierItem is what the desktops read now, and what
/// libappindicator stood in front of; speaking it directly is what keeps gtk3, and the glib
/// advisory behind it, out of the tree. See spec/framework.md.
#[cfg(target_os = "linux")]
pub struct Indicator {
	icon: Vec<ksni::Icon>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for Indicator {
	fn id(&self) -> String {
		identity::NAME.to_owned()
	}

	fn title(&self) -> String {
		identity::DISPLAY_NAME.to_owned()
	}

	/// The application's own pixels rather than a name from the icon theme, which would only
	/// find something once the application is installed and named to the theme's liking.
	fn icon_pixmap(&self) -> Vec<ksni::Icon> {
		self.icon.clone()
	}

	fn tool_tip(&self) -> ksni::ToolTip {
		ksni::ToolTip { title: identity::DISPLAY_NAME.to_owned(), ..Default::default() }
	}

	/// A left click, which shows the window as the menu's first item would.
	fn activate(&mut self, _x: i32, _y: i32) {
		system::press(Action::Show);
	}

	fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
		use ksni::menu::StandardItem;
		vec![
			StandardItem {
				label: format!("Show {}", identity::DISPLAY_NAME),
				activate: Box::new(|_| system::press(Action::Show)),
				..Default::default()
			}
			.into(),
			ksni::MenuItem::Separator,
			StandardItem {
				label: format!("Quit {}", identity::DISPLAY_NAME),
				activate: Box::new(|_| system::press(Action::Quit)),
				..Default::default()
			}
			.into(),
		]
	}
}

#[cfg(target_os = "linux")]
mod system {
	use anyhow::{Context as _, Result};

	use super::{Action, Indicator, Tray, artwork};

	/// Presses since the window last looked. The item answers on ksni's own thread, so what it
	/// hears queues here and the window's tick takes it, which is the shape the other systems'
	/// crates hand us through their global channels.
	static PRESSES: std::sync::Mutex<Vec<Action>> = std::sync::Mutex::new(Vec::new());

	pub(super) fn press(action: Action) {
		PRESSES.lock().unwrap_or_else(|held| held.into_inner()).push(action);
	}

	pub(super) fn build() -> Result<Tray> {
		use ksni::blocking::TrayMethods as _;
		let art = artwork()?;
		let icon =
			ksni::Icon { width: art.width as i32, height: art.height as i32, data: art.argb32() };
		let indicator = Indicator { icon: vec![icon] };
		// ksni runs the service on a thread of its own, so this returns with the item up.
		let handle = indicator.spawn().context("put the item on the session bus")?;
		Ok(Tray(handle))
	}

	pub(super) fn poll() -> Vec<Action> {
		std::mem::take(&mut *PRESSES.lock().unwrap_or_else(|held| held.into_inner()))
	}
}

use system::build;

/// Every press since the last look.
pub fn poll() -> Vec<Action> {
	system::poll()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn both_icons_decode_to_square_rgba() {
		for bytes in
			[&include_bytes!("../assets/tray/glyph-44.png")[..], include_bytes!("../assets/tray/icon-64.png")]
		{
			let art = decode(bytes).unwrap();
			assert_eq!(art.width, art.height, "the tray's artwork is square");
			assert_eq!(art.rgba.len(), (art.width * art.height * 4) as usize, "four bytes a pixel");
		}
	}

	/// The one conversion nothing on this machine draws, so nothing on this machine would catch
	/// it: a StatusNotifierItem's pixmap is ARGB32 in network byte order, and the decoder hands
	/// us RGBA.
	#[test]
	fn the_indicators_pixels_are_argb_and_the_decoders_are_rgba() {
		let art = Artwork { width: 1, height: 1, rgba: vec![0x11, 0x22, 0x33, 0xff] };
		assert_eq!(art.argb32(), vec![0xff, 0x11, 0x22, 0x33], "alpha leads, then red, green, blue");
	}
}
