//! The category sheet, in four faces. It opens on the presets, each a switch, with Edit, Reorder
//! and Add under them. Edit turns the chips into doors to each preset's extension list. Reorder
//! shrinks the sheet to one line pointing at the sidebar, where the rows are dragged into order.
//! Add opens the custom form: a name, an icon, extensions and text the name contains -- or,
//! under Advanced, the regular expression written out.

use gpui::{
	Context, IntoElement, KeyDownEvent, Role, SharedString, Window, deferred, div, prelude::*, px,
};

use crate::app::{CategoryForm, CategorySheet, PresetForm, Rdm};
use crate::download::{Category, Combine, pattern_for_rule};
use crate::ui::icon::{Icon, icon};
use crate::ui::text_input::TextInput;
use crate::ui::theme::{Palette, Tint, format_hex, parse_color};
use crate::ui::tooltip::tooltip;
use crate::ui::{button, icon_button, sidebar, status_bar, toolbar};

/// The pattern field's placeholder: one worked example, and what the engine allows, as a comment.
const PATTERN_EXAMPLE: &str = r"^(?!.*\.mp4$).*$  // lookahead and lookbehind are supported";

/// The color field's placeholder: one hex value, the way most people will write one. The rest
/// of what the field reads is behind the question mark beside it.
const COLOR_EXAMPLES: &str = "#3b4252";

/// The short rule, as the question mark's tooltip.
const COLOR_RULE: &str = "Hex, rgb() or hsl(); press for the full guide";

/// The full guide, unfolded under the field on a press: one line per shape, with examples.
const COLOR_GUIDE: [&str; 4] = [
	"Hex: #3b4252, #3b4252ff, #abc, #abcf",
	"rgb(59, 66, 82), rgba(59 66 82 / 0.5), rgb(23%, 26%, 32%)",
	"hsl(220, 16%, 28%), hsla(220 16% 28% / 1)",
	"Alpha is read and dropped. Named colors are not read.",
];

/// Under the pattern field.
const ADVANCED_HINT: &str = "Enter a regular expression to match against whole file names.";

impl Rdm {
	/// The plus beside the Categories heading: the presets, or whatever face is already up.
	pub(crate) fn open_category_sheet(&mut self, _: &mut Window, cx: &mut Context<Self>) {
		if self.category_sheet.is_none() {
			self.category_sheet = Some(CategorySheet::Presets { editing: false });
		}
		cx.notify();
	}

	fn escape_to_presets(rdm: &gpui::Entity<Rdm>) -> impl Fn(&mut Window, &mut gpui::App) + 'static {
		let rdm = rdm.clone();
		move |_, cx| rdm.update(cx, |this, cx| this.back_to_presets(cx))
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

