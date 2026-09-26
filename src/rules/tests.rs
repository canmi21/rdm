use std::path::PathBuf;
use std::sync::Arc;

use super::checksum::{self, Algo, Encoding, Source};
use super::resolve::{Resolution, family_swaps, resolve, same_site};
use super::template::{self, Template};
use super::*;
use crate::engine::Checksum;
use crate::engine::testing::{Options, TestServer, body};
use crate::testing::scratch;

fn repository_rules() -> Texts {
	read_tree(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("rules"))
}

#[test]
fn every_rule_in_the_repository_reads_and_matches_its_own_examples() {
	let texts = repository_rules();
	assert!(
		texts.len() >= 5,
		"the tree is read at every depth: {:?}",
		texts.iter().map(|t| &t.0).collect::<Vec<_>>()
	);
	let compiled = compile(&[(Layer::Synced, texts)]);
	let problems: Vec<_> = compiled.problems.iter().filter(|p| !p.contains("authority")).collect();
	assert!(problems.is_empty(), "{problems:?}");
	for entry in &compiled.entries {
		let pattern = Template::parse(&entry.pattern).unwrap();
		assert!(!entry.examples.is_empty(), "{} has no example", entry.id);
		for example in &entry.examples {
			let captures =
				pattern.matches(example).unwrap_or_else(|| panic!("{} does not match {example}", entry.id));
			for mirror in &entry.mirror {
				let filled =
					template::fill(mirror, &captures).unwrap_or_else(|e| panic!("{}: {e}", entry.id));
				assert!(reqwest::Url::parse(&filled).is_ok(), "{}: {filled}", entry.id);
			}
			for source in &entry.checksum {
				for text in source.templates() {
					template::fill(text, &captures).unwrap_or_else(|e| panic!("{}: {e}", entry.id));
				}
			}
		}
	}
	for family in &compiled.families {
		for example in &family.examples {
			assert!(family.prefixes.iter().any(|p| example.starts_with(p)), "{}: {example}", family.id);
			assert_eq!(family_swaps(&compiled, example).len(), family.prefixes.len() - 1);
		}
	}
	// Every built-in file is one of the repository's.
	for (path, _) in built_in() {
		assert!(
			repository_rules().iter().any(|(p, _)| *p == path),
			"{path} is built in and not in rules/"
		);
	}
}

#[test]
fn a_template_splits_a_name_from_its_version_and_ignores_the_query() {
	let npm = Template::parse(
		"https://registry.npmjs.org/{package:(?:@[^/]+/)?[^/]+}/-/{base}-{version:[0-9][^/]*}.tgz",
	)
	.unwrap();
	let dom = npm.matches("https://registry.npmjs.org/react-dom/-/react-dom-19.1.0.tgz").unwrap();
	assert_eq!((dom["package"].as_str(), dom["version"].as_str()), ("react-dom", "19.1.0"));
	let scoped = npm.matches("https://registry.npmjs.org/@types/node/-/node-22.0.0.tgz?x=1").unwrap();
	assert_eq!(scoped["package"], "@types/node");
	assert_eq!(
		scoped["url"], "https://registry.npmjs.org/@types/node/-/node-22.0.0.tgz",
		"the query is left off"
	);
	let braces = Template::parse("https://x.test/{sha:[0-9a-f]{7,40}}/{path+}").unwrap();
	assert_eq!(braces.matches("https://x.test/abcdef1/a/b.txt").unwrap()["path"], "a/b.txt");
	assert!(braces.matches("https://x.test/main/a.txt").is_none(), "a branch is not a commit");
	assert!(Template::parse("https://x.test/{url}").is_err());
	assert!(template::fill("{missing}", &template::Captures::new()).is_err());
}

