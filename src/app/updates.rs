//! The window's side of the update check: a check at launch and every few minutes after,
//! a card in the corner when a newer build is published, and a system notification when the
//! window is not the one in front. The check itself is `src/update.rs`. See spec/release.md.

use std::sync::mpsc;
use std::time::Duration;

use gpui::{Context, IntoElement, Role, Task, Window, div, prelude::*, px};

use crate::app::Rdm;
use crate::identity;
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button;
use crate::ui::status_bar;
use crate::update::{self, Available, Manifest, Region};

/// What the check knows and what it last said.
#[derive(Default)]
pub struct Updates {
	/// Asked of the traces once per run; asked again only if no trace answered.
	pub region: Option<Region>,
	pub checking: bool,
	/// Whether the check under way was asked for, whose answer is shown whatever it is.
	by_hand: bool,
	/// The last manifest read, and what it means for this binary.
	pub latest: Option<Manifest>,
	pub available: Option<Available>,
	/// What the last check came to, for the settings row: the build it found, or why not.
	pub outcome: Option<Result<u64, String>>,
	/// The build the card was closed on; it stays closed until a newer one.
	pub dismissed: Option<u64>,
	/// The build the system was told about; it is told once per build.
	pub notified: Option<u64>,
	/// Whether the window is the one in front, kept by the activation observer.
	pub active: bool,
	/// The read of the check under way, polled below.
	_poll: Option<Task<()>>,
}

/// One outcome of a check as the runtime hands it back.
struct Answer {
	region: Region,
	result: Result<Manifest, String>,
}

