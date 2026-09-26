//! Work the application does on its own while it runs -- the update check and the rules sync --
//! on one loop. Each job has its interval; none runs while a download is crawling, since a request
//! on a connection moving kilobytes a second takes from the download sharing it. See
//! spec/release.md, "Background work".

use std::sync::mpsc;
use std::time::{Duration, Instant};

use gpui::{Context, Task, Window};

use crate::app::Rdm;
use crate::download::{Download, Status};

/// How often the loop looks at what is due.
pub(crate) const TICK: Duration = Duration::from_secs(30);
/// Every running download together under this is crawling, and background work waits.
pub(crate) const SLOW: u64 = 1024 * 1024;
/// How often the rules are fetched again after the one at start.
pub(crate) const RULES_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Job {
	Updates,
	Rules,
}

impl Job {
	fn every(self) -> Duration {
		match self {
			Job::Updates => crate::update::EVERY,
			Job::Rules => RULES_EVERY,
		}
	}
}

/// When each job is next due. Every job is due at start.
#[derive(Debug)]
pub(crate) struct Schedule {
	next: Vec<(Job, Instant)>,
}

impl Schedule {
	pub(crate) fn new(now: Instant) -> Schedule {
		Schedule { next: vec![(Job::Updates, now), (Job::Rules, now)] }
	}

	pub(crate) fn due(&self, now: Instant) -> Vec<Job> {
		self.next.iter().filter(|(_, at)| *at <= now).map(|(job, _)| *job).collect()
	}

	pub(crate) fn ran(&mut self, job: Job, now: Instant) {
		for (each, at) in &mut self.next {
			if *each == job {
				*at = now + job.every();
			}
		}
	}
}

/// Whether the downloads running now are moving slowly enough that background work would take from
/// them: some running, and all of them together under a megabyte a second.
pub(crate) fn crawling(downloads: &[Download]) -> bool {
	let mut running = downloads.iter().filter(|d| d.status == Status::Downloading).peekable();
	running.peek().is_some() && running.map(|d| d.speed).sum::<u64>() < SLOW
}

/// Where the last rules sync stands, for the rules window to say.
#[derive(Default)]
pub(crate) struct RulesSync {
	pub(crate) running: bool,
	pub(crate) status: Option<String>,
	/// How the last sync ended, None before the first: what the rules window's button shows until the
	/// next one starts.
	pub(crate) succeeded: Option<bool>,
	_poll: Option<Task<()>>,
}

impl Rdm {
	/// Starts the loop. Returned so the caller keeps the task alive.
	pub(crate) fn start_background(
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
				if this.update(cx, |this, cx| this.run_background(cx)).is_err() {
					break;
				}
				cx.background_executor().timer(TICK).await;
			}
		})
	}

	/// What is due, run, unless a download is crawling, in which case everything waits for a later
	/// tick rather than being skipped.
	fn run_background(&mut self, cx: &mut Context<Self>) {
		if crawling(&self.downloads) {
			return;
		}
		let now = Instant::now();
		for job in self.schedule.due(now) {
			match job {
				Job::Updates => {
					if self.preferences.check_updates {
						self.check_for_updates(false, cx);
					}
				}
				Job::Rules => self.sync_rules(cx),
			}
			self.schedule.ran(job, now);
		}
	}

	/// Fetches the repository's rules into the synced layer on the engine's runtime, and reloads them
	/// once they are in place. A sync asked for while one is under way joins it.
	pub(crate) fn sync_rules(&mut self, cx: &mut Context<Self>) {
		if self.rules_sync.running {
			return;
		}
		let Some(paths) = &self.paths else { return };
		let places = paths.rule_places();
		let settings = self.preferences.engine_settings(self.proxy_in_use().as_deref());
		let Ok(client) = crate::engine::client::build(&settings, false) else { return };
		self.rules_sync.running = true;
		let receiver = self.engine.run(async move {
			let fetched =
				crate::rules::sync::fetch(client, crate::rules::sync::Sources::repository()).await?;
			crate::rules::sync::apply(&places.synced, &fetched).map_err(|e| e.to_string())?;
			Ok::<_, String>((fetched.files.len(), fetched.from))
		});
		self.rules_sync._poll = Some(cx.spawn(async move |this, cx| {
			let deadline = Instant::now() + Duration::from_secs(120);
			loop {
				cx.background_executor().timer(Duration::from_millis(200)).await;
				let answer = match receiver.try_recv() {
					Ok(answer) => answer,
					Err(mpsc::TryRecvError::Empty) if Instant::now() < deadline => continue,
					Err(_) => Err("no answer".to_owned()),
				};
				let _ = this.update(cx, |this, cx| this.finish_rules_sync(answer, cx));
				break;
			}
		}));
		cx.notify();
	}

	fn finish_rules_sync(
		&mut self,
		answer: Result<(usize, &'static str), String>,
		cx: &mut Context<Self>,
	) {
		self.rules_sync.running = false;
		self.rules_sync.succeeded = Some(answer.is_ok());
		let at = chrono::Local::now().format("%H:%M");
		self.rules_sync.status = Some(match answer {
			Ok((count, from)) => {
				self.reload_rules(cx);
				format!("Synced {count} files from {from} at {at}")
			}
			Err(error) => format!("The sync at {at} failed: {error}"),
		});
		cx.notify();
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_job_is_due_at_start_and_again_after_its_interval() {
		let start = Instant::now();
		let mut schedule = Schedule::new(start);
		assert_eq!(schedule.due(start), [Job::Updates, Job::Rules]);
		schedule.ran(Job::Updates, start);
		schedule.ran(Job::Rules, start);
		assert!(schedule.due(start + Duration::from_secs(60)).is_empty());
		assert_eq!(schedule.due(start + crate::update::EVERY), [Job::Updates]);
		assert_eq!(schedule.due(start + RULES_EVERY), [Job::Updates, Job::Rules]);
	}

	#[test]
	fn background_work_waits_while_downloads_crawl_and_not_while_they_move() {
		let mut downloads = crate::download::sample();
		for d in &mut downloads {
			d.status = Status::Completed;
			d.speed = 0;
		}
		assert!(!crawling(&downloads), "nothing running");
		downloads[0].status = Status::Downloading;
		downloads[0].speed = 300 * 1024;
		assert!(crawling(&downloads), "kilobytes a second");
		downloads[1].status = Status::Downloading;
		downloads[1].speed = 800 * 1024;
		assert!(!crawling(&downloads), "together over a megabyte a second");
	}
}
