//! A track with one handle or two, pressed or dragged along its length, for a value set roughly
//! by hand beside a field that sets it exactly. GPUI ships none. See spec/ui.md.

use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
	App, Bounds, Context, DispatchPhase, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
	MouseUpEvent, Pixels, Render, SharedString, Window, canvas, div, prelude::*, px, relative,
};

use crate::ui::LeavesFocus;
use crate::ui::theme;

/// What a move of a handle does, decided by whoever owns the slider: which handle, where along
/// the track it now is, from 0 at the left to 1 at the right, and the level the drag is at, so the
/// owner can round its own value to the same step.
type OnChange = Box<dyn Fn(usize, f32, usize, &mut Window, &mut App)>;

/// Where a position lands at a level, in the owner's terms: a limit on round rates, a part of a
/// file on round sizes. The slider knows only positions; what is round is the owner's to say.
type Snap = Box<dyn Fn(usize, f32, &App) -> f32>;

/// A drag moves through levels, coarse to exact, and going finer is zooming into one gap of the
/// level above: the gaps it went into are kept, and leaving the last of them goes back out. See
/// spec/ui.md, "A slider reads the hand's intent from its pauses".
pub const EXACT: usize = 3;

/// A hand this still, in points, is holding the value: it does not move, and the time counts
/// towards a pause.
const DEADZONE: f32 = 3.0;
/// How long a hand has to hold before its next move goes one level finer.
const DWELL: Duration = Duration::from_millis(300);
/// How long a hand has to hang between two places, away from both, before the level goes finer:
/// the number it wants is not on this level. Not at the finest stepped level, which goes to exact
/// only on a pause.
const LINGER: Duration = Duration::from_millis(600);
/// How far from a place, as a share of its gap, a hand is hanging between two rather than on one.
const AWAY: f32 = 0.25;
/// How far past a gap's edge, as a share of the gap, a hand has to go to be outside it, and at
/// least how many points.
const EXIT: f32 = 0.3;
const EXIT_POINTS: f32 = 8.0;
/// How long a hand has to stay outside the gap to have left it: fine work near an edge crosses it
/// and comes back, and a band that vanished on every crossing was the first complaint about it.
const EXIT_HOLD: Duration = Duration::from_millis(500);
/// A throw: this share of the track covered within `THROW_WINDOW` goes straight to the coarsest
/// level, however deep the drag was.
const THROW_SPAN: f32 = 0.2;
const THROW_WINDOW: Duration = Duration::from_millis(150);
/// How finely each level's places are found, by asking the snap at this many points along the track.
const PROBES: usize = 2000;
/// Two positions closer than this are one place.
const SAME: f32 = 1e-5;

/// Steps of a share of the track, for a slider whose owner says nothing about what is round.
fn even(level: usize, position: f32) -> f32 {
	match [0.1, 0.025, 0.005].get(level) {
		Some(step) => ((position / step).round() * step).clamp(0.0, 1.0),
		None => position,
	}
}

/// The gap between two neighbouring places that holds `at`; on a place, the gap on the side
/// `toward` points to.
fn gap(places: &[f32], at: f32, toward: f32) -> Option<(f32, f32)> {
	if places.len() < 2 {
		return None;
	}
	let i = places.iter().rposition(|p| *p <= at + SAME).unwrap_or(0);
	let on = (places[i] - at).abs() <= SAME;
	let i = if (on && toward < 0.0) || i + 1 == places.len() { i.max(1) - 1 } else { i };
	Some((places[i], places[i + 1]))
}

