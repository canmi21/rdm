//! Whether a newer build is published: the nightly's `latest.json`, read every few minutes from
//! wherever answers, compared by build number with the one this binary was made as. Only the
//! noticing is here; fetching the file and replacing the binary are not written yet. See
//! spec/release.md.

use std::time::Duration;

use serde::Deserialize;

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

	/// The file for this system, the first named for it: the dmg, the zip, the AppImage. The
	/// Linux tarball is listed after the AppImage, so the AppImage is what a Linux build gets.
	pub fn asset_for(&self, target: &str) -> Option<&Asset> {
		self.assets.iter().find(|a| a.target == target)
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

/// A build newer than this one, and where its file for this system is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Available {
	pub build: u64,
	pub version: String,
	pub file: String,
	pub sha256: String,
}

/// What a manifest means for this binary: a newer build, or nothing to do. A hand build has no
/// number, so anything published is newer than it; the caller decides whether to say so.
pub fn compare(manifest: &Manifest, this: Option<u64>) -> Option<Available> {
	if manifest.build <= this.unwrap_or(0) {
		return None;
	}
	let asset = manifest.asset_for(identity::TARGET)?;
	Some(Available {
		build: manifest.build,
		version: manifest.version.clone(),
		file: asset.file.clone(),
		sha256: asset.sha256.clone(),
	})
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

/// A client for these small requests: short timeouts, since a check is repeated soon anyway.
pub fn client() -> reqwest::Client {
	reqwest::Client::builder()
		.user_agent(format!("rdm/{}", identity::VERSION))
		.connect_timeout(Duration::from_secs(10))
		.timeout(Duration::from_secs(20))
		.build()
		.expect("a client with no proxy settings to fail on")
}

#[cfg(test)]
mod tests {
	use super::*;

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
	fn the_manifest_names_one_file_per_system_and_linux_gets_the_appimage() {
		let manifest = Manifest::parse(MANIFEST).unwrap();
		assert_eq!(manifest.build, 8);
		assert_eq!(manifest.asset_for("linux-x64").unwrap().kind, "AppImage");
		assert_eq!(manifest.asset_for("macos-arm64").unwrap().file, "rdm-nightly-macos-arm64.dmg");
		assert!(manifest.asset_for("macos-x64").is_none());
	}

	#[test]
	fn a_newer_build_is_available_and_an_older_or_equal_one_is_not() {
		let manifest = Manifest::parse(MANIFEST).unwrap();
		let newer = compare(&manifest, Some(7)).expect("8 is newer than 7");
		assert_eq!((newer.build, newer.version.as_str()), (8, "2026.9.5"));
		assert!(newer.file.contains(identity::TARGET));
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
		assert!(manifest.build > 0 && manifest.asset_for(identity::TARGET).is_some(), "{manifest:?}");
	}

	#[test]
	fn the_channel_is_spelled_lowercase_in_the_file() {
		assert_eq!(serde_json::to_string(&Channel::Nightly).unwrap(), "\"nightly\"");
		assert_eq!(serde_json::from_str::<Channel>("\"nightly\"").unwrap(), Channel::Nightly);
	}
}