	/// Reorder takes the keyboard so Escape can finish it; nothing else on the face wants it.
	pub(crate) fn start_reorder(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
		self.category_sheet = Some(CategorySheet::Reorder);
		if let Some(window) = window {
			window.focus(&self.reorder_focus, cx);
		}
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

	/// A click outside closes the sheet only while there is nothing to lose. The presets act at
	/// once, so that face always closes; a preset's list applies each change as it is made, so
	/// its editor closes unless something is typed in its field; the custom form closes only
	/// while nothing has been typed or switched. Reorder has no card to press outside of in this
	/// sense: its washes close it themselves, and the sidebar is the work. See spec/ui.md.
	pub(crate) fn dismiss_category_sheet(&mut self, cx: &mut Context<Self>) {
		let clean = match &self.category_sheet {
			None | Some(CategorySheet::Presets { .. }) => true,
			Some(CategorySheet::Reorder) => false,
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

	pub(crate) fn toggle_color_help(&mut self, cx: &mut Context<Self>) {
		self.color_help = !self.color_help;
		cx.notify();
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
	fn effective_pattern(&self, form: &CategoryForm, cx: &Context<Self>) -> String {
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
	fn pattern_report(&self, form: &CategoryForm, cx: &Context<Self>) -> Result<usize, String> {
		let pattern = self.effective_pattern(form, cx);
		if pattern.is_empty() {
			return Ok(0);
		}
		let probe = Category::new(0, "probe", form.icon, &pattern)?;
		Ok(self.downloads.iter().filter(|d| probe.matches(d)).count())
	}

	pub(crate) fn render_category_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		match &self.category_sheet {
			None => deferred(div()).priority(2),
			Some(CategorySheet::Presets { editing }) => self.presets_face(*editing, cx),
			Some(CategorySheet::Reorder) => self.reorder_face(cx),
			Some(CategorySheet::Preset(form)) => self.preset_face(form, cx),
			Some(CategorySheet::Custom(form)) => self.custom_face(form, cx),
		}
	}

	/// The card every face sits in, over a backdrop that takes every mouse event so nothing
	/// behind it can be pressed through. A press outside the card asks to dismiss, except on the
	/// reorder face, where outside the card is the sidebar and a press there is the start of a
	/// drag.
	fn sheet_card(
		&self,
		id: &'static str,
		width: f32,
		dismiss_outside: bool,
		cx: &mut Context<Self>,
	) -> gpui::Stateful<gpui::Div> {
		let p = self.palette;
		div()
			.id(id)
			.debug_selector(move || id.to_owned())
			.flex()
			.flex_col()
			.gap_3()
			.w(px(width))
			.p_4()
			.rounded_lg()
			.border_1()
			.border_color(p.border)
			.bg(p.panel)
			.shadow_lg()
			.when(dismiss_outside, |s| {
				s.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_category_sheet(cx)))
			})
	}

	/// The title and the cross. On the presets the cross closes; a level down it steps back.
	fn title_row(
		&self,
		title: SharedString,
		back: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.flex()
			.items_center()
			.justify_between()
			.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(title))
			.child(icon_button(
				p,
				"category-close",
				Icon::X,
				"Close",
				true,
				cx.listener(
					move |this, _, _, cx| {
						if back { this.back_to_presets(cx) } else { this.close_category_sheet(cx) }
					},
				),
			))
	}

	fn presets_face(&self, editing: bool, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let presets: Vec<_> = Category::PRESETS
			.iter()
			.map(|preset| {
				let name = preset.name;
				let on = self.categories.iter().find(|c| c.name == name).map(|c| c.id);
				let lit = on.is_some();
				// While editing, a chip that is on opens its list and shows a pencil to say so; one
				// that is off has no list and goes quiet.
				let quiet = editing && !lit;
				let color = if lit { p.text } else { p.muted };
				div()
					.id(SharedString::from(format!("preset:{name}")))
					.role(if editing { Role::Button } else { Role::CheckBox })
					.aria_label(format!("Preset: {name}"))
					.when(!editing, |s| {
						s.aria_toggled(if lit { gpui::Toggled::True } else { gpui::Toggled::False })
					})
					.debug_selector(|| format!("preset:{name}"))
					.flex()
					.items_center()
					.gap_1p5()
					.px_2()
					.py_1()
					.rounded_md()
					.text_xs()
					.text_color(if quiet { p.border } else { color })
					.when(lit, |s| s.bg(p.selection))
					.when(!lit && !editing, move |s| s.hover(move |s| s.bg(p.hover)))
					.when(!quiet, |s| s.cursor_pointer())
					.when(!editing, |s| {
						s.on_click(cx.listener(move |this, _, _, cx| this.toggle_preset(name, cx)))
					})
					.when_some(on.filter(|_| editing), |s, id| {
						s.on_click(
							cx.listener(move |this, _, window, cx| this.open_preset_editor(id, Some(window), cx)),
						)
					})
					.child(icon(if editing && lit { Icon::Pencil } else { preset.icon }, color).size_3p5())
					.child(name)
			})
			.collect();
		deferred(
			backdrop(p).child(
				self
					.sheet_card("category-sheet", 480.0, true, cx)
					.child(self.title_row("Categories".into(), false, cx))
					.child(section(p.muted, "Presets"))
					.child(div().flex().flex_wrap().gap_1().children(presets))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.gap_3()
									.child(word(
										p,
										"edit",
										"Edit",
										editing,
										cx.listener(|this, _, _, cx| this.toggle_preset_editing(cx)),
									))
									.child(word(
										p,
										"reorder",
										"Reorder",
										false,
										cx.listener(|this, _, window, cx| this.start_reorder(Some(window), cx)),
									)),
							)
							.child(button(
								p,
								"category-add",
								Icon::Plus,
								"Add",
								true,
								cx.listener(|this, _, window, cx| this.open_custom_form(Some(window), cx)),
							)),
					),
			),
		)
		.priority(2)
	}

	/// One line pointing at the sidebar; Escape finishes, and so does a press anywhere that is
	/// neither this line nor the categories, since every drop is written as it lands and there
	/// is nothing to lose. A drag never counts: it begins on a row, not on a wash. The backdrop
	/// leaves the sidebar's column alone, since that is where the work is; the sidebar dims its
	/// own filters, above the categories, so the categories are the one lit thing. See spec/ui.md.
	fn reorder_face(&self, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let side = px(sidebar::WIDTH);
		let wash = |id: &'static str, cx: &mut Context<Self>| {
			div().id(id).absolute().occlude().bg(p.dim).on_mouse_down(
				gpui::MouseButton::Left,
				cx.listener(|this, _, _, cx| this.close_category_sheet(cx)),
			)
		};
		deferred(
			div()
				.absolute()
				.inset_0()
				.child(wash("wash-top", cx).top_0().left_0().w(side).h(px(toolbar::HEIGHT)))
				.child(wash("wash-bottom", cx).bottom_0().left_0().w(side).h(px(status_bar::HEIGHT)))
				.child(
					wash("wash-right", cx)
						.top_0()
						.bottom_0()
						.left(side)
						.right_0()
						.flex()
						.items_center()
						.justify_center()
						.child(
							self
								.sheet_card("category-sheet", 400.0, false, cx)
								.track_focus(&self.reorder_focus)
								.on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
									if event.keystroke.key == "escape" {
										this.close_category_sheet(cx);
									}
								}))
								// The card is not outside: a press on it stays on it.
								.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
								.flex_row()
								.items_center()
								.gap_3()
								.child(icon(Icon::ArrowLeft, p.accent).size_4())
								.child(
									div()
										.flex_1()
										.text_xs()
										.child("Drag the categories in the sidebar into the order you want."),
								),
						),
				),
		)
		.priority(2)
	}

	/// A preset's list: the built-in extensions and the added ones as chips that switch, a field
	/// that adds more, and Reset while anything has been changed. Every change applies and is
	/// written as it is made, like the preset switches themselves.
	fn preset_face(&self, form: &PresetForm, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let id = form.id;
		let Some(category) = self.categories.iter().find(|c| c.id == id) else {
			return deferred(div()).priority(2);
		};
		let Some((preset, overrides)) = category.preset.as_ref() else {
			return deferred(div()).priority(2);
		};
		// A built-in extension switches off and back; an added one is simply dropped.
		let chips: Vec<_> = preset
			.base()
			.into_iter()
			.map(|e| (!overrides.removed.contains(&e), e))
			.chain(overrides.added.iter().map(|e| (true, e.clone())))
			.map(|(on, extension)| {
				let label: SharedString = extension.clone().into();
				let selector = extension.clone();
				div()
					.id(SharedString::from(format!("extension:{extension}")))
					.role(Role::CheckBox)
					.aria_label(format!("Extension: {extension}"))
					.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(move || format!("extension:{selector}"))
					.px_2()
					.py_1()
					.rounded_md()
					.cursor_pointer()
					.text_xs()
					.text_color(if on { p.text } else { p.muted })
					.when(on, |s| s.bg(p.selection))
					.when(!on, move |s| s.line_through().hover(move |s| s.bg(p.hover)))
					.on_click(
						cx.listener(move |this, _, _, cx| this.set_preset_extension(id, &extension, !on, cx)),
					)
					.child(label)
			})
			.collect();
		let changed = !overrides.is_empty();
		let (icon_now, color_now) = (category.icon, category.color);
		deferred(
			backdrop(p).child(
				self
					.sheet_card("category-sheet", 480.0, true, cx)
					.child(self.title_row(preset.name.into(), true, cx))
					// The name alone above; every color it could wear on the line under it.
					.child(self.color_row(color_now, form.custom.clone(), cx))
					.child(section(p.muted, "Extensions"))
					.child(div().flex().flex_wrap().gap_1().children(chips))
					.child(
						div().flex().items_center().gap_3().child(div().flex_1().child(form.add.clone())).when(
							changed,
							|s| {
								s.child(word(
									p,
									"reset",
									"Reset",
									false,
									cx.listener(move |this, _, _, cx| this.reset_preset(id, cx)),
								))
							},
						),
					)
					.child(section(p.muted, "Icon"))
					.child(self.icon_picker(
						icon_now,
						move |this, choice, cx| this.set_category_icon(id, choice, cx),
						cx,
					)),
			),
		)
		.priority(2)
	}

	/// The fourteen glyphs a category may be drawn with, the chosen one lit.
	fn icon_picker(
		&self,
		chosen: Icon,
		on_pick: impl Fn(&mut Rdm, Icon, &mut Context<Rdm>) + Clone + 'static,
		cx: &mut Context<Self>,
	) -> gpui::Div {
		let p = self.palette;
		let icons: Vec<_> = Icon::CATEGORY_CHOICES
			.into_iter()
			.map(|choice| {
				let on = choice == chosen;
				let on_pick = on_pick.clone();
				div()
					.id(SharedString::from(format!("icon:{}", choice.name())))
					.role(Role::RadioButton)
					.aria_label(format!("Icon: {}", choice.name()))
					.aria_selected(on)
					.debug_selector(|| format!("icon:{}", choice.name()))
					.flex()
					.items_center()
					.justify_center()
					.size_7()
					.rounded_sm()
					.cursor_pointer()
					.group("icon-choice")
					.tooltip(tooltip(choice.name()))
					.when(on, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| on_pick(this, choice, cx)))
					.child(
						icon(choice, if on { p.text } else { p.muted })
							.size_4()
							.when(!on, move |s| s.group_hover("icon-choice", move |s| s.text_color(p.text))),
					)
			})
			.collect();
		div().flex().flex_wrap().gap_1().children(icons)
	}

	/// The current color as a dot that opens the picker.
	fn swatch(
		&self,
		id: &'static str,
		color: u32,
		open: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.id(id)
			.role(Role::Button)
			.aria_label("Color")
			.debug_selector(|| "button:Color".to_owned())
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size_6()
			.rounded_sm()
			.cursor_pointer()
			.tooltip(tooltip("Color"))
			.when(open, |s| s.bg(p.selection))
			.when(!open, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(|this, _, _, cx| this.toggle_color_picker(cx)))
			.child(div().size_3p5().rounded_full().bg(p.hue(color)))
	}

	fn custom_face(&self, form: &CategoryForm, cx: &mut Context<Self>) -> gpui::Deferred {
		let p = self.palette;
		let report = self.pattern_report(form, cx);
		let ready = !form.name.read(cx).content.trim().is_empty()
			&& !self.effective_pattern(form, cx).is_empty()
			&& report.is_ok();
		// Only Advanced reports: the basic fields always compile, and a count under them read as
		// noise. The report shrinks and truncates; unbounded, a long engine message pushed the
		// button clean out of the card, where a click on it read as a click outside.
		let verdict = match &report {
			Err(error) => div().text_color(p.failure).child(error.clone()),
			Ok(0) => div().text_color(p.muted).child("Matches none of the current downloads"),
			Ok(n) => div().text_color(p.muted).child(format!("Matches {n} of the current downloads")),
		}
		.flex_1()
		.min_w_0()
		.truncate()
		.text_xs();
		let advanced = form.advanced;
		let combine = form.combine;
		// The two words are a switch: one is always chosen, and it says how the fields combine
		// when both are filled.
		let segment = |which: Combine, label: &'static str, cx: &mut Context<Self>| {
			let on = combine == which;
			div()
				.id(label)
				.role(Role::RadioButton)
				.aria_label(label)
				.aria_selected(on)
				.debug_selector(move || format!("combine:{label}"))
				.px_1p5()
				.py_0p5()
				.rounded_sm()
				.cursor_pointer()
				.text_xs()
				.text_color(if on { p.text } else { p.muted })
				.when(on, |s| s.bg(p.selection))
				.when(!on, move |s| s.hover(move |s| s.text_color(p.text)))
				.on_click(cx.listener(move |this, _, _, cx| this.set_combine(which, cx)))
				.child(label)
		};
		let advanced_word = div()
			.id("advanced")
			.role(Role::Button)
			.aria_label("Advanced")
			.debug_selector(|| "button:Advanced".to_owned())
			// Sized to its words: the rest of the row is not a way to open it.
			.flex()
			.flex_none()
			.items_center()
			.gap_1()
			.text_xs()
			.text_color(p.muted)
			.cursor_pointer()
			.hover(move |s| s.text_color(p.text))
			.on_click(cx.listener(|this, _, window, cx| this.toggle_advanced(Some(window), cx)))
			.child(icon(if advanced { Icon::ChevronDown } else { Icon::ChevronRight }, p.muted).size_3())
			.child("Advanced");
		let create = button(
			p,
			"category-confirm",
			Icon::Plus,
			"Create",
			ready,
			cx.listener(|this, _, _, cx| this.submit_category(cx)),
		);
		deferred(
			backdrop(p).child(
				self
					.sheet_card("category-sheet", 560.0, true, cx)
					.child(self.title_row("New category".into(), true, cx))
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(div().flex_1().min_w_0().child(form.name.clone()))
							.child(self.swatch("color", form.color, form.color_open, cx)),
					)
					.when(form.color_open, |s| s.child(self.color_row(form.color, form.custom.clone(), cx)))
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(div().flex_1().min_w_0().child(form.extensions.clone()))
							.child(
								div()
									.flex()
									.flex_none()
									.gap_0p5()
									.p_0p5()
									.rounded_md()
									.bg(p.track)
									.child(segment(Combine::And, "AND", cx))
									.child(segment(Combine::Or, "OR", cx)),
							)
							.child(div().flex_1().min_w_0().child(form.contains.clone()))
							.child(toggle(
								p,
								"match-case",
								Icon::CaseSensitive,
								"Match case",
								form.match_case,
								cx.listener(|this, _, _, cx| this.toggle_match_case(cx)),
							))
							.child(toggle(
								p,
								"ignore-space",
								Icon::Space,
								"Ignore spaces",
								form.ignore_space,
								cx.listener(|this, _, _, cx| this.toggle_ignore_space(cx)),
							)),
					)
					.child(self.icon_picker(
						form.icon,
						|this, choice, cx| this.choose_category_icon(choice, cx),
						cx,
					))
					// Closed, Advanced and Create share a line. Open, the pattern unfolds between
					// them and Create goes to the bottom with the report.
					.map(|s| {
						if advanced {
							s.child(div().flex().child(advanced_word))
								.child(form.pattern.clone())
								.child(section(p.muted, ADVANCED_HINT))
								.child(div().flex().items_center().justify_between().child(verdict).child(create))
						} else {
							s.child(
								div().flex().items_center().justify_between().child(advanced_word).child(create),
							)
						}
					}),
			),
		)
		.priority(2)
	}

	/// One line of every color a category could wear: the nine named hues, then a field for one
	/// of the user's own -- hex, rgb() or hsl(), the placeholder showing each -- with a dot after
	/// it that previews what is typed and, once it reads as a color, is a swatch like the others.
	/// The field fills what the hues leave, so the line is the card's width.
	fn color_row(
		&self,
		current: u32,
		custom: gpui::Entity<TextInput>,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let swatches: Vec<_> = Tint::CYCLE
			.into_iter()
			.map(|tint| {
				let color = tint.rgb();
				let on = color == current;
				let name = format_hex(color);
				let selector = name.clone();
				div()
					.id(SharedString::from(format!("swatch:{name}")))
					.role(Role::RadioButton)
					.aria_label(format!("Color {name}"))
					.aria_selected(on)
					.debug_selector(move || format!("swatch:{selector}"))
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size_6()
					.rounded_sm()
					.cursor_pointer()
					.when(on, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| this.choose_color(color, cx)))
					.child(div().size_3p5().rounded_full().bg(p.hue(color)))
			})
			.collect();
		let typed = custom.read(cx).content.trim().to_owned();
		let parsed = parse_color(&typed);
		let on = parsed == Some(current);
		let preview = div()
			.id("swatch-custom")
			.role(Role::RadioButton)
			.aria_label("Your color")
			.aria_selected(on)
			.debug_selector(|| "swatch:custom".to_owned())
			.flex()
			.flex_none()
			.items_center()
			.justify_center()
			.size_6()
			.rounded_sm()
			.tooltip(tooltip(if parsed.is_some() { "Your color" } else { "Not a color yet" }))
			.when(on, |s| s.bg(p.selection))
			.when_some(parsed, |s, color| {
				s.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
					// Chosen from the dot, the text is kept with the category as if entered.
					match &this.category_sheet {
						Some(CategorySheet::Preset(form)) => {
							let (id, text) = (form.id, typed.clone());
							this.set_category_custom_color(id, &text, cx);
						}
						_ => this.choose_color(color, cx),
					}
				}))
			})
			.child(match parsed {
				Some(color) => div().size_3p5().rounded_full().bg(p.hue(color)),
				None => div().size_3p5().rounded_full().border_1().border_color(p.border),
			});
		let guide = div()
			.debug_selector(|| "color-guide".to_owned())
			.flex()
			.flex_col()
			.gap_0p5()
			.px_1()
			.text_xs()
			.text_color(p.muted)
			.children(COLOR_GUIDE.iter().map(|line| div().child(*line)));
		let row = div()
			.flex()
			.items_center()
			.gap_1()
			.children(swatches)
			.child(div().flex_1().min_w_0().ml_1().child(custom))
			.child(preview)
			// A question mark after the field: the rule on hover, the whole guide on a press.
			.child(
				div()
					.id("color-help")
					.role(Role::Button)
					.aria_label("Color formats")
					.debug_selector(|| "button:Color formats".to_owned())
					.flex()
					.flex_none()
					.items_center()
					.justify_center()
					.size_6()
					.rounded_sm()
					.cursor_pointer()
					.group("color-help")
					.tooltip(tooltip(COLOR_RULE))
					.when(self.color_help, |s| s.bg(p.selection))
					.on_click(cx.listener(|this, _, _, cx| this.toggle_color_help(cx)))
					.child(
						icon(Icon::CircleQuestion, if self.color_help { p.text } else { p.muted })
							.size_4()
							.when(!self.color_help, move |s| {
								s.group_hover("color-help", move |s| s.text_color(p.text))
							}),
					),
			);
		div().flex().flex_col().gap_1p5().child(row).when(self.color_help, |s| s.child(guide))
	}
}

