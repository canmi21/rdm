//! A track with one handle or two, pressed or dragged along its length, for a value set roughly
//! by hand beside a field that sets it exactly. GPUI ships none. See spec/ui.md.

use std::cell::Cell;
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

/// A drag moves through levels, coarse to exact. Speed only ever makes it coarser; it grows finer
/// only when the hand stops and then moves again, because slowing down is reading the number, and
/// a number that starts to jitter just as it is read is the failure this exists to avoid. See
/// spec/ui.md, "A slider reads the hand's intent from its pauses".
pub const EXACT: usize = 3;

/// At or above this pace, in points a second, a drag is a throw and goes to the coarsest level.
const THROW: f32 = 500.0;
/// At or above this, a drag finer than the middle level comes back up to it.
const STRIDE: f32 = 200.0;
/// A hand this still, in points, is holding the value: it does not move, and the time counts
/// towards a pause.
const DEADZONE: f32 = 3.0;
/// How long a hand has to hold before its next move goes one level finer.
const DWELL: Duration = Duration::from_millis(300);

/// The least time between two measurements of the pace; events closer than this are one sample.
const SAMPLE: Duration = Duration::from_millis(12);
/// How long the pace takes to follow the hand, in seconds, so one jittery event changes nothing.
const SETTLE: f32 = 0.06;

/// The coarsest level a pace allows, or none for a pace that allows any.
fn ceiling(pace: f32) -> Option<usize> {
	if pace >= THROW {
		Some(0)
	} else if pace >= STRIDE {
		Some(1)
	} else {
		None
	}
}

/// Steps of a share of the track, for a slider whose owner says nothing about what is round.
fn even(level: usize, position: f32) -> f32 {
	match [0.1, 0.025, 0.005].get(level) {
		Some(step) => ((position / step).round() * step).clamp(0.0, 1.0),
		None => position,
	}
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
	/// How fast the held handle is being moved, in points a second, smoothed over `SETTLE`.
	pace: f32,
	/// Where the pointer was when the pace was last measured, and when.
	sampled: Option<(Pixels, Instant)>,
	/// The level the drag is at, 0 the coarsest and `EXACT` the pointer itself.
	level: usize,
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
			pace: 0.0,
			sampled: None,
			level: 0,
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
			on_change(index, position, self.level, window, cx);
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
		self.pace = 0.0;
		self.sampled = Some((event.position.x, Instant::now()));
		self.rest = Some((event.position.x, Instant::now()));
		self.offset = 0.0;
		// A press lands where it is made; the drag that follows starts coarse, since a drag begins
		// by going somewhere.
		self.level = EXACT;
		self.move_handle(index, position, window, cx);
		self.level = 0;
	}

	/// The pace brought up to date with the pointer at `x`, when a sample's time has passed.
	fn measure(&mut self, x: Pixels) {
		let now = Instant::now();
		let Some((from, at)) = self.sampled else {
			self.sampled = Some((x, now));
			return;
		};
		let elapsed = now.duration_since(at);
		if elapsed < SAMPLE {
			return;
		}
		let seconds = elapsed.as_secs_f32();
		let pace = ((x - from) / px(1.0)).abs() / seconds;
		// The longer since the last sample, the more the new one counts: after a pause it is all.
		let weight = 1.0 - (-seconds / SETTLE).exp();
		self.pace += (pace - self.pace) * weight;
		self.sampled = Some((x, now));
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
		self.measure(x);
		let ceiling = ceiling(self.pace);
		if let Some(ceiling) = ceiling.filter(|c| *c < self.level) {
			// Faster than the level allows: coarser at once, and back under the pointer.
			self.level = ceiling;
			self.offset = 0.0;
		}
		let (rest, since) = self.rest.unwrap_or((x, now));
		if ((x - rest) / px(1.0)).abs() <= DEADZONE {
			// Held: the value stays what it is while the number is read.
			return;
		}
		if ceiling.is_none() && self.level < EXACT && now.duration_since(since) >= DWELL {
			// A pause, then a move: one level finer, carrying on from where the handle is.
			if let Some(from) = self.along(rest) {
				self.offset = self.handles[index] - from;
			}
			self.level += 1;
		}
		self.rest = Some((x, now));
		let Some(along) = self.along(x) else { return };
		let position = self.snapped(self.level, (along + self.offset).clamp(0.0, 1.0), cx);
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
		// The places the level a drag is at lands on, drawn on the track so what a move will reach
		// is seen before it is reached; none when two would be closer than a few points, where they
		// would be a texture rather than marks.
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		let mut ticks: Vec<f32> = Vec::new();
		if dragging && self.level < EXACT && width > 0.0 {
			for k in 0..=400 {
				let at = self.snapped(self.level, k as f32 / 400.0, cx);
				if ticks.last().is_none_or(|last| (at - last).abs() > 1e-4) {
					ticks.push(at);
				}
			}
			if ticks.windows(2).any(|w| (w[1] - w[0]) * width < 6.0) {
				ticks.clear();
			}
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
									slider.pace = 0.0;
									slider.sampled = None;
									slider.rest = None;
									slider.offset = 0.0;
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
	use gpui::{Modifiers, TestAppContext, VisualTestContext, point};

	use super::*;

	#[test]
	fn speed_only_ever_makes_a_drag_coarser() {
		assert_eq!(ceiling(0.0), None, "a slow hand keeps whatever level it is at");
		assert_eq!(ceiling(STRIDE - 1.0), None);
		assert_eq!(ceiling(STRIDE), Some(1));
		assert_eq!(ceiling(THROW), Some(0), "a throw is the coarsest");
		assert!((even(0, 0.437) - 0.4).abs() < 1e-6);
		assert!((even(1, 0.437) - 0.425).abs() < 1e-6);
		assert_eq!(even(EXACT, 0.437), 0.437, "exact is the pointer");
	}

	#[gpui::test]
	fn a_pause_then_a_move_goes_one_level_finer_from_where_the_handle_is(cx: &mut TestAppContext) {
		let window = cx.update(|cx| {
			cx.open_window(Default::default(), |_, cx| cx.new(|_| Slider::new("Limit", vec![0.5]))).unwrap()
		});
		let mut cx = VisualTestContext::from_window(window.into(), cx);
		let slider = window.root(&mut cx).unwrap();
		cx.update(|window, _| window.refresh());
		cx.run_until_parked();
		let track = slider.read_with(&cx, |slider, _| slider.track.get()).expect("the track was drawn");
		let y = track.center().y;
		let width = track.size.width / px(1.0);
		let at = |along: f32| point(track.left() + track.size.width * along, y);
		let handle = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.handles[0]);
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		cx.simulate_mouse_move(at(0.63), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "a drag starts on the coarsest steps");
		// Events arrive every few milliseconds while a hand is on the track; one after a sample's
		// time is what brings the pace's anchor up to the hand before it stops.
		std::thread::sleep(SAMPLE + Duration::from_millis(3));
		cx.simulate_mouse_move(point(at(0.63).x + px(DEADZONE - 1.0), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "a hand this still holds the value");
		std::thread::sleep(DWELL + Duration::from_millis(50));
		cx.simulate_mouse_move(point(at(0.63).x + px(12.0), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		let finer = handle(&mut cx);
		assert_eq!(slider.read_with(&cx, |slider, _| slider.level), 1, "the pause bought one level");
		let expected = even(1, 0.6 + (12.0 - (DEADZONE - 1.0)) / width);
		assert!((finer - expected).abs() < 1e-6, "and it carried on from 0.6, not from the pointer: {finer}");
		cx.simulate_mouse_up(at(0.65), MouseButton::Left, Modifiers::default());
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