pub struct Slider {
	/// Where each handle is along the track, 0 to 1, in order from the left.
	handles: Vec<f32>,
	/// The handle a press took hold of, until the release.
	dragging: Option<usize>,
	/// The track as last drawn, which a pointer's x is measured against.
	track: Rc<Cell<Option<Bounds<Pixels>>>>,
	label: SharedString,
	on_change: Option<OnChange>,
	snap: Option<Snap>,
	/// Every stepped level's places, found when a handle is taken hold of.
	places: Vec<Vec<f32>>,
	/// The gaps the drag went into, one a level: its length is the level, 0 the coarsest and
	/// `EXACT` the pointer itself, and the last is the gap a move has to leave to go back out.
	gaps: Vec<(f32, f32)>,
	/// Since when the hand has hung between two places, away from both.
	hanging: Option<Instant>,
	/// Since when the hand has been outside the gap the drag went into.
	leaving: Option<Instant>,
	/// Where the handle was lately, for a throw to be seen in.
	recent: VecDeque<(Instant, f32)>,
	/// Where the hand came to rest and since when; a move within `DEADZONE` of it is not a move.
	rest: Option<(Pixels, Instant)>,
	/// How far the handle sits from the pointer, as a share of the track: set when a level grows
	/// finer, so the handle carries on from its round place instead of jumping to the pointer.
	offset: f32,
}

impl Slider {
	pub fn new(label: impl Into<SharedString>, handles: Vec<f32>) -> Self {
		Self {
			handles,
			dragging: None,
			track: Rc::default(),
			label: label.into(),
			on_change: None,
			snap: None,
			places: Vec::new(),
			gaps: Vec::new(),
			hanging: None,
			leaving: None,
			recent: VecDeque::new(),
			rest: None,
			offset: 0.0,
		}
	}

	pub fn on_change(mut self, f: impl Fn(usize, f32, usize, &mut Window, &mut App) + 'static) -> Self {
		self.on_change = Some(Box::new(f));
		self
	}

	pub fn snap(mut self, f: impl Fn(usize, f32, &App) -> f32 + 'static) -> Self {
		self.snap = Some(Box::new(f));
		self
	}

	fn snapped(&self, level: usize, position: f32, cx: &App) -> f32 {
		if level >= EXACT {
			return position;
		}
		match &self.snap {
			Some(snap) => snap(level, position, cx),
			None => even(level, position),
		}
	}

	fn level(&self) -> usize {
		self.gaps.len()
	}

	/// The places a level lands on, in order, found by asking the snap along the whole track.
	fn find_places(&self, level: usize, cx: &App) -> Vec<f32> {
		let mut places: Vec<f32> = Vec::new();
		for k in 0..=PROBES {
			let at = self.snapped(level, k as f32 / PROBES as f32, cx);
			if places.last().is_none_or(|last| (at - last).abs() > SAME) {
				places.push(at);
			}
		}
		places
	}

	/// The coarsest level a value is a place of, and the gaps of every coarser one around it: a
	/// value left on a round rate starts the next drag at that rate's level, and one left between
	/// every level's places starts it exact.
	fn start_at(&mut self, value: f32, cx: &App) {
		self.gaps.clear();
		for level in 0..EXACT {
			if (self.snapped(level, value, cx) - value).abs() <= SAME {
				return;
			}
			match gap(&self.places[level], value, 1.0) {
				Some(around) => self.gaps.push(around),
				None => return,
			}
		}
	}

	/// One level finer, into the gap of this level that holds `at`.
	fn go_finer(&mut self, at: f32, toward: f32) {
		let level = self.level();
		if let Some(around) = self.places.get(level).and_then(|places| gap(places, at, toward)) {
			self.gaps.push(around);
		}
		self.hanging = None;
	}

	pub fn handles(&self) -> &[f32] {
		&self.handles
	}

	/// The handles moved to follow a value set elsewhere, without telling the owner, who set it.
	pub fn set(&mut self, handles: Vec<f32>, cx: &mut Context<Self>) {
		if self.handles != handles {
			self.handles = handles;
			cx.notify();
		}
	}

	/// Where along the track a pointer is, 0 to 1, over the track or past either end.
	fn position_at(&self, x: Pixels) -> Option<f32> {
		self.along(x).map(|at| at.clamp(0.0, 1.0))
	}

