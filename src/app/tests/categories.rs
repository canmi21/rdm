//! The sidebar's categories: presets, custom rules, colors, order and the list's scrolling.

use super::*;

/// One list of chips, two things to do with it: switches while nothing is being coloured, doors
/// while something is. The same turn the presets face makes under Edit.
#[gpui::test]
fn an_extensions_colour_is_set_from_the_chips_and_given_back_by_inherit(cx: &mut TestAppContext) {
	use crate::ui::category_sheet::{CategorySheet, Shading};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	// Edit turns the preset chips into doors; without it a press is the switch it looks like.
	click(&mut cx, "button:Edit");
	click(&mut cx, "preset:Documents");
	let id = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
		assert_eq!(form.shading, Shading::Off, "the chips are switches to start with");
		form.id
	});
	// Pressing a chip switches an extension off while nothing is being coloured.
	click(&mut cx, "extension:rtf");
	rdm.read_with(&cx, |rdm, _| {
		let category = crate::category::Category::find(&rdm.categories, id).unwrap();
		assert!(!category.extensions().contains(&"rtf".to_owned()), "the chip was a switch");
	});
	// Colors turns them into doors; the same chip now opens rather than switches.
	click(&mut cx, "button:Colors");
	click(&mut cx, "extension:docx");
	rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
		assert_eq!(form.shading, Shading::One("docx".to_owned()), "the chip was a door");
		let category = crate::category::Category::find(&rdm.categories, id).unwrap();
		assert!(category.extensions().contains(&"docx".to_owned()), "and switched nothing");
	});
	// A swatch now paints that extension and leaves the category alone.
	let before = rdm
		.read_with(&cx, |rdm, _| crate::category::Category::find(&rdm.categories, id).unwrap().color);
	rdm.update(&mut cx, |rdm, cx| rdm.choose_color(0x00ff00, cx));
	rdm.read_with(&cx, |rdm, _| {
		let category = crate::category::Category::find(&rdm.categories, id).unwrap();
		assert_eq!(category.shade("notes.docx"), 0x00ff00, "the extension took the colour");
		assert_eq!(category.color, before, "and the category kept its own");
		assert_eq!(category.shade("notes.rtf"), before, "as did every extension without one");
	});
	// Inherit gives it back.
	click(&mut cx, "button:Inherit");
	rdm.read_with(&cx, |rdm, _| {
		let category = crate::category::Category::find(&rdm.categories, id).unwrap();
		assert_eq!(category.shade("notes.docx"), before, "back to the category's own");
	});
}

#[gpui::test]
fn the_custom_form_adds_a_rule_and_advanced_exposes_the_pattern(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	assert!(cx.debug_bounds("preset:Videos").is_some(), "the sheet opens on the presets");
	assert!(cx.debug_bounds("button:Advanced").is_none(), "the form is a level down");
	click(&mut cx, "button:Add");
	let (name, extensions, pattern) = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		(form.name.clone(), form.extensions.clone(), form.pattern.clone())
	});
	cx.update(|window, cx| {
		name.update(cx, |input, cx| input.replace_text_in_range(None, "Rust", window, cx));
		extensions.update(cx, |input, cx| input.replace_text_in_range(None, "rs, rlib", window, cx));
	});
	assert!(cx.debug_bounds("category-sheet").is_some());
	click(&mut cx, "button:Advanced");
	let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
	assert_eq!(
		derived, r"(?i)\.(rs|rlib)$",
		"opening Advanced fills the pattern from the basic fields"
	);
	cx.update(|window, cx| {
		pattern.update(cx, |input, cx| input.replace_text_in_range(None, "(", window, cx));
	});
	let card = cx.debug_bounds("category-sheet").unwrap();
	let create = cx.debug_bounds("button:Create").unwrap();
	assert!(
		card.contains(&create.center()),
		"the Create button stays inside the card however long the report"
	);
	click(&mut cx, "button:Create");
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))),
			"a pattern that does not compile is not added"
		)
	});
	cx.update(|window, cx| {
		pattern.update(cx, |input, cx| {
			let end = input.content.len();
			input.replace_text_in_range(Some(end - 1..end), "", window, cx)
		});
	});
	click(&mut cx, "icon:globe");
	click(&mut cx, "button:Create");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.category_sheet.is_none());
		let rust = rdm.categories.iter().find(|c| c.name == "Rust").expect("added");
		assert_eq!(rust.icon, Icon::Globe);
		assert!(rdm.categories.last().unwrap().is_catch_all(), "Other stays last");
	});
	assert!(cx.debug_bounds("filter:Rust").is_some(), "the sidebar lists the new category");
}

