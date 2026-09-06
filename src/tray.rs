//! The icon in the system's tray -- the menu bar's right on macOS, the notification area on
//! Windows, the panel's indicator area on Linux -- with the system's own menu under it: show
//! the window, quit. The window closing still quits the application; the tray is a way back to
//! the window and a way out, and nothing more yet. See spec/ui.md.
//!
//! macOS and Windows create the icon on the main thread, whose loop is gpui's. Linux needs a
//! gtk loop, which gpui does not run, so the icon lives on a thread of its own that runs one;
//! the events come back through the crates' global channels either way, polled by the window's
//! tick. The icon is a PNG rendered from the artwork by `mise run icon`: on macOS the bare
//! glyph as a template image, which the system tints for the menu bar's light or dark; elsewhere
//! the full icon, since those trays draw an icon as it is.

use anyhow::{Context as _, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::identity;

/// What a press in the tray asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
	Show,
	Quit,
}

const SHOW: &str = "tray-show";
const QUIT: &str = "tray-quit";

/// The icon, kept for as long as the application runs: dropping it removes it from the tray.
pub struct Tray(#[allow(dead_code)] TrayIcon);

impl gpui::Global for Tray {}

/// Puts the icon in the tray. A tray that cannot be made -- no session bus, no indicator
/// library -- is reported and done without; the window is whole without it.
#[cfg(not(target_os = "linux"))]
pub fn install(cx: &mut gpui::App) {
	match build() {
		Ok(icon) => cx.set_global(Tray(icon)),
		Err(error) => eprintln!("no tray icon: {error:#}"),
	}
}

#[cfg(target_os = "linux")]
pub fn install(_cx: &mut gpui::App) {
	std::thread::Builder::new()
		.name("tray".to_owned())
		.spawn(|| {
			if let Err(error) = gtk::init() {
				eprintln!("no tray icon: gtk could not start: {error}");
				return;
			}
			match build() {
				Ok(_icon) => gtk::main(),
				Err(error) => eprintln!("no tray icon: {error:#}"),
			}
		})
		.expect("spawn the tray thread");
}

fn build() -> Result<TrayIcon> {
	let menu = Menu::new();
	menu.append_items(&[
		&MenuItem::with_id(SHOW, format!("Show {}", identity::DISPLAY_NAME), true, None),
		&PredefinedMenuItem::separator(),
		&MenuItem::with_id(QUIT, format!("Quit {}", identity::DISPLAY_NAME), true, None),
	])?;
	let (bytes, template): (&[u8], bool) = if cfg!(target_os = "macos") {
		(include_bytes!("../assets/tray/glyph-44.png"), true)
	} else {
		(include_bytes!("../assets/tray/icon-64.png"), false)
	};
	let icon = decode(bytes)?;
	TrayIconBuilder::new()
		.with_id(identity::NAME)
		.with_menu(Box::new(menu))
		.with_tooltip(identity::DISPLAY_NAME)
		.with_icon(icon)
		.with_icon_as_template(template)
		.build()
		.context("put the icon in the tray")
}

/// A PNG as the tray wants it: straight RGBA bytes.
fn decode(bytes: &[u8]) -> Result<tray_icon::Icon> {
	let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
	decoder.set_transformations(png::Transformations::normalize_to_color8());
	let mut reader = decoder.read_info().context("read the png")?;
	let mut buffer = vec![0; reader.output_buffer_size().context("a png with a size")?];
	let info = reader.next_frame(&mut buffer).context("decode the png")?;
	anyhow::ensure!(info.color_type == png::ColorType::Rgba, "the tray icon must be RGBA");
	buffer.truncate(info.buffer_size());
	Ok(tray_icon::Icon::from_rgba(buffer, info.width, info.height)?)
}

/// Every press since the last look: a menu item, or on Windows and Linux a left click on the
/// icon itself, which opens the window as the menu's first item would.
pub fn poll() -> Vec<Action> {
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn both_icons_decode_to_square_rgba() {
		let glyph = decode(include_bytes!("../assets/tray/glyph-44.png")).unwrap();
		let icon = decode(include_bytes!("../assets/tray/icon-64.png")).unwrap();
		let _ = (glyph, icon);
	}
}