	/// The same, not held to the track, so an offset handle can still reach either end.
	fn along(&self, x: Pixels) -> Option<f32> {
		let track = self.track.get()?;
		let width = track.size.width;
		(width > px(0.0)).then(|| (x - track.left()) / width)
	}

	/// A handle moved to `position`, held between its neighbours so a pair never crosses, and the
	/// owner told.
	fn move_handle(&mut self, index: usize, position: f32, window: &mut Window, cx: &mut Context<Self>) {
		let low = if index > 0 { self.handles[index - 1] } else { 0.0 };
		let high = self.handles.get(index + 1).copied().unwrap_or(1.0);
		let position = position.clamp(low, high);
		self.handles[index] = position;
		if let Some(on_change) = &self.on_change {
			on_change(index, position, self.level(), window, cx);
		}
		cx.notify();
	}

	fn press(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(position) = self.position_at(event.position.x) else { return };
		if self.handles.is_empty() {
			return;
		}
		// The nearest handle takes the press. Of a pair at the same place, the one on the side the
		// press is, so two pushed together can still be pulled apart.
		let distance = |at: f32| (at - position).abs();
		let nearest = self.handles.iter().copied().map(distance).fold(f32::INFINITY, f32::min);
		let closest: Vec<usize> =
			(0..self.handles.len()).filter(|&i| distance(self.handles[i]) <= nearest).collect();
		let index = match closest.as_slice() {
			[.., last] if closest.len() > 1 && position > self.handles[closest[0]] => *last,
			[first, ..] => *first,
			[] => return,
		};
		self.dragging = Some(index);
		let now = Instant::now();
		self.rest = Some((event.position.x, now));
		self.hanging = None;
		self.leaving = None;
		self.recent.clear();
		self.places = (0..EXACT).map(|level| self.find_places(level, cx)).collect();
		// The level is the value's own, so a drag picks up where the last one left off.
		let value = self.handles[index];
		self.start_at(value, cx);
		// A press on the handle takes hold of it where it is; one elsewhere on the track brings it
		// there, on a place of its level.
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		if width > 0.0 && (value - position).abs() * width <= 7.0 {
			self.offset = value - position;
			cx.notify();
		} else {
			self.offset = 0.0;
			let landed = self.snapped(self.level(), position, cx);
			self.move_handle(index, landed, window, cx);
		}
	}

	fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(index) = self.dragging else { return };
		if event.pressed_button != Some(MouseButton::Left) {
			self.dragging = None;
			cx.notify();
			return;
		}
		let x = event.position.x;
		let now = Instant::now();
		let Some(along) = self.along(x) else { return };
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(1.0);
		let at = (along + self.offset).clamp(0.0, 1.0);

		// A throw goes straight out to the coarsest level, under the pointer.
		self.recent.push_back((now, at));
		while self.recent.front().is_some_and(|(t, _)| now.duration_since(*t) > THROW_WINDOW) {
			self.recent.pop_front();
		}
		let (low, high) = self.recent.iter().fold((at, at), |(l, h), (_, a)| (l.min(*a), h.max(*a)));
		if self.level() > 0 && high - low >= THROW_SPAN {
			self.gaps.clear();
			self.offset = 0.0;
			self.hanging = None;
			self.leaving = None;
		}
		let at = (along + self.offset).clamp(0.0, 1.0);

		// Leaving the gap the drag went into, and staying out, goes back out to the level above, as
		// often as the hand is outside gaps.
		let outside = |(lo, hi): (f32, f32)| {
			let margin = ((hi - lo) * EXIT).max(EXIT_POINTS / width);
			at < lo - margin || at > hi + margin
		};
		match (self.gaps.last().copied().is_some_and(outside), self.leaving) {
			(false, _) => self.leaving = None,
			(true, None) => self.leaving = Some(now),
			(true, Some(since)) if now.duration_since(since) >= EXIT_HOLD => {
				while self.gaps.last().copied().is_some_and(outside) {
					self.gaps.pop();
				}
				self.leaving = None;
				self.hanging = None;
			}
			(true, Some(_)) => {}
		}

