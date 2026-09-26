//! A single-line text field. GPUI ships no widget for this; the shape is Zed's `examples/input.rs`
//! (Apache-2.0), trimmed to one line and drawn in this application's palette.

use std::ops::Range;
use std::rc::Rc;

use gpui::{
	App, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, ElementId, ElementInputHandler,
	Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, LayoutId, MouseButton,
	MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, ShapedLine, SharedString,
	Style, TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, prelude::*,
	px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::ui::icon::{Icon, icon};
use crate::ui::theme;

mod element;
mod ime;
#[cfg(test)]
mod tests;

use element::TextElement;

actions!(
	text_input,
	[
		Backspace,
		Delete,
		Left,
		Right,
		SelectLeft,
		SelectRight,
		SelectAll,
		Home,
		End,
		Paste,
		Cut,
		Copy,
		Confirm,
		Cancel,
		FocusNext
	]
);

/// Bound once, in main: the keys every text field answers to. The clipboard and select-all
/// take the system's own modifier: Command on macOS, Control everywhere else, where there is
/// no Command and a field that answers only to it cannot be pasted into.
pub fn key_bindings() -> Vec<gpui::KeyBinding> {
	use gpui::KeyBinding;
	let context = Some("TextInput");
	let modifier = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
	let with = |key: &str| format!("{modifier}-{key}");
	vec![
		KeyBinding::new("backspace", Backspace, context),
		KeyBinding::new("delete", Delete, context),
		KeyBinding::new("left", Left, context),
		KeyBinding::new("right", Right, context),
		KeyBinding::new("shift-left", SelectLeft, context),
		KeyBinding::new("shift-right", SelectRight, context),
		KeyBinding::new(&with("a"), SelectAll, context),
		KeyBinding::new(&with("v"), Paste, context),
		KeyBinding::new(&with("c"), Copy, context),
		KeyBinding::new(&with("x"), Cut, context),
		KeyBinding::new("home", Home, context),
		KeyBinding::new("end", End, context),
		KeyBinding::new("cmd-left", Home, context),
		KeyBinding::new("cmd-right", End, context),
		KeyBinding::new("enter", Confirm, context),
		KeyBinding::new("escape", Cancel, context),
		KeyBinding::new("tab", FocusNext, context),
	]
}

/// What Enter does, decided by whoever owns the field.
type OnConfirm = Rc<dyn Fn(&str, &mut Window, &mut App)>;
type OnCancel = Rc<dyn Fn(&mut Window, &mut App)>;

pub struct TextInput {
	focus_handle: FocusHandle,
	pub content: SharedString,
	placeholder: SharedString,
	selected_range: Range<usize>,
	selection_reversed: bool,
	marked_range: Option<Range<usize>>,
	last_layout: Option<ShapedLine>,
	last_bounds: Option<Bounds<Pixels>>,
	/// How far the line is shifted left so the cursor stays in view: a one-line field scrolls
	/// under its cursor, the way every native field does, rather than wrapping or eliding.
	pub scroll: Pixels,
	is_selecting: bool,
	/// Where the pointer is while a selection is being dragged, inside the field or not.
	drag_at: Option<Point<Pixels>>,
	/// A glyph drawn inside the box before the text, for a field whose purpose is a shape.
	leading: Option<Icon>,
	/// A unit drawn inside the box after a value, in a label's grey: the value is read with it.
	trailing: Option<SharedString>,
	/// Enter was pressed; the owning window decides what that means.
	on_confirm: Option<OnConfirm>,
	on_cancel: Option<OnCancel>,
}

impl TextInput {
	pub fn new(placeholder: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
		Self {
			focus_handle: cx.focus_handle(),
			content: "".into(),
			placeholder: placeholder.into(),
			selected_range: 0..0,
			selection_reversed: false,
			marked_range: None,
			last_layout: None,
			last_bounds: None,
			scroll: px(0.0),
			is_selecting: false,
			drag_at: None,
			leading: None,
			trailing: None,
			on_confirm: None,
			on_cancel: None,
		}
	}

	pub fn on_confirm(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
		self.on_confirm = Some(Rc::new(f));
		self
	}

	pub fn on_cancel(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
		self.on_cancel = Some(Rc::new(f));
		self
	}

	pub fn with_leading(mut self, glyph: Icon) -> Self {
		self.leading = Some(glyph);
		self
	}

	pub fn with_trailing(mut self, unit: impl Into<SharedString>) -> Self {
		self.trailing = Some(unit.into());
		self
	}

	/// Replaces the whole text and puts the cursor at its end.
	pub fn set_content(&mut self, text: &str, cx: &mut Context<Self>) {
		self.content = text.to_owned().into();
		self.selected_range = self.content.len()..self.content.len();
		self.marked_range = None;
		cx.notify();
	}

	pub fn focus(&self) -> FocusHandle {
		self.focus_handle.clone()
	}

	fn focus_next(&mut self, _: &FocusNext, window: &mut Window, cx: &mut Context<Self>) {
		window.focus_next(cx);
	}

	// Enter and Escape reach their owner once this field is done with itself, not from inside its
	// own update: an owner reads its fields when it acts on them, and reading this one from in here
	// panicked. The New Task sheet did exactly that on Enter, and the application went down with it.
	fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(on_cancel) = self.on_cancel.clone() {
			window.defer(cx, move |window, cx| on_cancel(window, cx));
		}
	}

	fn confirm(&mut self, _: &Confirm, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(on_confirm) = self.on_confirm.clone() {
			let text = self.content.trim().to_owned();
			window.defer(cx, move |window, cx| on_confirm(&text, window, cx));
		}
	}

	fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			self.move_to(self.previous_boundary(self.cursor_offset()), cx);
		} else {
			self.move_to(self.selected_range.start, cx)
		}
	}

	fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			self.move_to(self.next_boundary(self.selected_range.end), cx);
		} else {
			self.move_to(self.selected_range.end, cx)
		}
	}

	fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
		self.select_to(self.previous_boundary(self.cursor_offset()), cx);
	}

	fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
		self.select_to(self.next_boundary(self.cursor_offset()), cx);
	}

	fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
		self.move_to(0, cx);
		self.select_to(self.content.len(), cx)
	}

	fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
		self.move_to(0, cx);
	}

	fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
		self.move_to(self.content.len(), cx);
	}

	fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			let prev = self.previous_boundary(self.cursor_offset());
			if self.cursor_offset() == prev {
				return;
			}
			self.select_to(prev, cx)
		}
		self.replace_text_in_range(None, "", window, cx)
	}

	fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
		if self.selected_range.is_empty() {
			let next = self.next_boundary(self.cursor_offset());
			if self.cursor_offset() == next {
				return;
			}
			self.select_to(next, cx)
		}
		self.replace_text_in_range(None, "", window, cx)
	}

	fn on_mouse_down(
		&mut self,
		event: &MouseDownEvent,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.is_selecting = true;
		self.drag_at = Some(event.position);
		if event.modifiers.shift {
			self.select_to(self.index_for_mouse_position(event.position), cx);
		} else {
			self.move_to(self.index_for_mouse_position(event.position), cx)
		}
	}

	fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
		self.is_selecting = false;
		self.drag_at = None;
	}

	/// The pointer moved during a drag, wherever it is; a button let go outside the window ends
	/// the drag here, since the release was never heard.
	fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
		if event.pressed_button != Some(MouseButton::Left) {
			self.is_selecting = false;
			self.drag_at = None;
			return;
		}
		self.drag_at = Some(event.position);
		self.select_to(self.index_for_mouse_position(event.position), cx);
	}

	/// One character further toward the side the pointer is held past, for a frame of a drag;
	/// whether it moved, so a drag held at the line's end asks for no more frames.
	fn step_selection(&mut self, rightward: bool) -> bool {
		let cursor = self.cursor_offset();
		let to = if rightward { self.next_boundary(cursor) } else { self.previous_boundary(cursor) };
		if to != cursor {
			self.extend_selection(to);
		}
		to != cursor
	}

	fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
			self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
		}
	}

	fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
		if !self.selected_range.is_empty() {
			cx.write_to_clipboard(ClipboardItem::new_string(
				self.content[self.selected_range.clone()].to_string(),
			));
		}
	}

	fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
		if !self.selected_range.is_empty() {
			cx.write_to_clipboard(ClipboardItem::new_string(
				self.content[self.selected_range.clone()].to_string(),
			));
			self.replace_text_in_range(None, "", window, cx)
		}
	}

	fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		self.selected_range = offset..offset;
		cx.notify()
	}

	fn cursor_offset(&self) -> usize {
		if self.selection_reversed { self.selected_range.start } else { self.selected_range.end }
	}

	fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
		if self.content.is_empty() {
			return 0;
		}
		let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref()) else {
			return 0;
		};
		if position.y < bounds.top() {
			return 0;
		}
		if position.y > bounds.bottom() {
			return self.content.len();
		}
		line.closest_index_for_x(position.x - bounds.left() + self.scroll)
	}

	fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
		self.extend_selection(offset);
		cx.notify()
	}

	fn extend_selection(&mut self, offset: usize) {
		if self.selection_reversed {
			self.selected_range.start = offset
		} else {
			self.selected_range.end = offset
		};
		if self.selected_range.end < self.selected_range.start {
			self.selection_reversed = !self.selection_reversed;
			self.selected_range = self.selected_range.end..self.selected_range.start;
		}
	}

	fn offset_from_utf16(&self, offset: usize) -> usize {
		let mut utf8_offset = 0;
		let mut utf16_count = 0;
		for ch in self.content.chars() {
			if utf16_count >= offset {
				break;
			}
			utf16_count += ch.len_utf16();
			utf8_offset += ch.len_utf8();
		}
		utf8_offset
	}

	fn offset_to_utf16(&self, offset: usize) -> usize {
		let mut utf16_offset = 0;
		let mut utf8_count = 0;
		for ch in self.content.chars() {
			if utf8_count >= offset {
				break;
			}
			utf8_count += ch.len_utf8();
			utf16_offset += ch.len_utf16();
		}
		utf16_offset
	}

	/// A range that slices the content safely: inside it, ordered, and on character boundaries.
	/// The input method's offsets arrive in UTF-16 and are converted, so this is a guard for the
	/// arithmetic around them rather than for the conversion.
	fn clamped(&self, range: Range<usize>) -> Range<usize> {
		let floor = |mut i: usize| {
			i = i.min(self.content.len());
			while !self.content.is_char_boundary(i) {
				i -= 1;
			}
			i
		};
		let (start, end) = (floor(range.start), floor(range.end));
		start.min(end)..end.max(start)
	}

	fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
		self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
	}

	fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
		self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
	}

	fn previous_boundary(&self, offset: usize) -> usize {
		self
			.content
			.grapheme_indices(true)
			.rev()
			.find_map(|(idx, _)| (idx < offset).then_some(idx))
			.unwrap_or(0)
	}

	fn next_boundary(&self, offset: usize) -> usize {
		self
			.content
			.grapheme_indices(true)
			.find_map(|(idx, _)| (idx > offset).then_some(idx))
			.unwrap_or(self.content.len())
	}
}

