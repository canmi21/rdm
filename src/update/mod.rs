//! Whether a newer build is published, and getting it: the nightly's `latest.json`, read every
//! few minutes from wherever answers, compared by build number with the one this binary was
//! made as; then the file for this system, fetched by the same addresses and checked against
//! the manifest's sha256 before `install` puts it in place. See spec/release.md.

pub mod install;

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use sha2::Digest;

use crate::identity;

/// The channels a build can follow. One for now; the daily comes when it is cut.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
	#[default]
	Nightly,
}

impl Channel {
	pub fn name(self) -> &'static str {
		match self {
			Channel::Nightly => "Nightly",
		}
	}

	/// The release's tag on GitHub, which both routes address the files by.
	fn tag(self) -> &'static str {
		match self {
			Channel::Nightly => "nightly",
		}
	}
}

/// What the automatic update does with a newer build once the check has found one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
	/// Fetch it and put it in place; the card then offers the restart.
	#[default]
	Install,
	/// Fetch it and keep it; the card then offers the install, which is instant.
	Download,
	/// Say so and do nothing; the card offers the install, which fetches first.
	Notify,
}

impl Policy {
	pub const ALL: [Policy; 3] = [Policy::Install, Policy::Download, Policy::Notify];

	pub fn name(self) -> &'static str {
		match self {
			Policy::Install => "Download and install",
			Policy::Download => "Download only",
			Policy::Notify => "Notify only",
		}
	}
}

/// How often the manifest is asked for while the application runs.
pub const EVERY: Duration = Duration::from_secs(5 * 60);

/// What `latest.json` says: the build it names and the files it holds. Mirrors what
/// `pkgs/latest.py` writes.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Manifest {
	pub channel: String,
	pub version: String,
	pub build: u64,
	pub sha: String,
	pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Asset {
	pub target: String,
	pub kind: String,
	pub file: String,
	pub size: u64,
	pub sha256: String,
}

impl Manifest {
	pub fn parse(text: &str) -> Result<Manifest, String> {
		serde_json::from_str(text).map_err(|e| format!("latest.json: {e}"))
	}
}

/// Where the reader is, as far as choosing a route goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
	China,
	Elsewhere,
}

/// The two places every file is, in the order to try them: GitHub's own address first except
/// in China, where the author's CDN goes first; the other is the fallback either way, since
/// GitHub has its outages and a CDN its gaps.
pub fn routes(channel: Channel, region: Region, file: &str) -> [String; 2] {
	let tag = channel.tag();
	let github =
		format!("https://github.com/{}/releases/download/{tag}/{file}", identity::REPOSITORY);
	let cdn = format!("https://cdn.ffoni.com/github/release/{}/{tag}/{file}", identity::APPLICATION);
	match region {
		Region::China => [cdn, github],
		Region::Elsewhere => [github, cdn],
	}
}

/// The addresses that say where the reader is: Cloudflare's trace on the author's two hosts,
/// each a backup for the other.
pub const TRACES: [&str; 2] =
	["https://canmi.net/cdn-cgi/trace", "https://cdn.ffoni.com/cdn-cgi/trace"];

/// Reads `loc=XX` out of a trace. Anything that is not China is elsewhere, including a trace
/// with no location in it.
pub fn region_of_trace(text: &str) -> Region {
	let loc = text.lines().find_map(|line| line.strip_prefix("loc=")).map(str::trim);
	if loc == Some("CN") { Region::China } else { Region::Elsewhere }
}

/// The number this binary was built as: the run number, which only grows; None for a build
/// made by hand, which carries none.
pub fn this_build() -> Option<u64> {
	identity::BUILD.and_then(|b| b.parse().ok())
}

/// A build newer than this one, and its files for this system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Available {
	pub build: u64,
	pub version: String,
	pub assets: Vec<Asset>,
}

impl Available {
	/// The file of one kind, the one the place this runs from is replaced with.
	pub fn asset(&self, kind: &str) -> Option<&Asset> {
		self.assets.iter().find(|a| a.kind == kind)
	}
}