		// Hanging between two places, away from both, for long enough: the number is not on this
		// level. The finest stepped level goes to exact only on a pause.
		let level = self.level();
		let mut finer = false;
		if level + 1 < EXACT {
			let away = self.places.get(level).and_then(|p| gap(p, at, 0.0)).is_some_and(|(lo, hi)| {
				(at - lo).min(hi - at) > (hi - lo) * AWAY
			});
			match (away, self.hanging) {
				(false, _) => self.hanging = None,
				(true, None) => self.hanging = Some(now),
				(true, Some(since)) if now.duration_since(since) >= LINGER => {
					self.go_finer(at, 0.0);
					finer = true;
				}
				(true, Some(_)) => {}
			}
		}

		let (rest, since) = self.rest.unwrap_or((x, now));
		if !finer && ((x - rest) / px(1.0)).abs() <= DEADZONE {
			// Held: the value stays what it is while the number is read.
			return;
		}
		if !finer && self.level() < EXACT && now.duration_since(since) >= DWELL {
			// A pause, then a move: one level finer, into the gap on the side the hand moved to,
			// carrying on from where the handle is rather than from where the pointer is.
			let held = self.handles[index];
			if let Some(from) = self.along(rest) {
				self.offset = held - from;
			}
			self.go_finer(held, ((x - rest) / px(1.0)).signum());
		}
		self.rest = Some((x, now));
		let at = (along + self.offset).clamp(0.0, 1.0);
		let position = self.snapped(self.level(), at, cx);
		self.move_handle(index, position, window, cx);
	}
}

impl Render for Slider {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let track = self.track.clone();
		let slider = cx.weak_entity();
		let dragging = self.dragging.is_some();
		// The filled part: between the handles of a pair, or from the left end to a lone handle.
		let (from, to) = match self.handles.as_slice() {
			[] => (0.0, 0.0),
			[one] => (0.0, *one),
			[first, .., last] => (*first, *last),
		};
		let label = self.label.clone();
		let pair = self.handles.len() > 1;
		// The gap the drag went into, as a band on the track, and the places of its level inside it,
		// so both where a move lands and where leaving goes back out are seen before they happen.
		// No places when two would be closer than a few points, where they would be a texture.
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		let band = self.gaps.last().copied().filter(|_| dragging);
		let (lo, hi) = band.unwrap_or((0.0, 1.0));
		let mut ticks: Vec<f32> = match self.places.get(self.level()) {
			Some(places) if dragging => {
				places.iter().copied().filter(|p| *p >= lo - SAME && *p <= hi + SAME).collect()
			}
			_ => Vec::new(),
		};
		if ticks.windows(2).any(|w| (w[1] - w[0]) * width < 6.0) {
			ticks.clear();
		}
		let ticks = ticks.into_iter();
		div()
			.id(SharedString::from(format!("slider:{label}")))
			.debug_selector({
				let label = label.clone();
				move || format!("slider:{label}")
			})
			.relative()
			.w_full()
			.min_w(px(60.0))
			.h(px(20.0))
			.cursor_pointer()
			.leaves_focus()
			.on_mouse_down(MouseButton::Left, cx.listener(Self::press))
			.child(
				// Where the track is, for a pointer to be measured against, and while a handle is held,
				// the window's listeners: an element hears the pointer only while it is over the
				// element, and a drag leaves the track as often as it stays on it.
				canvas(
					move |bounds, _, _| track.set(Some(bounds)),
					move |_, _, window, _| {
						if !dragging {
							return;
						}
						let moved = slider.clone();
						window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
							if phase == DispatchPhase::Bubble {
								let _ = moved.update(cx, |slider, cx| slider.drag(event, window, cx));
							}
						});
						let released = slider.clone();
						window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
							if phase == DispatchPhase::Bubble {
								let _ = released.update(cx, |slider, cx| {
									slider.dragging = None;
									slider.rest = None;
									slider.offset = 0.0;
									slider.hanging = None;
									slider.leaving = None;
									slider.recent.clear();
									cx.notify();
								});
							}
						});
					},
				)
				.absolute()
				.left_0()
				.top_0()
				.size_full(),
			)
			.child(div().absolute().left_0().right_0().top(px(8.0)).h(px(4.0)).rounded_full().bg(p.track))
			.children(band.map(|(lo, hi)| {
				div()
					.absolute()
					.top(px(5.0))
					.h(px(10.0))
					.left(relative(lo))
					.w(relative(hi - lo))
					.rounded_sm()
					.bg(p.accent.opacity(0.22))
			}))
			.child(
				div()
					.absolute()
					.top(px(8.0))
					.h(px(4.0))
					.left(relative(from))
					.w(relative((to - from).max(0.0)))
					.rounded_full()
					.bg(p.accent),
			)
			.children(ticks.map(|at| {
				div().absolute().top(px(7.0)).h(px(6.0)).w(px(1.0)).left(relative(at)).bg(p.muted.opacity(0.6))
			}))
			.children(self.handles.iter().enumerate().map(|(index, &at)| {
				let name = match (pair, index) {
					(false, _) => label.to_string(),
					(true, 0) => format!("{label} start"),
					(true, _) => format!("{label} end"),
				};
				div()
					.id(("handle", index))
					.role(gpui::Role::Slider)
					.aria_label(name)
					.aria_numeric_value(f64::from((at * 100.0).round()))
					.absolute()
					.top(px(3.0))
					.left(relative(at))
					.ml(px(-7.0))
					.size(px(14.0))
					.rounded_full()
					.border_1()
					.border_color(p.border)
					.bg(p.text)
			}))
	}
}

