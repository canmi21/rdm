//! New Task and a download's own settings, with the rules' mirrors and checksums.

use super::*;

#[gpui::test]
fn add_task_reads_the_clipboard_names_junk_and_offers_a_pages_files(cx: &mut TestAppContext) {
	use crate::engine::testing::{Options, TestServer};
	let page = TestServer::start(
		b"<a href=\"tool.zip\">tool</a> <a href=\"notes.pdf\">notes</a>".to_vec(),
		Options { content_type: Some("text/html".into()), ..Options::default() },
	);
	let (rdm, mut cx) = open(cx);
	cx.write_to_clipboard(gpui::ClipboardItem::new_string("example.org/a.zip".into()));
	click(&mut cx, "button:Add Task");
	let input = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().input.clone());
	assert_eq!(
		input.read_with(&cx, |i, _| i.content.to_string()),
		"https://example.org/a.zip",
		"the clipboard is read as an address, scheme supplied"
	);
	cx.update(|window, cx| {
		input.update(cx, |i, cx| i.set_content("not an address at all", cx));
		let _ = window;
	});
	click(&mut cx, "button:Continue");
	assert!(cx.debug_bounds("add-error").is_some(), "junk is named as such");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_some(), "the sheet stays"));

	// A server that refuses is said in a line, and what it said waits behind Details.
	let refusing = TestServer::start(vec![], Options { status: Some(403), ..Options::default() });
	let secret = refusing.url("/secret.bin").to_string();
	cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&secret, cx)));
	click(&mut cx, "button:Continue");
	let mut refused = false;
	for _ in 0..200 {
		std::thread::sleep(Duration::from_millis(10));
		rdm.update(&mut cx, |rdm, cx| rdm.poll_add(cx));
		cx.run_until_parked();
		if rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().is_some_and(|s| s.problem.is_some())) {
			refused = true;
			break;
		}
	}
	assert!(refused, "the refusal came back");
	rdm.read_with(&cx, |rdm, _| {
		let sheet = rdm.adding.as_ref().unwrap();
		let problem = sheet.problem.as_ref().unwrap();
		assert_eq!(problem.summary, "The server answered 403 Forbidden.");
		assert!(problem.detail.is_some(), "the whole text is kept");
		assert!(sheet.found.is_none(), "and the first screen stays");
	});
	assert!(cx.debug_bounds("add-detail").is_none(), "the details start closed");
	click(&mut cx, "button:Details");
	assert!(cx.debug_bounds("add-detail").is_some(), "and open when asked");

	let address = page.url("/downloads/").to_string();
	cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&address, cx)));
	click(&mut cx, "button:Continue");
	// The engine looks at the address on its own threads; the pump collects the answer.
	let mut seen = false;
	for _ in 0..200 {
		std::thread::sleep(Duration::from_millis(10));
		rdm.update(&mut cx, |rdm, cx| rdm.poll_add(cx));
		cx.run_until_parked();
		if rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().is_some_and(|s| s.confirm.is_some())) {
			seen = true;
			break;
		}
	}
	assert!(seen, "the address was recognised as a page");
	assert!(cx.debug_bounds("add-page").is_some());
	click(&mut cx, "link:tool.zip");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_some(), "the sheet stays up for more");
		let added = rdm.downloads.iter().find(|d| d.name == "tool.zip").expect("queued");
		assert!(added.url.ends_with("/downloads/tool.zip"), "{}", added.url);
	});
	click(&mut cx, "link:tool.zip");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.downloads.iter().filter(|d| d.name == "tool.zip").count(), 1, "once");
	});
	click(&mut cx, "button:Close");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none()));
}