/// A window too short to hold the categories scrolls them. Before the list had a scroller of its
/// own it ran off the bottom edge, where the rows below the fold could not be reached at all --
/// no scroll bar, no wheel, nothing but a taller window.
#[gpui::test]
fn a_short_window_scrolls_the_categories_rather_than_burying_them(cx: &mut TestAppContext) {
	use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase, point, px, size};

	let (rdm, mut cx) = open(cx);
	// Every preset, which is more than a short window holds.
	rdm.update(&mut cx, |rdm, cx| {
		rdm.categories = crate::category::Category::PRESETS
			.iter()
			.enumerate()
			.map(|(at, preset)| {
				crate::category::Category::from_preset(at as u64 + 1, preset.name, Default::default())
					.expect("a preset compiles")
			})
			.collect();
		cx.notify();
	});
	cx.simulate_resize(size(px(900.0), px(360.0)));
	cx.run_until_parked();
	let first = cx.debug_bounds("filter:Videos").expect("the first category is drawn");
	assert!(
		cx.debug_bounds("filter:Torrents").is_none_or(|last| last.origin.y > first.origin.y),
		"the last category is below the first, not on top of it"
	);
	// A wheel over the sidebar moves the rows under it.
	cx.simulate_event(ScrollWheelEvent {
		position: first.center(),
		delta: ScrollDelta::Pixels(point(px(0.0), px(-90.0))),
		modifiers: Modifiers::default(),
		touch_phase: TouchPhase::Moved,
	});
	cx.run_until_parked();
	let after = cx.debug_bounds("filter:Videos").expect("the first category is still drawn");
	assert!(after.origin.y < first.origin.y, "the categories scrolled: {after:?} against {first:?}");
	// The filters above them did not go anywhere: only the categories scroll.
	let all = cx.debug_bounds("filter:All Tasks").expect("the state filters are drawn");
	assert!(all.origin.y < after.origin.y, "the filters stay above the categories");
}

/// The fold tells the reader there is more: the list fades where it was cut, at whichever end has
/// something past it, and nowhere else. A list that fits shows neither fade.
#[gpui::test]
fn the_categories_fade_at_the_fold_and_only_there(cx: &mut TestAppContext) {
	use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase, point, px, size};

	let (rdm, mut cx) = open(cx);
	// The two questions the fades are painted from, asked of the scroller after it was laid out --
	// which is where the fades ask them too. See src/ui/sidebar.rs.
	let fades = |rdm: &Entity<Rdm>, cx: &mut VisualTestContext| {
		rdm.read_with(cx, |rdm, _| {
			let scroll = &rdm.categories_scroll;
			(crate::ui::sidebar::cut_above(scroll), crate::ui::sidebar::cut_below(scroll))
		})
	};
	// A window with room for every category shows no fade at all.
	cx.simulate_resize(size(px(900.0), px(900.0)));
	cx.run_until_parked();
	assert_eq!(fades(&rdm, &mut cx), (false, false), "a list that fits is cut nowhere");

	rdm.update(&mut cx, |rdm, cx| {
		rdm.categories = crate::category::Category::PRESETS
			.iter()
			.enumerate()
			.map(|(at, preset)| {
				crate::category::Category::from_preset(at as u64 + 1, preset.name, Default::default())
					.expect("a preset compiles")
			})
			.collect();
		cx.notify();
	});
	cx.simulate_resize(size(px(900.0), px(360.0)));
	cx.run_until_parked();
	assert_eq!(fades(&rdm, &mut cx), (false, true), "at the top, only the bottom is cut");

	let over = cx.debug_bounds("filter:Videos").expect("the first category is drawn").center();
	let wheel = |cx: &mut VisualTestContext, by: f32| {
		cx.simulate_event(ScrollWheelEvent {
			position: over,
			delta: ScrollDelta::Pixels(point(px(0.0), px(by))),
			modifiers: Modifiers::default(),
			touch_phase: TouchPhase::Moved,
		});
		cx.run_until_parked();
	};
	wheel(&mut cx, -40.0);
	assert_eq!(fades(&rdm, &mut cx), (true, true), "in the middle, both ends are cut");
	wheel(&mut cx, -10_000.0);
	assert_eq!(fades(&rdm, &mut cx), (true, false), "at the bottom, only the top is cut");
}

#[gpui::test]
fn a_preset_row_toggles_the_category_in_and_out(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	let before = rdm.read_with(&cx, |rdm, _| rdm.categories.len());
	// Archives rather than eBooks: the seed takes the common presets, and a row can only be
	// toggled out of the sidebar if it was in it. See `Category::COMMON`.
	click(&mut cx, "preset:Archives");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.categories.len(), before - 1));
	click(&mut cx, "preset:Archives");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.categories.len(), before);
		assert!(rdm.categories.last().unwrap().is_catch_all());
	});
}

