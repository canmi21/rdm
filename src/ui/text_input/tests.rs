//! The field driven headlessly: its keys, its scrolling under the cursor, its drags, and the input
//! method's path -- marked text replaced and re-marked as a composition goes on, then committed.

use gpui::{Modifiers, TestAppContext, VisualTestContext};

use super::*;

fn field(cx: &mut TestAppContext) -> (Entity<TextInput>, VisualTestContext) {
	let window = cx.update(|cx| {
		cx.open_window(Default::default(), |_, cx| cx.new(|cx| TextInput::new("", cx))).unwrap()
	});
	let mut cx = VisualTestContext::from_window(window.into(), cx);
	let input = window.root(&mut cx).unwrap();
	(input, cx)
}

#[gpui::test]
fn enter_and_escape_reach_their_callbacks_after_the_field_is_done_with_itself(
	cx: &mut TestAppContext,
) {
	use std::cell::RefCell;
	use std::rc::Rc;
	// What a sheet does on Enter: read the field Enter was pressed in. Called from inside the
	// field's own update, that read panicked, and took the application down with it.
	let field: Rc<RefCell<Option<Entity<TextInput>>>> = Rc::default();
	let seen: Rc<RefCell<Vec<String>>> = Rc::default();
	cx.update(|cx| cx.bind_keys(key_bindings()));
	let window = cx.update(|cx| {
		let (field, seen) = (field.clone(), seen.clone());
		cx.open_window(Default::default(), move |_, cx| {
			let (confirmed, cancelled) = ((field.clone(), seen.clone()), (field.clone(), seen.clone()));
			let input = cx.new(|cx| {
				TextInput::new("", cx)
					.on_confirm(move |_, _, cx| {
						let input = confirmed.0.borrow().clone().unwrap();
						confirmed.1.borrow_mut().push(format!("confirm {}", input.read(cx).content));
					})
					.on_cancel(move |_, cx| {
						let input = cancelled.0.borrow().clone().unwrap();
						cancelled.1.borrow_mut().push(format!("cancel {}", input.read(cx).content));
					})
			});
			*field.borrow_mut() = Some(input.clone());
			input
		})
		.unwrap()
	});
	let mut cx = VisualTestContext::from_window(window.into(), cx);
	let input = window.root(&mut cx).unwrap();
	cx.update(|window, cx| {
		window.focus(&input.read(cx).focus(), cx);
		input
			.update(cx, |input, cx| input.replace_text_in_range(None, "https://a.example/x", window, cx));
	});
	cx.simulate_keystrokes("enter");
	cx.simulate_keystrokes("escape");
	cx.run_until_parked();
	assert_eq!(*seen.borrow(), ["confirm https://a.example/x", "cancel https://a.example/x"]);
}

#[gpui::test]
fn a_long_line_scrolls_under_the_cursor_and_back(cx: &mut TestAppContext) {
	let (input, mut cx) = field(cx);
	let long = "https://example.org/".to_owned() + &"segment/".repeat(40) + "file.bin";
	cx.update(|window, cx| {
		window.focus(&input.read(cx).focus(), cx);
		input.update(cx, |input, cx| input.replace_text_in_range(None, &long, window, cx));
	});
	cx.update(|window, _| window.refresh());
	cx.run_until_parked();
	let scrolled = input.read_with(&cx, |input, _| input.scroll);
	assert!(scrolled > px(0.0), "the cursor at the end pulled the line left: {scrolled:?}");
	cx.update(|window, cx| {
		input.update(cx, |input, cx| input.move_to(0, cx));
		window.refresh();
	});
	cx.run_until_parked();
	assert_eq!(input.read_with(&cx, |input, _| input.scroll), px(0.0), "home scrolls back");
}

#[gpui::test]
fn a_drag_held_past_the_left_edge_selects_to_the_start(cx: &mut TestAppContext) {
	let (input, mut cx) = field(cx);
	let long = "https://example.org/".to_owned() + &"segment/".repeat(40) + "file.bin";
	cx.update(|window, cx| {
		window.focus(&input.read(cx).focus(), cx);
		input.update(cx, |input, cx| input.replace_text_in_range(None, &long, window, cx));
	});
	cx.update(|window, _| window.refresh());
	cx.run_until_parked();
	let bounds = input.read_with(&cx, |input, _| input.last_bounds).expect("the field was drawn");
	let y = bounds.center().y;
	cx.simulate_mouse_down(
		point(bounds.right() - px(4.0), y),
		MouseButton::Left,
		Modifiers::default(),
	);
	// Just past the edge and held there: the pointer alone reaches only what is in view, and
	// the rest comes a character a frame.
	let past = point(bounds.left() - px(4.0), y);
	cx.simulate_mouse_move(past, MouseButton::Left, Modifiers::default());
	for _ in 0..1000 {
		if input.read_with(&cx, |input, _| input.selected_range.start == 0) {
			break;
		}
		cx.update(|window, _| window.refresh());
		cx.run_until_parked();
	}
	// The last step lands in a paint, after that frame's scroll was set; the next frame shows it.
	cx.update(|window, _| window.refresh());
	cx.run_until_parked();
	let (range, scroll) =
		input.read_with(&cx, |input, _| (input.selected_range.clone(), input.scroll));
	assert_eq!(range.start, 0, "the selection reached the start of the address");
	assert!(range.end + 2 >= long.len(), "and kept its other end where the press was: {range:?}");
	assert_eq!(scroll, px(0.0), "with the line scrolled back to show the start");
	cx.simulate_mouse_up(past, MouseButton::Left, Modifiers::default());
	assert!(!input.read_with(&cx, |input, _| input.is_selecting), "a release outside ends it");
}

#[gpui::test]
fn a_composition_replaces_its_own_marked_text_and_commits(cx: &mut TestAppContext) {
	let (input, mut cx) = field(cx);
	cx.update(|window, cx| {
		input.update(cx, |input, cx| {
			input.replace_text_in_range(None, "url ", window, cx);
			// Pinyin as it is typed: each keystroke re-marks the whole composition.
			input.replace_and_mark_text_in_range(None, "n", Some(0..1), window, cx);
			input.replace_and_mark_text_in_range(None, "ni", Some(0..2), window, cx);
			input.replace_and_mark_text_in_range(None, "ni h", Some(0..4), window, cx);
			// Committed: two characters, three bytes each, replacing the marked run.
			input.replace_text_in_range(None, "你好", window, cx);
			assert_eq!(input.content.as_ref(), "url 你好");
			assert_eq!(input.selected_range, 10..10);
			assert!(input.marked_range.is_none());
			// Typing on after the commit lands after the characters, not inside one.
			input.replace_text_in_range(None, "!", window, cx);
			assert_eq!(input.content.as_ref(), "url 你好!");
		});
	});
}

#[gpui::test]
fn offsets_past_the_content_are_clamped_rather_than_sliced(cx: &mut TestAppContext) {
	let (input, mut cx) = field(cx);
	cx.update(|window, cx| {
		input.update(cx, |input, cx| {
			input.replace_text_in_range(None, "好", window, cx);
			input.replace_and_mark_text_in_range(Some(0..1), "a", Some(5..9), window, cx);
			// The commit replaces the marked run, whatever selection was asked for past the end.
			input.replace_text_in_range(None, "b", window, cx);
			assert_eq!(input.content.as_ref(), "b");
		});
	});
}