#[gpui::test]
fn a_file_is_shown_before_it_is_added_and_its_connections_are_changed_afterwards(
	cx: &mut TestAppContext,
) {
	use crate::engine::testing::{Options, TestServer, body};
	let server = TestServer::start(body(300_000), Options::default());
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Add Task");
	let input = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().input.clone());
	let address = server.url("/tool.bin").to_string();
	cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&address, cx)));
	click(&mut cx, "button:Continue");
	let mut seen = false;
	for _ in 0..200 {
		std::thread::sleep(Duration::from_millis(10));
		rdm.update(&mut cx, |rdm, cx| rdm.poll_add(cx));
		cx.run_until_parked();
		if rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().is_some_and(|s| s.found.is_some())) {
			seen = true;
			break;
		}
	}
	assert!(seen, "the address was looked at and found to be a file");
	assert!(cx.debug_bounds("add-found").is_some(), "the file is shown before it is added");
	rdm.read_with(&cx, |rdm, cx| {
		assert!(rdm.downloads.iter().all(|d| d.url != address), "not added yet");
		let sheet = rdm.adding.as_ref().unwrap();
		assert_eq!(sheet.range_end.read(cx).content.as_ref(), "300000", "the whole file, prefilled");
	});
	let (name, checksum) = rdm.read_with(&cx, |rdm, _| {
		let s = rdm.adding.as_ref().unwrap();
		(s.name.clone(), s.checksum.clone())
	});
	cx.update(|_, cx| checksum.update(cx, |i, cx| i.set_content("not-a-hash", cx)));
	click(&mut cx, "button:Download");
	assert!(cx.debug_bounds("add-error").is_some(), "a checksum that is none is refused");
	// More options: the folder, a limit of its own and a part of the file.
	click(&mut cx, "button:More options");
	assert!(cx.debug_bounds("add-more").is_some(), "the fields are shown");
	assert!(
		cx.debug_bounds("button:Back").is_some(),
		"the second screen goes back rather than show the address"
	);
	let (start, end, limit) = rdm.read_with(&cx, |rdm, _| {
		let s = rdm.adding.as_ref().unwrap();
		(s.range_start.clone(), s.range_end.clone(), s.limit.clone())
	});
	// The limit is a slider and a field in MB/s, each following the other.
	cx.update(|_, cx| limit.update(cx, |i, cx| i.set_content("10", cx)));
	cx.run_until_parked();
	let at =
		rdm.read_with(&cx, |rdm, cx| rdm.adding.as_ref().unwrap().limit_slider.read(cx).handles()[0]);
	assert!((at - 0.45).abs() < 1e-3, "ten MB/s is halfway along the scale: {at}");
	rdm.update(&mut cx, |rdm, cx| rdm.slide_limit(1.0, cx));
	cx.run_until_parked();
	assert!(limit.read_with(&cx, |i, _| i.content.is_empty()), "the far end is no limit");
	cx.update(|_, cx| {
		name.update(cx, |i, cx| i.set_content("renamed.bin", cx));
		checksum.update(cx, |i, cx| i.set_content("d41d8cd98f00b204e9800998ecf8427e", cx));
		start.update(cx, |i, cx| i.set_content("0", cx));
		end.update(cx, |i, cx| i.set_content("1000", cx));
		limit.update(cx, |i, cx| i.set_content("500", cx));
	});
	click(&mut cx, "button:Download");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_some(), "a checksum is not kept for a part of the file")
	});
	cx.update(|_, cx| checksum.update(cx, |i, cx| i.set_content("", cx)));
	click(&mut cx, "button:Download");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_none(), "added and closed");
		let row = rdm.downloads.iter().find(|d| d.url == address).expect("the download");
		assert_eq!(row.connections, None, "not asked at New Task: the settings' default, auto");
		assert_eq!(row.name, "renamed.bin");
		assert!(row.mirrors.is_empty(), "mirrors are not asked");
		assert_eq!(row.checksum, None);
		assert_eq!(row.range.as_deref(), Some("0-1000"));
		assert_eq!(row.speed_limit, Some(500 * 1024 * 1024), "the field is MB/s");
	});
	// How it downloads is changed afterwards, as the download's window does, and kept on the row.
	let id = rdm.read_with(&cx, |rdm, _| rdm.downloads.iter().find(|d| d.url == address).unwrap().id);
	rdm.update(&mut cx, |rdm, cx| rdm.set_task_connections(id, Some(4), cx));
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.download(id).and_then(|d| d.connections), Some(4)));
}

