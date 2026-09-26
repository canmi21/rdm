//! The slider drawn: the track, its places at the level shown, the handles and the zoom's glyphs.

use super::*;

impl Render for Slider {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let track = self.track.clone();
		let slider = cx.weak_entity();
		let dragging = self.dragging.is_some();
		// While zoomed, every value is drawn where the zoomed track puts it.
		let zoom = self.zoom();
		let shown = |value: f32| view(zoom, value).clamp(0.0, 1.0);
		// The filled part: between the handles of a pair, or from the scale's start to a lone handle.
		// On a zoomed track it stays within the band: the ends stand for no value, and a fill running
		// into them covered the way out they are marked with.
		let (lo_fill, hi_fill) = if zoom.is_some() { (MARGIN, 1.0 - MARGIN) } else { (0.0, 1.0) };
		let band = |at: f32| at.clamp(lo_fill, hi_fill);
		let (from, to) = match self.handles.as_slice() {
			[] => (lo_fill, lo_fill),
			[one] => (lo_fill, band(shown(*one))),
			[first, .., last] => (band(shown(*first)), band(shown(*last))),
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
		// Back out: the two ends of a zoomed track, each marked as a way out. The hand in one for long
		// enough is ready to go, and then both point back to the middle, where going is.
		let leaving = self.edge.map_or(0.0, |since| share(since, EDGE_HOLD));
		let ready = leaving >= 1.0;
		let edge_right = self.pointer + self.offset > 0.5;
		if dragging && (self.hanging.is_some() || swinging.is_some() || (self.edge.is_some() && !ready))
		{
			window.request_animation_frame();
		}
		let held = self.dragging;
		let end = |right: bool| {
			let fill = if self.edge.is_some() && edge_right == right { leaving } else { 0.0 };
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
				// Pointing out, and back in once the hand is ready: going is coming back to the middle,
				// and both ends say so, since the handle may be covering the one it is in.
				.child(if right != ready { "\u{203A}" } else { "\u{2039}" })
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
						// A glyph fades in inside the held handle as a hold nears what it will do:
						// chevrons pointing apart as a dive nears, the gap about to be spread across the
						// track, and together in an end, the track to fold back to the level above as the
						// hand comes back to the middle.
						let out = leaving;
						let (open, glyph) = match held == Some(index) {
							true if deeper > 0.0 => (deeper, Some(Icon::ChevronsLeftRight)),
							true if out > 0.0 => (out, Some(Icon::ChevronsRightLeft)),
							_ => (0.0, None),
						};
						div()
							.id(("handle", index))
							.role(gpui::Role::Slider)
							.aria_label(name)
							.aria_numeric_value(f64::from((at * 100.0).round()))
							.absolute()
							.top(px(10.0 - KNOB_H / 2.0))
							.left(relative(shown(at)))
							.ml(px(-KNOB_W / 2.0))
							.w(px(KNOB_W))
							.h(px(KNOB_H))
							.rounded_full()
							.border_1()
							.border_color(p.border)
							.bg(p.text)
							.shadow_sm()
							.flex()
							.items_center()
							.justify_center()
							.when_some(glyph, |s, glyph| {
								s.child(icon(glyph, p.window.opacity(open)).size(px(GLYPH)))
							})
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