#[cfg(test)]
mod tests {
	use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};

	use super::*;

	fn one_gap(slider: &Entity<Slider>, cx: &mut VisualTestContext, (lo, hi): (f32, f32)) -> bool {
		let gaps = slider.read_with(cx, |slider, _| slider.gaps.clone());
		gaps.len() == 1 && (gaps[0].0 - lo).abs() < 1e-5 && (gaps[0].1 - hi).abs() < 1e-5
	}

	#[test]
	fn a_gap_is_the_one_holding_a_value_or_on_a_place_the_one_it_leans_to() {
		let places = [0.0, 0.1, 0.2, 0.3];
		assert_eq!(gap(&places, 0.15, 0.0), Some((0.1, 0.2)));
		assert_eq!(gap(&places, 0.2, 1.0), Some((0.2, 0.3)));
		assert_eq!(gap(&places, 0.2, -1.0), Some((0.1, 0.2)));
		assert_eq!(gap(&places, 0.3, 1.0), Some((0.2, 0.3)), "the last place has only the gap before it");
		assert_eq!(gap(&places, 0.0, -1.0), Some((0.0, 0.1)), "and the first only the one after");
		assert!((even(0, 0.437) - 0.4).abs() < 1e-6);
		assert!((even(1, 0.437) - 0.425).abs() < 1e-6);
		assert_eq!(even(EXACT, 0.437), 0.437, "exact is the pointer");
	}

	fn opened(handles: Vec<f32>, cx: &mut TestAppContext) -> (Entity<Slider>, VisualTestContext, Bounds<Pixels>) {
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |_, cx| cx.new(|_| Slider::new("Limit", handles))).unwrap()
		});
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let slider = window.root(&mut cx).unwrap();
		cx.update(|window, _| window.refresh());
		cx.run_until_parked();
		let track = slider.read_with(&cx, |slider, _| slider.track.get()).expect("the track was drawn");
		(slider, cx, track)
	}

	#[gpui::test]
	fn a_pause_then_a_move_goes_one_level_finer_from_where_the_handle_is(cx: &mut TestAppContext) {
		let (slider, mut cx, track) = opened(vec![0.5], cx);
		let y = track.center().y;
		let width = track.size.width / px(1.0);
		let at = |along: f32| point(track.left() + track.size.width * along, y);
		let handle = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.handles[0]);
		let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		assert_eq!(level(&mut cx), 0, "a value on a coarse place starts coarse");
		cx.simulate_mouse_move(at(0.61), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "on the coarsest places");
		cx.simulate_mouse_move(point(at(0.61).x + px(DEADZONE - 1.0), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "a hand this still holds the value");
		std::thread::sleep(DWELL + Duration::from_millis(50));
		cx.simulate_mouse_move(point(at(0.61).x + px(12.0), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "the pause bought one level");
		let expected = even(1, 0.6 + 12.0 / width);
		assert!((handle(&mut cx) - expected).abs() < 1e-6, "carried on from 0.6, not from the pointer");
		assert!(one_gap(&slider, &mut cx, (0.6, 0.7)), "into the gap it moved to");
		// Leaving that gap, and staying out, goes back out; a crossing that comes back does not.
		std::thread::sleep(THROW_WINDOW);
		cx.simulate_mouse_move(at(0.76), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "just outside is not yet left");
		cx.simulate_mouse_move(at(0.68), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "and a crossing that came back is still inside");
		// Out, and moving on out there, never still for a pause's time.
		for along in [0.77, 0.775, 0.78] {
			cx.simulate_mouse_move(at(along), MouseButton::Left, Modifiers::default());
			std::thread::sleep(EXIT_HOLD / 2 + Duration::from_millis(10));
		}
		cx.simulate_mouse_move(at(0.785), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "outside for long enough is the level above");
		cx.simulate_mouse_up(at(0.785), MouseButton::Left, Modifiers::default());
	}

	#[gpui::test]
	fn hanging_between_two_places_goes_finer_and_a_value_between_them_starts_there(cx: &mut TestAppContext) {
		let (slider, mut cx, track) = opened(vec![0.5], cx);
		let y = track.center().y;
		let at = |along: f32| point(track.left() + track.size.width * along, y);
		let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		cx.simulate_mouse_move(at(0.545), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0);
		std::thread::sleep(LINGER + Duration::from_millis(50));
		cx.simulate_mouse_move(at(0.551), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "between 0.5 and 0.6, away from both, for long enough");
		let value = slider.read_with(&cx, |slider, _| slider.handles[0]);
		assert!((value - 0.55).abs() < 1e-6, "on the finer places: {value}");
		cx.simulate_mouse_up(at(0.551), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		// The next press starts where this one left off: 0.55 is a place of the middle level only.
		cx.simulate_mouse_down(at(0.55), MouseButton::Left, Modifiers::default());
		assert_eq!(level(&mut cx), 1, "a value between coarse places starts at its own level");
		assert!(one_gap(&slider, &mut cx, (0.5, 0.6)), "and in the gap it holds");
		cx.simulate_mouse_up(at(0.55), MouseButton::Left, Modifiers::default());
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
		assert!((handles[1] - 0.7).abs() < 0.01 && handles[0] == 0.2, "the end handle took it: {handles:?}");
		cx.simulate_mouse_move(point(track.right() + px(40.0), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(slider.read_with(&cx, |slider, _| slider.handles[1]), 1.0, "past the end is the end");
		cx.simulate_mouse_move(at(0.05), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(slider.read_with(&cx, |slider, _| slider.handles[1]), 0.2, "and never past the other");
		cx.simulate_mouse_up(at(0.05), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!(slider.read_with(&cx, |slider, _| slider.dragging.is_none()), "a release lets go");
	}
}
