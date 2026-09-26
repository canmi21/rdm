//! Linux: the item on the session bus. StatusNotifierItem is what the desktops read now, and what
//! libappindicator stood in front of; speaking it directly is what keeps gtk3, and the glib
//! advisory behind it, out of the tree. The item answers on ksni's own thread, so it keeps its own
//! copy of the summary and frame, handed over whenever they change. See spec/framework.md.

use anyhow::{Context as _, Result};

use super::art::Artwork;
use super::{Action, Summary};
use crate::identity;

pub struct Indicator {
	icon: Vec<ksni::Icon>,
	summary: Summary,
}

fn pixmap(art: Artwork) -> ksni::Icon {
	ksni::Icon { width: art.width as i32, height: art.height as i32, data: art.argb32() }
}

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
		ksni::ToolTip {
			title: identity::DISPLAY_NAME.to_owned(),
			description: self.summary.tooltip.clone(),
			..Default::default()
		}
	}

	/// A left click, which shows the window as the menu's show item would.
	fn activate(&mut self, _x: i32, _y: i32) {
		press(Action::Show);
	}

	fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
		use ksni::menu::StandardItem;
		let summary = &self.summary;
		let item = |label: String, enabled: bool, action: Option<Action>| -> ksni::MenuItem<Self> {
			StandardItem {
				label,
				enabled,
				activate: Box::new(move |_| {
					if let Some(action) = action {
						press(action);
					}
				}),
				..Default::default()
			}
			.into()
		};
		let mut menu = vec![item(summary.status.clone(), false, None)];
		for row in &summary.rows {
			menu.push(item(row.label.clone(), true, Some(Action::Open(row.id))));
		}
		menu.extend([
			ksni::MenuItem::Separator,
			item(format!("Show {}", identity::DISPLAY_NAME), true, Some(Action::Show)),
			item("New Task…".to_owned(), true, Some(Action::NewTask)),
			ksni::MenuItem::Separator,
			item("Pause All".to_owned(), summary.can_pause, Some(Action::PauseAll)),
			item("Resume All".to_owned(), summary.can_resume, Some(Action::ResumeAll)),
			ksni::MenuItem::Separator,
			item("Rules…".to_owned(), true, Some(Action::Rules)),
		]);
		menu.push(if summary.syncing {
			item("Syncing Rules…".to_owned(), false, None)
		} else {
			item("Sync Rules Now".to_owned(), true, Some(Action::SyncRules))
		});
		if let Some(note) = &summary.sync_note {
			menu.push(item(note.clone(), false, None));
		}
		menu.extend([
			ksni::MenuItem::Separator,
			item("Settings…".to_owned(), true, Some(Action::Settings)),
			item("Check for Updates…".to_owned(), true, Some(Action::CheckUpdates)),
			ksni::MenuItem::Separator,
			item(format!("Quit {}", identity::DISPLAY_NAME), true, Some(Action::Quit)),
		]);
		menu
	}
}

/// Presses since the window last looked. The item answers on ksni's own thread, so what it hears
/// queues here and the window's tick takes it, which is the shape the other systems' crates hand
/// us through their global channels.
static PRESSES: std::sync::Mutex<Vec<Action>> = std::sync::Mutex::new(Vec::new());

fn press(action: Action) {
	PRESSES.lock().unwrap_or_else(|held| held.into_inner()).push(action);
}

pub struct System(ksni::blocking::Handle<Indicator>);

pub fn build(art: Artwork, summary: &Summary) -> Result<System> {
	use ksni::blocking::TrayMethods as _;
	let indicator = Indicator { icon: vec![pixmap(art)], summary: summary.clone() };
	// ksni runs the service on a thread of its own, so this returns with the item up.
	let handle = indicator.spawn().context("put the item on the session bus")?;
	Ok(System(handle))
}

impl System {
	pub fn set_icon(&mut self, art: Artwork) {
		let icon = pixmap(art);
		self.0.update(move |indicator| indicator.icon = vec![icon]);
	}

	pub fn set_summary(&mut self, summary: &Summary) {
		let summary = summary.clone();
		self.0.update(move |indicator| indicator.summary = summary);
	}
}

pub fn poll() -> Vec<Action> {
	std::mem::take(&mut *PRESSES.lock().unwrap_or_else(|held| held.into_inner()))
}
