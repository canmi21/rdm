//! The window's side of the update: a check at launch and every few minutes after, a card in
//! the corner when a newer build is published, a system notification when the window is not
//! the one in front, and from the card the download, the install and the restart. The check,
//! the download and the install themselves are `src/update`. See spec/release.md.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gpui::{Context, IntoElement, Role, Task, Window, div, prelude::*, px};

use crate::app::Rdm;
use crate::download::format_bytes;
use crate::identity;
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button;
use crate::ui::status_bar;
use crate::ui::theme::Palette;
use crate::update::{self, Available, Manifest, Region, install};

/// What the check knows and what it last said.
pub struct Updates {
	/// The number this binary was built as, None for one made by hand. Read once from the
	/// build's environment; a test sets it, since the test binary is built in that environment
	/// too and would otherwise carry the run's number.
	pub this: Option<u64>,
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
	/// How far the install of the available build has come.
	pub stage: Stage,
	/// The read of the check or the install under way, polled below.
	_poll: Option<Task<()>>,
}

/// The install, from the card's button to the restart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
	/// Nothing started; the card offers Install.
	Offered,
	/// The file on its way in, so far and out of.
	Downloading { done: u64, total: Option<u64> },
	/// The file is whole and checked; it is being put in place.
	Installing,
	/// The new build is in place; the card offers Restart, which launches it and quits this.
	Installed { launch: PathBuf },
	/// What went wrong; the card offers to try again.
	Failed(String),
}

impl Default for Updates {
	fn default() -> Self {
		Updates {
			this: update::this_build(),
			region: None,
			checking: false,
			by_hand: false,
			latest: None,
			available: None,
			outcome: None,
			dismissed: None,
			notified: None,
			active: false,
			stage: Stage::Offered,
			_poll: None,
		}
	}
}

/// One outcome of a check as the runtime hands it back.
struct Answer {
	region: Region,
	result: Result<Manifest, String>,
}

/// What the runtime counts while the file comes in, read by the window between polls. `done`
/// at its ceiling means the file is whole and the install has begun.
#[derive(Default)]
struct Counted {
	done: AtomicU64,
	total: AtomicU64,
}

