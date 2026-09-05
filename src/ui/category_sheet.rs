//! A new category: its name, an icon from a short list, and a pattern over file names.

use gpui::{Context, IntoElement, Role, SharedString, Window, deferred, div, prelude::*, px};

use crate::app::{CategoryForm, Rdm};
use crate::download::Category;
use crate::ui::icon::{Icon, icon};
use crate::ui::text_input::TextInput;
use crate::ui::{button, icon_button};

impl Rdm {
	pub(crate) fn open_category_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		if self.category_form.is_none() {
			let rdm = cx.entity();
			let cancel = rdm.clone();
			let name = cx.new(|cx| {
				TextInput::new("Name", cx)
					.on_cancel(move |_, cx| cancel.update(cx, |this, cx| this.close_category_form(cx)))
			});
			let cancel = rdm.clone();
			let pattern = cx.new(|cx| {
				TextInput::new(r"(?i)\.(rs|py|ts)$", cx)
					.on_confirm(move |_, _, cx| rdm.update(cx, |this, cx| this.submit_category(cx)))
					.on_cancel(move |_, cx| cancel.update(cx, |this, cx| this.close_category_form(cx)))
			});
			self.category_form = Some(CategoryForm { name, pattern, icon: Icon::Code });
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

	pub(crate) fn submit_category(&mut self, cx: &mut Context<Self>) {
		let Some(form) = &self.category_form else { return };
		let name = form.name.read(cx).content.to_string();
		let pattern = form.pattern.read(cx).content.to_string();
		if self.add_category(&name, form.icon, &pattern, cx).is_ok() {
			self.close_category_form(cx);
		}
	}

	/// What the pattern as typed would do right now: an error from the engine, or how many of the
	/// current downloads it catches. Shown as it is typed, so a rule is checked before it exists.
	fn pattern_report(&self, form: &CategoryForm, cx: &Context<Self>) -> Result<usize, String> {
		let pattern = form.pattern.read(cx).content.to_string();
		if pattern.trim().is_empty() {
			return Ok(0);
		}
		let probe = Category::new(0, "probe", form.icon, pattern.trim())?;
		Ok(self.downloads.iter().filter(|d| probe.matches(d)).count())
	}

	pub(crate) fn category_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(form) = &self.category_form else {
			return deferred(div()).priority(2);
		};
		let report = self.pattern_report(form, cx);
		let ready = !form.name.read(cx).content.trim().is_empty()
			&& !form.pattern.read(cx).content.trim().is_empty()
			&& report.is_ok();
		let chosen = form.icon;
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
		let verdict = match &report {
			Err(error) => div().text_xs().text_color(p.failure).child(error.clone()),
			Ok(0) => div().text_xs().text_color(p.muted).child("Matches none of the current downloads"),
			Ok(n) => {
				div().text_xs().text_color(p.muted).child(format!("Matches {n} of the current downloads"))
			}
		};
		deferred(
			div().absolute().inset_0().flex().items_center().justify_center().bg(p.dim).child(
				div()
					.id("category-sheet")
					.flex()
					.flex_col()
					.gap_3()
					.w(px(440.0))
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
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("New category"))
							.child(icon_button(
								p,
								"category-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_category_form(cx)),
							)),
					)
					.child(field(p.muted, "Name", form.name.clone()))
					.child(
						div()
							.flex()
							.flex_col()
							.gap_1()
							.child(div().text_xs().text_color(p.muted).child("Icon"))
							.child(div().flex().flex_wrap().gap_1().children(icons)),
					)
					.child(field(
						p.muted,
						"Pattern, a regular expression over the file name",
						form.pattern.clone(),
					))
					.child(verdict)
					.child(div().flex().justify_end().child(button(
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

fn field(
	label: gpui::Hsla,
	name: &'static str,
	input: gpui::Entity<TextInput>,
) -> impl IntoElement {
	div().flex().flex_col().gap_1().child(div().text_xs().text_color(label).child(name)).child(input)
}
