//! macOS and Windows: `tray-icon`, with `muda`'s menu under it. The menu is built again only when
//! its shape changes -- another download named, a sync note come or gone -- and otherwise has its
//! words and switches set in place, so a menu held open follows along. See spec/ui.md.

use anyhow::{Context as _, Result};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use super::art::Artwork;
use super::{Action, Summary};
use crate::identity;

const SHOW: &str = "tray-show";
const NEW: &str = "tray-new";
const PAUSE: &str = "tray-pause";
const RESUME: &str = "tray-resume";
const RULES: &str = "tray-rules";
const SYNC: &str = "tray-sync";
const SETTINGS: &str = "tray-settings";
const UPDATES: &str = "tray-updates";
const QUIT: &str = "tray-quit";
/// A download's item is this and its id.
const OPEN: &str = "tray-open-";

pub struct System {
	icon: TrayIcon,
	menu: Option<Built>,
}

/// The items whose words change, kept to be set in place.
struct Built {
	shape: (Vec<u64>, bool),
	status: MenuItem,
	rows: Vec<MenuItem>,
	pause: MenuItem,
	resume: MenuItem,
	sync: MenuItem,
	note: Option<MenuItem>,
}

impl Built {
	fn new(summary: &Summary) -> Result<(Built, Menu)> {
		let menu = Menu::new();
		let status = MenuItem::new(&summary.status, false, None);
		let rows: Vec<MenuItem> = summary
			.rows
			.iter()
			.map(|row| MenuItem::with_id(format!("{OPEN}{}", row.id), &row.label, true, None))
			.collect();
		let pause = MenuItem::with_id(PAUSE, "Pause All", summary.can_pause, None);
		let resume = MenuItem::with_id(RESUME, "Resume All", summary.can_resume, None);
		let sync = MenuItem::with_id(SYNC, sync_label(summary), !summary.syncing, None);
		let note = summary.sync_note.as_ref().map(|note| MenuItem::new(note, false, None));
		let separator = PredefinedMenuItem::separator;
		menu.append(&status)?;
		for row in &rows {
			menu.append(row)?;
		}
		menu.append_items(&[
			&separator(),
			&MenuItem::with_id(SHOW, format!("Show {}", identity::DISPLAY_NAME), true, None),
			&MenuItem::with_id(NEW, "New Task…", true, None),
			&separator(),
			&pause,
			&resume,
			&separator(),
			&MenuItem::with_id(RULES, "Rules…", true, None),
			&sync,
		])?;
		if let Some(note) = &note {
			menu.append(note)?;
		}
		menu.append_items(&[
			&separator(),
			&MenuItem::with_id(SETTINGS, "Settings…", true, None),
			&MenuItem::with_id(UPDATES, "Check for Updates…", true, None),
			&separator(),
			&MenuItem::with_id(QUIT, format!("Quit {}", identity::DISPLAY_NAME), true, None),
		])?;
		let built = Built { shape: shape(summary), status, rows, pause, resume, sync, note };
		Ok((built, menu))
	}

	fn fill(&self, summary: &Summary) {
		self.status.set_text(&summary.status);
		for (item, row) in self.rows.iter().zip(&summary.rows) {
			item.set_text(&row.label);
		}
		self.pause.set_enabled(summary.can_pause);
		self.resume.set_enabled(summary.can_resume);
		self.sync.set_text(sync_label(summary));
		self.sync.set_enabled(!summary.syncing);
		if let (Some(item), Some(note)) = (&self.note, &summary.sync_note) {
			item.set_text(note);
		}
	}
}

fn shape(summary: &Summary) -> (Vec<u64>, bool) {
	(summary.rows.iter().map(|row| row.id).collect(), summary.sync_note.is_some())
}

fn sync_label(summary: &Summary) -> &'static str {
	if summary.syncing { "Syncing Rules…" } else { "Sync Rules Now" }
}

pub fn build(art: Artwork, summary: &Summary) -> Result<System> {
	let (built, menu) = Built::new(summary)?;
	let icon = tray_icon::Icon::from_rgba(art.rgba, art.width, art.height)?;
	let tray = TrayIconBuilder::new()
		.with_id(identity::NAME)
		.with_menu(Box::new(menu))
		.with_tooltip(&summary.tooltip)
		.with_icon(icon)
		.with_icon_as_template(cfg!(target_os = "macos"))
		.build()
		.context("put the icon in the tray")?;
	Ok(System { icon: tray, menu: Some(built) })
}

impl System {
	/// On macOS through the call that names the template every time: tray-icon's plain `set_icon`
	/// sets the image as not a template there, which draws the black frame black on a dark menu bar.
	/// The other call does nothing elsewhere.
	pub fn set_icon(&mut self, art: Artwork) {
		if let Ok(icon) = tray_icon::Icon::from_rgba(art.rgba, art.width, art.height) {
			#[cfg(target_os = "macos")]
			let _ = self.icon.set_icon_with_as_template(Some(icon), true);
			#[cfg(not(target_os = "macos"))]
			let _ = self.icon.set_icon(Some(icon));
		}
	}

	pub fn set_summary(&mut self, summary: &Summary) {
		let _ = self.icon.set_tooltip(Some(&summary.tooltip));
		match &self.menu {
			Some(built) if built.shape == shape(summary) => built.fill(summary),
			_ => match Built::new(summary) {
				Ok((built, menu)) => {
					self.icon.set_menu(Some(Box::new(menu)));
					self.menu = Some(built);
				}
				Err(error) => eprintln!("could not build the tray menu: {error:#}"),
			},
		}
	}
}

/// Every press since the last look: a menu item, or on Windows a left click on the icon itself,
/// which opens the window as the menu's show item would. On macOS a click opens the menu, which
/// is how the menu bar works.
pub fn poll() -> Vec<Action> {
	let mut actions = Vec::new();
	while let Ok(event) = MenuEvent::receiver().try_recv() {
		let id = event.id.0.as_str();
		actions.push(match id {
			SHOW => Action::Show,
			NEW => Action::NewTask,
			PAUSE => Action::PauseAll,
			RESUME => Action::ResumeAll,
			RULES => Action::Rules,
			SYNC => Action::SyncRules,
			SETTINGS => Action::Settings,
			UPDATES => Action::CheckUpdates,
			QUIT => Action::Quit,
			_ => match id.strip_prefix(OPEN).and_then(|n| n.parse().ok()) {
				Some(download) => Action::Open(download),
				None => continue,
			},
		});
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
