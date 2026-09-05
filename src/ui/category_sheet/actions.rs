//! How the sheet moves between its faces and what each control on them does to the form.

use gpui::{Context, Window, prelude::*};

use crate::app::Rdm;
use crate::category::{Category, Combine, pattern_for_rule};
use crate::ui::category_sheet::{CategoryForm, CategorySheet, PresetForm};
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;
use crate::ui::theme::{Tint, parse_color};

/// The pattern field's placeholder: one worked example, and what the engine allows, as a comment.
const PATTERN_EXAMPLE: &str = r"^(?!.*\.mp4$).*$  // lookahead and lookbehind are supported";

/// The color field's placeholder: one hex value, the way most people will write one. The rest
/// of what the field reads is behind the question mark beside it.
const COLOR_EXAMPLES: &str = "#3b4252";

/// The full guide, laid over the form on a press: one line per shape, named, with examples.
const COLOR_GUIDE: [(&str, &str); 3] = [
	("HEX", "#3b4252, #3b4252ff, #abc, #abcf"),
	("RGB", "rgb(59, 66, 82), rgba(59 66 82 / 0.5), rgb(23%, 26%, 32%)"),
	("HSL", "hsl(220, 16%, 28%), hsla(220 16% 28% / 1)"),
];

/// After the shapes: what the field does with what it does not keep.
const COLOR_NOTE: &str = "Alpha is read and dropped. Named colors are not read.";

impl Rdm {
	/// The plus beside the Categories heading: the presets, or whatever face is already up.
	pub(crate) fn open_category_sheet(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if self.category_sheet.is_none() {
			self.category_sheet = Some(CategorySheet::Presets { editing: false });
		}
		cx.notify();
	}

