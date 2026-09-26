//! Settings: its pages, notices, updates, and the dropdowns its rows open.

use super::*;

#[gpui::test]
fn the_colorful_categories_switch_flips_the_preference(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_categories, "on to start with"));
	click(&mut cx, "button:Settings");
	cx.run_until_parked();
	click(&mut cx, "section:Appearance");
	click(&mut cx, "switch:settings.label.colorful");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.colorful_categories));
	click(&mut cx, "switch:settings.label.colorful");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.colorful_categories));
}

/// Notifications is a page of one row an occasion, and each row is a choice of where -- not a
/// switch, since where a notice goes is the question and whether it goes at all is one of the
/// answers.
#[gpui::test]
fn notifications_are_one_row_an_occasion_and_the_choice_is_kept(cx: &mut TestAppContext) {
	use crate::notify::{Occasion, Style};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Settings");
	cx.run_until_parked();
	click(&mut cx, "section:Notifications");
	for occasion in Occasion::ALL {
		let selector: &'static str = format!("setting:{}", occasion.label()).leak();
		assert!(cx.debug_bounds(selector).is_some(), "a row for {}", occasion.label());
	}
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(
			rdm.preferences.notice(Occasion::Finished),
			Style::Window,
			"a finished download opens the dialog, which is the notice with something to do next"
		);
		assert_eq!(
			rdm.preferences.notice(Occasion::Queue),
			Style::Silent,
			"or the last would say it twice"
		);
	});
	// The choice is the user's and is kept, one occasion at a time.
	rdm.update(&mut cx, |rdm, cx| rdm.set_notice(Occasion::Finished, Style::InApp, cx));
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.notice(Occasion::Finished), Style::InApp);
		assert_eq!(rdm.preferences.notice(Occasion::Failed), Style::System, "and only that one");
	});
}

/// What the window is told, the window shows: a card in the corner that goes at a press. The
/// system's own notification centre is not something a headless test can see, so what is checked
/// here is that the choice is honoured, not what the system does with it.
#[gpui::test]
fn a_notice_meant_for_the_window_lands_in_the_corner_and_goes_at_a_press(cx: &mut TestAppContext) {
	use crate::notify::{Occasion, Style};
	let (rdm, mut cx) = open(cx);
	assert!(cx.debug_bounds("notice:0").is_none(), "nothing said yet");
	rdm.update(&mut cx, |rdm, cx| {
		rdm.set_notice(Occasion::Finished, Style::InApp, cx);
		rdm.tell_of(
			Occasion::Finished,
			crate::notify::Notice::new("Download finish", "debian.iso"),
			cx,
		);
	});
	cx.run_until_parked();
	assert!(cx.debug_bounds("notice:0").is_some(), "the corner says so");
	click(&mut cx, "notice:0");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.notices.is_empty(), "and a press takes it away"));
	// Told to say nothing, it says nothing anywhere.
	rdm.update(&mut cx, |rdm, cx| {
		rdm.set_notice(Occasion::Finished, Style::Silent, cx);
		rdm.tell_of(
			Occasion::Finished,
			crate::notify::Notice::new("Download finish", "debian.iso"),
			cx,
		);
	});
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(rdm.notices.is_empty(), "silent is silent"));
	// And a notice meant for a window of its own does not also land in the corner: the places
	// are places, not degrees, so a notice goes to exactly one of them.
	rdm.update(&mut cx, |rdm, cx| {
		rdm.set_notice(Occasion::Finished, Style::Window, cx);
		rdm.tell_of(
			Occasion::Finished,
			crate::notify::Notice::new("Download finish", "debian.iso"),
			cx,
		);
	});
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.notices.is_empty(), "the corner is not where this one was sent");
	});
}

#[gpui::test]
fn settings_has_sections_and_a_search_that_cuts_across_them(cx: &mut TestAppContext) {
	use crate::ui::settings_sheet::Section;
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Settings");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.settings.as_ref().map(|s| s.section), Some(Section::General))
	});
	assert!(cx.debug_bounds("setting:settings.label.download_folder").is_some());
	assert!(
		cx.debug_bounds("setting:settings.label.colorful").is_none(),
		"another section's rows are not shown"
	);
	click(&mut cx, "section:Appearance");
	assert!(cx.debug_bounds("setting:settings.label.colorful").is_some());
	assert!(cx.debug_bounds("setting:settings.label.download_folder").is_none());
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	cx.update(|_, cx| search.update(cx, |i, cx| i.set_content("speed", cx)));
	cx.run_until_parked();
	assert!(
		cx.debug_bounds("setting:settings.label.speed_limit").is_some(),
		"a search shows every section's matches"
	);
	assert!(cx.debug_bounds("setting:settings.label.colorful").is_none());
	cx.update(|_, cx| search.update(cx, |i, cx| i.set_content("nothing like this", cx)));
	cx.run_until_parked();
	assert!(cx.debug_bounds("setting:settings.label.speed_limit").is_none(), "no match, no rows");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.settings_open(), "escape in the field closes"));
}