#[gpui::test]
fn reorder_drags_a_sidebar_row_onto_another_and_other_stays_last(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Reorder");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.reordering()));
	let names = |rdm: &Rdm| rdm.categories.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
	let before = rdm.read_with(&cx, |rdm, _| names(rdm));
	assert_eq!(&before[..2], ["Videos", "Audio"]);
	let drag = |cx: &mut VisualTestContext, from: &'static str, onto: &'static str| {
		let start = cx.debug_bounds(from).unwrap().center();
		let end = cx.debug_bounds(onto).unwrap().center();
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: start,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse: false,
		});
		for position in [start + gpui::point(px(0.0), px(6.0)), end] {
			cx.simulate_event(MouseMoveEvent {
				position,
				pressed_button: Some(MouseButton::Left),
				modifiers: Modifiers::default(),
			});
		}
		cx.simulate_event(MouseUpEvent {
			button: MouseButton::Left,
			position: end,
			modifiers: Modifiers::default(),
			click_count: 1,
		});
	};
	drag(&mut cx, "filter:Videos", "filter:Programs");
	rdm.read_with(&cx, |rdm, _| {
		let after = names(rdm);
		assert_eq!(after[0], "Audio", "{after:?}");
		assert_eq!(
			after.iter().position(|n| n == "Videos"),
			before.iter().position(|n| n == "Programs")
		);
		assert_eq!(rdm.filter, Filter::All, "a row in reorder mode does not filter");
	});
	drag(&mut cx, "filter:Audio", "filter:Other");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.categories.last().unwrap().is_catch_all(), "Other is not a drop target");
		assert_eq!(names(rdm)[0], "Audio");
	});
	// Every drop is already written, so a press anywhere but the categories finishes.
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "Escape finishes"));
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Reorder");
	let card = cx.debug_bounds("category-sheet").unwrap().center();
	cx.simulate_click(card, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| assert!(rdm.reordering(), "the hint itself is not outside"));
	let row = cx.debug_bounds("row:3").unwrap().center();
	cx.simulate_click(row, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.category_sheet.is_none(), "a press on the list finishes");
		assert_eq!(rdm.selected, None, "and reaches nothing behind the wash");
	});
}

#[gpui::test]
fn edit_opens_a_presets_list_where_extensions_switch_and_are_added(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "preset:Archives");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.categories.iter().any(|c| c.name == "Archives")));
	click(&mut cx, "button:Edit");
	click(&mut cx, "preset:Archives");
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			matches!(rdm.category_sheet, Some(CategorySheet::Presets { editing: true })),
			"a preset that is off has no list to open"
		)
	});
	click(&mut cx, "preset:Videos");
	let add = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
		form.add.clone()
	});
	cx.update(|window, cx| window.focus(&add.read(cx).focus(), cx));
	click(&mut cx, "extension:mkv");
	assert!(
		cx.update(|window, cx| add.read(cx).focus().is_focused(window)),
		"a chip is a switch: pressing it leaves the field's focus alone"
	);
	cx.update(|window, cx| {
		add.update(cx, |input, cx| input.replace_text_in_range(None, "xyz, zyx", window, cx))
	});
	// The tests bind no keys; main does. The action is what Enter is bound to.
	cx.dispatch_action(crate::ui::text_input::Confirm);
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let list = video.extensions();
		assert!(!list.contains(&"mkv".to_owned()));
		assert_eq!(&list[list.len() - 2..], ["xyz", "zyx"]);
	});
	assert_eq!(add.read_with(&cx, |input, _| input.content.to_string()), "", "the field clears");
	click(&mut cx, "extension:xyz");
	click(&mut cx, "extension:mkv");
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let list = video.extensions();
		assert!(list.contains(&"mkv".to_owned()) && !list.contains(&"xyz".to_owned()));
		assert_eq!(list.last().map(String::as_str), Some("zyx"));
	});
	click(&mut cx, "icon:film");
	click(&mut cx, "swatch:#bf616a");
	click(&mut cx, "button:Reset");
	rdm.read_with(&cx, |rdm, _| {
		let video = rdm.categories.iter().find(|c| c.name == "Videos").unwrap();
		let shipped = Category::preset("Videos").unwrap();
		assert_eq!(video.extensions(), shipped.extensions());
		assert_eq!((video.icon, video.color), (shipped.icon, shipped.color), "icon and color too");
		assert!(!video.differs_from_preset());
	});
	assert!(cx.debug_bounds("button:Reset").is_none(), "nothing left to reset");
	click(&mut cx, "button:Close");
	rdm.read_with(&cx, |rdm, _| {
		assert!(matches!(rdm.category_sheet, Some(CategorySheet::Presets { editing: false })))
	});
}