#[gpui::test]
fn the_transfer_fields_apply_on_enter_and_say_no_to_nonsense(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	rdm.update(&mut cx, |rdm, cx| {
		rdm.apply_setting("settings.label.speed_limit", "2m", cx);
		rdm.apply_setting("settings.label.connections", "32", cx);
	});
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.speed_limit, Some(2 * 1024 * 1024));
		assert_eq!(rdm.preferences.connections, Some(32));
	});
	rdm.update(&mut cx, |rdm, cx| rdm.apply_setting("settings.label.connections", "lots", cx));
	cx.run_until_parked();
	click(&mut cx, "section:Transfers");
	assert!(cx.debug_bounds("settings-complaint").is_some(), "nonsense is said no to under its row");
	rdm.update(&mut cx, |rdm, cx| {
		rdm.apply_setting("settings.label.connections", "auto", cx);
		rdm.apply_setting("settings.label.speed_limit", "", cx);
	});
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!((rdm.preferences.connections, rdm.preferences.speed_limit), (None, None));
	});
}

/// New Task looked at `address` and the rules answered; the sheet on its second screen.
fn look_until_resolved(rdm: &Entity<Rdm>, cx: &mut VisualTestContext, address: &str) {
	click(cx, "button:Add Task");
	let input = rdm.read_with(cx, |rdm, _| rdm.adding.as_ref().unwrap().input.clone());
	let address = address.to_owned();
	cx.update(|_, cx| input.update(cx, |i, cx| i.set_content(&address, cx)));
	click(cx, "button:Continue");
	for _ in 0..500 {
		std::thread::sleep(Duration::from_millis(10));
		rdm.update(cx, |rdm, cx| rdm.poll_add(cx));
		cx.run_until_parked();
		if rdm.read_with(cx, |rdm, _| {
			rdm.adding.as_ref().is_some_and(|s| s.found.is_some() && s.resolved.is_some())
		}) {
			return;
		}
	}
	panic!("the rules did not answer in time");
}

/// An origin and a mirror serving the same file, a family rule naming both in the synced layer,
/// and the rules reloaded; `custom` goes in the custom layer as it is.
fn mirrored(
	rdm: &Entity<Rdm>,
	cx: &mut VisualTestContext,
	custom: &str,
) -> (TestServerHold, String) {
	use crate::engine::testing::{Options, TestServer, body};
	let data = body(60_000);
	let origin = TestServer::start(data.clone(), Options::default());
	let mirror = TestServer::start(data, Options::default());
	let family = format!(
		"[[family]]\nid = \"t\"\nprefixes = [\"{}\", \"{}\"]\n",
		origin.url("/"),
		mirror.url("/")
	);
	let mirror_address = mirror.url("/f.bin").to_string();
	let address = origin.url("/f.bin").to_string();
	rdm.update(cx, |rdm, _| {
		let places = rdm.paths.as_ref().unwrap().rule_places();
		std::fs::create_dir_all(&places.synced).unwrap();
		std::fs::write(places.synced.join("t.toml"), family).unwrap();
		std::fs::create_dir_all(&places.custom).unwrap();
		std::fs::write(places.custom.join("test.toml"), custom).unwrap();
		rdm.rules = std::sync::Arc::new(crate::rules::load(&places));
	});
	(TestServerHold(vec![origin, mirror]), format!("{address}\n{mirror_address}"))
}