/// A development build runs the check and keeps its answer -- which is what makes a broken
/// manifest or an unreachable route visible in Settings -- and offers nothing: no card, no
/// notification, no install started on its own.
#[gpui::test]
fn a_development_build_checks_and_says_nothing(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let manifest: crate::update::Manifest = serde_json::from_str(
		r#"{ "channel": "nightly", "version": "2026.9.5", "build": 99, "sha": "abc", "assets": [
			{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 1, "sha256": "aa" },
			{ "target": "windows-x64", "kind": "zip", "file": "rdm-nightly-windows-x64.zip", "size": 1, "sha256": "bb" },
			{ "target": "linux-x64", "kind": "AppImage", "file": "rdm-nightly-linux-x64.AppImage", "size": 1, "sha256": "cc" },
			{ "target": "linux-arm64", "kind": "AppImage", "file": "rdm-nightly-linux-arm64.AppImage", "size": 1, "sha256": "dd" }
		] }"#,
	)
	.unwrap();
	rdm.read_with(&cx, |rdm, _| {
		assert!(!rdm.updates.announces, "which is what a build from the working tree is");
	});
	rdm.update(&mut cx, |rdm, _| rdm.updates.this = None);
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest, true, cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_none(), "no card, though it was asked for by hand");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(
			rdm.updates.available.as_ref().map(|a| a.build),
			Some(99),
			"the check ran and its answer is kept, so Settings can show what it came to"
		);
		assert_eq!(rdm.updates.outcome, Some(Ok(99)));
		assert!(rdm.updates.notified.is_empty(), "and the system was told nothing");
		assert_eq!(rdm.updates.stage, crate::app::updates::Stage::Offered, "nothing was started");
	});
}

#[gpui::test]
fn a_newer_build_puts_a_card_in_the_corner_until_waved_away(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let manifest = crate::update::Manifest::parse(
		r#"{ "channel": "nightly", "version": "2026.9.5", "build": 99, "sha": "abc", "assets": [
			{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 1, "sha256": "aa" },
			{ "target": "windows-x64", "kind": "zip", "file": "rdm-nightly-windows-x64.zip", "size": 1, "sha256": "bb" },
			{ "target": "linux-x64", "kind": "AppImage", "file": "rdm-nightly-linux-x64.AppImage", "size": 1, "sha256": "cc" },
			{ "target": "linux-arm64", "kind": "AppImage", "file": "rdm-nightly-linux-arm64.AppImage", "size": 1, "sha256": "dd" }
		] }"#,
	)
	.unwrap();
	assert!(cx.debug_bounds("toast:update").is_none(), "nothing known, nothing shown");
	// A hand build is shown a newer build only when it asked. The test binary made in CI
	// carries the run's number, so the test says which build it is; and the tests are a
	// development build, which announces nothing unless asked to, so the test says that too.
	rdm.update(&mut cx, |rdm, _| {
		rdm.updates.this = None;
		rdm.updates.announces = true;
	});
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest.clone(), false, cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_none(), "a hand build was not asked");
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest, true, cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_some(), "build 99 is newer than no number");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.updates.available.as_ref().map(|a| a.build), Some(99));
		assert!(
			rdm.update_status().starts_with("2026.9.5 (99) is the latest"),
			"{}",
			rdm.update_status()
		);
	});
	assert!(cx.debug_bounds("button:Install").is_some(), "the card offers the install");
	click(&mut cx, "button:Later");
	cx.run_until_parked();
	assert!(cx.debug_bounds("toast:update").is_none(), "waved away for this build");
	// The settings row names the check and its outcome, under Updates.
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	click(&mut cx, "section:Updates");
	assert!(cx.debug_bounds("setting:settings.label.update_channel").is_some());
	assert!(cx.debug_bounds("setting:settings.label.check_for_updates").is_some());
	assert!(cx.debug_bounds("button:Check now").is_some());
}

