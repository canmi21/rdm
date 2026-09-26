//! One download from start to finish: probe, plan, open the file, run connections until the
//! plan is complete, and rename. The scheduler here is what turns the planner's arithmetic
//! into connections: it grows their number as the server proves it can take more, retries a
//! segment that fails, and writes the plan beside the file as it goes. See spec/engine.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Url;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::engine::control::{self, Control};
use crate::engine::error::{Error, Result};
use crate::engine::limiter::Limiter;
use crate::engine::probe::{Probe, probe};
use crate::engine::segments::{Plan, Span};
use crate::engine::settings::Settings;
use crate::engine::worker::{Job, Outcome, fetch};
use crate::engine::writer::Writer;

mod schedule;
#[cfg(test)]
mod tests;

use schedule::schedule;

/// What to download and where. The name is the server's unless given; the range is the whole
/// file unless given.
#[derive(Clone, Debug)]
pub struct Request {
	pub url: Url,
	pub directory: PathBuf,
	pub file_name: Option<String>,
	/// Only this part of the file, `start..end` with `end` None meaning to the file's end.
	pub range: Option<(u64, Option<u64>)>,
	/// Other addresses of the same file. Connections are spread across them, and a connection
	/// that fails moves to the next; the first address is the one probed and the one whose
	/// validator is trusted, so a mirror is checked by size alone.
	pub mirrors: Vec<Url>,
	pub settings: Settings,
}

impl Request {
	pub fn new(url: Url, directory: impl Into<PathBuf>) -> Request {
		Request {
			url,
			directory: directory.into(),
			file_name: None,
			range: None,
			mirrors: Vec::new(),
			settings: Settings::default(),
		}
	}
}

/// The numbers the window shows, kept current by the connections themselves.
#[derive(Debug, Default)]
pub struct Progress {
	pub done: AtomicU64,
	pub total: AtomicU64,
	pub connections: AtomicU64,
	/// Bytes per second over the last moment, smoothed.
	pub speed: AtomicU64,
}

/// What a finished download hands back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
	pub path: PathBuf,
	pub size: u64,
	pub probe: Probe,
}

/// The reason connections were stopped, so the caller can tell a pause from a failure.
pub struct Handle {
	pub cancel: CancellationToken,
	pub progress: Arc<Progress>,
	pub limit: Limiter,
	/// What the probe learnt, the moment it learnt it, so a snapshot can name the file and its
	/// size while the download runs rather than only once it is done.
	pub probed: Mutex<Option<Probe>>,
	/// The plan the connections are working through, shared with them rather than copied: the
	/// same `Arc` the scheduler cuts segments out of. A snapshot reads it to say how the file is
	/// being divided and how far each part has come, which is the whole of what a download's
	/// window shows about its workers. Nothing until the plan is made, and left where it was when
	/// the run ends, so a paused download still says how it was cut. See spec/engine.md.
	pub plan: Mutex<Option<Arc<Mutex<Plan>>>>,
	/// The most connections this download may hold open, as it stands now. The scheduler reads it
	/// every time it looks for room rather than keeping the number it started with, so the count
	/// can be changed while the download runs -- which is what a download's own window offers.
	///
	/// **Raising it is felt at once and lowering it is felt as connections finish.** The loop
	/// that reads this only ever starts connections; it has no way to take a byte back from one
	/// that is already reading, and cutting a connection off mid-segment would throw away what it
	/// had. So a lower number is a ceiling the download drifts down to. See spec/engine.md.
	pub ceiling: AtomicU64,
	/// Whether the download grows its own connection count and steals halves of segments, or was
	/// cut into a fixed number at the start. Read beside `ceiling` and changed with it.
	pub auto: AtomicBool,
	/// How many connections the server has shown it will take: written before the run with what
	/// an earlier download from the same host learnt, zero for nothing known, and kept current by
	/// the scheduler as it learns more. See spec/engine.md, "The server decides how many
	/// connections it takes".
	pub learned: AtomicU64,
	/// Whether the server turned a connection away during the run, which is what makes `learned`
	/// a limit rather than only as far as the run happened to go.
	pub crowded: AtomicBool,
}

impl Handle {
	pub fn new() -> Handle {
		Handle {
			cancel: CancellationToken::new(),
			progress: Arc::new(Progress::default()),
			limit: Limiter::unlimited(),
			probed: Mutex::new(None),
			plan: Mutex::new(None),
			// Nothing until the settings are read; the scheduler writes both before it starts.
			ceiling: AtomicU64::new(0),
			learned: AtomicU64::new(0),
			crowded: AtomicBool::new(false),
			auto: AtomicBool::new(true),
		}
	}
}

impl Default for Handle {
	fn default() -> Self {
		Handle::new()
	}
}