	/// Escape in a field: the sheet's rule, the same as a press outside it.
	fn escape_to_presets(rdm: &gpui::Entity<Rdm>) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
		let rdm = rdm.clone();
		move |_, cx| rdm.update(cx, |this, cx| this.dismiss_category_sheet(cx))
	}

	/// Add on the presets face: the custom form, with the name focused. The control socket has no
	/// window to focus in and passes none.
	pub(crate) fn open_custom_form(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
		if !matches!(self.category_sheet, Some(CategorySheet::Custom(_))) {
			let rdm = cx.entity();
			// Escape steps back to the presets; Enter submits from any field.
			let submit = |rdm: &gpui::Entity<Rdm>| {
				let rdm = rdm.clone();
				move |_: &str, _: &mut Window, cx: &mut gpui::App| {
					rdm.update(cx, |this, cx| this.submit_category(cx))
				}
			};
			let field = |placeholder: &'static str, cx: &mut Context<Self>| {
				cx.new(|cx| {
					TextInput::new(placeholder, cx)
						.on_cancel(Self::escape_to_presets(&rdm))
						.on_confirm(submit(&rdm))
				})
			};
			// The color it would get anyway, shown so the swatch is never blank.
			let color = Tint::cycle(self.categories.iter().map(|c| c.id).max().unwrap_or(0) + 1).rgb();
			let custom = cx.new(|cx| {
				TextInput::new(COLOR_EXAMPLES, cx).on_cancel(Self::escape_to_presets(&rdm)).on_confirm({
					let rdm = rdm.clone();
					move |text, _, cx| {
						if let Some(color) = parse_color(text) {
							rdm.update(cx, |this, cx| this.choose_color(color, cx));
						}
					}
				})
			});
			// The dot beside the field previews what is typed, so the form redraws as it changes.
			cx.observe(&custom, |_, _, cx| cx.notify()).detach();
			let form = CategoryForm {
				name: field("Name", cx),
				extensions: field("Extensions: rs, py, ts", cx),
				contains: field("Name contains", cx),
				combine: Combine::And,
				match_case: false,
				ignore_space: false,
				pattern: field(PATTERN_EXAMPLE, cx),
				icon: Icon::Code,
				color,
				color_open: false,
				custom,
				advanced: false,
			};
			self.category_sheet = Some(CategorySheet::Custom(form));
		}
		if let (Some(CategorySheet::Custom(form)), Some(window)) = (&self.category_sheet, window) {
			window.focus(&form.name.read(cx).focus(), cx);
		}
		cx.notify();
	}

	/// Edit on the presets face: the chips open their lists instead of switching.
	pub(crate) fn toggle_preset_editing(&mut self, cx: &mut Context<Self>) {
		if let Some(CategorySheet::Presets { editing }) = &mut self.category_sheet {
			*editing = !*editing;
			cx.notify();
		}
	}

	/// A preset's extension list, by the category's id. Only a preset that is on has a list to
	/// edit; any other id is left alone.
	pub(crate) fn open_preset_editor(
		&mut self,
		id: u64,
		window: Option<&mut Window>,
		cx: &mut Context<Self>,
	) {
		if !self.categories.iter().any(|c| c.id == id && c.preset.is_some()) {
			return;
		}
		let rdm = cx.entity();
		let confirm = rdm.clone();
		let add = cx.new(|cx| {
			TextInput::new("Add extensions: rs, py", cx)
				.on_cancel(Self::escape_to_presets(&rdm))
				// Deferred: Enter arrives from inside the field's own update, and clearing the field
				// is a second update of the same entity, which cannot nest.
				.on_confirm(move |text, _, cx| {
					let text = text.to_owned();
					let rdm = confirm.clone();
					cx.defer(move |cx| {
						rdm.update(cx, |this, cx| {
							this.add_preset_extensions(id, &text, cx);
							if let Some(CategorySheet::Preset(form)) = &this.category_sheet {
								form.add.update(cx, |input, cx| input.set_content("", cx));
							}
						})
					});
				})
		});
		// The user's own color follows the category: the field starts with what was written last.
		let saved = self
			.categories
			.iter()
			.find(|c| c.id == id)
			.and_then(|c| c.custom_color.clone())
			.unwrap_or_default();
		let custom = cx.new(|cx| {
			let mut input =
				TextInput::new(COLOR_EXAMPLES, cx).on_cancel(Self::escape_to_presets(&rdm)).on_confirm({
					let rdm = rdm.clone();
					move |text, _, cx| {
						let text = text.to_owned();
						rdm.update(cx, |this, cx| this.set_category_custom_color(id, &text, cx));
					}
				});
			input.set_content(&saved, cx);
			input
		});
		cx.observe(&custom, |_, _, cx| cx.notify()).detach();
		if let Some(window) = window {
			window.focus(&add.read(cx).focus(), cx);
		}
		self.category_sheet = Some(CategorySheet::Preset(PresetForm { id, add, custom }));
		cx.notify();
	}

	pub(crate) fn start_reorder(&mut self, cx: &mut Context<Self>) {
		self.category_sheet = Some(CategorySheet::Reorder);
		cx.notify();
	}

	/// The cross on a second-level face, and Escape in its fields: one level up, not out.
	pub(crate) fn back_to_presets(&mut self, cx: &mut Context<Self>) {
		self.category_sheet = Some(CategorySheet::Presets { editing: false });
		cx.notify();
	}

	/// The swatch beside the new category's name opens and closes the picker under it. A preset
	/// being edited shows its picker always.
	pub(crate) fn toggle_color_picker(&mut self, cx: &mut Context<Self>) {
		if let Some(CategorySheet::Custom(form)) = &mut self.category_sheet {
			form.color_open = !form.color_open;
			cx.notify();
		}
	}

	/// A swatch pressed: the face's color. On the preset face the color is the category's and
	/// is written at once. The field is left as the user wrote it; a named hue does not erase
	/// their own.
	pub(crate) fn choose_color(&mut self, color: u32, cx: &mut Context<Self>) {
		match &mut self.category_sheet {
			Some(CategorySheet::Custom(form)) => form.color = color,
			Some(CategorySheet::Preset(form)) => {
				let id = form.id;
				self.set_category_color(id, color, cx);
			}
			_ => return,
		}
		cx.notify();
	}

	/// A click outside, or Escape, closes the sheet only while there is nothing to lose. The
	/// presets act at once, and so does every drop while reordering, so those faces always
	/// close; a preset's list applies each change as it is made, so its editor closes unless
	/// something is typed in its field; the custom form closes only while nothing has been typed
	/// or switched. See spec/ui.md.
	pub(crate) fn dismiss_category_sheet(&mut self, cx: &mut Context<Self>) {
		// A press under the guide is a press on the guide's backdrop, not outside this sheet.
		if self.guide.is_some() {
			return;
		}
		let clean = match &self.category_sheet {
			// The presets act at once, and so does every drop while reordering.
			None | Some(CategorySheet::Presets { .. }) | Some(CategorySheet::Reorder) => true,
			Some(CategorySheet::Preset(form)) => {
				let saved = self
					.categories
					.iter()
					.find(|c| c.id == form.id)
					.and_then(|c| c.custom_color.as_deref())
					.unwrap_or_default();
				form.add.read(cx).content.trim().is_empty() && form.custom.read(cx).content.trim() == saved
			}
			Some(CategorySheet::Custom(form)) => {
				form.name.read(cx).content.trim().is_empty()
					&& form.extensions.read(cx).content.trim().is_empty()
					&& form.contains.read(cx).content.trim().is_empty()
					&& form.pattern.read(cx).content.trim().is_empty()
					&& form.combine == Combine::And
					&& !form.match_case
					&& !form.ignore_space
					&& form.icon == Icon::Code
					&& !form.color_open
					&& form.custom.read(cx).content.trim().is_empty()
			}
		};
		if clean {
			self.close_category_sheet(cx);
		}
	}

	pub(crate) fn close_category_sheet(&mut self, cx: &mut Context<Self>) {
		self.category_sheet = None;
		cx.notify();
	}

	fn form(&self) -> Option<&CategoryForm> {
		match &self.category_sheet {
			Some(CategorySheet::Custom(form)) => Some(form),
			_ => None,
		}
	}

	fn form_mut(&mut self) -> Option<&mut CategoryForm> {
		match &mut self.category_sheet {
			Some(CategorySheet::Custom(form)) => Some(form),
			_ => None,
		}
	}

	pub(crate) fn choose_category_icon(&mut self, icon: Icon, cx: &mut Context<Self>) {
		if let Some(form) = self.form_mut() {
			form.icon = icon;
			cx.notify();
		}
	}

	pub(crate) fn set_combine(&mut self, combine: Combine, cx: &mut Context<Self>) {
		if let Some(form) = self.form_mut() {
			form.combine = combine;
			cx.notify();
		}
	}

	pub(crate) fn toggle_match_case(&mut self, cx: &mut Context<Self>) {
		if let Some(form) = self.form_mut() {
			form.match_case = !form.match_case;
			cx.notify();
		}
	}

	pub(crate) fn show_color_guide(&mut self, cx: &mut Context<Self>) {
		self.show_guide(
			crate::ui::guide::Guide {
				title: "Colors",
				about: "Any of these forms works in the field beside the swatches.",
				lines: &COLOR_GUIDE,
				note: COLOR_NOTE,
			},
			cx,
		);
	}

	pub(crate) fn toggle_ignore_space(&mut self, cx: &mut Context<Self>) {
		if let Some(form) = self.form_mut() {
			form.ignore_space = !form.ignore_space;
			cx.notify();
		}
	}

	/// Opening Advanced shows the pattern the basic fields stand for, editable from there on; the
	/// basic fields are then only a way to start over.
	pub(crate) fn toggle_advanced(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
		let Some(form) = self.form() else { return };
		let advanced = !form.advanced;
		let derived = self.basic_pattern(form, cx);
		let pattern = form.pattern.clone();
		if advanced && pattern.read(cx).content.is_empty() {
			pattern.update(cx, |input, cx| input.set_content(&derived, cx));
		}
		if let Some(form) = self.form_mut() {
			form.advanced = advanced;
		}
		if let (true, Some(window)) = (advanced, window) {
			window.focus(&pattern.read(cx).focus(), cx);
		}
		cx.notify();
	}

	/// What the basic fields and switches say, as one pattern.
	fn basic_pattern(&self, form: &CategoryForm, cx: &Context<Self>) -> String {
		pattern_for_rule(
			&form.extensions.read(cx).content,
			&form.contains.read(cx).content,
			form.combine,
			!form.match_case,
			form.ignore_space,
		)
	}

	/// The pattern that would run: the written one under Advanced, the basic fields' otherwise.
	pub(super) fn effective_pattern(&self, form: &CategoryForm, cx: &Context<Self>) -> String {
		if form.advanced {
			form.pattern.read(cx).content.trim().to_owned()
		} else {
			self.basic_pattern(form, cx)
		}
	}

	pub(crate) fn submit_category(&mut self, cx: &mut Context<Self>) {
		let Some(form) = self.form() else { return };
		let name = form.name.read(cx).content.to_string();
		let pattern = self.effective_pattern(form, cx);
		let custom = Some(form.custom.read(cx).content.trim().to_owned()).filter(|t| !t.is_empty());
		if self.add_category(&name, form.icon, Some(form.color), custom, &pattern, cx).is_ok() {
			self.close_category_sheet(cx);
		}
	}

	/// What the rule would do right now: the engine's error, or how many current downloads it
	/// catches. Shown as it is typed, so a rule is checked before it exists.
	pub(super) fn pattern_report(
		&self,
		form: &CategoryForm,
		cx: &Context<Self>,
	) -> Result<usize, String> {
		let pattern = self.effective_pattern(form, cx);
		if pattern.is_empty() {
			return Ok(0);
		}
		let probe = Category::new(0, "probe", form.icon, &pattern)?;
		Ok(self.downloads.iter().filter(|d| probe.matches(d)).count())
	}
}