#[gpui::test]
fn the_update_settings_are_switches_and_a_choice_that_follows_the_switch(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	// The update rows have a section of their own; General was a dozen rows in one run.
	click(&mut cx, "section:Updates");
	assert!(cx.debug_bounds("setting:settings.label.check_for_updates").is_some());
	assert!(cx.debug_bounds("setting:settings.label.automatic_updates").is_some());
	assert!(cx.debug_bounds("choice:Download and install").is_some(), "the policy, while automatic");
	click(&mut cx, "choice:Download only");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.update_policy, crate::update::Policy::Download);
	});
	click(&mut cx, "switch:settings.label.automatic_updates");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.auto_update));
	assert!(cx.debug_bounds("choice:Download only").is_none(), "no policy without the switch");
	click(&mut cx, "switch:settings.label.check_for_updates");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.preferences.check_updates));
}

/// The Names rows: two switches and a choice whose options follow the transport. Searching for
/// them rather than scrolling to them, since they sit at the end of a section that is longer than
/// the pane and a row with no bounds would fail this for the wrong reason.
#[gpui::test]
fn the_name_rows_follow_the_switches_and_choosing_a_server_fills_the_field(
	cx: &mut TestAppContext,
) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	// By their group's name: a search matches a row's label, its note or the heading it sits
	// under, and "Names" is the one word every row here answers to.
	cx.update(|_, cx| search.update(cx, |field, cx| field.set_content("names", cx)));
	cx.run_until_parked();

	// The default: our own resolver, on port 53, on the machine's own servers. The two offered
	// are named by their address here, because that is the name anybody has for them.
	assert!(cx.debug_bounds("setting:settings.label.dns_force_system").is_some());
	assert!(cx.debug_bounds("setting:settings.label.dns_https").is_some());
	assert!(cx.debug_bounds("choice:Follow system").is_some());
	assert!(cx.debug_bounds("choice:1.1.1.1").is_some());
	assert!(cx.debug_bounds("choice:8.8.8.8").is_some());
	// Nothing to write while the machine's own servers are the ones asked, so no field to write
	// it in: one that is ignored is worse than none.
	assert!(cx.debug_bounds("setting:settings.label.name_servers").is_none());

	// Choosing one fills the field beside it, so what is being asked is on screen.
	click(&mut cx, "choice:1.1.1.1");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.dns_servers, crate::dns::Servers::Cloudflare);
		assert_eq!(rdm.preferences.dns_servers_written, "1.1.1.1");
	});
	assert!(cx.debug_bounds("setting:settings.label.name_servers").is_some(), "and now there is");

	// Over HTTPS a server is a URL, so the machine's own are not offered and the operators' names
	// are what the two are called.
	click(&mut cx, "switch:settings.label.dns_https");
	cx.run_until_parked();
	assert!(cx.debug_bounds("choice:Follow system").is_none(), "a machine names no DoH URL");
	assert!(cx.debug_bounds("choice:Cloudflare").is_some());
	assert!(cx.debug_bounds("choice:Google").is_some());
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.preferences.dns_servers_written, "https://cloudflare-dns.com/dns-query");
	});

	// Forcing a transport that is not in use says nothing, so that switch arrives with this one
	// and leaves with it.
	assert!(cx.debug_bounds("setting:settings.label.dns_force_https").is_some());
	click(&mut cx, "switch:settings.label.dns_force_https");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.dns_force_https));
	click(&mut cx, "switch:settings.label.dns_https");
	cx.run_until_parked();
	assert!(cx.debug_bounds("setting:settings.label.dns_force_https").is_none());

	// And handing the whole business back to the machine leaves nothing under it to set.
	// The domains the machine answers for are set here whatever the transport is.
	assert!(cx.debug_bounds("setting:settings.label.system_domains").is_some());

	click(&mut cx, "switch:settings.label.dns_force_system");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| assert!(rdm.preferences.dns_force_system));
	assert!(cx.debug_bounds("setting:settings.label.dns_https").is_none());
	assert!(cx.debug_bounds("choice:Cloudflare").is_none());
	assert!(cx.debug_bounds("setting:settings.label.system_domains").is_none());
}

