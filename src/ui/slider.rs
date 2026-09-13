//! A track with one handle or two, pressed or dragged along its length, for a value set roughly
//! by hand beside a field that sets it exactly. GPUI ships none. See spec/ui.md.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
	App, Bounds, Context, DispatchPhase, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
	MouseUpEvent, Pixels, Render, SharedString, Window, canvas, div, prelude::*, px, relative,
};

use crate::ui::LeavesFocus;
use crate::ui::theme;

/// What a move of a handle does, decided by whoever owns the slider: which handle, and where along
/// the track it now is, from 0 at the left to 1 at the right.
type OnChange = Box<dyn Fn(usize, f32, &mut Window, &mut App)>;

pub struct Slider {
	/// Where each handle is along the track, 0 to 1, in order from the left.
	handles: Vec<f32>,
	/// The handle a press took hold of, until the release.
	dragging: Option<usize>,
	/// The track as last drawn, which a pointer's x is measured against.
	track: Rc<Cell<Option<Bounds<Pixels>>>>,
	label: SharedString,
	on_change: Option<OnChange>,
}

impl Slider {
	pub fn new(label: impl Into<SharedString>, handles: Vec<f32>) -> Self {
		Self { handles, dragging: None, track: Rc::default(), label: label.into(), on_change: None }
	}

	pub fn on_change(mut self, f: impl Fn(usize, f32, &mut Window, &mut App) + 'static) -> Self {
		self.on_change = Some(Box::new(f));
		self
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
		let track = self.track.get()?;
		let width = track.size.width;
		(width > px(0.0)).then(|| ((x - track.left()) / width).clamp(0.0, 1.0))
	}

	/// A handle moved to `position`, held between its neighbours so a pair never crosses, and the
	/// owner told.
	fn move_handle(&mut self, index: usize, position: f32, window: &mut Window, cx: &mut Context<Self>) {
		let low = if index > 0 { self.handles[index - 1] } else { 0.0 };
		let high = self.handles.get(index + 1).copied().unwrap_or(1.0);
		let position = position.clamp(low, high);
		self.handles[index] = position;
		if let Some(on_change) = &self.on_change {
			on_change(index, position, window, cx);
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
		self.move_handle(index, position, window, cx);
	}

	fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
		let Some(index) = self.dragging else { return };
		if event.pressed_button != Some(MouseButton::Left) {
			self.dragging = None;
			cx.notify();
			return;
		}
		if let Some(position) = self.position_at(event.position.x) {
			self.move_handle(index, position, window, cx);
		}
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
