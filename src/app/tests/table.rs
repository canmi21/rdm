//! The table: sorting, the funnel over the rows, and every way a column's width is dragged.

use super::*;

#[gpui::test]
fn a_title_cycles_ascending_descending_default(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "sort:Size");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!((rdm.sort, rdm.ascending), (SortKey::Size, true));
		let sizes: Vec<u64> = rdm.shown().iter().map(|d| d.size).collect();
		assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{sizes:?}");
	});
	click(&mut cx, "sort:Size");
	rdm.read_with(&cx, |rdm, _| assert_eq!((rdm.sort, rdm.ascending), (SortKey::Size, false)));
	click(&mut cx, "sort:Size");
	rdm
		.read_with(&cx, |rdm, _| assert!(rdm.default_order(), "a third click returns to newest first"));
}

#[gpui::test]
fn the_funnel_menu_narrows_within_the_sidebar_and_all_clears_it(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "button:Filter by status");
	click(&mut cx, "chip:Completed");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.status, Some(Status::Completed));
		assert!(!rdm.filter_open, "choosing closes the menu");
		assert!(rdm.shown().iter().all(|d| d.status == Status::Completed));
	});
	click(&mut cx, "button:Filter by status");
	click(&mut cx, "chip:All Tasks");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.status, None));
}

/// The funnel cuts inside whatever the sidebar chose, so it offers only the statuses that state
/// holds; a status the next state cannot hold is let go rather than left to empty the list under
/// a funnel lit with a word that cannot match anything.
#[gpui::test]
fn the_funnel_offers_what_the_state_holds_and_lets_go_of_what_it_cannot(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "filter:Unfinished");
	click(&mut cx, "button:Filter by status");
	click(&mut cx, "chip:Paused");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.status, Some(Status::Paused), "Unfinished holds what is paused");
		assert!(rdm.shown().iter().all(|d| d.status == Status::Paused));
	});
	click(&mut cx, "filter:Completed");
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.filter, Filter::Completed);
		assert_eq!(rdm.status, None, "and Completed holds nothing paused, so the cut is dropped");
		assert!(rdm.shown().iter().all(|d| Filter::Completed.statuses().contains(&d.status)));
	});
}

/// An overlay takes the mouse from what it covers. The funnel's menu floats over the rows, and
/// a press on the card itself -- not on one of its lines -- must not reach the row underneath.
/// GPUI hit-tests from the top down and stops at the first element that occludes; one that does
/// not occlude is a picture, and everything behind it goes on answering a pointer that is
/// nowhere near it.
#[gpui::test]
fn the_funnel_menu_takes_the_mouse_from_the_rows_it_covers(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	// Enough rows that the list reaches the corner the menu is drawn in. With the sample alone
	// the menu floats over empty space and a press through it would prove nothing.
	rdm.update(&mut cx, |rdm, cx| {
		let sample = crate::download::sample();
		let mut many = Vec::new();
		for round in 0..12u64 {
			for (at, download) in sample.iter().enumerate() {
				let mut copy = download.clone();
				copy.id = 100 + round * 10 + at as u64;
				many.push(copy);
			}
		}
		rdm.downloads = many;
		rdm.selected = Some(100);
		cx.notify();
	});
	cx.run_until_parked();
	// The menu is opened and closed through the window rather than by pressing the funnel, so
	// that the only pointer events in this test are the ones it is about.
	let show = |rdm: &Entity<Rdm>, cx: &mut VisualTestContext, open: bool| {
		rdm.update(cx, |rdm, cx| rdm.toggle_filter_menu(open, cx));
		cx.run_until_parked();
	};
	show(&rdm, &mut cx, true);
	let menu = cx.debug_bounds("menu").expect("the menu is drawn");
	// Two points inside the card's top-left: on the menu, on no line of it, and over the list.
	// The pointer is moved there before it presses, which is what a hand does and what makes the
	// press read the same hit test a hover reads.
	let corner = menu.origin + gpui::point(gpui::px(2.0), gpui::px(2.0));
	let press = |cx: &mut VisualTestContext| {
		cx.simulate_mouse_move(corner, None, Modifiers::default());
		cx.simulate_click(corner, Modifiers::default());
	};
	// With nothing over it that point is a row, which is what gives the press below something to
	// reach if the menu lets it through.
	show(&rdm, &mut cx, false);
	press(&mut cx);
	let covered = rdm.read_with(&cx, |rdm, _| rdm.selected);
	assert_ne!(covered, Some(100), "the menu's corner is over a row");
	show(&rdm, &mut cx, true);
	press(&mut cx);
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.selected, covered, "the row under the menu was not reached through it");
	});
}

