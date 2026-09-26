//! Sheets and windows: what a press outside, Escape and focus do to them.

use super::*;

#[gpui::test]
fn a_sheet_swallows_clicks_and_only_a_clean_one_closes_from_outside(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let row = cx.debug_bounds("row:3").unwrap().center();
	click(&mut cx, "button:New category");
	// The row is under the backdrop now: a click there reaches nothing behind, and the presets
	// have nothing to lose, so the sheet takes it as a request to close.
	cx.simulate_click(row, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.selected, None, "the row behind the sheet was not pressed");
		assert!(rdm.category_sheet.is_none(), "an untouched sheet closes from a click outside");
	});
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let name = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		form.name.clone()
	});
	cx.update(|window, cx| {
		name.update(cx, |input, cx| input.replace_text_in_range(None, "Rust", window, cx))
	});
	cx.simulate_click(row, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))),
			"typed text is not thrown away by a click outside"
		);
		assert_eq!(rdm.selected, None);
	});
	click(&mut cx, "button:Close");
	rdm.read_with(&cx, |rdm, _| {
		assert!(
			matches!(rdm.category_sheet, Some(CategorySheet::Presets { .. })),
			"the form's cross steps back to the presets"
		)
	});
	click(&mut cx, "button:Close");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "the presets' cross closes"));
}

#[gpui::test]
fn the_guide_lies_over_the_form_and_leaves_it_alone(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let name = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		form.name.clone()
	});
	cx.update(|window, cx| {
		name.update(cx, |input, cx| input.replace_text_in_range(None, "Kept", window, cx));
	});
	click(&mut cx, "button:Color");
	click(&mut cx, "button:Color formats");
	assert_eq!(cx.windows().len(), 1, "no window of its own");
	assert!(cx.debug_bounds("guide").is_some());
	// A press outside the guide closes the guide, and does not reach the form under it.
	let row = cx.debug_bounds("row:3").unwrap().center();
	cx.simulate_click(row, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.guide.is_none(), "the guide has nothing to keep");
		assert!(matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))), "the form stays");
		assert_eq!(rdm.selected, None, "and the row behind was not pressed");
	});
	click(&mut cx, "button:Color formats");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.guide.is_none(), "Escape closes the guide");
		assert!(rdm.category_sheet.is_some(), "and only the guide: the form has text in it");
	});
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.category_sheet.is_some(), "a form with text is closed by its cross alone")
	});
	click(&mut cx, "button:Close");
	click(&mut cx, "button:Close");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none()));
}

#[gpui::test]
fn the_press_that_brings_the_window_back_does_nothing_else(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseUpEvent};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	let row = cx.debug_bounds("row:3").unwrap().center();
	let press = |cx: &mut VisualTestContext, first_mouse: bool| {
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: row,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse,
		});
		cx.simulate_event(MouseUpEvent {
			button: MouseButton::Left,
			position: row,
			modifiers: Modifiers::default(),
			click_count: 1,
		});
	};
	press(&mut cx, true);
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.category_sheet.is_some(), "the first press only brought the window back");
	});
	press(&mut cx, false);
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none(), "the next press counts"));
	press(&mut cx, true);
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, None, "nor does a row take it"));
	press(&mut cx, false);
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, Some(3)));
}

#[gpui::test]
fn escape_closes_whatever_clean_sheet_is_on_top(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| {
		assert!(rdm.category_sheet.is_none(), "the presets have nothing to keep")
	});
	click(&mut cx, "button:Settings");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.settings_open()));
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(!rdm.settings_open()));
	click(&mut cx, "button:Add Task");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.adding.is_none(), "an empty Add Task goes too"));
}

#[gpui::test]
fn a_press_on_a_sheet_that_lands_on_no_field_drops_the_fields_focus(cx: &mut TestAppContext) {
	use gpui::{point, px};
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:New category");
	click(&mut cx, "button:Add");
	let name = rdm.read_with(&cx, |rdm, _| {
		let Some(CategorySheet::Custom(form)) = &rdm.category_sheet else { panic!("the form is up") };
		form.name.clone()
	});
	let focused =
		|cx: &mut VisualTestContext| cx.update(|window, cx| name.read(cx).focus().is_focused(window));
	assert!(focused(&mut cx), "a form opens with its first field ready to type into");
	// The card's padding: inside the sheet, on nothing.
	let card = cx.debug_bounds("category-sheet").expect("the form's card");
	cx.simulate_click(point(card.origin.x + px(6.0), card.origin.y + px(6.0)), Modifiers::default());
	assert!(!focused(&mut cx), "a press on the card takes the keyboard away from the field");
	cx.update(|window, cx| window.focus(&name.read(cx).focus(), cx));
	click(&mut cx, "toggle:Match case");
	assert!(focused(&mut cx), "a switch leaves it: the typing carries on after the press");
	click(&mut cx, "icon:film");
	assert!(!focused(&mut cx), "a press on a picker takes it, like any other control");
	rdm.read_with(&cx, |rdm, _| {
		assert!(matches!(rdm.category_sheet, Some(CategorySheet::Custom(_))), "the form stays");
	});
	// The switch made the form dirty, so only its cross leaves it, back to the presets, which
	// hold nothing and close from Escape.
	click(&mut cx, "button:Close");
	cx.simulate_keystrokes("escape");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.category_sheet.is_none()));
	click(&mut cx, "button:Settings");
	let search = rdm.read_with(&cx, |rdm, _| rdm.settings.as_ref().unwrap().search.clone());
	assert!(
		!cx.update(|window, cx| search.read(cx).focus().is_focused(window)),
		"Settings is a place, not a form: nothing in it takes the keyboard on opening"
	);
}

#[gpui::test]
fn opening_a_download_adds_one_window_and_removing_it_closes_it(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 2);
	rdm.update(&mut cx, |rdm, cx| rdm.open_download(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 2, "a second request raises the window, it does not open another");
	rdm.update(&mut cx, |rdm, cx| rdm.remove(2, cx));
	cx.run_until_parked();
	assert_eq!(cx.windows().len(), 1);
}
