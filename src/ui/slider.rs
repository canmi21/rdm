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
/// While zoomed, the gap fills the track but for this share at each end, where the value stops and
/// going in goes back out a level -- one level for each time the hand goes in, however long it
/// stays; the next level out is back to the middle and in again.
const MARGIN: f32 = 0.05;
/// How long a hand has to be in an end to go back out: short, since going in is the ask, and long
/// enough that fine work brushing an end does not go out by accident.
const EDGE_HOLD: Duration = Duration::from_millis(200);
/// The handle: a white pill lying along the track, flatter than the round dot it replaced and
/// wider, so it reads as something to slide and gives the hand more to take hold of. A press within
/// half its width takes it where it is.
const KNOB_W: f32 = 18.0;
const KNOB_H: f32 = 10.0;
/// As a dive nears the held handle is pressed flat, to this share of its height and this much wider,
/// and on going in springs back past its shape and settles, over `SPRING`. See spec/ui.md, "A slider
/// reads the hand's intent from its pauses".
const FLAT_H: f32 = 0.55;
const FLAT_W: f32 = 1.3;
const SPRING: Duration = Duration::from_millis(320);
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
/// margins.
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
/// track is redrawn, instead of the track moving under a pointer that stays. `at` is kept off the
/// ends, which would otherwise put the value in one.
fn around(width: f32, value: f32, at: f32) -> (f32, f32) {
	let at = at.clamp(MARGIN + 0.02, 1.0 - MARGIN - 0.02);
	let lo = value - (at - MARGIN) / (1.0 - 2.0 * MARGIN) * width;
	(lo, lo + width)
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
	/// Which end that is: the right one, or the left.
	edge_right: bool,
	/// Whether going into an end goes out a level: not again after it has, until the hand has been
	/// back in the middle, so one visit to an end is one level however long it lasts.
	armed: bool,
	/// Where the pointer was when the drag last went a level finer: until it has moved from there,
	/// nothing goes finer again, so a hand that stays after going in stays at that level.
	settled: Option<Pixels>,
	/// When the drag last went a level finer, for the handle's spring back.
	dived: Option<Instant>,
	/// Where the hand came to rest and since when; a move within `DEADZONE` of it is not a move.
	rest: Option<(Pixels, Instant)>,
	/// Where the pointer was last, along the track, for a zoom that changes while it is still.
	pointer: f32,
	/// Back out on the whole track, the handle waits where it is until the pointer comes to it, on
	/// this side of it: the pointer is in an end and the value is not, and the whole track has no
	/// zoom to place around the pointer. See spec/ui.md, "A slider reads the hand's intent from its
	/// pauses".
	pickup: Option<f32>,
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
			edge_right: false,
			armed: true,
			settled: None,
			dived: None,
			rest: None,
			pointer: 0.0,
			pickup: None,
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
				Some((lo, hi)) => self.gaps.push(around(hi - lo, value, at)),
				None => return,
			}
		}
	}

	/// How wide a zoom into this level's gap around `value` is.
	fn gap_width(&self, level: usize, value: f32) -> Option<f32> {
		let (lo, hi) = self.places.get(level).and_then(|places| gap(places, value, 0.0))?;
		Some(hi - lo)
	}

	/// One level finer: the track redrawn as a gap of this level, around `value` at `at`.
	fn zoom_in(&mut self, value: f32, at: f32) {
		if let Some(width) = self.gap_width(self.level(), value) {
			self.gaps.push(around(width, value, at));
		}
		self.forget();
		self.dived = Some(Instant::now());
	}

	/// One level coarser, the handle's value where it was, and the zoom that is now shown placed
	/// around it at the pointer, as it was when it was entered.
	fn zoom_out(&mut self, index: usize) {
		self.gaps.pop();
		let value = self.handles[index];
		let at = self.pointer + self.offset;
		match self.gaps.pop() {
			Some((lo, hi)) => self.gaps.push(around(hi - lo, value, at)),
			None => self.pickup = Some((at - value).signum()),
		}
		self.forget();
		self.armed = false;
		self.rest = self.rest.map(|(x, _)| (x, Instant::now()));
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

	/// The hand went into an end: if it is still there when the hold is up, the track goes back out,
	/// whether or not it moved in the meantime.
	fn hold_edge(&mut self, since: Instant, cx: &mut Context<Self>) {
		cx.spawn(async move |this, cx| {
			cx.background_executor().timer(EDGE_HOLD).await;
			let _ = this.update(cx, |this, cx| {
				if let (Some(index), Some(edge)) = (this.dragging, this.edge)
					&& edge == since
					&& this.armed
				{
					this.zoom_out(index);
					cx.notify();
				}
			});
		})
		.detach();
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
		self.rest = Some((event.position.x, Instant::now()));
		self.forget();
		self.armed = true;
		self.settled = None;
		self.pickup = None;
		self.pointer = position;
		self.places = (0..EXACT).map(|level| self.find_places(level, cx)).collect();
		// A press on the handle takes hold of it where it is, at the value's own level, so a drag
		// picks up where the last one left off; one elsewhere on the track brings it there, on a
		// coarse place.
		let value = self.handles[index];
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		if width > 0.0 && (value - position).abs() * width <= KNOB_W / 2.0 {
			self.offset = value - position;
			self.start_at(value, value, cx);
			cx.notify();
		} else {
			self.gaps.clear();
			self.offset = 0.0;
			let landed = self.snapped(0, position, cx);
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

		// An end of a zoomed track stops the value, and going in goes back out a level, once.
		if self.level() > 0 && !(MARGIN..=1.0 - MARGIN).contains(&at) {
			self.edge_right = at > 1.0 - MARGIN;
			if self.armed {
				match self.edge {
					None => {
						self.edge = Some(now);
						self.hold_edge(now, cx);
					}
					Some(since) if now.duration_since(since) >= EDGE_HOLD => self.zoom_out(index),
					Some(_) => {}
				}
			}
			self.rest = Some((x, now));
			cx.notify();
			return;
		}
		self.edge = None;
		self.armed = true;

		// Back on the whole track, the handle waits for the pointer to reach it.
		if let Some(side) = self.pickup {
			let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(1.0);
			let value = self.handles[index];
			if (at - value).signum() == side && (at - value).abs() * width > KNOB_W / 2.0 {
				self.rest = Some((x, now));
				return;
			}
			self.pickup = None;
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
						self.zoom_in(wanted, at);
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
						self.zoom_in(wanted, at);
						self.settled = Some(x);
						finer = true;
					}
					(true, Some(_)) => {}
				}
			}
		}

		let (rest, _) = self.rest.unwrap_or((x, now));
		if !finer && ((x - rest) / px(1.0)).abs() <= DEADZONE {
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
		self.pickup = None;
		self.settled = None;
		cx.notify();
	}
}