/// A choice whose words will not fit side by side is a dropdown: closed it shows the one that is
/// chosen, and the alternatives arrive in a panel under the button when it is pressed.
///
/// **It names no option.** `Agent::offered` leaves out the disguise that would be this machine
/// telling the truth, so the set is a different set on every system -- and an assertion naming
/// one of them passes here and fails on two of the four runners, which is exactly what it did.
/// The options are read from the same place the row reads them. See spec/workflow.md.
#[gpui::test]
fn a_long_choice_is_a_dropdown_that_opens_under_its_button(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	cx.update(|_, cx| search.update(cx, |field, cx| field.set_content("user agent", cx)));
	cx.run_until_parked();

	// Whatever this system offers, the one that is not chosen now.
	let offered = crate::agent::Agent::offered();
	let chosen = rdm.read_with(&cx, |rdm, _| rdm.preferences.agent);
	let other = *offered.iter().find(|agent| **agent != chosen).expect("a second disguise");
	let other_name: &'static str = other.name();

	assert!(cx.debug_bounds("choice:settings.label.user_agent").is_some(), "the closed button");
	assert!(cx.debug_bounds("menu:settings.label.user_agent").is_none(), "with nothing open");
	assert!(cx.debug_bounds(other_name).is_none(), "and no option of it on the pane");

	rdm.update(&mut cx, |rdm, cx| rdm.toggle_settings_menu("settings.label.user_agent", cx));
	cx.run_until_parked();
	assert!(cx.debug_bounds("menu:settings.label.user_agent").is_some(), "pressing opens it");

	click(&mut cx, "choice:settings.label.user_agent");
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.settings.as_ref().unwrap().menu.is_none(), "and pressing again closes it");
	});

	// Escape answers the menu before the sheet: the panel is the topmost thing while it is open.
	rdm.update(&mut cx, |rdm, cx| rdm.toggle_settings_menu("settings.label.user_agent", cx));
	cx.run_until_parked();
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.settings_open(), "the sheet is still up");
		assert!(rdm.settings.as_ref().unwrap().menu.is_none(), "and the panel is not");
	});
}

/// The panel belongs to the button, not to the pointer: it opens under the button's bottom left
/// whatever part of the button was pressed, and it costs the pane no room, so the rows around it
/// stay where they were. They did not -- the menu used to be a child of the row, and the row it
/// was not drawn for still paid a flex gap for the empty element that stood in for it, so every
/// row under it slid down four points as a menu opened. See spec/ui.md.
#[gpui::test]
fn a_dropdown_opens_under_its_button_and_moves_nothing(cx: &mut TestAppContext) {
	use crate::ui::settings_sheet::Section;
	use gpui::{Modifiers, point, px};
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	rdm.update(&mut cx, |rdm, cx| rdm.set_settings_section(Section::Network, cx));
	cx.run_until_parked();

	let button = cx.debug_bounds("choice:settings.label.user_agent").expect("the closed button");
	// The row under it, which is the one the empty stand-in used to push down.
	let under = cx.debug_bounds("setting:settings.label.user_agent").expect("the row under it");

	// Pressed at its right edge, as far from its left corner as the button allows.
	cx.simulate_click(point(button.right() - px(6.0), button.center().y), Modifiers::default());
	cx.run_until_parked();
	let menu = cx.debug_bounds("menu:settings.label.user_agent").expect("the panel");
	assert_eq!(menu.origin.x, button.origin.x, "the panel's left edge is the button's");
	assert_eq!(menu.origin.y, button.bottom() + px(4.0), "and it hangs four points under it");
	assert_eq!(cx.debug_bounds("setting:settings.label.user_agent"), Some(under), "nothing moved");
	assert_eq!(cx.debug_bounds("choice:settings.label.user_agent"), Some(button), "the button too");
}

/// An open menu rides the pane it was opened in: the pane scrolls, the row moves, and the panel
/// moves exactly as far, being laid out against the button rather than pinned to a point of the
/// window. Scrolled far enough that the button leaves what the pane shows, the menu goes with
/// it rather than hanging over the card on its own. See spec/ui.md.
#[gpui::test]
fn an_open_menu_follows_its_button_and_lets_go_when_it_leaves(cx: &mut TestAppContext) {
	use crate::ui::settings_sheet::Section;
	use gpui::{ScrollDelta, ScrollWheelEvent, point, px};
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	// Network is longer than the card, so its pane is one that scrolls.
	rdm.update(&mut cx, |rdm, cx| rdm.set_settings_section(Section::Network, cx));
	cx.run_until_parked();
	rdm.update(&mut cx, |rdm, cx| rdm.toggle_settings_menu("settings.label.user_agent", cx));
	cx.run_until_parked();
	let button = cx.debug_bounds("choice:settings.label.user_agent").expect("the button");
	let menu = cx.debug_bounds("menu:settings.label.user_agent").expect("the panel");

	let scroll = |cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, by: f32| {
		cx.simulate_event(ScrollWheelEvent {
			position: at,
			delta: ScrollDelta::Pixels(point(px(0.0), px(by))),
			..Default::default()
		});
		cx.run_until_parked();
	};

	scroll(&mut cx, button.center(), -40.0);
	let moved = cx.debug_bounds("choice:settings.label.user_agent").expect("the button, moved");
	let panel = cx.debug_bounds("menu:settings.label.user_agent").expect("the panel, moved");
	assert_eq!(moved.origin.y, button.origin.y - px(40.0), "the button rode the scroll");
	assert_eq!(panel.origin.y, menu.origin.y - px(40.0), "and the panel rode it exactly as far");
	assert_eq!(panel.origin.x, moved.origin.x, "still under the button");

	// Far enough that the button is no longer within what the pane shows.
	let sheet = cx.debug_bounds("settings-sheet").expect("the card").center();
	scroll(&mut cx, sheet, -400.0);
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.settings.as_ref().unwrap().menu.is_none(), "the menu let go with its button");
		assert!(rdm.settings_open(), "and the sheet is still up");
	});
	assert!(cx.debug_bounds("menu:settings.label.user_agent").is_none(), "nothing is drawn for it");
}

