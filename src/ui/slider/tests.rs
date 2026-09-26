use gpui::{Entity, Modifiers, Point, TestAppContext, VisualTestContext, point};

use super::*;

/// One zoom, a gap this wide, drawing `value` where the pointer is.
fn zoomed(
	slider: &Entity<Slider>,
	cx: &mut VisualTestContext,
	width: f32,
	value: f32,
	at: f32,
) -> bool {
	let gaps = slider.read_with(cx, |slider, _| slider.gaps.clone());
	gaps.len() == 1
		&& ((gaps[0].1 - gaps[0].0) - width).abs() < 1e-5
		&& (view(gaps.first().copied(), value) - at).abs() < 1e-4
}

#[test]
fn a_gap_is_the_one_holding_a_value_and_a_zoomed_track_draws_it_across_the_middle() {
	let places = [0.0, 0.1, 0.2, 0.3];
	assert_eq!(gap(&places, 0.15, 0.0), Some((0.1, 0.2)));
	assert_eq!(gap(&places, 0.2, 1.0), Some((0.2, 0.3)));
	assert_eq!(gap(&places, 0.2, -1.0), Some((0.1, 0.2)));
	assert_eq!(gap(&places, 0.3, 1.0), Some((0.2, 0.3)), "the last place has only the gap before it");
	assert_eq!(gap(&places, 0.0, -1.0), Some((0.0, 0.1)), "and the first only the one after");
	let zoom = Some((0.4, 0.5));
	assert!((view(zoom, 0.4) - MARGIN).abs() < 1e-6, "the gap's start is at the left margin");
	assert!((view(zoom, 0.5) - (1.0 - MARGIN)).abs() < 1e-6, "its end at the right one");
	assert!(
		(unview(zoom, 0.5) - 0.45).abs() < 1e-6,
		"the middle of the track is the middle of the gap"
	);
	assert_eq!(unview(zoom, 0.01), 0.4, "an end holds the value at the gap's edge");
	assert_eq!(unview(None, 0.3), 0.3, "the whole track is its scale, end to end");
	assert_eq!(view(None, 1.0), 1.0);
	assert!((even(0, 0.437) - 0.4).abs() < 1e-6, "ten steps across the track");
	assert!((even(1, 0.437) - 0.44).abs() < 1e-6, "ten in each of those");
	assert!((even(2, 0.4371) - 0.437).abs() < 1e-6, "and ten again");
	assert_eq!(steps(EXACT), None, "exact has none");
}

/// The slider with room around it, as it has in a sheet: a press past its ends lands in the window.
struct Room(Entity<Slider>);

impl Render for Room {
	fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
		div().p(px(40.0)).child(self.0.clone())
	}
}

fn opened(
	handles: Vec<f32>,
	cx: &mut TestAppContext,
) -> (Entity<Slider>, VisualTestContext, Bounds<Pixels>) {
	let window = cx.update(|cx| {
		cx.open_window(Default::default(), |_, cx| {
			let slider = cx.new(|_| Slider::new("Limit", handles));
			cx.new(|_| Room(slider))
		})
		.unwrap()
	});
	let mut cx = VisualTestContext::from_window(window.into(), cx);
	let slider = window.root(&mut cx).unwrap().read_with(&cx, |room, _| room.0.clone());
	cx.update(|window, _| window.refresh());
	cx.run_until_parked();
	let track = slider.read_with(&cx, |slider, _| slider.track.get()).expect("the track was drawn");
	(slider, cx, track)
}