/// The band that takes a column's drag is laid over the titles on either side of the boundary,
/// twice the width of the line, because a hot zone the width of the line was missed more often
/// than hit. It takes the mouse too: a press on the line is a press on the handle, and the title
/// under its edge must not also read it as a click and re-sort the column.
#[gpui::test]
fn the_resize_band_takes_the_press_from_the_title_behind_it(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	let before = rdm.read_with(&cx, |rdm, _| (rdm.sort, rdm.ascending));
	let band = cx.debug_bounds("resize:Size").expect("the handle is drawn");
	let title = cx.debug_bounds("sort:Size").expect("the title is drawn");
	let over_both = gpui::point(band.origin.x + band.size.width - gpui::px(2.0), title.center().y);
	assert!(
		title.contains(&over_both),
		"the band does lie over the title, which is the case at issue"
	);
	cx.simulate_click(over_both, Modifiers::default());
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!((rdm.sort, rdm.ascending), before, "pressing a boundary is not sorting by it");
	});
}

#[gpui::test]
fn a_row_selects_and_the_view_switch_redraws_it(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	click(&mut cx, "row:3");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, Some(3)));
	click(&mut cx, "view:Grid");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.view, View::Grid));
	click(&mut cx, "row:3");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.selected, None));
}

#[gpui::test]
fn dragging_a_header_edge_resizes_that_column(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};
	let (rdm, mut cx) = open(cx);
	let before = rdm.read_with(&cx, |rdm, _| rdm.width(Column::Size));
	let handle = cx.debug_bounds("resize:Size").expect("a handle after the Size title");
	let start = handle.center();
	cx.simulate_event(MouseDownEvent {
		button: MouseButton::Left,
		position: start,
		modifiers: Modifiers::default(),
		click_count: 1,
		first_mouse: false,
	});
	let moved = point(start.x + px(40.0), start.y);
	cx.simulate_event(MouseMoveEvent {
		position: moved,
		pressed_button: Some(MouseButton::Left),
		modifiers: Modifiers::default(),
	});
	cx.simulate_event(MouseUpEvent {
		button: MouseButton::Left,
		position: moved,
		modifiers: Modifiers::default(),
		click_count: 1,
	});
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(
			rdm.width(Column::Size),
			before - 40.0,
			"the boundary followed the pointer right, so the column narrowed"
		);
		assert!(rdm.resizing.is_none(), "the drag ends with the button");
	});
}

#[gpui::test]
fn reset_under_appearance_puts_every_column_width_back(cx: &mut TestAppContext) {
	let (rdm, mut cx) = open(cx);
	rdm.update(&mut cx, |rdm, cx| {
		rdm.widths = [200.0, 90.0, 60.0, 70.0, 80.0];
		rdm.open_settings(cx);
	});
	cx.run_until_parked();
	click(&mut cx, "section:Appearance");
	click(&mut cx, "button:Reset");
	rdm.read_with(&cx, |rdm, _| assert_eq!(rdm.widths, Column::DEFAULT_WIDTHS));
}