/// The row whose setting is written on one line and chosen on another is not called the same
/// thing twice. The search reads what is on screen, so the second name finds it too.
#[gpui::test]
fn the_two_user_agent_rows_are_named_apart_and_both_are_searchable(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_settings(cx));
	cx.run_until_parked();
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	cx.update(|_, cx| search.update(cx, |field, cx| field.set_content("what is sent", cx)));
	cx.run_until_parked();
	assert!(
		cx.debug_bounds("setting:settings.label.user_agent").is_some(),
		"the field row answers to the name it is drawn under"
	);
}

/// The card belongs to the window's bottom right corner, and a window is resized. What is
/// checked is the gap rather than the point: 12 from the right, and the status bar and 12 from
/// the bottom, whatever the window's size, since a card placed at a point rather than a gap is a
/// card that is in the corner of the size it was opened at and nowhere near one after a drag.
#[gpui::test]
fn the_update_card_keeps_to_the_corner_however_the_window_is_sized(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let manifest = crate::update::Manifest::parse(
		r#"{ "channel": "nightly", "version": "2026.9.5", "build": 99, "sha": "abc", "assets": [
			{ "target": "macos-arm64", "kind": "dmg", "file": "rdm-nightly-macos-arm64.dmg", "size": 1, "sha256": "aa" },
			{ "target": "windows-x64", "kind": "zip", "file": "rdm-nightly-windows-x64.zip", "size": 1, "sha256": "bb" },
			{ "target": "linux-x64", "kind": "AppImage", "file": "rdm-nightly-linux-x64.AppImage", "size": 1, "sha256": "cc" },
			{ "target": "linux-arm64", "kind": "AppImage", "file": "rdm-nightly-linux-arm64.AppImage", "size": 1, "sha256": "dd" }
		] }"#,
	)
	.unwrap();
	rdm.update(&mut cx, |rdm, _| {
		rdm.updates.this = None;
		rdm.updates.announces = true;
	});
	rdm.update(&mut cx, |rdm, cx| rdm.apply_manifest(manifest, true, cx));
	cx.run_until_parked();
	for extent in [(900.0, 600.0), (1280.0, 900.0), (720.0, 380.0)] {
		let (width, height) = extent;
		cx.simulate_resize(gpui::size(gpui::px(width), gpui::px(height)));
		cx.run_until_parked();
		cx.draw(gpui::Point::default(), gpui::size(gpui::px(width), gpui::px(height)), |_, _| {
			gpui::div()
		});
		let card = cx.debug_bounds("toast:update").expect("the card is drawn");
		assert_eq!(width - f32::from(card.right()), 12.0, "12 from the right at {width}x{height}");
		assert_eq!(
			height - f32::from(card.bottom()),
			crate::ui::status_bar::HEIGHT + 12.0,
			"clear of the status bar at {width}x{height}"
		);
	}
	// A notice arriving stacks above the card rather than moving it: the corner is one column,
	// laid out from its bottom, so what is already there stays where it is.
	let card = cx.debug_bounds("toast:update").expect("the card is drawn");
	rdm.update(&mut cx, |rdm, cx| {
		rdm.set_notice(crate::notify::Occasion::Finished, crate::notify::Style::InApp, cx);
		rdm.tell_of(
			crate::notify::Occasion::Finished,
			crate::notify::Notice::new("Download finish", "debian.iso"),
			cx,
		);
	});
	cx.run_until_parked();
	let notice = cx.debug_bounds("notice:0").expect("the notice is drawn");
	assert_eq!(cx.debug_bounds("toast:update"), Some(card), "the card did not move");
	assert!(notice.bottom() <= card.origin.y, "and the notice sits above it");
}
