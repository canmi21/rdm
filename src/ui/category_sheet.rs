//! A new category, three ways: a preset switched on, a custom one from a name, an icon and a list
//! of extensions, or -- under Advanced -- a regular expression written out.

use gpui::{Context, IntoElement, Role, SharedString, Window, deferred, div, prelude::*, px};

use crate::app::{CategoryForm, Rdm};
use crate::download::{Category, pattern_for_extensions};
use crate::ui::icon::{Icon, icon};
use crate::ui::text_input::TextInput;
use crate::ui::{button, icon_button};

impl Rdm {
	pub(crate) fn open_category_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.category_form.is_none() {
			let rdm = cx.entity();
			let close = |rdm: &gpui::Entity<Rdm>| {
				let rdm = rdm.clone();
				move |_: &mut Window, cx: &mut gpui::App| {
					rdm.update(cx, |this, cx| this.close_category_form(cx))
				}
			};
			let submit = |rdm: &gpui::Entity<Rdm>| {
				let rdm = rdm.clone();
				move |_: &str, _: &mut Window, cx: &mut gpui::App| {
					rdm.update(cx, |this, cx| this.submit_category(cx))
				}
			};
			let name =
				cx.new(|cx| TextInput::new("Name", cx).on_cancel(close(&rdm)).on_confirm(submit(&rdm)));
			let extensions = cx
				.new(|cx| TextInput::new("rs, py, ts", cx).on_cancel(close(&rdm)).on_confirm(submit(&rdm)));
			let pattern =
				cx.new(|cx| TextInput::new("", cx).on_cancel(close(&rdm)).on_confirm(submit(&rdm)));
			self.category_form =
				Some(CategoryForm { name, extensions, pattern, icon: Icon::Code, advanced: false });
		}
		if let Some(form) = &self.category_form {
			window.focus(&form.name.read(cx).focus(), cx);
		}
		cx.notify();
	}

	pub(crate) fn close_category_form(&mut self, cx: &mut Context<Self>) {
		self.category_form = None;
		cx.notify();
	}

	pub(crate) fn choose_category_icon(&mut self, icon: Icon, cx: &mut Context<Self>) {
		if let Some(form) = &mut self.category_form {
			form.icon = icon;
			cx.notify();
		}
	}

	/// Opening Advanced shows the pattern the extensions stand for, editable from there on; the
	/// extensions field is then only a way to start over.
	pub(crate) fn toggle_advanced(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(form) = &self.category_form else { return };
		let advanced = !form.advanced;
		let derived = pattern_for_extensions(&form.extensions.read(cx).content);
		let pattern = form.pattern.clone();
		if advanced && pattern.read(cx).content.is_empty() {
			pattern.update(cx, |input, cx| input.set_content(&derived, cx));
		}
		if let Some(form) = &mut self.category_form {
			form.advanced = advanced;
		}
		if advanced {
			window.focus(&pattern.read(cx).focus(), cx);
		}
		cx.notify();
	}

	/// The pattern that would run: the written one under Advanced, the extensions' otherwise.
	fn effective_pattern(&self, form: &CategoryForm, cx: &Context<Self>) -> String {
		if form.advanced {
			form.pattern.read(cx).content.trim().to_owned()
		} else {
			pattern_for_extensions(&form.extensions.read(cx).content)
		}
	}

	pub(crate) fn submit_category(&mut self, cx: &mut Context<Self>) {
		let Some(form) = &self.category_form else { return };
		let name = form.name.read(cx).content.to_string();
		let pattern = self.effective_pattern(form, cx);
		if self.add_category(&name, form.icon, &pattern, cx).is_ok() {
			self.close_category_form(cx);
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

	pub(crate) fn category_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(form) = &self.category_form else {
			return deferred(div()).priority(2);
		};
		let report = self.pattern_report(form, cx);
		let ready = !form.name.read(cx).content.trim().is_empty()
			&& !self.effective_pattern(form, cx).is_empty()
			&& report.is_ok();
		let chosen = form.icon;
		let presets: Vec<_> = Category::PRESETS
			.iter()
			.map(|(name, glyph, _)| {
				let name: &'static str = name;
				let on = self.categories.iter().any(|c| c.name == name);
				div()
					.id(SharedString::from(format!("preset:{name}")))
					.role(Role::CheckBox)
					.aria_label(format!("Preset: {name}"))
					.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(|| format!("preset:{name}"))
					.flex()
					.items_center()
					.gap_1p5()
					.px_2()
					.py_1()
					.rounded_md()
					.cursor_pointer()
					.text_xs()
					.text_color(if on { p.text } else { p.muted })
					.when(on, |s| s.bg(p.selection))
					.when(!on, move |s| s.hover(move |s| s.bg(p.hover)))
					.on_click(cx.listener(move |this, _, _, cx| this.toggle_preset(name, cx)))
					.child(icon(*glyph, if on { p.text } else { p.muted }).size_3p5())
					.child(name)
			})
			.collect();
		let icons: Vec<_> = Icon::CATEGORY_CHOICES
			.into_iter()
			.map(|choice| {
				let on = choice == chosen;
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
					.when(on, |s| s.bg(p.selection))
					.on_click(cx.listener(move |this, _, _, cx| this.choose_category_icon(choice, cx)))
					.child(
						icon(choice, if on { p.text } else { p.muted })
							.size_4()
							.when(!on, move |s| s.group_hover("icon-choice", move |s| s.text_color(p.text))),
					)
			})
			.collect();
		// The report shrinks and truncates; unbounded, a long engine message pushed the Add button
		// clean out of the card, where a click on it read as a click outside.
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
		deferred(
			div().absolute().inset_0().flex().items_center().justify_center().bg(p.dim).child(
				div()
					.id("category-sheet")
					.debug_selector(|| "category-sheet".to_owned())
					.flex()
					.flex_col()
					.gap_3()
					.w(px(480.0))
					.p_4()
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.bg(p.panel)
					.shadow_lg()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_category_form(cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Categories"))
							.child(icon_button(
								p,
								"category-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_category_form(cx)),
							)),
					)
					.child(section(p.muted, "Presets"))
					.child(div().flex().flex_wrap().gap_1().children(presets))
					.child(section(p.muted, "Custom"))
					.child(
						div()
							.flex()
							.gap_2()
							.child(div().flex_1().child(form.name.clone()))
							.child(div().flex_1().child(form.extensions.clone())),
					)
					.child(div().flex().flex_wrap().gap_1().children(icons))
					.child(
						div()
							.id("advanced")
							.role(Role::Button)
							.aria_label("Advanced")
							.debug_selector(|| "button:Advanced".to_owned())
							.flex()
							.items_center()
							.gap_1()
							.text_xs()
							.text_color(p.muted)
							.cursor_pointer()
							.hover(move |s| s.text_color(p.text))
							.on_click(cx.listener(|this, _, window, cx| this.toggle_advanced(window, cx)))
							.child(
								icon(if advanced { Icon::ChevronDown } else { Icon::ChevronRight }, p.muted)
									.size_3(),
							)
							.child("Advanced: a regular expression over the file name"),
					)
					.when(advanced, |s| s.child(form.pattern.clone()))
					.child(div().flex().items_center().justify_between().child(verdict).child(button(
						p,
						"category-confirm",
						Icon::Plus,
						"Add",
						ready,
						cx.listener(|this, _, _, cx| this.submit_category(cx)),
					))),
			),
		)
		.priority(2)
	}
}

fn section(color: gpui::Hsla, title: &'static str) -> impl IntoElement {
	div().text_xs().text_color(color).child(title)
}