impl Rdm {
	/// Starts the loop: a check now, then one every `update::EVERY` for as long as the window
	/// lives. Returned so the caller keeps the task alive.
	pub(crate) fn start_update_checks(
		&mut self,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Task<()> {
		self.updates.active = window.is_window_active();
		cx.observe_window_activation(window, |this, window, _| {
			this.updates.active = window.is_window_active();
		})
		.detach();
		cx.spawn(async move |this, cx| {
			loop {
				if this.update(cx, |this, cx| this.check_for_updates(false, cx)).is_err() {
					break;
				}
				cx.background_executor().timer(update::EVERY).await;
			}
		})
	}

	/// One check, on the engine's runtime; the answer is polled back onto the window. A check
	/// asked for while one is under way joins it.
	pub(crate) fn check_for_updates(&mut self, by_hand: bool, cx: &mut Context<Self>) {
		self.updates.by_hand |= by_hand;
		if self.updates.checking {
			return;
		}
		self.updates.checking = true;
		let channel = self.preferences.update_channel;
		let known = self.updates.region;
		let receiver = self.engine.run(async move {
			let client = update::client();
			let region = match known {
				Some(region) => region,
				None => update::region(&client).await,
			};
			Answer { region, result: update::fetch(&client, channel, region).await }
		});
		self.updates._poll = Some(cx.spawn(async move |this, cx| {
			let deadline = std::time::Instant::now() + Duration::from_secs(90);
			loop {
				cx.background_executor().timer(Duration::from_millis(200)).await;
				let answer = match receiver.try_recv() {
					Ok(answer) => answer,
					Err(mpsc::TryRecvError::Empty) if std::time::Instant::now() < deadline => continue,
					Err(_) => Answer { region: Region::Elsewhere, result: Err("no answer".to_owned()) },
				};
				let _ = this.update(cx, |this, cx| this.finish_update_check(answer, cx));
				break;
			}
		}));
		cx.notify();
	}

	fn finish_update_check(&mut self, answer: Answer, cx: &mut Context<Self>) {
		let by_hand = std::mem::take(&mut self.updates.by_hand);
		self.updates.checking = false;
		self.updates.region = Some(answer.region);
		match answer.result {
			Ok(manifest) => self.apply_manifest(manifest, by_hand, cx),
			Err(error) => self.updates.outcome = Some(Err(error)),
		}
		cx.notify();
	}

	/// What a manifest means here. A hand build has no number and is never behind on its own;
	/// only a check asked for shows it what is published.
	pub(crate) fn apply_manifest(
		&mut self,
		manifest: Manifest,
		by_hand: bool,
		cx: &mut Context<Self>,
	) {
		self.updates.outcome = Some(Ok(manifest.build));
		let this = update::this_build();
		let available = update::compare(&manifest, this).filter(|_| this.is_some() || by_hand);
		self.updates.latest = Some(manifest);
		if let Some(available) = &available
			&& self.updates.notified != Some(available.build)
			&& !self.updates.active
		{
			self.updates.notified = Some(available.build);
			notify(available, cx);
		}
		self.updates.available = available;
		cx.notify();
	}

	pub(crate) fn dismiss_update(&mut self, cx: &mut Context<Self>) {
		self.updates.dismissed = self.updates.available.as_ref().map(|a| a.build);
		cx.notify();
	}

	/// Opens the file's address in the browser: the install itself is not written yet, and the
	/// browser is the one thing every system can hand a download to.
	pub(crate) fn get_update(&mut self, cx: &mut Context<Self>) {
		if let Some(available) = &self.updates.available {
			let region = self.updates.region.unwrap_or(Region::Elsewhere);
			let [first, _] = update::routes(self.preferences.update_channel, region, &available.file);
			cx.open_url(&first);
		}
	}

	/// The settings row's word on the last check.
	pub(crate) fn update_status(&self) -> String {
		if self.updates.checking {
			return "Checking".to_owned();
		}
		match (&self.updates.outcome, update::this_build()) {
			(None, _) => "Not checked yet".to_owned(),
			(Some(Err(error)), _) => format!("Could not check: {error}"),
			(Some(Ok(build)), Some(this)) if *build > this => format!("Build {build} is available"),
			(Some(Ok(build)), Some(_)) => format!("Build {build} is the latest, and this is it"),
			(Some(Ok(build)), None) => format!("Build {build} is the latest; this build has no number"),
		}
	}

	/// The card in the corner, over the list and above the status bar, while a newer build is
	/// published and has not been waved away.
	pub(crate) fn update_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
		let p = self.palette;
		let available = self.updates.available.as_ref()?;
		if self.updates.dismissed == Some(available.build) {
			return None;
		}
		let title = format!("Build {} is ready", available.build);
		let detail = format!("{} of {}", available.version, self.preferences.update_channel.name());
		Some(
			div()
				.absolute()
				.bottom(px(status_bar::HEIGHT + 12.0))
				.right(px(12.0))
				.id("update-toast")
				.role(Role::Alert)
				.debug_selector(|| "toast:update".to_owned())
				.flex()
				.items_center()
				.gap_3()
				.px_3()
				.py_2()
				.rounded_md()
				.border_1()
				.border_color(p.border)
				.bg(p.panel)
				.shadow_md()
				.child(icon(Icon::Download, p.accent).size_4())
				.child(
					div()
						.flex()
						.flex_col()
						.child(div().text_sm().child(title))
						.child(div().text_xs().text_color(p.muted).child(detail)),
				)
				.child(
					div()
						.id("update-get")
						.role(Role::Button)
						.aria_label("Get")
						.debug_selector(|| "button:Get".to_owned())
						.px_2()
						.py_0p5()
						.rounded_sm()
						.text_xs()
						.text_color(p.accent)
						.cursor_pointer()
						.hover(move |s| s.bg(p.hover))
						.on_click(cx.listener(|this, _, _, cx| this.get_update(cx)))
						.child("Get"),
				)
				.child(icon_button(
					p,
					"update-later",
					Icon::X,
					"Later",
					true,
					cx.listener(|this, _, _, cx| this.dismiss_update(cx)),
				)),
		)
	}
}

/// Tells the system, for a window that is not in front. Failing to is nothing to report: the
/// card is still there when the window is.
fn notify(available: &Available, cx: &mut Context<Rdm>) {
	let body = format!("Build {} of {} is ready to download.", available.build, available.version);
	cx.background_executor()
		.spawn(async move {
			// macOS delivers a notification only on behalf of an installed bundle; a binary
			// run from the build tree has none, and the call fails quietly.
			#[cfg(target_os = "macos")]
			let _ = notify_rust::set_application(identity::BUNDLE_ID);
			let _ = notify_rust::Notification::new().summary(identity::DISPLAY_NAME).body(&body).show();
		})
		.detach();
}
