//! The field as the system's input method sees it: text by UTF-16 range, the selection, marked text
//! while a composition goes on, and where a character is on screen.

use super::*;

impl EntityInputHandler for TextInput {
	fn text_for_range(
		&mut self,
		range_utf16: Range<usize>,
		actual_range: &mut Option<Range<usize>>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<String> {
		let range = self.clamped(self.range_from_utf16(&range_utf16));
		actual_range.replace(self.range_to_utf16(&range));
		Some(self.content[range].to_string())
	}

	fn selected_text_range(
		&mut self,
		_ignore_disabled_input: bool,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<UTF16Selection> {
		Some(UTF16Selection {
			range: self.range_to_utf16(&self.selected_range),
			reversed: self.selection_reversed,
		})
	}

	fn marked_text_range(
		&self,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<Range<usize>> {
		self.marked_range.as_ref().map(|range| self.range_to_utf16(range))
	}

	fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
		self.marked_range = None;
	}

	fn replace_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		_: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = range_utf16
			.as_ref()
			.map(|range_utf16| self.range_from_utf16(range_utf16))
			.or(self.marked_range.clone())
			.unwrap_or(self.selected_range.clone());
		self.content =
			(self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
		self.selected_range = range.start + new_text.len()..range.start + new_text.len();
		self.marked_range.take();
		cx.notify();
	}

	fn replace_and_mark_text_in_range(
		&mut self,
		range_utf16: Option<Range<usize>>,
		new_text: &str,
		new_selected_range_utf16: Option<Range<usize>>,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let range = range_utf16
			.as_ref()
			.map(|range_utf16| self.range_from_utf16(range_utf16))
			.or(self.marked_range.clone())
			.unwrap_or(self.selected_range.clone());
		self.content =
			(self.content[0..range.start].to_owned() + new_text + &self.content[range.end..]).into();
		self.marked_range =
			if new_text.is_empty() { None } else { Some(range.start..range.start + new_text.len()) };
		// The selection the input method asks for is relative to the text it just inserted. Zed's
		// example added `range.end` to the end offset, which put the selection past the content as
		// soon as a composition replaced anything, and the next replacement sliced out of bounds.
		self.selected_range = new_selected_range_utf16
			.as_ref()
			.map(|range_utf16| self.range_from_utf16(range_utf16))
			.map(|new_range| range.start + new_range.start..range.start + new_range.end)
			.unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
		self.selected_range = self.clamped(self.selected_range.clone());
		cx.notify();
	}

	fn bounds_for_range(
		&mut self,
		range_utf16: Range<usize>,
		bounds: Bounds<Pixels>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<Bounds<Pixels>> {
		let last_layout = self.last_layout.as_ref()?;
		let range = self.range_from_utf16(&range_utf16);
		Some(Bounds::from_corners(
			point(bounds.left() - self.scroll + last_layout.x_for_index(range.start), bounds.top()),
			point(bounds.left() - self.scroll + last_layout.x_for_index(range.end), bounds.bottom()),
		))
	}

	fn character_index_for_point(
		&mut self,
		point: gpui::Point<Pixels>,
		_window: &mut Window,
		_cx: &mut Context<Self>,
	) -> Option<usize> {
		let line_point = self.last_bounds?.localize(&point)?;
		let last_layout = self.last_layout.as_ref()?;
		let utf8_index = last_layout.index_for_x(point.x - line_point.x)?;
		Some(self.offset_to_utf16(utf8_index))
	}
}