/// What a manifest means for this binary: a newer build, or nothing to do. A hand build has no
/// number, so anything published is newer than it; the caller decides whether to say so.
pub fn compare(manifest: &Manifest, this: Option<u64>) -> Option<Available> {
	if manifest.build <= this.unwrap_or(0) {
		return None;
	}
	let assets: Vec<Asset> =
		manifest.assets.iter().filter(|a| a.target == identity::TARGET).cloned().collect();
	if assets.is_empty() {
		return None;
	}
	Some(Available { build: manifest.build, version: manifest.version.clone(), assets })
}

/// A version as its numbers. Versions here are the day they were made, `YYYY.M.D` without
/// leading zeros, so read as text 2026.10.1 would sort before 2026.9.6; read as numbers it does
/// not. A part that is no number counts as nothing, which is the nearest thing to an answer for
/// a version this build should never have been handed.
fn ordinal(version: &str) -> Vec<u64> {
	version.split('.').map(|part| part.parse().unwrap_or(0)).collect()
}

/// Whether an update is worth telling the system about, given what it was last told. Only news
/// is: a higher version, or the same version built again with a higher number. The same build
/// found again by the next check five minutes later, or by the next run of the application, is
/// the same news and is not repeated. See src/app/updates.rs.
pub fn worth_telling(version: &str, build: u64, told: Option<(&str, u64)>) -> bool {
	let Some((last, last_build)) = told else { return true };
	match ordinal(version).cmp(&ordinal(last)) {
		std::cmp::Ordering::Greater => true,
		std::cmp::Ordering::Equal => build > last_build,
		std::cmp::Ordering::Less => false,
	}
}

/// Asks the traces where the reader is, the first that answers. No answer is elsewhere: GitHub
/// first, the CDN behind it.
pub async fn region(client: &reqwest::Client) -> Region {
	for trace in TRACES {
		if let Ok(response) = client.get(trace).send().await
			&& let Ok(text) = response.text().await
		{
			return region_of_trace(&text);
		}
	}
	Region::Elsewhere
}

/// Fetches the channel's manifest by the first route that answers with one.
pub async fn fetch(
	client: &reqwest::Client,
	channel: Channel,
	region: Region,
) -> Result<Manifest, String> {
	let mut last = String::from("no route");
	for url in routes(channel, region, "latest.json") {
		match client.get(&url).send().await {
			Ok(response) if response.status().is_success() => match response.text().await {
				Ok(text) => return Manifest::parse(&text),
				Err(e) => last = e.to_string(),
			},
			Ok(response) => last = format!("{url}: {}", response.status()),
			Err(e) => last = e.to_string(),
		}
	}
	Err(last)
}

/// Fetches one file by the first address that delivers it whole, into `dest`, and checks it
/// against `sha256` as it lands; `progress` is told the bytes so far and the total when the
/// server said one. A file that does not match is removed and the next address tried, since
/// a mirror can be stale; the last error is the one reported.
pub async fn download(
	client: &reqwest::Client,
	urls: &[String],
	dest: &Path,
	sha256: &str,
	progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
) -> Result<(), String> {
	use futures::StreamExt;
	let mut last = String::from("no address");
	for url in urls {
		let attempt: Result<(), String> = async {
			let response = client.get(url).send().await.map_err(|e| e.to_string())?;
			if !response.status().is_success() {
				return Err(format!("{url}: {}", response.status()));
			}
			let total = response.content_length();
			let mut file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
			let mut hasher = sha2::Sha256::new();
			let mut done = 0u64;
			let mut stream = response.bytes_stream();
			while let Some(chunk) = stream.next().await {
				let chunk = chunk.map_err(|e| e.to_string())?;
				file.write_all(&chunk).map_err(|e| format!("{}: {e}", dest.display()))?;
				hasher.update(&chunk);
				done += chunk.len() as u64;
				progress(done, total);
			}
			file.flush().map_err(|e| e.to_string())?;
			let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
			if digest != sha256 {
				return Err(format!("{url}: the file is not the one the manifest names"));
			}
			Ok(())
		}
		.await;
		match attempt {
			Ok(()) => return Ok(()),
			Err(error) => {
				let _ = std::fs::remove_file(dest);
				last = error;
			}
		}
	}
	Err(last)
}