impl Render for Slider {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let track = self.track.clone();
		let slider = cx.weak_entity();
		let dragging = self.dragging.is_some();
		// While zoomed, every value is drawn where the zoomed track puts it.
		let zoom = self.zoom();
		let shown = |value: f32| view(zoom, value).clamp(0.0, 1.0);
		// The filled part: between the handles of a pair, or from the left end to a lone handle.
		// On a zoomed track it stays within the band: the ends stand for no value, and a fill running
		// into them covered the way out they are marked with.
		let (from, to) = match self.handles.as_slice() {
			[] => (0.0, 0.0),
			[one] => (0.0, shown(*one)),
			[first, .., last] => (shown(*first), shown(*last)),
		};
		let (from, to) = match zoom {
			Some(_) => (from.clamp(MARGIN, 1.0 - MARGIN), to.clamp(MARGIN, 1.0 - MARGIN)),
			None => (from, to),
		};
		let label = self.label.clone();
		let pair = self.handles.len() > 1;
		// The gap a drag went into fills the track as a band, with its level's places across it; no
		// places when two would be closer than a few points, where they would be a texture.
		let width = self.track.get().map(|t| t.size.width / px(1.0)).unwrap_or(0.0);
		let (lo, hi) = zoom.unwrap_or((0.0, 1.0));
		let mut ticks: Vec<f32> = match self.places.get(self.level()) {
			Some(places) if dragging => places
				.iter()
				.copied()
				.filter(|p| *p >= lo - SAME && *p <= hi + SAME && (0.0..=1.0).contains(p))
				.map(|p| view(zoom, p))
				.collect(),
			_ => Vec::new(),
		};
		if ticks.windows(2).any(|w| (w[1] - w[0]) * width < 6.0) {
			ticks.clear();
		}
		let ticks = ticks.into_iter();

