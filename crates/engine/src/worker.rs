//! One connection, one segment: ask for the bytes from where the segment stands to where it
//! ends, and write them as they come. The segment's end can move closer while this runs --
//! that is how a free connection takes the far half -- so the end is read from the shared plan
//! at every chunk and never trusted from the request. See spec/engine.md.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use reqwest::header::{CONTENT_RANGE, IF_RANGE, RANGE};
use reqwest::{Client, StatusCode, Url};
use tokio_util::sync::CancellationToken;

use crate::error::{Error, Result};
use crate::limiter::Limiter;
use crate::segments::Plan;
use crate::writer::Writer;

/// Everything a connection needs, handed to it whole.
pub struct Job {
	pub client: Client,
	pub url: Url,
	/// Which segment of the plan this connection owns.
	pub index: usize,
	pub plan: Arc<Mutex<Plan>>,
	/// Sent as If-Range on any request that does not start at the file's first byte, so a
	/// changed file comes back as 200 and is caught rather than spliced.
	pub validator: Option<String>,
	/// The server honours ranges. Off, the request carries none and the answer must be the
	/// whole file from the start.
	pub ranges: bool,
	/// Where the span starts in the source; the file on disk starts at zero, so a byte at
	/// source position `p` lands at `p - base`.
	pub base: u64,
	pub writer: Writer,
	pub limits: Vec<Limiter>,
	pub idle_timeout: Duration,
	pub cancel: CancellationToken,
	/// Called with each chunk's length as it lands, for the speed readout, and once with zero
	/// when the first byte arrives, which is the signal that this connection is worth another.
	pub progress: Arc<dyn Fn(usize) + Send + Sync>,
}

/// How a connection ended: its segment complete, or the body over before the segment was, which
/// for a file of unknown length is the only way to learn the length.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
	Complete,
	/// The server closed the body at this position of the file; the segment was open-ended.
	EndOfFile(u64),
}

pub async fn fetch(job: Job) -> Result<Outcome> {
	let (position, end, open_ended) = {
		let plan = job.plan.lock().unwrap();
		let segment = plan.segments[job.index];
		(segment.position(), segment.span.end, segment.span.end == u64::MAX)
	};
	if !open_ended && position >= end {
		return Ok(Outcome::Complete);
	}
	let mut request = job.client.get(job.url.clone());
	if job.ranges {
		// The far end is left open even when the segment has one: the segment may grow again
		// if a later split is undone, and reading past the end is cheaper than a new request.
		// It is cut at the end by the writer, not by the server.
		request = request.header(RANGE, format!("bytes={position}-"));
		if position > 0
			&& let Some(validator) = &job.validator
		{
			request = request.header(IF_RANGE, validator);
		}
	}
	let response = tokio::select! {
		r = request.send() => r?,
		_ = job.cancel.cancelled() => return Err(Error::Cancelled),
	};
	let status = response.status();
	if job.ranges {
		match status {
			StatusCode::PARTIAL_CONTENT => {
				let start = response
					.headers()
					.get(CONTENT_RANGE)
					.and_then(|v| v.to_str().ok())
					.and_then(|v| v.strip_prefix("bytes "))
					.and_then(|v| v.split('-').next())
					.and_then(|v| v.parse::<u64>().ok());
				if start != Some(position) {
					return Err(Error::Changed);
				}
			}
			// 200 to a ranged request from the first byte is a server that ignores ranges:
			// harmless for the one segment that starts there. From anywhere else it means the
			// file changed under If-Range, or the server lied about ranges; either way what
			// is on disk cannot be continued.
			StatusCode::OK if position == 0 => {}
			StatusCode::OK => return Err(Error::Changed),
			StatusCode::RANGE_NOT_SATISFIABLE => return Err(Error::OutOfRange),
			other => return Err(Error::Refused { status: other.as_u16() }),
		}
	} else if !status.is_success() {
		return Err(Error::Refused { status: status.as_u16() });
	}
	let mut stream = response.bytes_stream();
	let mut at = position;
	let mut first = true;
	loop {
		let next = tokio::select! {
			chunk = tokio::time::timeout(job.idle_timeout, stream.next()) => chunk,
			_ = job.cancel.cancelled() => return Err(Error::Cancelled),
		};
		let chunk: Bytes = match next {
			Ok(Some(Ok(chunk))) => chunk,
			Ok(Some(Err(e))) => return Err(Error::Http(e)),
			Ok(None) => break,
			Err(_) => {
				return Err(Error::ShortBody { want: end.saturating_sub(position), got: at - position });
			}
		};
		if first {
			first = false;
			(job.progress)(0);
		}
		// The end may have moved closer since the last chunk; write only what is still ours.
		let end_now = job.plan.lock().unwrap().segments[job.index].span.end;
		if at >= end_now {
			break;
		}
		let keep = ((end_now - at).min(chunk.len() as u64)) as usize;
		let bytes = chunk.slice(..keep);
		for limit in &job.limits {
			limit.take(bytes.len()).await;
		}
		job.writer.write_at(at - job.base, bytes).await?;
		at += keep as u64;
		let overran = {
			// The end may have moved again while the write was in flight, to somewhere inside
			// what was just written. The bytes are right where they are -- the connection that
			// took the far half will write the same ones -- so the segment is simply complete.
			let mut plan = job.plan.lock().unwrap();
			let segment = &mut plan.segments[job.index];
			segment.done = (at - segment.span.start).min(segment.span.len());
			at >= segment.span.end
		};
		(job.progress)(keep);
		if keep < chunk.len() || overran {
			// The segment ended inside this chunk; the rest belongs to the connection that took
			// the far half. Dropping the stream closes the connection.
			break;
		}
	}
	let end_now = job.plan.lock().unwrap().segments[job.index].span.end;
	if end_now == u64::MAX {
		let mut plan = job.plan.lock().unwrap();
		plan.segments[job.index].span.end = at;
		plan.span.end = at;
		return Ok(Outcome::EndOfFile(at));
	}
	if at < end_now {
		return Err(Error::ShortBody { want: end_now - position, got: at - position });
	}
	Ok(Outcome::Complete)
}