#[test]
fn layers_merge_by_priority_and_the_highest_match_is_used_whole() {
	let rule = |id: &str, priority: i32| {
		format!(
			"[[entry]]\nid = \"{id}\"\npriority = {priority}\nmatch = \"https://x.test/{{file}}\"\nmirror = [\"https://{id}.test/{{file}}\"]\n"
		)
	};
	let compiled = compile(&[
		(Layer::BuiltIn, vec![("a.toml".into(), rule("built-in", 0))]),
		(
			Layer::Synced,
			vec![("b.toml".into(), rule("synced", 0) + "[[authority]]\nhost = \"evil.test\"\n")],
		),
		(
			Layer::Custom,
			vec![("c.toml".into(), rule("custom-low", -5)), ("d.toml".into(), "not toml [".into())],
		),
	]);
	let order: Vec<&str> = compiled.entries.iter().map(|e| e.id.as_str()).collect();
	assert_eq!(order, ["synced", "built-in", "custom-low"], "priority, then the higher layer");
	assert_eq!(compiled.entry_for("https://x.test/f.bin").unwrap().0.id, "synced");
	assert!(!compiled.is_authority("evil.test"), "a sync cannot name an authority");
	assert_eq!(compiled.problems.len(), 2, "{:?}", compiled.problems);
	let raised = compile(&[
		(Layer::BuiltIn, vec![("a.toml".into(), rule("built-in", 0))]),
		(Layer::Custom, vec![("c.toml".into(), rule("custom", 0))]),
	]);
	assert_eq!(
		raised.entry_for("https://x.test/f.bin").unwrap().0.id,
		"custom",
		"at equal priority the user's"
	);
}

#[test]
fn a_choice_is_remembered_for_the_domain_and_the_user_s_wins() {
	let dir = scratch("choices");
	remember(&dir, "ftp.gnu.org", Choice::Auto).unwrap();
	remember(&dir, "ftp.gnu.org", Choice::Never).unwrap();
	remember(&dir, "example.org", Choice::Auto).unwrap();
	let compiled = compile(&[
		(
			Layer::Synced,
			vec![("s.toml".into(), "[[domain]]\nhost = \"example.org\"\nmirror = \"never\"\n".into())],
		),
		(Layer::Custom, read_tree(&dir)),
	]);
	assert_eq!(
		compiled.choice_for("ftp.gnu.org"),
		Some(Choice::Never),
		"the later choice replaced the first"
	);
	assert_eq!(
		compiled.choice_for("mirror.example.org"),
		Some(Choice::Auto),
		"a domain covers its hosts; the user's wins"
	);
	assert_eq!(compiled.choice_for("gnu.org"), None);
	assert_eq!(domain_of("www.Example.org"), "example.org");
}

#[test]
fn a_checksum_is_read_in_every_form_it_comes_in() {
	let hex = "4cf9f2741e6c465ffdb7c26f38056a59e2a2544b51f7cc128ef28337eeae4d8e";
	let want = Checksum::Sha256(hex.into());
	assert_eq!(checksum::normalize(&format!("sha256:{hex}"), None, None), Some(want.clone()));
	assert_eq!(checksum::normalize(hex, Some(Algo::Sha256), None), Some(want.clone()));
	// The same bytes as base64, as jsDelivr gives them.
	let base64 = "TPnydB5sRl/9t8JvOAVqWeKiVEtR98wSjvKDN+6uTY4=";
	assert_eq!(
		checksum::normalize(base64, Some(Algo::Sha256), Some(Encoding::Base64)),
		Some(want.clone())
	);
	assert_eq!(
		checksum::normalize(&format!("sha256-{base64}"), None, None),
		Some(want.clone()),
		"npm's form"
	);
	let sums = format!("{hex}  ripgrep.tar.gz\n0000  other\nSHA256 (bsd.bin) = {hex}\n");
	assert_eq!(checksum::from_sums(&sums, "ripgrep.tar.gz", None), Some(want.clone()));
	assert_eq!(checksum::from_sums(&sums, "bsd.bin", None), Some(want));
	assert_eq!(checksum::from_sums(&sums, "missing", None), None);
	assert!(checksum::glob("*checksums*", "gh_2.1_checksums.txt"));
	assert!(checksum::glob("SHA256SUMS*", "SHA256SUMS"));
	assert!(!checksum::glob("*checksums*", "gh_2.1_linux.tar.gz"));
	let document: serde_json::Value = serde_json::from_str(
		r#"{"assets":[{"name":"a","digest":"x"},{"name":"b.tar.gz","digest":"y"}]}"#,
	)
	.unwrap();
	assert_eq!(checksum::select(&document, "assets[name=b.tar.gz].digest").unwrap(), "y");
	assert!(checksum::select(&document, "assets[name=c].digest").is_none());
}

fn captures_for(url: &str, pattern: &str) -> template::Captures {
	Template::parse(pattern).unwrap().matches(url).unwrap()
}