#[gpui::test]
fn a_drag_stops_where_the_name_column_would_vanish(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, point, px};
	let (rdm, mut cx) = open(cx);
	let handle = cx.debug_bounds("resize:Size").unwrap();
	let start = handle.center();
	cx.simulate_event(MouseDownEvent {
		button: MouseButton::Left,
		position: start,
		modifiers: Modifiers::default(),
		click_count: 1,
		first_mouse: false,
	});
	cx.simulate_event(MouseMoveEvent {
		position: point(px(0.0), start.y),
		pressed_button: Some(MouseButton::Left),
		modifiers: Modifiers::default(),
	});
	let name = cx.debug_bounds("sort:Name").expect("the name title is still drawn");
	assert!(
		f32::from(name.size.width) >= crate::ui::list::NAME_MIN - 12.0,
		"name column kept {:?}",
		name.size.width
	);
	let added = cx.debug_bounds("sort:Added").unwrap();
	let viewport = cx.update(|w, _| w.viewport_size());
	assert!(added.right() <= viewport.width, "the last column stays inside the window");
	rdm.read_with(&cx, |rdm, _| assert!(rdm.resizing.is_some()));
}

/// The stop is a ceiling, not a place the boundary bounces off: it held one drag's starting width
/// against the widths as they stood, so once the column passed it the two disagreed, the ceiling
/// fell as the column rose, and the boundary alternated across the pointer -- stopping short,
/// twitching there, and letting a fresh press take half of what was left.
#[gpui::test]
fn a_drag_past_the_stop_holds_there_and_a_second_takes_no_more(cx: &mut TestAppContext) {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, point, px};

	/// One press on the Size handle and twenty points of leftward travel per move, which widens
	/// the column, with the width after every move so the walk itself can be read.
	fn drag_left(rdm: &Entity<Rdm>, cx: &mut VisualTestContext, from: Point<Pixels>) -> Vec<f32> {
		cx.simulate_event(MouseDownEvent {
			button: MouseButton::Left,
			position: from,
			modifiers: Modifiers::default(),
			click_count: 1,
			first_mouse: false,
		});
		let widths = (1u8..=25)
			.map(|step| {
				cx.simulate_event(MouseMoveEvent {
					position: point(from.x - px(20.0 * f32::from(step)), from.y),
					pressed_button: Some(MouseButton::Left),
					modifiers: Modifiers::default(),
				});
				rdm.read_with(cx, |rdm, _| rdm.width(Column::Size))
			})
			.collect();
		cx.simulate_event(MouseUpEvent {
			button: MouseButton::Left,
			position: from,
			modifiers: Modifiers::default(),
			click_count: 1,
		});
		widths
	}

	let (rdm, mut cx) = open(cx);
	let start = cx.debug_bounds("resize:Size").unwrap().center();
	let first = drag_left(&rdm, &mut cx, start);
	assert!(
		first.windows(2).all(|w| w[1] >= w[0]),
		"the pointer only went left, so the column only widened: {first:?}"
	);
	let stop = *first.last().unwrap();
	let second = drag_left(&rdm, &mut cx, start);
	assert_eq!(
		second.last().copied(),
		Some(stop),
		"the same travel from the same place reaches the same stop, however often it is asked"
	);
}

/// The width a window opens at is worked out from what the table takes, so that the name column
/// is left the room it was sized for. The two are written apart -- one in `ui`, one in the table's
/// own widths -- and this is what keeps them agreeing.
#[gpui::test]
fn a_first_launch_leaves_the_name_column_the_room_it_was_sized_for(cx: &mut TestAppContext) {
	use gpui::{px, size};

	let (rdm, cx) = open(cx);
	cx.simulate_resize(size(px(crate::ui::FIRST_WIDTH), px(crate::ui::FIRST_HEIGHT)));
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(rdm.widths, Column::DEFAULT_WIDTHS, "a first launch drags no column");
		assert_eq!(rdm.name_width(&rdm.drawn()), crate::ui::NAME_ROOM);
	});
}

