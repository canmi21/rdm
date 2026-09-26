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
use crate::ui::icon::{Icon, icon};
use crate::ui::theme;

#[cfg(test)]
mod tests;
mod view;

/// What a move of a handle does, decided by whoever owns the slider: which handle, where along
/// the track it now is, from 0 at the left to 1 at the right, and the level the drag is at, so the
/// owner can round its own value to the same step.
type OnChange = Box<dyn Fn(usize, f32, usize, &mut Window, &mut App)>;

/// Where a position lands at a level, in the owner's terms: a limit on round rates, a part of a
/// file on round sizes. The slider knows only positions; what is round is the owner's to say.
type Snap = Box<dyn Fn(usize, f32, &App) -> f32>;

/// A drag moves through levels, coarse to exact, and going finer is zooming into one gap of the
/// level above: the track is redrawn as that gap, and its two ends go back out. See spec/ui.md,
/// "A slider reads the hand's intent from its pauses".
pub const EXACT: usize = 3;

/// A hand this still, in points, is holding the value: it does not move, and the time counts
/// towards a pause.
const DEADZONE: f32 = 3.0;
/// A drag goes a level finer on one of two things and nothing else; a rest is not one, since a hand
/// rests to read the number. See spec/ui.md, "A slider reads the hand's intent from its pauses".
///
/// How long either of the two has to be held to go in: a hang, the hand between two places and away
/// from both; or a swing, from a place to its neighbour and back, then the hand inside their gap.
/// Long, and the same for both, because a hand between two places may only be unsure, and it is
/// the length of the stay that says it wants what is between. The swing had a short hold of its own
/// at first, and went in before the hand had decided anything.
const LINGER: Duration = Duration::from_millis(1000);
/// How far from a place, as a share of its gap, a hand is hanging between two rather than on one.
const AWAY: f32 = 0.25;
/// While zoomed, the gap fills the track but for this share at each end, where the value stops.
/// Going into one and back out of it to the middle goes back out a level: the end is where the hand
/// says it means to, and coming back is the doing of it, so one visit is one level however long it
/// lasts, and a hand in an end has not left yet.
const MARGIN: f32 = 0.05;
/// How long a hand has to be in an end before coming back from it goes out a level: long enough that
/// fine work brushing an end and coming straight back does not go out by accident.
const EDGE_HOLD: Duration = Duration::from_millis(200);
/// The handle: a white pill lying along the track, flatter than the round dot it replaced and
/// wider, so it reads as something to slide and gives the hand more to take hold of. A press within
/// half its width takes it where it is.
const KNOB_W: f32 = 18.0;
const KNOB_H: f32 = 10.0;
/// As a hold nears what it will do, a glyph this size fades in inside the held handle, saying which;
/// the handle itself keeps its size. See spec/ui.md, "A slider reads the hand's intent from its
/// pauses".
const GLYPH: f32 = 8.0;
/// How finely each level's places are found, by asking the snap at this many points along the track.
const PROBES: usize = 2000;
/// Two positions closer than this are one place.
const SAME: f32 = 1e-5;

/// How many steps a level cuts the whole track into: ten, and ten in each of those at every level
/// finer, so whatever the track shows -- the whole of it, or a gap zoomed to fill it -- is ten
/// steps. None at exact. See spec/ui.md, "A slider reads the hand's intent from its pauses".
pub fn steps(level: usize) -> Option<u32> {
	(level < EXACT).then(|| 10u32.pow(level as u32 + 1))
}