#[gpui::test]
fn zooms_keep_the_value_and_coming_back_from_an_end_goes_out(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![0.5], cx);
	let y = track.center().y;
	let width = track.size.width / px(1.0);
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	// Where a value is on the whole track.
	let on = |value: f32| at(view(None, value));
	let handle = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.handles[0]);
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	assert_eq!(level(&mut cx), 0, "a value on a coarse place starts coarse");
	cx.simulate_mouse_move(on(0.61), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "on the coarsest places");
	cx.simulate_mouse_move(
		point(on(0.61).x + px(DEADZONE - 1.0), y),
		MouseButton::Left,
		Modifiers::default(),
	);
	cx.run_until_parked();
	assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "a hand this still holds the value");
	// A rest and a move is reading the number, not asking for a finer one.
	std::thread::sleep(Duration::from_millis(400));
	cx.simulate_mouse_move(on(0.62), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 0, "a pause goes nowhere");
	// Hanging between 0.6 and 0.7, away from both, for long enough goes in.
	let entered = 0.645;
	cx.simulate_mouse_move(on(0.642), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	std::thread::sleep(LINGER + Duration::from_millis(50));
	cx.simulate_mouse_move(on(entered), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 1, "the hang bought one level");
	assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "going in did not change the value");
	assert!(
		zoomed(&slider, &mut cx, 0.1, 0.6, view(None, entered)),
		"a gap wide, with the value under the pointer"
	);
	// On the zoomed track the handle follows the pointer, a tenth across the band's width.
	cx.simulate_mouse_move(
		point(on(entered).x + px(36.0), y),
		MouseButton::Left,
		Modifiers::default(),
	);
	cx.run_until_parked();
	let expected = even(1, 0.6 + 36.0 / width / (1.0 - 2.0 * MARGIN) * 0.1);
	assert!((handle(&mut cx) - expected).abs() < 1e-6, "zoomed: {}", handle(&mut cx));
	// Into the left end: the value stops, and however long the hand stays, it has not gone yet.
	let before = handle(&mut cx);
	cx.simulate_mouse_move(at(0.02), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	std::thread::sleep(EDGE_HOLD + Duration::from_millis(20));
	cx.simulate_mouse_move(at(0.021), MouseButton::Left, Modifiers::default());
	cx.executor().advance_clock(Duration::from_secs(2));
	cx.run_until_parked();
	assert_eq!(handle(&mut cx), before, "an end stops the value");
	assert_eq!(level(&mut cx), 1, "a hand in an end has not gone out yet");
	// Back to the middle: out, with the value as it was.
	cx.simulate_mouse_move(at(0.2), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 0, "coming back from the end goes out");
	assert!(
		(handle(&mut cx) - 0.2).abs() < 1e-6,
		"on the whole track the handle lands under the pointer, its value that place's: {} from {before}",
		handle(&mut cx)
	);
	// And is the pointer's from there.
	cx.simulate_mouse_move(at(0.4), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert!((handle(&mut cx) - 0.4).abs() < 1e-6, "{}", handle(&mut cx));
	cx.simulate_mouse_up(at(0.4), MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn hanging_between_two_places_zooms_in_and_keeps_the_value(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![0.5], cx);
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	// Where a value is on the whole track.
	let on = |value: f32| at(view(None, value));
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	cx.simulate_mouse_move(on(0.545), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 0);
	std::thread::sleep(LINGER + Duration::from_millis(50));
	cx.simulate_mouse_move(on(0.551), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 1, "between 0.5 and 0.6, away from both, for long enough");
	let value = slider.read_with(&cx, |slider, _| slider.handles[0]);
	assert!((value - 0.5).abs() < 1e-6, "the value it held, unchanged: {value}");
	assert!(zoomed(&slider, &mut cx, 0.1, 0.5, view(None, 0.551)), "drawn under the pointer");
	cx.simulate_mouse_up(at(0.551), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 0, "a release leaves the track unzoomed");
}

#[gpui::test]
fn a_press_starts_at_the_level_of_the_value_it_takes(cx: &mut TestAppContext) {
	// 0.55 is a place of the middle level only: the last drag ended there.
	let (slider, mut cx, track) = opened(vec![0.55], cx);
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	cx.simulate_mouse_down(at(0.55), MouseButton::Left, Modifiers::default());
	assert_eq!(level(&mut cx), 1, "a value between coarse places starts at its own level");
	assert!(zoomed(&slider, &mut cx, 0.1, 0.55, 0.55), "zoomed around it, under the pointer");
	cx.simulate_mouse_up(at(0.55), MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn a_swing_from_a_place_to_its_neighbour_and_back_goes_into_their_gap(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![0.5], cx);
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	// Where a value is on the whole track.
	let on = |value: f32| at(view(None, value));
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	// 0.5, over to 0.6, back to 0.5: the number is between them.
	for value in [0.52, 0.58, 0.52] {
		cx.simulate_mouse_move(on(value), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
	}
	assert_eq!(level(&mut cx), 0, "the swing alone marks the gap");
	// Inside it, briefly: in.
	cx.simulate_mouse_move(on(0.53), MouseButton::Left, Modifiers::default());
	std::thread::sleep(LINGER + Duration::from_millis(20));
	cx.simulate_mouse_move(on(0.535), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 1, "a hold inside the swung gap goes into it");
	cx.simulate_mouse_up(at(0.535), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	// A swing to somewhere else is not one: 0.5, 0.6, 0.7 marks nothing.
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	// Ending on a place, so nothing hangs either.
	for value in [0.58, 0.68, 0.702] {
		cx.simulate_mouse_move(on(value), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
	}
	std::thread::sleep(LINGER + Duration::from_millis(20));
	cx.simulate_mouse_move(on(0.703), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 0, "passing through places is not a swing");
	cx.simulate_mouse_up(on(0.703), MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn a_hand_that_stays_after_going_in_stays_at_that_level(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![0.5], cx);
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	// Where a value is on the whole track.
	let on = |value: f32| at(view(None, value));
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	cx.simulate_mouse_move(on(0.642), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	std::thread::sleep(LINGER + Duration::from_millis(30));
	cx.simulate_mouse_move(on(0.645), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 1);
	// Zoomed around the value it held, 0.6, drawn where the pointer went in; 0.605, halfway to the
	// next place of this level, is a twentieth of the band on.
	let dived = on(0.645).x;
	let between = dived + track.size.width * (1.0 - 2.0 * MARGIN) * 0.05;
	// Still, but for the jitter of a still hand, for longer than a hang: not another level.
	for _ in 0..2 {
		std::thread::sleep(LINGER + Duration::from_millis(30));
		cx.simulate_mouse_move(
			point(dived + px(DEADZONE - 1.0), y),
			MouseButton::Left,
			Modifiers::default(),
		);
		cx.run_until_parked();
	}
	assert_eq!(level(&mut cx), 1, "a hand that stays after going in stays at that level");
	// Moved, and hung again: the next level.
	cx.simulate_mouse_move(point(between, y), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	std::thread::sleep(LINGER + Duration::from_millis(30));
	cx.simulate_mouse_move(point(between + px(1.0), y), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 2, "once it has moved, a hang goes in again");
	cx.simulate_mouse_up(point(between + px(1.0), y), MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn each_visit_to_an_end_and_back_is_one_level_out_and_the_value_stays(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![0.5], cx);
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	// Where a value is on the whole track.
	let on = |value: f32| at(view(None, value));
	let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
	let handle = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.handles[0]);
	let hang = |cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>| {
		cx.simulate_mouse_move(from, MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		std::thread::sleep(LINGER + Duration::from_millis(30));
		cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
	};
	let visit = |cx: &mut VisualTestContext, end: f32, back: f32| {
		cx.simulate_mouse_move(at(end), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		std::thread::sleep(EDGE_HOLD + Duration::from_millis(20));
		cx.simulate_mouse_move(at(end + 0.001), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		cx.simulate_mouse_move(at(back), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
	};
	cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
	hang(&mut cx, on(0.642), on(0.645));
	// Zoomed around 0.6, the value it held, drawn where the pointer went in; halfway to the next
	// place of this level is a twentieth of the band on.
	let between = on(0.645).x + track.size.width * (1.0 - 2.0 * MARGIN) * 0.05;
	hang(&mut cx, point(between, y), point(between + px(1.0), y));
	assert_eq!(level(&mut cx), 2, "two hangs, two levels in");
	// A brush of an end, straight back: nothing.
	cx.simulate_mouse_move(at(0.02), MouseButton::Left, Modifiers::default());
	cx.simulate_mouse_move(at(0.3), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(level(&mut cx), 2, "a brush of an end is not a visit");
	// The value the brush left, moved by the hand in the middle as a drag moves it.
	let value = handle(&mut cx);
	// Into the left end and back to the middle: one level out, the value as it was, and the zoom
	// gone back to drawn with it under the pointer.
	visit(&mut cx, 0.02, 0.3);
	assert_eq!(level(&mut cx), 1, "one level out");
	assert_eq!(handle(&mut cx), value, "going out does not change the value");
	let drawn =
		slider.read_with(&cx, |slider, _| view(slider.gaps.last().copied(), slider.handles[0]));
	assert!((drawn - 0.3).abs() < 1e-4, "drawn under the pointer: {drawn}");
	// And again by the right end: out to the whole track, the value still as it was.
	visit(&mut cx, 0.98, 0.7);
	assert_eq!(level(&mut cx), 0, "a second visit is a second level");
	assert!(
		(handle(&mut cx) - 0.7).abs() < 1e-6,
		"out on the whole track, under the pointer: {}",
		handle(&mut cx)
	);
	cx.simulate_mouse_up(at(0.7), MouseButton::Left, Modifiers::default());
}

#[gpui::test]
fn a_handle_at_the_end_is_taken_by_a_press_on_its_dot(cx: &mut TestAppContext) {
	let (slider, mut cx, track) = opened(vec![1.0], cx);
	let y = track.center().y;
	let centre = track.left() + track.size.width * view(None, 1.0);
	// Its outer half, and its centre: each takes the handle where it is.
	for x in [centre + px(KNOB_W / 2.0 - 1.0), centre] {
		cx.simulate_mouse_down(point(x, y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		let (held, value) = slider.read_with(&cx, |slider, _| (slider.dragging, slider.handles[0]));
		assert!(held.is_some() && value == 1.0, "a press on the handle takes it and moves nothing");
		cx.simulate_mouse_up(point(x, y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
	}
}

#[gpui::test]
fn a_press_takes_the_nearest_handle_and_a_drag_off_the_track_keeps_it(cx: &mut TestAppContext) {
	let window = cx.update(|cx| {
		cx.open_window(Default::default(), |_, cx| cx.new(|_| Slider::new("Range", vec![0.2, 0.8])))
			.unwrap()
	});
	let mut cx = VisualTestContext::from_window(window.into(), cx);
	let slider = window.root(&mut cx).unwrap();
	cx.update(|window, _| window.refresh());
	cx.run_until_parked();
	let track = slider.read_with(&cx, |slider, _| slider.track.get()).expect("the track was drawn");
	let y = track.center().y;
	let at = |along: f32| point(track.left() + track.size.width * along, y);
	cx.simulate_mouse_down(at(0.7), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	let handles = slider.read_with(&cx, |slider, _| slider.handles.clone());
	assert!(
		(handles[1] - 0.7).abs() < 0.01 && handles[0] == 0.2,
		"the end handle took it: {handles:?}"
	);
	cx.simulate_mouse_move(
		point(track.right() + px(40.0), y),
		MouseButton::Left,
		Modifiers::default(),
	);
	cx.run_until_parked();
	assert_eq!(slider.read_with(&cx, |slider, _| slider.handles[1]), 1.0, "past the end is the end");
	cx.simulate_mouse_move(at(0.05), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert_eq!(slider.read_with(&cx, |slider, _| slider.handles[1]), 0.2, "and never past the other");
	cx.simulate_mouse_up(at(0.05), MouseButton::Left, Modifiers::default());
	cx.run_until_parked();
	assert!(slider.read_with(&cx, |slider, _| slider.dragging.is_none()), "a release lets go");
}
