//! The tray's side of the main view: what it shows, worked out from the rows, and what its menu
//! asks for. See spec/ui.md, "An icon in the tray".

use gpui::{Context, Window};

use crate::app::Rdm;
use crate::download::{Status, format_speed};
use crate::tray::{Action, Look, Motion, Row, Summary};

/// How many downloads the menu names; the rest are counted in its first line.
const MOST_ROWS: usize = 5;
/// How much of a name a menu item keeps.
const NAME_CHARS: usize = 36;

impl Rdm {
	/// The tray's presses acted on, then what it should show handed to it. On the window's tick.
	pub(crate) fn pump_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		for action in crate::tray::poll() {
			self.act_for_tray(action, window, cx);
		}
		crate::tray::show(cx, self.tray_summary());
	}

	fn act_for_tray(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
		// What opens something in the main window brings it forward first, with every other window
		// of the application, as show does.
		if matches!(action, Action::Show | Action::NewTask | Action::Settings | Action::CheckUpdates) {
			cx.activate(true);
			for handle in cx.windows() {
				let _ = handle.update(cx, |_, window, _| window.activate_window());
			}
			window.activate_window();
		}
		match action {
			Action::Show => {}
			Action::NewTask => self.open_add(window, cx),
			Action::Open(id) => {
				cx.activate(true);
				self.open_download(id, cx);
			}
			Action::PauseAll => {
				let ids = self.ids_where(|s| matches!(s, Status::Downloading | Status::Queued));
				for id in ids {
					self.pause(id, cx);
				}
			}
			Action::ResumeAll => {
				for id in self.ids_where(|s| s == Status::Paused) {
					self.resume(id, cx);
				}
			}
			Action::Rules => {
				cx.activate(true);
				self.open_rules(cx);
			}
			Action::SyncRules => self.sync_rules(cx),
			Action::Settings => self.open_settings(cx),
			Action::CheckUpdates => self.check_for_updates(true, cx),
			Action::Quit => cx.quit(),
		}
	}

	fn ids_where(&self, wanted: impl Fn(Status) -> bool) -> Vec<u64> {
		self.downloads.iter().filter(|d| wanted(d.status)).map(|d| d.id).collect()
	}

	/// What the tray shows now: the look by what is most pressing -- a download moving, then the
	/// rules syncing, then something waiting -- and a dot while a failure has gone unseen.
	pub(crate) fn tray_summary(&self) -> Summary {
		let running: Vec<_> =
			self.downloads.iter().filter(|d| d.status == Status::Downloading).collect();
		let queued: Vec<_> = self.downloads.iter().filter(|d| d.status == Status::Queued).collect();
		let speed: u64 = running.iter().map(|d| d.speed).sum();
		let sync_failed = self.rules_sync.succeeded == Some(false) && !self.rules_sync.running;
		let motion = if !running.is_empty() {
			Motion::Downloading { progress: progress(&running) }
		} else if self.rules_sync.running {
			Motion::Syncing
		} else if !queued.is_empty() {
			Motion::Queued
		} else {
			Motion::Idle
		};
		let look = Look { motion, alert: sync_failed || self.unseen_failure };
		let status = match (running.len(), queued.len()) {
			(0, 0) => "Nothing downloading".to_owned(),
			(0, waiting) => format!("{waiting} waiting in the queue"),
			(moving, 0) => format!("Downloading {moving} at {}", format_speed(speed)),
			(moving, waiting) => {
				format!("Downloading {moving} at {}, {waiting} waiting", format_speed(speed))
			}
		};
		let rows = running
			.iter()
			.chain(&queued)
			.take(MOST_ROWS)
			.map(|d| {
				let progress = match d.status {
					Status::Queued => "waiting".to_owned(),
					_ if d.size > 0 => format!("{}%", d.received * 100 / d.size),
					_ => format_speed(d.speed),
				};
				Row { id: d.id, label: format!("{} — {progress}", shortened(&d.name)) }
			})
			.collect();
		let mut tooltip = crate::identity::DISPLAY_NAME.to_owned();
		if !running.is_empty() || !queued.is_empty() {
			tooltip = format!("{tooltip}: {}", status.to_lowercase());
		} else if self.rules_sync.running {
			tooltip = format!("{tooltip}: syncing the rules");
		}
		if self.unseen_failure {
			tooltip.push_str("\nA download failed");
		}
		if sync_failed {
			tooltip.push_str("\nThe last rules sync failed");
		}
		Summary {
			look,
			tooltip,
			status,
			rows,
			can_pause: !running.is_empty() || !queued.is_empty(),
			can_resume: self.downloads.iter().any(|d| d.status == Status::Paused),
			syncing: self.rules_sync.running,
			sync_note: if sync_failed { self.rules_sync.status.clone() } else { None },
		}
	}
}

/// Received over size across the running downloads, when every one of them knows its size; in
/// sixty-fourths, which is finer than the base line has pixels, so a byte arriving does not
/// redraw the icon.
fn progress(running: &[&crate::download::Download]) -> Option<f32> {
	if running.iter().any(|d| d.size == 0) {
		return None;
	}
	let size: u64 = running.iter().map(|d| d.size).sum();
	let received: u64 = running.iter().map(|d| d.received.min(d.size)).sum();
	Some((received as f64 / size as f64 * 64.0).floor() as f32 / 64.0)
}

fn shortened(name: &str) -> String {
	if name.chars().count() <= NAME_CHARS {
		return name.to_owned();
	}
	let kept: String = name.chars().take(NAME_CHARS - 1).collect();
	format!("{kept}…")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn progress_is_known_only_when_every_size_is() {
		let mut rows = crate::download::sample();
		rows.truncate(2);
		rows[0].size = 100;
		rows[0].received = 50;
		rows[1].size = 300;
		rows[1].received = 50;
		let running: Vec<_> = rows.iter().collect();
		assert_eq!(progress(&running), Some(0.25));
		rows[1].size = 0;
		let running: Vec<_> = rows.iter().collect();
		assert_eq!(progress(&running), None);
	}

	#[test]
	fn a_long_name_is_cut_with_an_ellipsis() {
		assert_eq!(shortened("short.iso"), "short.iso");
		let long = "a".repeat(50);
		assert_eq!(shortened(&long).chars().count(), NAME_CHARS);
		assert!(shortened(&long).ends_with('…'));
	}
}