/// A position on the nearest of its level's steps, for a slider whose owner says nothing more.
fn even(level: usize, position: f32) -> f32 {
	match steps(level) {
		Some(n) => ((position * n as f32).round() / n as f32).clamp(0.0, 1.0),
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

/// Where a value is drawn when the track shows `zoom`: the gap across the middle, its edges at the
/// margins. The whole track is its scale end to end, with no ends: a drag comes back out to it at
/// the pointer, so the far end gone out by is the scale's end, and there is nothing to hold a place
/// for.
fn view(zoom: Option<(f32, f32)>, value: f32) -> f32 {
	match zoom {
		Some((lo, hi)) if hi > lo => MARGIN + (1.0 - 2.0 * MARGIN) * (value - lo) / (hi - lo),
		_ => value,
	}
}

/// The value a place on the track stands for when it shows `zoom`, held to the gap and the track.
fn unview(zoom: Option<(f32, f32)>, at: f32) -> f32 {
	match zoom {
		Some((lo, hi)) if hi > lo => {
			(lo + (at - MARGIN) / (1.0 - 2.0 * MARGIN) * (hi - lo)).clamp(lo.max(0.0), hi.min(1.0))
		}
		_ => at.clamp(0.0, 1.0),
	}
}

/// A zoom a gap wide, placed so `value` is drawn at `at`: the handle stays under the pointer as the
/// track is redrawn, instead of the track moving under a pointer that stays. `at` may be in an end:
/// going back out leaves the pointer there, and a zoom placed as if it were not drew the handle
/// short of it, which read as the handle being pushed back.
/// Going in, `at` is kept off the ends instead, which would put the value in one and have it go back
/// out as soon as it went in.
fn around(width: f32, value: f32, at: f32) -> (f32, f32) {
	let lo = value - (at.clamp(0.0, 1.0) - MARGIN) / (1.0 - 2.0 * MARGIN) * width;
	(lo, lo + width)
}

/// A place on the track kept a little off both ends.
fn inside(at: f32) -> f32 {
	at.clamp(MARGIN + 0.02, 1.0 - MARGIN - 0.02)
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
	/// What the drag zoomed into, one a level: its length is the level, 0 the coarsest and `EXACT`
	/// the pointer itself, and the last is what the track shows. Each is a gap of the level above
	/// wide, placed around the value where the pointer was when it was entered.
	gaps: Vec<(f32, f32)>,
	/// Since when the hand has hung between two places, away from both.
	hanging: Option<Instant>,
	/// The last places of this level the handle landed on, newest last, for a swing to be seen in.
	visits: Vec<f32>,
	/// The gap a swing between its two ends marked, and since when the hand has been inside it.
	swung: Option<((f32, f32), Option<Instant>)>,
	/// Since when the hand has been in an end of a zoomed track.
	edge: Option<Instant>,
	/// Where the pointer was when the drag last went a level finer: until it has moved from there,
	/// nothing goes finer again, so a hand that stays after going in stays at that level.
	settled: Option<Pixels>,
	/// Where the hand came to rest and since when; a move within `DEADZONE` of it is not a move.
	rest: Option<(Pixels, Instant)>,
	/// Where the pointer was last, along the track, for a zoom that changes while it is still.
	pointer: f32,
	/// How far the handle was from the pointer when it was taken hold of, as a share of the track,
	/// so a press a little off its centre does not move it. Nothing else moves it apart from the
	/// pointer: a zoom is placed around the pointer instead.
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
			visits: Vec::new(),
			swung: None,
			edge: None,
			settled: None,
			rest: None,
			pointer: 0.0,
			offset: 0.0,
		}
	}

	pub fn on_change(
		mut self,
		f: impl Fn(usize, f32, usize, &mut Window, &mut App) + 'static,
	) -> Self {
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

	/// The gap the track shows while a drag is in one.
	fn zoom(&self) -> Option<(f32, f32)> {
		self.dragging.and(self.gaps.last().copied())
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

	/// The coarsest level a value is a place of, and a zoom for every coarser one around it, drawn at
	/// `at`: a value left on a round rate starts the next drag at that rate's level, and one left
	/// between every level's places starts it exact.
	fn start_at(&mut self, value: f32, at: f32, cx: &App) {
		self.gaps.clear();
		for level in 0..EXACT {
			if (self.snapped(level, value, cx) - value).abs() <= SAME {
				return;
			}
			match gap(&self.places[level], value, 1.0) {
				Some((lo, hi)) => self.gaps.push(around(hi - lo, value, inside(at))),
				None => return,
			}
		}
	}

	/// How wide a zoom into this level's gap around `value` is.
	fn gap_width(&self, level: usize, value: f32) -> Option<f32> {
		let (lo, hi) = self.places.get(level).and_then(|places| gap(places, value, 0.0))?;
		Some(hi - lo)
	}

	/// One level finer: the track redrawn as a gap of this level, around `value` at `at`. The value is
	/// the handle's, unchanged: going in changes where it is drawn, never what it is.
	fn zoom_in(&mut self, value: f32, at: f32) {
		if let Some(width) = self.gap_width(self.level(), value) {
			self.gaps.push(around(width, value, inside(at)));
		}
		self.forget();
	}

	/// One level coarser. The zoom now shown is placed around the handle's value at the pointer, as it
	/// was when it was entered, so the handle stays under the hand with its value as it was. The whole
	/// track has no zoom to place -- a place on it is its value -- so there the handle lands under the
	/// pointer and the value is what that place is: where the handle is matters more than what it
	/// held, and the whole track is the full scale anyway. Keeping the value there instead drew the
	/// handle away from the hand, and the hand found it somewhere it had not put it.
	fn zoom_out(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
		self.gaps.pop();
		let value = self.handles[index];
		let at = self.pointer + self.offset;
		self.forget();
		self.rest = self.rest.map(|(x, _)| (x, Instant::now()));
		match self.gaps.pop() {
			Some((lo, hi)) => self.gaps.push(around(hi - lo, value, at)),
			None => {
				self.offset = 0.0;
				let landed = self.snapped(0, unview(None, self.pointer), cx);
				self.move_handle(index, landed, window, cx);
			}
		}
	}

	/// What was being watched at the level just left: a hang, a swing, an end.
	fn forget(&mut self) {
		self.hanging = None;
		self.visits.clear();
		self.swung = None;
		self.edge = None;
	}

	/// The handle landed on `place`: kept, and a swing from a place to its neighbour and back marks
	/// the gap between them. Landing anywhere else unmarks it.
	fn visit(&mut self, place: f32) {
		if self.visits.last().is_some_and(|last| (last - place).abs() <= SAME) {
			return;
		}
		self.visits.push(place);
		if self.visits.len() > 3 {
			self.visits.remove(0);
		}
		if let &[a, b, again] = self.visits.as_slice()
			&& (a - again).abs() <= SAME
			&& let Some(places) = self.places.get(self.level())
			&& let Some((lo, hi)) = gap(places, a, (b - a).signum())
			&& (lo.min(hi) - a.min(b)).abs() <= SAME
			&& (hi.max(lo) - a.max(b)).abs() <= SAME
		{
			self.swung = Some(((lo, hi), None));
		} else if self
			.swung
			.is_some_and(|((lo, hi), _)| (place - lo).abs() > SAME && (place - hi).abs() > SAME)
		{
			self.swung = None;
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
	fn move_handle(
		&mut self,
		index: usize,
		position: f32,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
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
		let distance = |value: f32| (view(None, value) - position).abs();
		let nearest = self.handles.iter().copied().map(distance).fold(f32::INFINITY, f32::min);
		let closest: Vec<usize> =
			(0..self.handles.len()).filter(|&i| distance(self.handles[i]) <= nearest).collect();
		let index = match closest.as_slice() {
			[.., last] if closest.len() > 1 && position > view(None, self.handles[closest[0]]) => *last,
			[first, ..] => *first,
			[] => return,
		};
		self.dragging = Some(index);
		self.rest = Some((event.position.x, Instant::now()));
		self.forget();
		self.settled = None;
		self.pointer = position;
		self.places = (0..EXACT).map(|level| self.find_places(level, cx)).collect();
		// A press on the handle takes hold of it where it is, at the value's own level, so a drag
		// picks up where the last one left off; one elsewhere on the track brings it there, on a
		// coarse place.
		let value = self.handles[index];
		let drawn = view(None, value);
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		if width > 0.0 && (drawn - position).abs() * width <= KNOB_W / 2.0 {
			self.offset = drawn - position;
			self.start_at(value, drawn, cx);
			cx.notify();
		} else {
			self.gaps.clear();
			self.offset = 0.0;
			let landed = self.snapped(0, unview(None, position), cx);
			self.move_handle(index, landed, window, cx);
		}
	}

	fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(index) = self.dragging else { return };
		if event.pressed_button != Some(MouseButton::Left) {
			self.release(cx);
			return;
		}
		let x = event.position.x;
		let now = Instant::now();
		let Some(along) = self.along(x) else { return };
		self.pointer = along;
		let at = along + self.offset;

		// An end of a zoomed track stops the value and says the hand means to go back out; coming back
		// from it to the middle is going, one level, with the value where it was.
		if self.level() > 0 && !(MARGIN..=1.0 - MARGIN).contains(&at) {
			self.edge.get_or_insert(now);
			self.rest = Some((x, now));
			cx.notify();
			return;
		}
		if let Some(since) = self.edge.take()
			&& now.duration_since(since) >= EDGE_HOLD
		{
			self.zoom_out(index, window, cx);
			cx.notify();
			return;
		}

		// One level finer on a swing or a hang, and on nothing else.
		let wanted = unview(self.zoom(), at);
		let level = self.level();
		let mut finer = false;
		// Gone in and not moved since: nothing goes in again until the hand does.
		if self.settled.is_some_and(|from| ((x - from) / px(1.0)).abs() > DEADZONE) {
			self.settled = None;
		}
		if level < EXACT && self.edge.is_none() && self.settled.is_none() {
			// Swung between two neighbouring places: a hold inside their gap goes into it.
			if let Some(((lo, hi), since)) = self.swung {
				let inside = wanted > lo.min(hi) + SAME && wanted < lo.max(hi) - SAME;
				match (inside, since) {
					(false, _) => self.swung = Some(((lo, hi), None)),
					(true, None) => self.swung = Some(((lo, hi), Some(now))),
					(true, Some(since)) if now.duration_since(since) >= LINGER => {
						self.zoom_in(self.handles[index], at);
						self.settled = Some(x);
						finer = true;
					}
					(true, Some(_)) => {}
				}
			}
			// Hung between two places, away from both, for long enough.
			if !finer {
				let away = self
					.places
					.get(level)
					.and_then(|p| gap(p, wanted, 0.0))
					.is_some_and(|(lo, hi)| (wanted - lo).min(hi - wanted) > (hi - lo) * AWAY);
				match (away, self.hanging) {
					(false, _) => self.hanging = None,
					(true, None) => self.hanging = Some(now),
					(true, Some(since)) if now.duration_since(since) >= LINGER => {
						self.zoom_in(self.handles[index], at);
						self.settled = Some(x);
						finer = true;
					}
					(true, Some(_)) => {}
				}
			}
		}

		if finer {
			// Gone in: redrawn, and the value as it was until the hand moves it.
			self.rest = Some((x, now));
			cx.notify();
			return;
		}
		let (rest, _) = self.rest.unwrap_or((x, now));
		if ((x - rest) / px(1.0)).abs() <= DEADZONE {
			// Held: the value stays what it is while the number is read.
			return;
		}
		self.rest = Some((x, now));
		let value = unview(self.zoom(), at);
		let position = self.snapped(self.level(), value, cx);
		if self.level() < EXACT {
			self.visit(position);
		}
		self.move_handle(index, position, window, cx);
	}

	fn release(&mut self, cx: &mut Context<Self>) {
		self.dragging = None;
		self.gaps.clear();
		self.rest = None;
		self.offset = 0.0;
		self.forget();
		self.settled = None;
		cx.notify();
	}
}