impl Render for TextInput {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		div()
			// A node of its own in the accessibility tree, its text as the value, so VoiceOver and
			// `ctl tree` read what is typed rather than an unnamed group. One id serves every field
			// because each field is its own view, which GPUI puts in the id path. See
			// spec/workflow.md.
			.id("text-input")
			.role(gpui::Role::TextInput)
			.aria_value(self.content.clone())
			.aria_placeholder(self.placeholder.clone())
			.flex()
			.key_context("TextInput")
			.track_focus(&self.focus_handle(cx))
			.tab_stop(true)
			.cursor(CursorStyle::IBeam)
			.on_action(cx.listener(Self::backspace))
			.on_action(cx.listener(Self::delete))
			.on_action(cx.listener(Self::left))
			.on_action(cx.listener(Self::right))
			.on_action(cx.listener(Self::select_left))
			.on_action(cx.listener(Self::select_right))
			.on_action(cx.listener(Self::select_all))
			.on_action(cx.listener(Self::home))
			.on_action(cx.listener(Self::end))
			.on_action(cx.listener(Self::paste))
			.on_action(cx.listener(Self::cut))
			.on_action(cx.listener(Self::copy))
			.on_action(cx.listener(Self::confirm))
			.on_action(cx.listener(Self::cancel))
			.on_action(cx.listener(Self::focus_next))
			.on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
			.on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
			.w_full()
			.px_2()
			.py_1()
			.rounded_md()
			.border_1()
			.border_color(if self.focus_handle.is_focused(window) { p.accent } else { p.border })
			.bg(p.window)
			.line_height(px(20.))
			.gap_1p5()
			.items_center()
			.when_some(self.leading, |s, glyph| s.child(icon(glyph, p.muted).size_3p5()))
			.child(TextElement { input: cx.entity() })
			// Only beside a value: a placeholder says what an empty field means, and a unit after
			// it reads as part of the sentence.
			.when_some(self.trailing.clone().filter(|_| !self.content.is_empty()), |s, unit| {
				s.child(div().flex_none().text_color(p.muted).child(unit))
			})
	}
}

impl Focusable for TextInput {
	fn focus_handle(&self, _: &App) -> FocusHandle {
		self.focus_handle.clone()
	}
}