/// The card's one word that does something, and what it does.
type Action = (&'static str, fn(&mut Rdm, &mut Context<Rdm>));

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
	/// asked for while one is under way joins it; none is made while an install is.
	pub(crate) fn check_for_updates(&mut self, by_hand: bool, cx: &mut Context<Self>) {
		self.updates.by_hand |= by_hand;
		if self.updates.checking || !matches!(self.updates.stage, Stage::Offered | Stage::Failed(_)) {
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
	/// only a check asked for shows it what is published. A newer build than the one offered
	/// starts the offer over.
	pub(crate) fn apply_manifest(
		&mut self,
		manifest: Manifest,
		by_hand: bool,
		cx: &mut Context<Self>,
	) {
		self.updates.outcome = Some(Ok(manifest.build));
		let this = self.updates.this;
		let available = update::compare(&manifest, this).filter(|_| this.is_some() || by_hand);
		self.updates.latest = Some(manifest);
		if let Some(available) = &available
			&& self.updates.notified != Some(available.build)
			&& !self.updates.active
		{
			self.updates.notified = Some(available.build);
			notify(available, cx);
		}
		if available.as_ref().map(|a| a.build) != self.updates.available.as_ref().map(|a| a.build) {
			self.updates.stage = Stage::Offered;
		}
		self.updates.available = available;
		cx.notify();
	}

	pub(crate) fn dismiss_update(&mut self, cx: &mut Context<Self>) {
		self.updates.dismissed = self.updates.available.as_ref().map(|a| a.build);
		cx.notify();
	}

	/// Fetches the build's file for where this runs from, checks it, and puts it in place, all
	/// on the engine's runtime, the window polling how far it has come. What cannot be replaced
	/// -- a build in its build tree, an application still on its disk image -- is said on the
	/// card and nothing is fetched.
	pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
		let Some(available) = self.updates.available.clone() else { return };
		if matches!(self.updates.stage, Stage::Downloading { .. } | Stage::Installing) {
			return;
		}
		let place = match install::place() {
			Ok(place) => place,
			Err(error) => return self.fail_update(error, cx),
		};
		let Some(asset) = available.asset(place.kind()).cloned() else {
			return self.fail_update(format!("no {} in build {}", place.kind(), available.build), cx);
		};
		let Some(dir) = self.paths.as_ref().and_then(|p| p.state.parent()).map(|p| p.join("updates"))
		else {
			return self.fail_update("nowhere to keep the file".to_owned(), cx);
		};
		let region = self.updates.region.unwrap_or(Region::Elsewhere);
		let urls = update::routes(self.preferences.update_channel, region, &asset.file).to_vec();
		let counted = Arc::new(Counted::default());
		let counting = counted.clone();
		let receiver = self.engine.run(async move {
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
			let file = dir.join(&asset.file);
			let client = update::file_client();
			let counted = counting.clone();
			let progress = move |done: u64, total: Option<u64>| {
				counted.done.store(done, Ordering::Relaxed);
				counted.total.store(total.unwrap_or(0), Ordering::Relaxed);
			};
			update::download(&client, &urls, &file, &asset.sha256, &progress).await?;
			counting.done.store(u64::MAX, Ordering::Relaxed);
			let launch = install::install(&file, &place)?;
			let _ = std::fs::remove_dir_all(&dir);
			Ok::<PathBuf, String>(launch)
		});
		self.updates.stage = Stage::Downloading { done: 0, total: None };
		self.updates._poll = Some(cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(200)).await;
				let stage = match receiver.try_recv() {
					Ok(Ok(launch)) => Stage::Installed { launch },
					Ok(Err(error)) => Stage::Failed(error),
					Err(mpsc::TryRecvError::Disconnected) => {
						Stage::Failed("the install stopped without a word".to_owned())
					}
					Err(mpsc::TryRecvError::Empty) => {
						let done = counted.done.load(Ordering::Relaxed);
						let total = counted.total.load(Ordering::Relaxed);
						if done == u64::MAX {
							Stage::Installing
						} else {
							Stage::Downloading { done, total: (total > 0).then_some(total) }
						}
					}
				};
				let settled = !matches!(stage, Stage::Downloading { .. } | Stage::Installing);
				let updated = this.update(cx, |this, cx| {
					if this.updates.stage != stage {
						this.updates.stage = stage;
						cx.notify();
					}
				});
				if settled || updated.is_err() {
					break;
				}
			}
		}));
		cx.notify();
	}

	fn fail_update(&mut self, error: String, cx: &mut Context<Self>) {
		self.updates.stage = Stage::Failed(error);
		cx.notify();
	}

	/// Starts the installed build and quits this one. The downloads in flight are paused by the
	/// quit the way any quit pauses them, and continue from their plans in the new build.
	pub(crate) fn restart_into_update(&mut self, cx: &mut Context<Self>) {
		if let Stage::Installed { launch } = &self.updates.stage {
			match install::launch(launch) {
				Ok(()) => cx.quit(),
				Err(error) => self.fail_update(error, cx),
			}
		}
	}

	/// The settings row's word on the last check.
	pub(crate) fn update_status(&self) -> String {
		if self.updates.checking {
			return "Checking".to_owned();
		}
		let latest = self.updates.latest.as_ref().map(|m| label(&m.version, m.build));
		match (&self.updates.outcome, self.updates.this, latest) {
			(None, _, _) | (Some(Ok(_)), _, None) => "Not checked yet".to_owned(),
			(Some(Err(error)), _, _) => format!("Could not check: {error}"),
			(Some(Ok(build)), Some(this), Some(latest)) if *build > this => {
				format!("{latest} is available")
			}
			(Some(Ok(_)), Some(_), Some(latest)) => format!("{latest} is the latest, and this is it"),
			(Some(Ok(_)), None, Some(latest)) => {
				format!("{latest} is the latest; this build has no number")
			}
		}
	}

	/// The card in the corner, over the list and above the status bar, while a newer build is
	/// published and has not been waved away: what it is, how far it has come, and the one
	/// thing to press next.
	pub(crate) fn update_toast(&self, cx: &mut Context<Self>) -> Option<impl IntoElement + use<>> {
		let p = self.palette;
		let available = self.updates.available.as_ref()?;
		if self.updates.dismissed == Some(available.build) {
			return None;
		}
		let build = available.version.clone();
		let channel = self.preferences.update_channel.name();
		let (title, action): (String, Option<Action>) = match &self.updates.stage {
			Stage::Offered => (
				format!("{channel} {build} is ready"),
				Some(("Install", |this, cx| this.install_update(cx))),
			),
			Stage::Downloading { done, total } => (
				match total {
					Some(total) => {
						format!("Getting {build}, {} of {}", format_bytes(*done), format_bytes(*total))
					}
					None => format!("Getting {build}, {}", format_bytes(*done)),
				},
				None,
			),
			Stage::Installing => (format!("Installing {build}"), None),
			Stage::Installed { .. } => (
				format!("{build} is installed"),
				Some(("Restart", |this, cx| this.restart_into_update(cx))),
			),
			Stage::Failed(error) => {
				(format!("{build}: {error}"), Some(("Retry", |this, cx| this.install_update(cx))))
			}
		};
		let busy = action.is_none();
		Some(
			div()
				.absolute()
				.bottom(px(status_bar::HEIGHT + 12.0))
				.right(px(12.0))
				.max_w(px(420.0))
				.id("update-toast")
				.role(Role::Alert)
				.debug_selector(|| "toast:update".to_owned())
				.flex()
				.items_center()
				.gap_2()
				.pl_2p5()
				.pr_1()
				.py_1()
				.rounded_md()
				.border_1()
				.border_color(p.border)
				.bg(p.panel)
				.shadow_md()
				.child(icon(Icon::Download, p.accent).size_3p5().flex_none())
				.child(div().min_w_0().truncate().child(title))
				.when_some(action, |s, (word, run)| s.child(word_button(p, word, run, cx)))
				.when(!busy, |s| {
					s.child(icon_button(
						p,
						"update-later",
						Icon::X,
						"Later",
						true,
						cx.listener(|this, _, _, cx| this.dismiss_update(cx)),
					))
				}),
		)
	}
}

/// A build as Settings names it: the version, which is the day, and the run number in
/// brackets, the way the system's own About windows put it. The card and the notification say
/// the version alone; the number is for telling two builds of one day apart, which is a
/// settings matter.
fn label(version: &str, build: u64) -> String {
	format!("{version} ({build})")
}

/// The card's one word that does something.
fn word_button(
	p: Palette,
	word: &'static str,
	run: fn(&mut Rdm, &mut Context<Rdm>),
	cx: &mut Context<Rdm>,
) -> impl IntoElement {
	div()
		.id(gpui::SharedString::from(format!("update-{word}")))
		.role(Role::Button)
		.aria_label(word)
		.debug_selector(move || format!("button:{word}"))
		.flex_none()
		.ml_1()
		.px_2()
		.py_0p5()
		.rounded_sm()
		.text_color(p.accent)
		.cursor_pointer()
		.hover(move |s| s.bg(p.hover))
		.on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
		.child(word)
}

/// Tells the system, for a window that is not in front. Failing to is nothing to report: the
/// card is still there when the window is.
fn notify(available: &Available, cx: &mut Context<Rdm>) {
	let body = format!("{} is ready to install.", available.version);
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
