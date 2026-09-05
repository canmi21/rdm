//! Against a real network: public files that have been served with ranges for years, over HTTP
//! and HTTPS, large enough to be split. Ignored by default, since they need the network and
//! take seconds; `cargo test -- --ignored` runs them. Each is bounded, so a mirror that is down
//! fails the test rather than hanging it.

use std::path::PathBuf;
use std::time::Duration;

use crate::engine::{self, Connections, Limiter, Request, Settings};
use reqwest::Url;
use tokio_util::sync::CancellationToken;

fn scratch(name: &str) -> PathBuf {
	let dir = std::env::temp_dir().join(format!("rdm-mirror-{}-{name}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).unwrap();
	dir
}

fn handle() -> engine::Handle {
	engine::Handle {
		cancel: CancellationToken::new(),
		progress: std::sync::Arc::new(engine::Progress::default()),
		limit: Limiter::unlimited(),
	}
}

async fn fetch(
	url: &str,
	connections: Connections,
	range: Option<(u64, Option<u64>)>,
) -> engine::Finished {
	let dir = scratch(&format!("{}-{}", connections.max, range.map_or(0, |r| r.0)));
	let mut request = Request::new(Url::parse(url).unwrap(), &dir);
	request.settings = Settings { connections, ..Settings::default() };
	request.range = range;
	tokio::time::timeout(
		Duration::from_secs(180),
		engine::task::run(request, &handle(), Limiter::unlimited()),
	)
	.await
	.expect("finished within three minutes")
	.expect("downloaded")
}

/// 20 MB over plain HTTP from a test-file host that has served ranges for a decade.
const HTTP_20MB: &str = "http://ipv4.download.thinkbroadband.com/20MB.zip";
/// A few megabytes over HTTPS from a mirror that has served ranges for years; fetched twice
/// by its test, so kept modest.
const HTTPS_FILE: &str = "https://mirrors.edge.kernel.org/pub/software/scm/git/git-2.9.5.tar.gz";

#[tokio::test]
#[ignore = "needs the network"]
async fn http_with_eight_connections_lands_every_byte() {
	let done = fetch(HTTP_20MB, Connections { min: 1, max: 8, auto: true }, None).await;
	assert_eq!(done.size, 20 * 1024 * 1024);
	assert_eq!(std::fs::metadata(&done.path).unwrap().len(), done.size);
	assert!(done.probe.ranges);
}

#[tokio::test]
#[ignore = "needs the network"]
async fn https_split_and_single_give_the_same_bytes() {
	let split = fetch(HTTPS_FILE, Connections { min: 4, max: 8, auto: false }, None).await;
	let single = fetch(HTTPS_FILE, Connections { min: 1, max: 1, auto: false }, None).await;
	assert_eq!(split.size, single.size);
	assert_eq!(std::fs::read(&split.path).unwrap(), std::fs::read(&single.path).unwrap());
}

#[tokio::test]
#[ignore = "needs the network"]
async fn a_range_from_a_mirror_matches_the_slice_of_the_whole() {
	let whole = fetch(HTTP_20MB, Connections { min: 1, max: 4, auto: true }, None).await;
	let part = fetch(
		HTTP_20MB,
		Connections { min: 1, max: 2, auto: true },
		Some((1_000_000, Some(3_000_000))),
	)
	.await;
	let all = std::fs::read(&whole.path).unwrap();
	assert_eq!(std::fs::read(&part.path).unwrap(), &all[1_000_000..3_000_000]);
}
