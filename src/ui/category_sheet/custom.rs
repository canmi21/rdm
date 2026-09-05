//! The custom form: a name with its swatch, extensions and text the name contains with the
//! switches between them, the icon, and under Advanced the regular expression itself.

use gpui::{Context, Role, deferred, div, prelude::*};

use crate::app::Rdm;
use crate::category::Combine;
use crate::ui::backdrop;
use crate::ui::button;
use crate::ui::category_sheet::{CategoryForm, section, toggle};
use crate::ui::icon::{Icon, icon};

/// Under the pattern field.
const ADVANCED_HINT: &str = "Enter a regular expression to match against whole file names.";

impl Rdm {
	pub(super) fn custom_face(&self, form: &CategoryForm, cx: &mut Context<Self>) -> gpui::Deferred {
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
}