/// A press at a column's handle, the pointer walked to each offset from it in turn, then the
/// release. What every step left the row at, so a drag reads as the rows it went through.
fn drag(
	rdm: &Entity<Rdm>,
	cx: &mut VisualTestContext,
	handle: &'static str,
	steps: &[f32],
) -> Vec<[f32; 5]> {
	use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, point, px};
	let from = cx.debug_bounds(handle).expect("a handle to take hold of").center();
	cx.simulate_event(MouseDownEvent {
		button: MouseButton::Left,
		position: from,
		modifiers: Modifiers::default(),
		click_count: 1,
		first_mouse: false,
	});
	let rows = steps
		.iter()
		.map(|dx| {
			cx.simulate_event(MouseMoveEvent {
				position: point(from.x + px(*dx), from.y),
				pressed_button: Some(MouseButton::Left),
				modifiers: Modifiers::default(),
			});
			rdm.read_with(cx, |rdm, _| rdm.drawn())
		})
		.collect();
	cx.simulate_event(MouseUpEvent {
		button: MouseButton::Left,
		position: from,
		modifiers: Modifiers::default(),
		click_count: 1,
	});
	rows
}

/// The size the application opens at with nothing remembered. The default widths leave the name
/// column under its floor here, which is what made the old stop unreachable: it was derived from
/// that floor, so every column's ceiling sat below the width it already had, and the first move
/// of any press snapped it there -- twenty-four points for a one-point nudge, in both directions.
#[gpui::test]
fn a_nudge_at_a_handle_moves_the_boundary_by_the_nudge(cx: &mut TestAppContext) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	let before = rdm.read_with(&cx, |rdm, _| rdm.drawn());
	let narrower = drag(&rdm, &mut cx, "resize:Size", &[1.0]);
	assert_eq!(narrower[0][0], before[0] - 1.0, "a point of travel is a point of width");
	let wider = drag(&rdm, &mut cx, "resize:Size", &[-1.0]);
	assert_eq!(wider[0][0], before[0], "and the same the other way");
}

/// Widening takes from the left of the handle, and only from what is above a floor: the name
/// column first, since it holds the slack, then each fixed column between it and the handle,
/// nearest first.
#[gpui::test]
fn widening_squeezes_leftwards_until_everything_left_of_the_handle_is_on_its_floor(
	cx: &mut TestAppContext,
) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	let before = rdm.read_with(&cx, |rdm, _| rdm.drawn());
	let rows = drag(&rdm, &mut cx, "resize:Added", &[-100.0, -800.0, 0.0]);
	assert_eq!(rows[0][..3], before[..3], "the columns further left are not asked yet");
	assert!(
		rows[0][3] < before[3],
		"Status is nearest the handle, so it gives what the name could not"
	);
	assert_eq!(
		rows[1][..4],
		Column::MINS[..4],
		"far enough, and everything left of it is on its floor"
	);
	let name = rdm.read_with(&cx, |rdm, _| rdm.name_width(&rows[1]));
	assert_eq!(name, crate::ui::list::NAME_MIN, "the name column on its own floor with them");
	assert_eq!(rows[2], before, "and the way back gives back exactly what the way out took");
}

/// The window will not go narrower than every floor together, and at that width the table is
/// exactly its floors: what the window took from the widths on the way down, it gives back.
#[gpui::test]
fn at_the_windows_least_width_the_columns_are_their_floors_and_the_row_still_fits(
	cx: &mut TestAppContext,
) {
	use gpui::{px, size};
	let (rdm, mut cx) = open(cx);
	cx.simulate_resize(size(px(crate::ui::MIN_WIDTH), px(crate::ui::MIN_HEIGHT)));
	cx.run_until_parked();
	let (drawn, name) = rdm.read_with(&cx, |rdm, _| (rdm.drawn(), rdm.name_width(&rdm.drawn())));
	assert_eq!(drawn, Column::MINS, "every column on its floor");
	assert_eq!(name, crate::ui::list::NAME_MIN, "the name column included");
	let added = cx.debug_bounds("sort:Added").expect("the last title is drawn");
	assert!(
		added.right() <= px(crate::ui::MIN_WIDTH),
		"the row is inside the window, not overflowing it: {:?}",
		added.right()
	);
	cx.simulate_resize(size(px(960.0), px(600.0)));
	cx.run_until_parked();
	rdm.read_with(&cx, |rdm, _| {
		assert_eq!(
			rdm.drawn(),
			Column::DEFAULT_WIDTHS,
			"widened again, and back to what was asked for"
		);
	});
}