#[gpui::test]
fn a_color_is_picked_from_a_swatch_or_written_and_kept(cx: &mut TestAppContext) {
	use crate::ui::theme::Tint;
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	assert!(cx.debug_bounds("swatch:#b48ead").is_none(), "the picker waits behind the swatch");
	click(&mut cx, "button:Color");
	click(&mut cx, "swatch:#b48ead");
	let (name, extensions) = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		assert_eq!(form.color, Tint::Purple.rgb());
		(form.name.clone(), form.extensions.clone())
	});
	cx.update(|window, cx| {
		name.update(cx, |input, cx| input.replace_text_in_range(None, "Plum", window, cx));
		extensions.update(cx, |input, cx| input.replace_text_in_range(None, "plum", window, cx));
	});
	click(&mut cx, "button:Create");
	rdm.read_with(&cx, |rdm, _| {
		let plum = rdm.categories.iter().find(|c| c.name == "Plum").expect("added");
		assert_eq!(plum.color, Tint::Purple.rgb());
	});
	// A preset's color, written; the writing stays with the category beside the named hues.
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Edit");
	click(&mut cx, "preset:Audio");
	let custom = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Preset(form)) = &rdm.category_sheet else { panic!("the list is up") };
		form.custom.clone()
	});
	cx.update(|window, cx| {
		window.focus(&custom.read(cx).focus(), cx);
		custom
			.update(cx, |input, cx| input.replace_text_in_range(None, "rgb(170, 187, 204)", window, cx));
	});
	cx.dispatch_action(crate::ui::text_input::Confirm);
	cx.run_until_parked();
	click(&mut cx, "icon:globe");
	let audio = |rdm: &Rdm| rdm.categories.iter().find(|c| c.name == "Audio").unwrap().clone();
	rdm.read_with(&cx, |rdm, _| {
		let audio = audio(rdm);
		assert_eq!((audio.color, audio.icon), (0xaabbcc, Icon::Globe));
		assert_eq!(audio.custom_color.as_deref(), Some("rgb(170, 187, 204)"), "as written");
	});
	click(&mut cx, "swatch:#8fbcbb");
	rdm.read_with(&cx, |rdm, _| {
		let audio = audio(rdm);
		assert_eq!(audio.color, Tint::Teal.rgb());
		assert!(audio.custom_color.is_some(), "a named hue does not erase the written one");
	});
	click(&mut cx, "swatch:custom");
	rdm.read_with(&cx, |rdm, _| assert_eq!(audio(rdm).color, 0xaabbcc, "and it can be chosen again"));
	assert_eq!(
		custom.read_with(&cx, |input, _| input.content.to_string()),
		"rgb(170, 187, 204)",
		"the field keeps the user's spelling"
	);
}

#[gpui::test]
fn advanced_shares_a_line_with_create_until_it_opens(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let advanced = cx.debug_bounds("button:Advanced").unwrap();
	let create = cx.debug_bounds("button:Create").unwrap();
	assert!(
		(f32::from(advanced.center().y) - f32::from(create.center().y)).abs() < 1.0,
		"one line while closed"
	);
	let card = cx.debug_bounds("category-sheet").unwrap();
	assert!(advanced.size.width < card.size.width / 3.0, "Advanced is only as wide as its words");
	click(&mut cx, "button:Advanced");
	let create_open = cx.debug_bounds("button:Create").unwrap();
	assert!(create_open.top() > cx.debug_bounds("button:Advanced").unwrap().bottom());
	rdm.read_with(&cx, |rdm, _| {
		assert!(matches!(&rdm.category_sheet, Some(CategorySheet::Custom(f)) if f.advanced))
	});
}

#[gpui::test]
fn the_custom_form_combines_its_fields_by_the_switch(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let (extensions, contains, pattern) = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		(form.extensions.clone(), form.contains.clone(), form.pattern.clone())
	});
	cx.update(|window, cx| {
		extensions.update(cx, |input, cx| input.replace_text_in_range(None, "pdf", window, cx));
		contains.update(cx, |input, cx| input.replace_text_in_range(None, "rust book", window, cx));
	});
	click(&mut cx, "combine:OR");
	click(&mut cx, "toggle:Ignore spaces");
	click(&mut cx, "button:Advanced");
	let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
	assert_eq!(derived, r"(?:(?i:rust\s*book)|(?i:\.(pdf))$)", "case is ignored until Match case");
	click(&mut cx, "button:Advanced");
	click(&mut cx, "toggle:Match case");
	cx.update(|window, cx| {
		pattern.update(cx, |input, cx| input.set_content("", cx));
		let _ = window;
	});
	click(&mut cx, "button:Advanced");
	let derived = pattern.read_with(&cx, |input, _| input.content.to_string());
	assert_eq!(derived, r"(?:rust\s*book|(?i:\.(pdf))$)");
}
