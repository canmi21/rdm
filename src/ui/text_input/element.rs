//! The text itself: shaped, scrolled under the cursor, painted with the cursor or selection over
//! it. A custom element because it registers itself as the window's input handler while it is
//! painted.

use super::*;

/// The text itself: shaped, painted, with the cursor or selection over it. A custom element
/// because it registers itself as the window's input handler while it is painted.
pub(super) struct TextElement {
	pub(super) input: Entity<TextInput>,
}

pub(super) struct PrepaintState {
	line: Option<ShapedLine>,
	cursor: Option<PaintQuad>,
	selection: Option<PaintQuad>,
	scroll: Pixels,
}

impl IntoElement for TextElement {
	type Element = Self;

	fn into_element(self) -> Self::Element {
		self
	}
}

impl gpui::Element for TextElement {
	type RequestLayoutState = ();
	type PrepaintState = PrepaintState;

	fn id(&self) -> Option<ElementId> {
		None
	}

	fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
		None
	}

	fn request_layout(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		window: &mut Window,
		cx: &mut App,
	) -> (LayoutId, Self::RequestLayoutState) {
		let mut style = Style::default();
		style.size.width = relative(1.).into();
		style.size.height = window.line_height().into();
		(window.request_layout(style, [], cx), ())
	}

	fn prepaint(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
		window: &mut Window,
		cx: &mut App,
	) -> Self::PrepaintState {
		let p = theme::palette(window.is_window_active());
		let input = self.input.read(cx);
		let content = input.content.clone();
		let selected_range = input.selected_range.clone();
		let cursor = input.cursor_offset();
		let style = window.text_style();
		let (display_text, text_color) = if content.is_empty() {
			(input.placeholder.clone(), p.muted)
		} else {
			(content, style.color)
		};
		let run = TextRun {
			len: display_text.len(),
			font: style.font(),
			color: text_color,
			background_color: None,
			underline: None,
			strikethrough: None,
		};
		let runs = if let Some(marked_range) = input.marked_range.as_ref() {
			vec![
				TextRun { len: marked_range.start, ..run.clone() },
				TextRun {
					len: marked_range.end - marked_range.start,
					underline: Some(UnderlineStyle {
						color: Some(run.color),
						thickness: px(1.0),
						wavy: false,
					}),
					..run.clone()
				},
				TextRun { len: display_text.len() - marked_range.end, ..run },
			]
			.into_iter()
			.filter(|run| run.len > 0)
			.collect()
		} else {
			vec![run]
		};
		let font_size = style.font_size.to_pixels(window.rem_size());
		let line = window.text_system().shape_line(display_text, font_size, &runs, None);
		let cursor_pos = line.x_for_index(cursor);
		// The line scrolls under the cursor: shifted left just enough to keep the cursor inside
		// the field, back right when it moves toward the start, never past the line's end.
		let visible = bounds.size.width;
		let mut scroll = input.scroll;
		if cursor_pos - scroll > visible - px(2.0) {
			scroll = cursor_pos - visible + px(2.0);
		}
		if cursor_pos < scroll {
			scroll = cursor_pos;
		}
		scroll = scroll.min((line.width - visible).max(px(0.0))).max(px(0.0));
		let left = bounds.left() - scroll;
		let (selection, cursor) = if selected_range.is_empty() {
			(
				None,
				Some(fill(
					Bounds::new(
						point(left + cursor_pos, bounds.top()),
						size(px(1.5), bounds.bottom() - bounds.top()),
					),
					p.accent,
				)),
			)
		} else {
			(
				Some(fill(
					Bounds::from_corners(
						point(left + line.x_for_index(selected_range.start), bounds.top()),
						point(left + line.x_for_index(selected_range.end), bounds.bottom()),
					),
					p.selection,
				)),
				None,
			)
		};
		self.input.update(cx, |input, _| input.scroll = scroll);
		PrepaintState { line: Some(line), cursor, selection, scroll }
	}

	fn paint(
		&mut self,
		_id: Option<&GlobalElementId>,
		_inspector_id: Option<&gpui::InspectorElementId>,
		bounds: Bounds<Pixels>,
		_request_layout: &mut Self::RequestLayoutState,
		prepaint: &mut Self::PrepaintState,
		window: &mut Window,
		cx: &mut App,
	) {
		let focus_handle = self.input.read(cx).focus_handle.clone();
		window.handle_input(&focus_handle, ElementInputHandler::new(bounds, self.input.clone()), cx);
		// A drag that leaves the field keeps selecting. An element's own move listener hears the
		// pointer only while it is over the element, which stopped a selection a few characters
		// past either edge; the window's hears it anywhere. A pointer held still past an edge
		// moves the selection a character a frame, as a native field's does. See spec/ui.md.
		let (selecting, drag_at) = {
			let input = self.input.read(cx);
			(input.is_selecting, input.drag_at)
		};
		if selecting {
			let input = self.input.clone();
			window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
				if phase == DispatchPhase::Bubble {
					input.update(cx, |input, cx| input.drag_to(event, cx));
				}
			});
			if let Some(at) = drag_at
				&& (at.x < bounds.left() || at.x > bounds.right())
				&& self.input.update(cx, |input, _| input.step_selection(at.x > bounds.right()))
			{
				window.request_animation_frame();
			}
		}
		// Whatever scrolled out of the field is clipped, not drawn over the neighbours.
		let scroll = prepaint.scroll;
		let line = prepaint.line.take().expect("prepaint shaped the line");
		let selection = prepaint.selection.take();
		let cursor = prepaint.cursor.take().filter(|_| focus_handle.is_focused(window));
		window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
			if let Some(selection) = selection {
				window.paint_quad(selection)
			}
			line
				.paint(
					point(bounds.left() - scroll, bounds.top()),
					window.line_height(),
					gpui::TextAlign::Left,
					None,
					window,
					cx,
				)
				.ok();
			if let Some(cursor) = cursor {
				window.paint_quad(cursor);
			}
		});
		self.input.update(cx, |input, _cx| {
			input.last_layout = Some(line);
			input.last_bounds = Some(bounds);
		});
	}
}