#[tokio::test]
async fn a_source_that_fails_is_the_next_one_s_turn() {
	let hex = "4cf9f2741e6c465ffdb7c26f38056a59e2a2544b51f7cc128ef28337eeae4d8e";
	let api = TestServer::start(
		r#"{"assets":[{"name":"f.bin","digest":null},{"name":"SHA256SUMS","browser_download_url":"SUMS"}]}"#.to_owned()
			.into_bytes(),
		Options::default(),
	);
	let sums = TestServer::start(format!("{hex}  f.bin\n").into_bytes(), Options::default());
	let document = api.body();
	let listing = String::from_utf8(document)
		.unwrap()
		.replace("SUMS\"}", &format!("{}\"}}", sums.url("/SHA256SUMS")));
	api.set_body(listing.into_bytes());
	let captures =
		captures_for(&format!("{}f.bin", api.url("/")), &format!("{}{{file}}", api.url("/")));
	let client = reqwest::Client::new();
	let digest = Source::Json {
		url: api.url("/api").to_string(),
		pick: "assets[name={file}].digest".into(),
		algo: None,
		encoding: None,
	};
	assert_eq!(
		checksum::read(&client, &digest, &captures).await,
		None,
		"a null digest is no checksum"
	);
	let listed = Source::Sums {
		url: api.url("/api").to_string(),
		items: "assets".into(),
		name: "name".into(),
		link: "browser_download_url".into(),
		like: vec!["SHA256SUMS*".into()],
		algo: Some(Algo::Sha256),
	};
	assert_eq!(checksum::read(&client, &listed, &captures).await, Some(Checksum::Sha256(hex.into())));
}

#[tokio::test]
async fn a_mirror_is_kept_only_when_it_serves_the_same_file_and_used_only_with_a_checksum_from_elsewhere()
 {
	let data = body(50_000);
	let origin = TestServer::start(data.clone(), Options::default());
	let same = TestServer::start(data.clone(), Options::default());
	let short = TestServer::start(body(40_000), Options::default());
	let family = format!(
		"[[family]]\nid = \"t\"\nprefixes = [\"{}\", \"{}\", \"{}\"]\n",
		origin.url("/"),
		same.url("/"),
		short.url("/")
	);
	let rules = Arc::new(compile(&[(Layer::Synced, vec![("t.toml".into(), family)])]));
	let client = crate::engine::client::build(&crate::engine::Settings::default(), false).unwrap();
	let url = origin.url("/pkg/f.bin");
	let probe = crate::engine::probe(&client, url.clone()).await.unwrap();
	let found = resolve(rules.clone(), client, url, probe).await;
	assert_eq!(
		found.mirrors,
		vec![same.url("/pkg/f.bin")],
		"the one of another size is not the same file"
	);
	assert!(found.checksum.is_none());
	assert!(found.usable(&rules, false).is_empty(), "no checksum, no mirror");
	assert_eq!(found.usable(&rules, true).len(), 1, "a checksum the user typed is the source's");
	let from_mirror = Resolution {
		checksum: Some((Checksum::Md5("0".repeat(32)), "127.0.0.1".into())),
		..found.clone()
	};
	assert!(from_mirror.usable(&rules, false).is_empty(), "a mirror cannot vouch for itself");
	assert!(
		same_site("api.github.com", "github.com") && !same_site("fastly.jsdelivr.net", "github.com")
	);
}

/// Each built-in example that names a checksum source, answered by the real services.
#[tokio::test]
#[ignore = "reaches GitHub, npm, PyPI and jsDelivr"]
async fn the_built_in_rules_find_real_checksums() {
	let rules = compile(&[(Layer::BuiltIn, built_in())]);
	let client = crate::engine::client::build(&crate::engine::Settings::default(), false).unwrap();
	for entry in rules.entries.iter().filter(|e| !e.checksum.is_empty()) {
		for example in &entry.examples {
			let captures = Template::parse(&entry.pattern).unwrap().matches(example).unwrap();
			let mut found = None;
			for source in &entry.checksum {
				found = checksum::read(&client, source, &captures).await;
				if found.is_some() {
					break;
				}
			}
			assert!(found.is_some(), "{}: no checksum for {example}", entry.id);
		}
	}
}