struct TestServerHold(#[allow(dead_code)] Vec<crate::engine::testing::TestServer>);

#[gpui::test]
fn a_mirror_without_a_checksum_is_asked_about_and_never_is_remembered(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open_in(cx, "mirror-ask");
	let (_servers, addresses) = mirrored(&rdm, &mut cx, "");
	let (address, _) = addresses.split_once('\n').unwrap();
	look_until_resolved(&rdm, &mut cx, address);
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(
			rdm.adding.as_ref().unwrap().resolved.as_ref().unwrap().mirrors.len(),
			1,
			"the mirror was found"
		);
	});
	click(&mut cx, "button:Download");
	assert!(cx.debug_bounds("add-mirrors").is_some(), "no checksum, so the user is asked");
	let before = rdm.read_with(&cx, |rdm, _| rdm.downloads.len());
	click(&mut cx, "button:Never ask for this source");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_none(), "the sheet closed and the download went");
		assert_eq!(rdm.downloads.len(), before + 1);
		assert!(rdm.downloads.last().unwrap().mirrors.is_empty(), "from the source alone");
		assert_eq!(rdm.rules.choice_for("127.0.0.1"), Some(crate::rules::Choice::Never), "remembered");
		let written =
			std::fs::read_to_string(rdm.paths.as_ref().unwrap().custom_rules.join("choices.toml"))
				.unwrap();
		assert!(written.contains("never"), "{written}");
	});
	// And not asked again: the next download from there goes straight to the source.
	look_until_resolved(&rdm, &mut cx, address);
	click(&mut cx, "button:Download");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none(), "not asked a second time"));
}

#[gpui::test]
fn a_checksum_typed_on_the_question_sends_the_download_to_the_mirror_too(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open_in(cx, "mirror-typed");
	let (_servers, addresses) = mirrored(&rdm, &mut cx, "");
	let (address, mirror) = addresses.split_once('\n').unwrap();
	look_until_resolved(&rdm, &mut cx, address);
	click(&mut cx, "button:Download");
	assert!(cx.debug_bounds("add-mirrors").is_some());
	let field = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().checksum.clone());
	cx.update(|_, cx| {
		field.update(cx, |i, cx| i.set_content(&format!("md5:{}", "0".repeat(32)), cx))
	});
	click(&mut cx, "button:Download");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_none());
		let row = rdm.downloads.last().unwrap();
		assert_eq!(
			row.mirrors,
			vec![mirror.to_owned()],
			"a checksum the user typed vouches for the mirror"
		);
		assert!(row.checksum.is_some());
	});
}

#[gpui::test]
fn a_checksum_the_rules_find_is_filled_in_and_the_mirror_used_without_asking(
	cx: &mut TestAppContext,
) {
	use crate::engine::testing::{Options, TestServer};
	use sha2::Digest;
	let (rdm, mut cx) = open_in(cx, "mirror-found");
	let data = crate::engine::testing::body(60_000);
	let hex: String = sha2::Sha256::digest(&data).iter().map(|b| format!("{b:02x}")).collect();
	let api = TestServer::start(format!(r#"{{"sha256":"{hex}"}}"#).into_bytes(), Options::default());
	// An entry for the origin reading its checksum from the API, and the API's host an authority in
	// the custom layer: everything here is 127.0.0.1, and a mirror on the checksum's own host would
	// otherwise not be trusted with it.
	let (_servers, addresses) = mirrored(
		&rdm,
		&mut cx,
		&format!(
			"[[authority]]\nhost = \"127.0.0.1\"\n\n[[entry]]\nid = \"t\"\nmatch = \"http://127.0.0.1:{{port}}/{{file}}\"\n\n[[entry.checksum]]\nkind = \"json\"\nurl = \"{}\"\npick = \"sha256\"\n",
			api.url("/sum")
		),
	);
	let (address, mirror) = addresses.split_once('\n').unwrap();
	look_until_resolved(&rdm, &mut cx, address);
	let field = rdm.read_with(&cx, |rdm, _| rdm.adding.as_ref().unwrap().checksum.clone());
	assert_eq!(
		field.read_with(&cx, |i, _| i.content.to_string()),
		format!("sha256:{hex}"),
		"filled in where it can be seen"
	);
	click(&mut cx, "button:Download");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.adding.is_none(), "not asked: the checksum is the source's");
		let row = rdm.downloads.last().unwrap();
		assert_eq!(row.mirrors, vec![mirror.to_owned()]);
		assert_eq!(row.checksum.as_deref(), Some(format!("sha256:{hex}").as_str()));
	});
}