/// The wash over the whole window that the presets, preset and custom faces sit on.
fn backdrop(p: Palette) -> gpui::Div {
	div().absolute().inset_0().occlude().flex().items_center().justify_center().bg(p.dim)
}

fn section(color: gpui::Hsla, title: &'static str) -> impl IntoElement {
	div().text_xs().text_color(color).child(title)
}

/// A word that acts: Edit, Reorder, Reset. It brightens on hover and stays bright while it names
/// a mode that is on.
fn word(
	p: Palette,
	id: &'static str,
	label: &'static str,
	on: bool,
	on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
	div()
		.id(id)
		.role(Role::Button)
		.aria_label(label)
		.debug_selector(move || format!("button:{label}"))
		.text_xs()
		.text_color(if on { p.text } else { p.muted })
		.cursor_pointer()
		.hover(move |s| s.text_color(p.text))
		.on_click(on_click)
		.child(label)
}

/// An icon that is a switch: lit and backed while on, muted while off.
fn toggle(
	p: Palette,
	id: &'static str,
	glyph: Icon,
	label: &'static str,
	on: bool,
	on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
	div()
		.id(id)
		.role(Role::CheckBox)
		.aria_label(label)
		.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
		.debug_selector(move || format!("toggle:{label}"))
		.flex()
		.flex_none()
		.items_center()
		.justify_center()
		.size_6()
		.rounded_sm()
		.cursor_pointer()
		.group("toggle")
		.tooltip(tooltip(label))
		.when(on, |s| s.bg(p.selection))
		.on_click(on_click)
		.child(
			icon(glyph, if on { p.text } else { p.muted })
				.size_4()
				.when(!on, move |s| s.group_hover("toggle", move |s| s.text_color(p.text))),
		)
}