#[test]
fn the_user_s_order_moves_a_rule_of_any_layer_and_a_sync_s_does_not() {
	let rule =
		|id: &str| format!("[[entry]]\nid = \"{id}\"\nmatch = \"https://{id}.test/{{file}}\"\n");
	let dir = scratch("order");
	let layers = |custom: Texts| {
		compile(&[
			(Layer::BuiltIn, vec![("a.toml".into(), rule("first") + &rule("second"))]),
			(
				Layer::Synced,
				vec![("s.toml".into(), "[[order]]\nid = \"second\"\npriority = 99\n".into())],
			),
			(Layer::Custom, custom),
		])
	};
	let ids = |c: &Compiled| c.entries.iter().map(|e| e.id.clone()).collect::<Vec<_>>();
	assert_eq!(
		ids(&layers(read_tree(&dir))),
		["first", "second"],
		"a sync does not order the user's rules"
	);
	set_priorities(&dir, &[("second".into(), 1)]).unwrap();
	let moved = layers(read_tree(&dir));
	assert_eq!(ids(&moved), ["second", "first"]);
	assert_eq!(
		moved.entries[0].layer,
		Layer::BuiltIn,
		"moved, not copied: it is still the built-in rule"
	);
	set_priorities(&dir, &[("second".into(), -1)]).unwrap();
	assert_eq!(
		ids(&layers(read_tree(&dir))),
		["first", "second"],
		"moving again replaces the first move"
	);
	remember(&dir, "example.org", Choice::Never).unwrap();
	forget(&dir, "example.org").unwrap();
	assert_eq!(layers(read_tree(&dir)).choice_for("example.org"), None, "forgotten");
}

fn b64(bytes: &[u8]) -> String {
	const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
	let mut out = String::new();
	for chunk in bytes.chunks(3) {
		let n =
			chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
		for i in 0..4 {
			if i <= chunk.len() {
				out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
			} else {
				out.push('=');
			}
		}
	}
	out
}

/// A repository of one rule file, `a.toml`, as GitHub and jsDelivr would list and serve it.
struct Repo {
	_servers: Vec<TestServer>,
	sources: super::sync::Sources,
}

fn repo(github_tree: Options, listed_hash: Option<String>) -> Repo {
	use sha2::Digest;
	let file = b"[[entry]]\nid = \"a\"\nmatch = \"https://a.test/{file}\"\n".to_vec();
	let hash = listed_hash.unwrap_or_else(|| b64(&sha2::Sha256::digest(&file)));
	// Padded past one chunk, so a slow server is slow before the list is whole.
	let tree = format!(
		"{{\"tree\":[{{\"path\":\"rules\",\"type\":\"tree\"}},{{\"path\":\"rules/a.toml\",\"type\":\"blob\"}},{{\"path\":\"README.md\",\"type\":\"blob\"}}]}}{}",
		" ".repeat(9000)
	);
	let list = format!(
		"{{\"files\":[{{\"name\":\"/rules/a.toml\",\"hash\":\"{hash}\"}},{{\"name\":\"/src/main.rs\",\"hash\":\"x\"}}]}}"
	);
	let github = TestServer::start(tree.into_bytes(), github_tree);
	let raw = TestServer::start(file.clone(), Options::default());
	let jsdelivr = TestServer::start(list.into_bytes(), Options::default());
	let cdn = TestServer::start(file, Options::default());
	let sources = super::sync::Sources {
		github_tree: github.url("/tree").to_string(),
		github_raw: raw.url("/").to_string(),
		jsdelivr_list: jsdelivr.url("/list").to_string(),
		jsdelivr_raw: cdn.url("/").to_string(),
		patience: std::time::Duration::from_millis(500),
	};
	Repo { _servers: vec![github, raw, jsdelivr, cdn], sources }
}