/// A client for these small requests: short timeouts, since a check is repeated soon anyway.
pub fn client() -> reqwest::Client {
	crate::tls::install();
	reqwest::Client::builder()
		.user_agent(format!("rdm/{}", identity::VERSION))
		.connect_timeout(Duration::from_secs(10))
		.timeout(Duration::from_secs(20))
		.build()
		.expect("a client with no proxy settings to fail on")
}

/// A client for the file itself: no whole-request timeout, since a build is minutes on a slow
/// line, only the connect and a read that stalls.
pub fn file_client() -> reqwest::Client {
	crate::tls::install();
	reqwest::Client::builder()
		.user_agent(format!("rdm/{}", identity::VERSION))
		.connect_timeout(Duration::from_secs(10))
		.read_timeout(Duration::from_secs(60))
		.build()
		.expect("a client with no proxy settings to fail on")
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The rule the corner's card and the system's notification are both held to: the same news
	/// is told once. A restart is what used to undo it, since what had been told lived only in
	/// the run that told it.
	#[test]
	fn an_update_is_told_once_and_a_higher_one_is_told_again() {
		assert!(worth_telling("2026.9.6", 41, None), "nothing told yet, so anything is news");
		assert!(!worth_telling("2026.9.6", 41, Some(("2026.9.6", 41))), "the same build is not");
		assert!(worth_telling("2026.9.6", 42, Some(("2026.9.6", 41))), "the same day, built again");
		assert!(!worth_telling("2026.9.6", 40, Some(("2026.9.6", 41))), "and not an older build");
		assert!(worth_telling("2026.9.7", 1, Some(("2026.9.6", 99))), "a later day, whatever its run");
		assert!(!worth_telling("2026.9.5", 99, Some(("2026.9.6", 1))), "an earlier day is not news");
	}

	/// Read as text, "2026.10.1" sorts before "2026.9.6"; the version is the day it was made and
	/// October comes after September.
	#[test]
	fn a_version_is_ordered_by_its_numbers_and_not_by_its_text() {
		assert!("2026.10.1" < "2026.9.6", "which is what reading them as text would say");
		assert!(worth_telling("2026.10.1", 1, Some(("2026.9.6", 1))), "and what the numbers say");
		assert!(!worth_telling("2026.9.6", 1, Some(("2026.10.1", 1))));
	}

	const MANIFEST: &str = r#"{
		"channel": "nightly", "version": "2026.9.5", "build": 8, "sha": "abc",
		"assets": [
			{ "target": "linux-x64", "kind": "AppImage", "file": "rdm-nightly-linux-x64.AppImage", "size": 1, "sha256": "aa" },
			{ "target": "linux-x64", "kind": "tar.gz", "file": "rdm-nightly-linux-x64.tar.gz", "size": 1, "sha256": "bb" },
			{ "target": "linux-arm64", "kind": "AppImage", "file": "rdm-nightly-linux-arm64.AppImage", "size": 1, "sha256": "ee" },
			{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 1, "sha256": "cc" },
			{ "target": "windows-x64", "kind": "zip", "file": "rdm-nightly-windows-x64.zip", "size": 1, "sha256": "dd" }
		]
	}"#;

	#[test]
	fn the_manifest_names_the_files_of_every_system_by_target_and_kind() {
		let manifest = Manifest::parse(MANIFEST).unwrap();
		assert_eq!(manifest.build, 8);
		let of = |target: &str| manifest.assets.iter().filter(|a| a.target == target).count();
		assert_eq!((of("linux-x64"), of("macos-arm64"), of("macos-x64")), (2, 1, 0));
		assert_eq!(manifest.assets[3].file, "rdm-nightly-macos-arm64.dmg");
	}

	#[test]
	fn a_newer_build_is_available_and_an_older_or_equal_one_is_not() {
		let manifest = Manifest::parse(MANIFEST).unwrap();
		let newer = compare(&manifest, Some(7)).expect("8 is newer than 7");
		assert_eq!((newer.build, newer.version.as_str()), (8, "2026.9.5"));
		assert!(newer.assets.iter().all(|a| a.target == identity::TARGET));
		assert!(
			newer.asset(install::Place::Bundle(Default::default()).kind()).is_some()
				|| !cfg!(target_os = "macos")
		);
		assert!(compare(&manifest, Some(8)).is_none());
		assert!(compare(&manifest, Some(9)).is_none());
		assert!(compare(&manifest, None).is_some(), "a hand build has no number to be ahead of");
	}

	#[test]
	fn china_goes_to_the_cdn_first_and_everywhere_else_to_github() {
		let [first, second] = routes(Channel::Nightly, Region::China, "latest.json");
		assert!(first.starts_with("https://cdn.ffoni.com/github/release/rdm/nightly/"), "{first}");
		assert!(
			second.starts_with("https://github.com/canmi21/rdm/releases/download/nightly/"),
			"{second}"
		);
		let [first, _] = routes(Channel::Nightly, Region::Elsewhere, "x.dmg");
		assert_eq!(first, "https://github.com/canmi21/rdm/releases/download/nightly/x.dmg");
	}

	#[test]
	fn a_trace_says_where_the_reader_is_and_anything_unclear_is_elsewhere() {
		assert_eq!(region_of_trace("fl=1\nip=1.2.3.4\nloc=CN\ncolo=HKG\n"), Region::China);
		assert_eq!(region_of_trace("loc=US\n"), Region::Elsewhere);
		assert_eq!(region_of_trace("colo=IAD\n"), Region::Elsewhere);
		assert_eq!(region_of_trace(""), Region::Elsewhere);
	}

	/// Needs the network; run by hand with `--ignored`. See spec/workflow.md.
	#[tokio::test]
	#[ignore]
	async fn the_traces_answer_and_the_nightly_manifest_is_read_by_a_route() {
		let client = client();
		let region = region(&client).await;
		let manifest = fetch(&client, Channel::Nightly, region).await.unwrap();
		assert_eq!(manifest.channel, "nightly");
		assert!(
			manifest.build > 0 && manifest.assets.iter().any(|a| a.target == identity::TARGET),
			"{manifest:?}"
		);
	}

	#[tokio::test]
	async fn a_file_is_fetched_by_the_first_address_that_delivers_what_the_manifest_names() {
		use crate::engine::testing::{Options, TestServer, body};
		let data = body(50_000);
		let server = TestServer::start(data.clone(), Options::default());
		let dir = crate::testing::scratch("update-download");
		let dest = dir.join("file.bin");
		let digest: String = sha2::Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
		let dead = "http://127.0.0.1:1/file.bin".to_owned();
		let good = server.url("/file.bin").to_string();
		let client = file_client();
		let seen = std::sync::Mutex::new(0u64);
		let progress = |done: u64, _total: Option<u64>| *seen.lock().unwrap() = done;
		download(&client, &[dead.clone(), good.clone()], &dest, &digest, &progress).await.unwrap();
		assert_eq!(std::fs::read(&dest).unwrap(), data, "the dead address was passed over");
		assert_eq!(*seen.lock().unwrap(), data.len() as u64);
		let wrong = "0".repeat(64);
		let error = download(&client, &[good], &dest, &wrong, &progress).await.unwrap_err();
		assert!(error.contains("not the one the manifest names"), "{error}");
		assert!(!dest.exists(), "a file that does not match is not kept");
		let error = download(&client, &[dead], &dest, &digest, &progress).await.unwrap_err();
		assert!(!error.is_empty());
	}

	#[test]
	fn the_channel_and_the_policy_are_spelled_lowercase_in_the_file() {
		assert_eq!(serde_json::to_string(&Channel::Nightly).unwrap(), "\"nightly\"");
		assert_eq!(serde_json::from_str::<Channel>("\"nightly\"").unwrap(), Channel::Nightly);
		assert_eq!(serde_json::to_string(&Policy::Install).unwrap(), "\"install\"");
		assert_eq!(serde_json::from_str::<Policy>("\"download\"").unwrap(), Policy::Download);
		assert_eq!(Policy::default(), Policy::Install);
	}
}
