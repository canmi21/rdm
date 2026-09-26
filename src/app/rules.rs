//! The rules window's side of the main view: opening it, and what its buttons change. Everything
//! it writes goes to the custom layer, the one layer that is the user's. See spec/rules.md.

use gpui::{AppContext, Context, px, size};

use crate::app::Rdm;
use crate::ui::rules_window::RulesWindow;

impl Rdm {
	/// Opens the rules window, or brings it forward when it is already open.
	pub(crate) fn open_rules(&mut self, cx: &mut Context<Self>) {
		if let Some(handle) = &self.rules_window
			&& handle.update(cx, |_, window, _| window.activate_window()).is_ok()
		{
			return;
		}
		// Deferred for the reason `open_download` gives: the new window's first frame reads this.
		let rdm = cx.entity();
		cx.defer(move |cx| {
			let options = super::child_window(cx, "Rules", size(px(640.0), px(440.0)));
			let view = rdm.clone();
			if let Ok(handle) = cx.open_window(options, |window, cx| {
				#[cfg(target_os = "linux")]
				window.request_decorations(gpui::WindowDecorations::Client);
				#[cfg(not(target_os = "linux"))]
				let _ = window;
				cx.new(|cx| RulesWindow::new(view, cx))
			}) {
				rdm.update(cx, |this, _| this.rules_window = Some(handle));
			}
		});
	}

	/// Every layer read again from disk and merged: for a file edited by hand.
	pub(crate) fn reload_rules(&mut self, cx: &mut Context<Self>) {
		if let Some(paths) = &self.paths {
			self.rules = std::sync::Arc::new(crate::rules::load(&paths.rule_places()));
		}
		cx.notify();
	}

	/// Moves a rule one place up or down among the rules of its kind. Every rule of the kind is
	/// given a priority in the custom layer's order, ten apart and in the order shown after the
	/// move: rules of equal priority leave no room to put one between two others, and one given
	/// just past its neighbour's jumped past every rule that shared it. A rule that arrives later
	/// at the default priority lands below the ordered ones until it is moved. See spec/rules.md.
	pub(crate) fn move_rule(&mut self, id: &str, family: bool, up: bool, cx: &mut Context<Self>) {
		let mut ids: Vec<String> = if family {
			self.rules.families.iter().map(|f| f.id.clone()).collect()
		} else {
			self.rules.entries.iter().map(|e| e.id.clone()).collect()
		};
		let Some(at) = ids.iter().position(|other| other == id) else { return };
		let Some(to) = (if up { at.checked_sub(1) } else { Some(at + 1).filter(|n| *n < ids.len()) })
		else {
			return;
		};
		ids.swap(at, to);
		let count = ids.len() as i32;
		let priorities: Vec<(String, i32)> =
			ids.into_iter().enumerate().map(|(place, id)| (id, (count - place as i32) * 10)).collect();
		let Some(paths) = &self.paths else { return };
		if let Err(error) = crate::rules::set_priorities(&paths.custom_rules, &priorities) {
			eprintln!("could not move rule {id}: {error}");
		}
		self.reload_rules(cx);
	}

	/// Forgets what was chosen for a domain in New Task, so a download from it is asked again.
	pub(crate) fn forget_choice(&mut self, host: &str, cx: &mut Context<Self>) {
		let Some(paths) = &self.paths else { return };
		if let Err(error) = crate::rules::forget(&paths.custom_rules, host) {
			eprintln!("could not forget the choice for {host}: {error}");
		}
		self.reload_rules(cx);
	}

	/// Opens the custom layer's folder in the system's file manager, made first if it is not there,
	/// for writing rules by hand.
	pub(crate) fn open_custom_rules(&self) {
		if let Some(paths) = &self.paths {
			let _ = std::fs::create_dir_all(&paths.custom_rules);
			crate::reveal::open(&paths.custom_rules);
		}
	}
}