#[tokio::test]
async fn the_rules_come_from_github_when_it_answers_and_from_jsdelivr_when_it_does_not() {
	let client = reqwest::Client::new();
	let fine = repo(Options::default(), None);
	let fetched = super::sync::fetch(client.clone(), fine.sources.clone()).await.unwrap();
	assert_eq!(fetched.from, "GitHub");
	assert_eq!(
		fetched.files.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
		["a.toml"],
		"only rules/*.toml"
	);
	let down = repo(Options { status: Some(503), ..Options::default() }, None);
	assert_eq!(
		super::sync::fetch(client.clone(), down.sources.clone()).await.unwrap().from,
		"jsDelivr"
	);
	let slow = repo(
		Options { delay_per_chunk: std::time::Duration::from_secs(3), ..Options::default() },
		None,
	);
	let started = std::time::Instant::now();
	assert_eq!(
		super::sync::fetch(client.clone(), slow.sources.clone()).await.unwrap().from,
		"jsDelivr"
	);
	assert!(started.elapsed() < std::time::Duration::from_secs(3), "GitHub was not waited out");
	let wrong = repo(Options { status: Some(503), ..Options::default() }, Some(b64(&[0u8; 32])));
	let refused = super::sync::fetch(client, wrong.sources.clone()).await.unwrap_err();
	assert!(refused.contains("not what jsDelivr listed"), "{refused}");
}

#[test]
fn a_sync_replaces_the_synced_layer_whole() {
	let dir = scratch("sync-apply");
	let synced = dir.join("rules");
	std::fs::create_dir_all(synced.join("old")).unwrap();
	std::fs::write(synced.join("old/stale.toml"), "x").unwrap();
	let fetched = super::sync::Fetched {
		files: vec![("code/github.toml".into(), b"a".to_vec()), ("npm.toml".into(), b"b".to_vec())],
		from: "GitHub",
	};
	super::sync::apply(&synced, &fetched).unwrap();
	let names: Vec<String> = read_tree(&synced).into_iter().map(|(p, _)| p).collect();
	assert_eq!(names, ["code/github.toml", "npm.toml"], "what the repository has, and nothing else");
	assert!(!dir.join("rules.incoming").exists());
}

#[tokio::test]
#[ignore = "reaches GitHub and jsDelivr"]
async fn the_repository_s_rules_arrive_and_read() {
	let client = crate::engine::client::build(&crate::engine::Settings::default(), false).unwrap();
	let fetched = super::sync::fetch(client, super::sync::Sources::repository()).await.unwrap();
	let texts: Texts = fetched
		.files
		.iter()
		.map(|(p, b)| (p.clone(), String::from_utf8_lossy(b).into_owned()))
		.collect();
	let compiled = compile(&[(Layer::Synced, texts)]);
	assert!(!compiled.entries.is_empty() && !compiled.families.is_empty(), "from {}", fetched.from);
}

#[test]
fn a_rule_of_the_same_id_in_a_higher_layer_replaces_the_lower_one() {
	let rule = |mirror: &str| {
		format!(
			"[[entry]]\nid = \"same\"\nmatch = \"https://x.test/{{file}}\"\nmirror = [\"{mirror}\"]\n"
		)
	};
	let compiled = compile(&[
		(Layer::BuiltIn, vec![("a.toml".into(), rule("https://built-in.test/{file}"))]),
		(Layer::Synced, vec![("a.toml".into(), rule("https://synced.test/{file}"))]),
	]);
	assert_eq!(compiled.entries.len(), 1, "one rule to an id");
	assert_eq!(compiled.entries[0].layer, Layer::Synced);
	let built_in_and_repository =
		compile(&[(Layer::BuiltIn, built_in()), (Layer::Synced, repository_rules())]);
	let ids: Vec<&str> = built_in_and_repository.entries.iter().map(|e| e.id.as_str()).collect();
	let mut unique = ids.clone();
	unique.dedup();
	assert_eq!(
		ids.len(),
		compile(&[(Layer::Synced, repository_rules())]).entries.len(),
		"the synced copy stands in for the built-in one"
	);
	assert_eq!(ids.len(), unique.len());
}

#[test]
fn a_synced_authority_is_a_problem_only_when_it_is_not_one_already() {
	let file = |host: &str| format!("[[authority]]\nhost = \"{host}\"\n");
	let compiled = compile(&[
		(Layer::BuiltIn, vec![("a.toml".into(), file("cdn.test"))]),
		(Layer::Synced, vec![("a.toml".into(), file("cdn.test") + &file("evil.test"))]),
	]);
	assert_eq!(compiled.authorities, ["cdn.test"]);
	assert_eq!(compiled.problems.len(), 1, "{:?}", compiled.problems);
	assert!(compiled.problems[0].contains("evil.test"));
	let repository = compile(&[(Layer::BuiltIn, built_in()), (Layer::Synced, repository_rules())]);
	assert!(repository.problems.is_empty(), "the repository's own files: {:?}", repository.problems);
}