		// Hints for where the hand can go, each filling as its condition is met and full at the moment
		// it would act. See spec/ui.md, "A slider reads the hand's intent from its pauses".
		let now = Instant::now();
		let level = self.level();
		let share = |since: Instant, of: Duration| {
			(now.duration_since(since).as_secs_f32() / of.as_secs_f32()).min(1.0)
		};
		// Deeper: the held handle pressed flat as a hang goes on, or as the hold after a swing does.
		// A hang shows from a fifth of the way, so passing between two places does not flicker it.
		let swinging = self.swung.and_then(|(_, since)| since);
		let deeper = if dragging && level < EXACT && self.edge.is_none() {
			let hung = self.hanging.map_or(0.0, |since| ((share(since, LINGER) - 0.2) / 0.8).max(0.0));
			hung.max(swinging.map_or(0.0, |since| share(since, LINGER)))
		} else {
			0.0
		};
		// Back out: the two ends of a zoomed track, each marked as a way out, the one the hand is in
		// filling as it stays.
		let leaving = self.edge.map_or(0.0, |since| share(since, EDGE_HOLD));
		// The spring after a dive: a decaying swing about the handle's own shape, first taller and
		// narrower than it, then back through it and still.
		let spring = self.dived.map_or(0.0, |since| {
			let t = now.duration_since(since).as_secs_f32();
			if t < SPRING.as_secs_f32() {
				0.35 * (-t / 0.07).exp() * (std::f32::consts::TAU * t / 0.16).cos()
			} else {
				0.0
			}
		});
		let springing = spring != 0.0;
		if dragging
			&& (self.hanging.is_some() || swinging.is_some() || self.edge.is_some() || springing)
		{
			window.request_animation_frame();
		}
		let held = self.dragging;
		let end = |right: bool| {
			let fill =
				if self.armed && self.edge.is_some() && self.edge_right == right { leaving } else { 0.0 };
			div()
				.absolute()
				.top(px(4.0))
				.h(px(12.0))
				.w(relative(MARGIN))
				.when(right, |s| s.right_0())
				.when(!right, |s| s.left_0())
				.rounded_sm()
				.bg(p.accent.opacity(0.08 + 0.5 * fill))
				.flex()
				.items_center()
				// The chevron on the outer side, clear of a handle stopped at the band's edge.
				.when(right, |s| s.justify_end().pr(px(2.0)))
				.when(!right, |s| s.justify_start().pl(px(2.0)))
				.text_size(px(11.0))
				.line_height(px(12.0))
				.text_color(if fill > 0.0 { p.text } else { p.muted })
				.child(if right { "\u{203A}" } else { "\u{2039}" })
		};

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
			.leaves_focus()
			.child(
				// The track, half a handle in from each side, so a handle at either end sits inside the
				// slider's width with its outer edge on the column's, rather than half past it.
				div()
					.absolute()
					.top_0()
					.bottom_0()
					.left(px(KNOB_W / 2.0))
					.right(px(KNOB_W / 2.0))
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
										let _ = released.update(cx, |slider, cx| slider.release(cx));
									}
								});
							},
						)
						.absolute()
						.left_0()
						.top_0()
						.size_full(),
					)
					.child(
						div().absolute().left_0().right_0().top(px(8.0)).h(px(4.0)).rounded_full().bg(p.track),
					)
					.children(zoom.map(|_| {
						div()
							.absolute()
							.top(px(5.0))
							.h(px(10.0))
							.left(relative(MARGIN))
							.w(relative(1.0 - 2.0 * MARGIN))
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
						div()
							.absolute()
							.top(px(7.0))
							.h(px(6.0))
							.w(px(1.0))
							.left(relative(at))
							.bg(p.muted.opacity(0.6))
					}))
					// The ways out, over the fill and the places so nothing covers them.
					.when(zoom.is_some(), |s| s.child(end(false)).child(end(true)))
					.children(self.handles.iter().enumerate().map(|(index, &at)| {
						let name = match (pair, index) {
							(false, _) => label.to_string(),
							(true, 0) => format!("{label} start"),
							(true, _) => format!("{label} end"),
						};
						// The held handle is pressed flat as a dive nears, flattest at the moment it goes in,
						// then springs back.
						let (flat, bounce) = if held == Some(index) { (deeper, spring) } else { (0.0, 0.0) };
						let w = KNOB_W * (1.0 + (FLAT_W - 1.0) * flat - 0.5 * bounce);
						let h = KNOB_H * (1.0 - (1.0 - FLAT_H) * flat + bounce);
						div()
							.id(("handle", index))
							.role(gpui::Role::Slider)
							.aria_label(name)
							.aria_numeric_value(f64::from((at * 100.0).round()))
							.absolute()
							.top(px(10.0 - h / 2.0))
							.left(relative(shown(at)))
							.ml(px(-w / 2.0))
							.w(px(w))
							.h(px(h))
							.rounded_full()
							.border_1()
							.border_color(p.border)
							.bg(p.text)
							.shadow_sm()
					})),
			)
			.child(
				// What takes a press: the whole slider, which is the track and half a handle past each
				// end, since a handle at an end is centred on it. Pressed at its centre, a handle at the
				// far right used to take nothing at all.
				div()
					.id("press")
					.absolute()
					.top_0()
					.bottom_0()
					.left_0()
					.right_0()
					.cursor_pointer()
					.on_mouse_down(MouseButton::Left, cx.listener(Self::press)),
			)
	}
}

