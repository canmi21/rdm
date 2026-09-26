//! Keeping the synced layer as the repository has it: every `.toml` under `rules/`, fetched and
//! put in place whole. GitHub and jsDelivr are asked at once for the list; GitHub is used when it
//! answers in time and every file comes from it, and jsDelivr for all of it otherwise, each file
//! then checked against the SHA-256 its list gives. See spec/rules.md.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::rules::checksum::{Algo, Encoding, normalize};

/// The most a rule file may be, and the most files; a listing past either is not rules.
const MOST_BYTES: usize = 1024 * 1024;
const MOST_FILES: usize = 500;

/// Where the lists and files are asked for. `SOURCES` is the repository's; the tests have their own.
#[derive(Clone, Debug)]
pub struct Sources {
	/// GitHub's tree of the branch, recursive.
	pub github_tree: String,
	/// What a path under the repository is appended to for its raw bytes.
	pub github_raw: String,
	/// jsDelivr's flat listing of the branch, with each file's SHA-256.
	pub jsdelivr_list: String,
	pub jsdelivr_raw: String,
	/// How long GitHub's list may take before jsDelivr is used alone.
	pub patience: Duration,
}

impl Sources {
	pub fn repository() -> Sources {
		let repository = crate::identity::REPOSITORY;
		Sources {
			github_tree: format!("https://api.github.com/repos/{repository}/git/trees/main?recursive=1"),
			github_raw: format!("https://raw.githubusercontent.com/{repository}/main/"),
			jsdelivr_list: format!(
				"https://data.jsdelivr.com/v1/packages/gh/{repository}@main?structure=flat"
			),
			jsdelivr_raw: format!("https://cdn.jsdelivr.net/gh/{repository}@main/"),
			patience: Duration::from_secs(8),
		}
	}
}

/// The rule files as fetched, by their path under `rules/`, and who they came from.
#[derive(Debug)]
pub struct Fetched {
	pub files: Vec<(String, Vec<u8>)>,
	pub from: &'static str,
}

/// Every rule file, from GitHub when it answers in time and serves them all, from jsDelivr
/// otherwise. Nothing partial: either every file listed arrived, or this is an error.
pub async fn fetch(client: reqwest::Client, sources: Sources) -> Result<Fetched, String> {
	let jsdelivr = {
		let (client, url) = (client.clone(), sources.jsdelivr_list.clone());
		tokio::spawn(async move { list_jsdelivr(&client, &url).await })
	};
	let github =
		tokio::time::timeout(sources.patience, list_github(&client, &sources.github_tree)).await;
	if let Ok(Ok(paths)) = github {
		let mut files = Vec::new();
		for path in &paths {
			match get(&client, &format!("{}rules/{path}", sources.github_raw), sources.patience).await {
				Some(bytes) => files.push((path.clone(), bytes)),
				None => break,
			}
		}
		if files.len() == paths.len() {
			jsdelivr.abort();
			return Ok(Fetched { files, from: "GitHub" });
		}
	}
	let listed = jsdelivr.await.map_err(|e| e.to_string())??;
	let mut files = Vec::new();
	for (path, hash) in listed {
		let bytes = get(&client, &format!("{}rules/{path}", sources.jsdelivr_raw), sources.patience)
			.await
			.ok_or_else(|| format!("rules/{path} did not arrive"))?;
		if let Some(expected) = normalize(&hash, Some(Algo::Sha256), Some(Encoding::Base64)) {
			let computed = sha256(&bytes);
			if computed != expected.expected() {
				return Err(format!("rules/{path} is not what jsDelivr listed"));
			}
		}
		files.push((path, bytes));
	}
	Ok(Fetched { files, from: "jsDelivr" })
}

/// The `.toml` paths under `rules/` in GitHub's tree of the branch.
async fn list_github(client: &reqwest::Client, url: &str) -> Result<Vec<String>, String> {
	let document = json(client, url).await?;
	let tree = document.get("tree").and_then(Value::as_array).ok_or("no tree in GitHub's answer")?;
	let paths: Vec<String> = tree
		.iter()
		.filter(|item| item.get("type").and_then(Value::as_str) == Some("blob"))
		.filter_map(|item| item.get("path").and_then(Value::as_str))
		.filter_map(|path| path.strip_prefix("rules/"))
		.filter(|path| path.ends_with(".toml"))
		.map(str::to_owned)
		.collect();
	checked(paths)
}

/// The `.toml` paths under `rules/` in jsDelivr's listing, each with the SHA-256 it gives.
async fn list_jsdelivr(
	client: &reqwest::Client,
	url: &str,
) -> Result<Vec<(String, String)>, String> {
	let document = json(client, url).await?;
	let files =
		document.get("files").and_then(Value::as_array).ok_or("no files in jsDelivr's answer")?;
	let listed: Vec<(String, String)> = files
		.iter()
		.filter_map(|file| {
			let name = file.get("name").and_then(Value::as_str)?.strip_prefix("/rules/")?;
			let hash = file.get("hash").and_then(Value::as_str)?;
			name.ends_with(".toml").then(|| (name.to_owned(), hash.to_owned()))
		})
		.collect();
	checked(listed.iter().map(|(path, _)| path.clone()).collect())?;
	Ok(listed)
}

/// A listing that could be rules: some files, not too many, none reaching outside the folder.
fn checked(paths: Vec<String>) -> Result<Vec<String>, String> {
	if paths.is_empty() {
		return Err("the repository lists no rules".to_owned());
	}
	if paths.len() > MOST_FILES {
		return Err(format!("{} rule files is more than rules are", paths.len()));
	}
	if let Some(bad) = paths.iter().find(|p| p.split('/').any(|part| part == ".." || part.is_empty()))
	{
		return Err(format!("{bad} is not a path inside rules/"));
	}
	Ok(paths)
}

async fn json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
	let bytes = get(client, url, Duration::from_secs(15))
		.await
		.ok_or_else(|| format!("{url} did not answer"))?;
	serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn get(client: &reqwest::Client, url: &str, patience: Duration) -> Option<Vec<u8>> {
	let response = tokio::time::timeout(patience, client.get(url).send()).await.ok()?.ok()?;
	if !response.status().is_success() {
		return None;
	}
	let bytes = tokio::time::timeout(patience, response.bytes()).await.ok()?.ok()?;
	(bytes.len() <= MOST_BYTES).then(|| bytes.to_vec())
}

fn sha256(bytes: &[u8]) -> String {
	use sha2::Digest;
	sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The synced layer made the fetched files and nothing else: written to a folder beside it, which
/// then takes its place, so a sync that fails half way leaves the last one standing. The folder is
/// the application's; what a user put there goes. See spec/rules.md.
pub fn apply(synced: &Path, fetched: &Fetched) -> std::io::Result<()> {
	let staging = synced.with_extension("incoming");
	let _ = std::fs::remove_dir_all(&staging);
	for (path, bytes) in &fetched.files {
		let target = staging.join(path);
		if let Some(parent) = target.parent() {
			std::fs::create_dir_all(parent)?;
		}
		std::fs::write(target, bytes)?;
	}
	let _ = std::fs::remove_dir_all(synced);
	std::fs::rename(&staging, synced)
}
