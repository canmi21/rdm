//! Pacing: a token bucket that connections draw from before they read. One bucket per
//! download and one for the whole engine, both shared by every connection, so a limit is a
//! limit on the sum and not on each. The rate can be changed while connections are drawing,
//! which is what a settings window will do.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

struct Bucket {
	/// Bytes per second; None is no limit.
	rate: Option<u64>,
	tokens: f64,
	refilled: Instant,
}

/// Cloned freely; every clone draws from the same bucket.
#[derive(Clone)]
pub struct Limiter {
	bucket: Arc<Mutex<Bucket>>,
}

impl Limiter {
	pub fn new(rate: Option<u64>) -> Limiter {
		let bucket = Bucket { rate, tokens: rate.unwrap_or(0) as f64, refilled: Instant::now() };
		Limiter { bucket: Arc::new(Mutex::new(bucket)) }
	}

	pub fn unlimited() -> Limiter {
		Limiter::new(None)
	}

	pub fn rate(&self) -> Option<u64> {
		self.bucket.lock().unwrap().rate
	}

	/// A new rate takes effect on the next draw. The bucket keeps at most one second's worth,
	/// so a limit lowered mid-stream does not pay out a burst saved under the old one; a limit
	/// set where there was none starts with a second's worth, so the first draw is not a wait.
	pub fn set_rate(&self, rate: Option<u64>) {
		let mut bucket = self.bucket.lock().unwrap();
		let had_none = bucket.rate.is_none();
		bucket.rate = rate;
		if let Some(rate) = rate {
			bucket.tokens = if had_none { rate as f64 } else { bucket.tokens.min(rate as f64) };
			bucket.refilled = Instant::now();
		}
	}

	/// Waits until `bytes` may be transferred, then takes them. A draw larger than a second's
	/// worth is allowed through once the bucket is full, so a single large chunk does not wait
	/// forever; it just leaves the bucket in debt, and the next draws wait it out.
	pub async fn take(&self, bytes: usize) {
		loop {
			let wait = {
				let mut bucket = self.bucket.lock().unwrap();
				let Some(rate) = bucket.rate else { return };
				let now = Instant::now();
				let elapsed = now.duration_since(bucket.refilled).as_secs_f64();
				bucket.tokens = (bucket.tokens + elapsed * rate as f64).min(rate as f64);
				bucket.refilled = now;
				let need = bytes as f64;
				if bucket.tokens >= need || bucket.tokens >= rate as f64 {
					bucket.tokens -= need;
					return;
				}
				Duration::from_secs_f64((need.min(rate as f64) - bucket.tokens) / rate as f64)
			};
			tokio::time::sleep(wait.max(Duration::from_millis(1))).await;
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test(start_paused = true)]
	async fn a_limit_spaces_draws_out_to_the_rate() {
		let limiter = Limiter::new(Some(1000));
		let start = Instant::now();
		// The first second's worth is in the bucket already; the rest must be earned.
		for _ in 0..30 {
			limiter.take(100).await;
		}
		let elapsed = start.elapsed().as_secs_f64();
		assert!((1.9..2.2).contains(&elapsed), "3000 bytes at 1000/s from a full bucket: {elapsed}s");
	}

	#[tokio::test(start_paused = true)]
	async fn a_draw_larger_than_the_bucket_goes_through_once_it_is_full() {
		let limiter = Limiter::new(Some(100));
		let start = Instant::now();
		limiter.take(1000).await;
		limiter.take(1).await;
		let elapsed = start.elapsed().as_secs_f64();
		assert!(
			(8.9..9.2).contains(&elapsed),
			"the debt of 900 and the one byte are earned at 100/s: {elapsed}s"
		);
	}

	#[tokio::test(start_paused = true)]
	async fn no_limit_never_waits_and_a_change_applies_to_the_next_draw() {
		let limiter = Limiter::unlimited();
		let start = Instant::now();
		limiter.take(1 << 30).await;
		assert_eq!(start.elapsed(), Duration::ZERO);
		limiter.set_rate(Some(10));
		limiter.take(10).await;
		limiter.take(10).await;
		assert!(start.elapsed() >= Duration::from_millis(990), "{:?}", start.elapsed());
		limiter.set_rate(None);
		limiter.take(1 << 30).await;
		assert!(start.elapsed() < Duration::from_millis(1100));
	}
}