/// Runs a download to its end, or until `handle.cancel` is cancelled, in which case the plan is
/// left beside the partial file for a later run to continue. `global` is the engine's limiter,
/// shared with every other download.
pub async fn run(request: Request, handle: &Handle, global: Limiter) -> Result<Finished> {
	let settings =
		Settings { connections: request.settings.connections.clamped(), ..request.settings.clone() };
	// The probe's client, with its connection in the pool, is the first connection of the download:
	// dropped, the server would go on counting a socket it had not noticed close, and turn away a
	// connection it would have taken; kept idle, it would hold one of the places for the whole
	// download. See spec/engine.md, "The server decides how many connections it takes".
	let first = crate::engine::client::build(&settings, false)?;
	let probed = probe(&first, request.url.clone()).await?;
	*handle.probed.lock().unwrap() = Some(probed.clone());
	if let (Some(size), Some(limit)) = (probed.size, settings.max_size)
		&& size > limit
	{
		return Err(Error::TooLarge { size, limit });
	}
	let file_name = request.file_name.clone().unwrap_or_else(|| probed.file_name.clone());
	let target = request.directory.join(&file_name);

	// The span: the user's range clipped to the file, or the whole file, or -- when the server
	// would not say how long the file is -- open-ended, to be closed when the body ends.
	let span = match (request.range, probed.size) {
		(Some((start, end)), Some(size)) => {
			let end = end.map_or(size, |e| e.min(size));
			if start >= size || start >= end {
				return Err(Error::OutOfRange);
			}
			if !probed.ranges && start > 0 {
				return Err(Error::NoRanges);
			}
			Span::new(start, end)
		}
		(Some((start, end)), None) => {
			if !probed.ranges {
				return Err(Error::NoRanges);
			}
			Span::new(start, end.unwrap_or(u64::MAX))
		}
		(None, Some(size)) => Span::new(0, size),
		(None, None) => Span::new(0, u64::MAX),
	};
	let open_ended = span.end == u64::MAX;
	let validator = probed.validator().map(str::to_owned);

	// A plan left by an earlier run continues if it is for the same file: same address, same
	// span, and the same validator when the server gives one. Otherwise it is discarded and
	// the download starts over, since bytes from a different file are worth nothing.
	let saved = control::load(&target)?.filter(|c| {
		c.url == probed.url.as_str()
			&& c.plan.span == span
			&& (c.validator.is_none() || c.validator == validator)
			&& probed.ranges
	});
	let plan = match saved {
		Some(control) => control.plan,
		None if probed.ranges && !settings.connections.auto && !open_ended => {
			Plan::split(span, settings.connections.max, settings.min_segment)
		}
		None => Plan::whole(span),
	};
	let plan = Arc::new(Mutex::new(plan));
	// The same plan the connections cut and fill, handed to the handle so a snapshot can read it
	// without the scheduler having to report anything.
	*handle.plan.lock().unwrap() = Some(plan.clone());
	let writer =
		Writer::open(&target, (!open_ended).then(|| span.len()), settings.preallocate && !open_ended)?;
	let controls = Control::new(
		probed.url.as_str(),
		probed.size,
		validator.as_deref(),
		plan.lock().unwrap().clone(),
	);
	control::save(&target, &controls)?;

	handle.progress.total.store(span.len_or_zero(), Ordering::Relaxed);
	handle.progress.done.store(plan.lock().unwrap().done(), Ordering::Relaxed);
	handle.limit.set_rate(settings.speed_limit);

	let mut sources = vec![probed.url.clone()];
	sources.extend(request.mirrors.iter().cloned());
	let result = schedule(
		&settings,
		&probed,
		first,
		&sources,
		validator,
		plan.clone(),
		writer.clone(),
		handle,
		global,
	)
	.await;
	match result {
		Ok(()) => {
			let size = plan.lock().unwrap().span.len();
			let path = writer.finish(Some(size)).await?;
			control::remove(&target);
			Ok(Finished { path, size, probe: probed })
		}
		Err(e) => {
			// Whatever happened, the plan is written so a later run knows where things stand;
			// a cancelled download is a paused one until somebody removes its files.
			let _ = control::save(
				&target,
				&Control::new(
					probed.url.as_str(),
					probed.size,
					controls.validator.as_deref(),
					plan.lock().unwrap().clone(),
				),
			);
			Err(e)
		}
	}
}

trait LenOrZero {
	fn len_or_zero(self) -> u64;
}

impl LenOrZero for Span {
	fn len_or_zero(self) -> u64 {
		if self.end == u64::MAX { 0 } else { self.len() }
	}
}

/// Removes what a download left behind: the partial file and the plan. For a download the
/// user does not want continued.
pub fn discard(directory: &Path, file_name: &str) {
	let target = directory.join(file_name);
	let _ = std::fs::remove_file(control::part_path(&target));
	control::remove(&target);
}