#[cfg(test)]
mod tests {
	use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};

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
		assert_eq!(
			gap(&places, 0.3, 1.0),
			Some((0.2, 0.3)),
			"the last place has only the gap before it"
		);
		assert_eq!(gap(&places, 0.0, -1.0), Some((0.0, 0.1)), "and the first only the one after");
		let zoom = Some((0.4, 0.5));
		assert!((view(zoom, 0.4) - MARGIN).abs() < 1e-6, "the gap's start is at the left margin");
		assert!((view(zoom, 0.5) - (1.0 - MARGIN)).abs() < 1e-6, "its end at the right one");
		assert!(
			(unview(zoom, 0.5) - 0.45).abs() < 1e-6,
			"the middle of the track is the middle of the gap"
		);
		assert_eq!(unview(zoom, 0.01), 0.4, "an end holds the value at the gap's edge");
		assert_eq!(unview(None, 0.3), 0.3, "an unzoomed track is itself");
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
	fn a_zoom_is_placed_around_the_pointer_and_an_end_held_zooms_back_out(cx: &mut TestAppContext) {
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
		cx.simulate_mouse_move(
			point(at(0.61).x + px(DEADZONE - 1.0), y),
			MouseButton::Left,
			Modifiers::default(),
		);
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.6).abs() < 1e-6, "a hand this still holds the value");
		// A rest and a move is reading the number, not asking for a finer one.
		std::thread::sleep(Duration::from_millis(400));
		cx.simulate_mouse_move(at(0.62), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "a pause goes nowhere");
		// Hanging between 0.6 and 0.7, away from both, for long enough goes in.
		let entered = 0.645;
		cx.simulate_mouse_move(at(0.642), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		std::thread::sleep(LINGER + Duration::from_millis(50));
		cx.simulate_mouse_move(at(entered), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "the hang bought one level");
		assert!(
			zoomed(&slider, &mut cx, 0.1, entered, entered),
			"a gap wide, with the value under the pointer"
		);
		let entered_value = handle(&mut cx);
		// On the zoomed track the handle follows the pointer, a tenth across the band's width.
		cx.simulate_mouse_move(at(entered + 36.0 / width), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		let expected = even(1, entered + 36.0 / width / (1.0 - 2.0 * MARGIN) * 0.1);
		assert!(
			(entered_value - even(1, entered)).abs() < 1e-6,
			"on the finer places where the hand was"
		);
		assert!((handle(&mut cx) - expected).abs() < 1e-6, "zoomed: {}", handle(&mut cx));
		// Into the left end: the value stops, and held there, still, the track goes back out.
		let before = handle(&mut cx);
		cx.simulate_mouse_move(at(0.02), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(handle(&mut cx), before, "an end stops the value");
		assert_eq!(level(&mut cx), 1, "not yet out");
		cx.executor().advance_clock(EDGE_HOLD + Duration::from_millis(10));
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "held in the end, still, goes back out");
		// Back on the whole track the handle waits for the pointer to come to it, then follows.
		cx.simulate_mouse_move(at(0.3), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(handle(&mut cx), before, "short of the handle, nothing moves");
		cx.simulate_mouse_move(at(0.81), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!((handle(&mut cx) - 0.8).abs() < 1e-6, "past it, the handle is the pointer's again");
		cx.simulate_mouse_up(at(0.81), MouseButton::Left, Modifiers::default());
	}

	#[gpui::test]
	fn hanging_between_two_places_zooms_in_and_a_value_between_them_starts_there(
		cx: &mut TestAppContext,
	) {
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
		assert!((value - 0.55).abs() < 1e-6, "on the finer places, where the hand was: {value}");
		assert!(zoomed(&slider, &mut cx, 0.1, 0.551, 0.551), "around the pointer");
		cx.simulate_mouse_up(at(0.551), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "a release leaves the track unzoomed");
		// The next press starts where this one left off: 0.55 is a place of the middle level only.
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
		let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		// 0.5, over to 0.6, back to 0.5: the number is between them.
		for along in [0.52, 0.58, 0.52] {
			cx.simulate_mouse_move(at(along), MouseButton::Left, Modifiers::default());
			cx.run_until_parked();
		}
		assert_eq!(level(&mut cx), 0, "the swing alone marks the gap");
		// Inside it, briefly: in.
		cx.simulate_mouse_move(at(0.53), MouseButton::Left, Modifiers::default());
		std::thread::sleep(LINGER + Duration::from_millis(20));
		cx.simulate_mouse_move(at(0.535), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "a hold inside the swung gap goes into it");
		cx.simulate_mouse_up(at(0.535), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		// A swing to somewhere else is not one: 0.5, 0.6, 0.7 marks nothing.
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		for along in [0.58, 0.68, 0.72] {
			cx.simulate_mouse_move(at(along), MouseButton::Left, Modifiers::default());
			cx.run_until_parked();
		}
		std::thread::sleep(LINGER + Duration::from_millis(20));
		cx.simulate_mouse_move(at(0.725), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "passing through places is not a swing");
		cx.simulate_mouse_up(at(0.725), MouseButton::Left, Modifiers::default());
	}

	#[gpui::test]
	fn a_hand_that_stays_after_going_in_stays_at_that_level(cx: &mut TestAppContext) {
		let (slider, mut cx, track) = opened(vec![0.5], cx);
		let y = track.center().y;
		let at = |along: f32| point(track.left() + track.size.width * along, y);
		let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		cx.simulate_mouse_move(at(0.642), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		std::thread::sleep(LINGER + Duration::from_millis(30));
		cx.simulate_mouse_move(at(0.645), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1);
		// Still, but for the jitter of a still hand, for longer than a hang: not another level.
		for _ in 0..2 {
			std::thread::sleep(LINGER + Duration::from_millis(30));
			cx.simulate_mouse_move(
				point(at(0.645).x + px(DEADZONE - 1.0), y),
				MouseButton::Left,
				Modifiers::default(),
			);
			cx.run_until_parked();
		}
		assert_eq!(level(&mut cx), 1, "a hand that stays after going in stays at that level");
		// Moved, and hung again: the next level.
		cx.simulate_mouse_move(at(0.66), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		std::thread::sleep(LINGER + Duration::from_millis(30));
		cx.simulate_mouse_move(at(0.6602), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 2, "once it has moved, a hang goes in again");
		cx.simulate_mouse_up(at(0.6602), MouseButton::Left, Modifiers::default());
	}

	#[gpui::test]
	fn one_visit_to_an_end_is_one_level_out_however_long_it_lasts(cx: &mut TestAppContext) {
		let (slider, mut cx, track) = opened(vec![0.5], cx);
		let y = track.center().y;
		let at = |along: f32| point(track.left() + track.size.width * along, y);
		let level = |cx: &mut VisualTestContext| slider.read_with(cx, |slider, _| slider.level());
		let hang = |cx: &mut VisualTestContext, from: f32, to: f32| {
			cx.simulate_mouse_move(at(from), MouseButton::Left, Modifiers::default());
			cx.run_until_parked();
			std::thread::sleep(LINGER + Duration::from_millis(30));
			cx.simulate_mouse_move(at(to), MouseButton::Left, Modifiers::default());
			cx.run_until_parked();
		};
		cx.simulate_mouse_down(at(0.5), MouseButton::Left, Modifiers::default());
		hang(&mut cx, 0.642, 0.645);
		hang(&mut cx, 0.66, 0.6602);
		assert_eq!(level(&mut cx), 2, "two hangs, two levels in");
		// Into the left end: one level out.
		cx.simulate_mouse_move(at(0.02), MouseButton::Left, Modifiers::default());
		cx.executor().advance_clock(EDGE_HOLD + Duration::from_millis(10));
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "one level out");
		// Staying, still or moving, is not another.
		cx.executor().advance_clock(Duration::from_secs(2));
		cx.simulate_mouse_move(at(0.021), MouseButton::Left, Modifiers::default());
		cx.executor().advance_clock(EDGE_HOLD * 3);
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 1, "however long the hand stays in the end");
		// Back to the middle, and into an end again: the next level out.
		cx.simulate_mouse_move(at(0.5), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		cx.simulate_mouse_move(at(0.98), MouseButton::Left, Modifiers::default());
		cx.executor().advance_clock(EDGE_HOLD + Duration::from_millis(10));
		cx.run_until_parked();
		assert_eq!(level(&mut cx), 0, "a second visit is a second level");
		cx.simulate_mouse_up(at(0.98), MouseButton::Left, Modifiers::default());
	}

	#[gpui::test]
	fn a_handle_at_the_end_is_taken_by_a_press_on_its_dot(cx: &mut TestAppContext) {
		let (slider, mut cx, track) = opened(vec![1.0], cx);
		let y = track.center().y;
		// The dot is centred on the track's end, so half of it lies past the track.
		cx.simulate_mouse_down(
			point(track.right() + px(3.0), y),
			MouseButton::Left,
			Modifiers::default(),
		);
		cx.run_until_parked();
		assert!(
			slider.read_with(&cx, |slider, _| slider.dragging.is_some()),
			"the dot's outer half takes the press"
		);
		cx.simulate_mouse_up(
			point(track.right() + px(3.0), y),
			MouseButton::Left,
			Modifiers::default(),
		);
		cx.run_until_parked();
		cx.simulate_mouse_down(point(track.right(), y), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!(slider.read_with(&cx, |slider, _| slider.dragging.is_some()), "and so does its centre");
		cx.simulate_mouse_up(point(track.right(), y), MouseButton::Left, Modifiers::default());
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
		assert_eq!(
			slider.read_with(&cx, |slider, _| slider.handles[1]),
			1.0,
			"past the end is the end"
		);
		cx.simulate_mouse_move(at(0.05), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert_eq!(
			slider.read_with(&cx, |slider, _| slider.handles[1]),
			0.2,
			"and never past the other"
		);
		cx.simulate_mouse_up(at(0.05), MouseButton::Left, Modifiers::default());
		cx.run_until_parked();
		assert!(slider.read_with(&cx, |slider, _| slider.dragging.is_none()), "a release lets go");
	}
}
