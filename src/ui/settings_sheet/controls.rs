//! One row drawn with its control -- a switch, a choice, a field or a button -- and how a choice
//! decides between segments and a dropdown.

use super::*;

impl Rdm {
	/// One setting: its name on the left, and on the right the value it has or the switch that
	/// changes it. The switch is a track with a knob, lit while on.
	pub(super) fn setting_row(
		&self,
		p: crate::ui::theme::Palette,
		row: &Row,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let label = row.label;
		// A choice is drawn one of two ways and its words decide which: see `segments_fit`.
		let dropdown = match &row.control {
			Control::Choice { options, .. } => !segments_fit(options),
			_ => false,
		};
		let open = self.settings.as_ref().is_some_and(|sheet| sheet.menu == Some(label));
		let right = match &row.control {
			Control::Value(value) => {
				div().text_color(p.muted).truncate().child(value.clone()).into_any_element()
			}
			Control::Switch { on, set } => {
				let (on, set) = (*on, *set);
				div()
					.id(SharedString::from(format!("switch:{label}")))
					.role(Role::CheckBox)
					.aria_label(label)
					.aria_toggled(if on { gpui::Toggled::True } else { gpui::Toggled::False })
					.debug_selector(move || format!("switch:{label}"))
					.flex()
					.items_center()
					.w(px(30.0))
					.h(px(18.0))
					.p_px()
					.rounded_full()
					.cursor_pointer()
					.leaves_focus()
					.bg(if on { p.accent } else { p.track })
					.when(!on, |s| s.justify_start())
					.when(on, |s| s.justify_end())
					.on_click(cx.listener(move |this, _, _, cx| set(this, !on, cx)))
					.child(div().size(px(14.0)).rounded_full().bg(p.text))
					.into_any_element()
			}
			// One word and a chevron, the options in a panel hung off the button's bottom edge.
			// A long set has no other shape: side by side its words run off the pane, and the
			// fourth disguise was drawn where nothing could reach it. See `segments_fit`.
			Control::Choice { options, chosen, set } if dropdown => {
				let shown = options.get(*chosen).copied().unwrap_or_default();
				let (chosen, set) = (*chosen, *set);
				div()
					.id(SharedString::from(format!("choice:{label}")))
					.role(Role::Button)
					.aria_label(label)
					.debug_selector(move || format!("choice:{label}"))
					.flex()
					.items_center()
					.justify_between()
					.gap_2()
					.w(px(200.0))
					.flex_none()
					.px_2()
					.py_0p5()
					.rounded_sm()
					.border_1()
					.border_color(if open { p.accent } else { p.border })
					.bg(p.track)
					.cursor_pointer()
					.leaves_focus()
					// The panel is placed against this, so the button is a positioned box even
					// while nothing is open. Where the press landed within it does not come into
					// it: a menu belongs to the control, not to the pointer, and an activation
					// with no pointer behind it -- the keyboard, the control socket -- opens the
					// same menu in the same place.
					.relative()
					.on_click(cx.listener(move |this, _, _, cx| this.toggle_settings_menu(label, cx)))
					.child(div().min_w_0().truncate().child(shown))
					.child(icon(Icon::ChevronDown, p.muted).size_3())
					.when(open, |s| s.child(self.choice_menu(p, label, options, chosen, set, cx)))
					.into_any_element()
			}
			// A segmented control: one track with the segments inside it, so the alternatives read
			// as one control offering a choice rather than a button and some loose words, which
			// is what lit and unlit text with nothing around it read as. It wraps, so a set that
			// is a little too wide costs a second line rather than a menu.
			Control::Choice { options, chosen, set } => {
				let (chosen, set) = (*chosen, *set);
				div()
					.flex()
					.flex_wrap()
					.items_center()
					.p_px()
					.gap_px()
					.rounded_md()
					.bg(p.track)
					.children(options.iter().enumerate().map(|(index, option)| {
						let on = index == chosen;
						div()
							.id(SharedString::from(format!("choice:{label}:{option}")))
							.role(Role::RadioButton)
							.aria_label(*option)
							.aria_selected(on)
							.debug_selector(move || format!("choice:{option}"))
							.px_2()
							.py_0p5()
							.rounded_sm()
							.cursor_pointer()
							.leaves_focus()
							.text_color(if on { p.text } else { p.muted })
							.when(on, |s| s.bg(p.selection))
							.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
							.on_click(cx.listener(move |this, _, _, cx| set(this, index, cx)))
							.child(*option)
					}))
					.into_any_element()
			}
			Control::Field { input } => {
				div().w(px(132.0)).flex_none().child(input.clone()).into_any_element()
			}
			// The status wraps rather than truncating, up to the width below. What it says is the
			// whole point of the row -- which build was found, or why the check could not be
			// read -- and a sentence cut at `2026.9.6 (102) is the la` has answered nothing. The
			// word beside it keeps to the first line, so the row still reads as one action.
			Control::Action { word, note, run } => {
				let (word, run) = (*word, *run);
				div()
					.flex()
					.items_start()
					.gap_3()
					.min_w_0()
					.child(div().max_w(px(240.0)).text_color(p.muted).child(note.clone()))
					.child(
						div()
							.id(SharedString::from(format!("action:{label}")))
							.role(Role::Button)
							.aria_label(word)
							.debug_selector(move || format!("button:{word}"))
							.flex_none()
							.px_2()
							.py_0p5()
							.rounded_sm()
							.text_color(p.accent)
							.cursor_pointer()
							.leaves_focus()
							.hover(move |s| s.bg(p.hover))
							.on_click(cx.listener(move |this, _, _, cx| run(this, cx)))
							.child(word),
					)
					.into_any_element()
			}
		};
		// A segmented control is as wide as all of its words at once and does not fit beside a
		// label, so it goes under one. A dropdown is one word and a chevron and stays on the line
		// with everything else.
		// A segmented control is as wide as all of its words and goes under the label; a dropdown
		// is one word and a chevron and stays on the line with everything else.
		let stacked = matches!(row.control, Control::Choice { .. }) && !dropdown;
		let note = crate::i18n::t(row.note);
		// An action sizes itself: its status wraps within its own ceiling, so capping and
		// truncating the whole thing here would undo the wrapping a line below.
		let fixed = matches!(
			row.control,
			Control::Switch { .. } | Control::Field { .. } | Control::Action { .. }
		) || dropdown;
		let title = crate::i18n::t(row.title.unwrap_or(row.label));
		let line = div()
			.flex()
			.when(!stacked, |s| s.justify_between().items_center().gap_4())
			.when(stacked, |s| s.flex_col().items_start().gap_1p5())
			// The label gives way and the control does not: a control clipped to nothing is a
			// control that cannot be pressed, which is what happened when these were the other way
			// round and the switches stopped answering.
			.child(div().when(!stacked, |s| s.flex_1().min_w_0()).truncate().child(title))
			// A switch, a field and a dropdown are the size they are; a value or a status is as
			// long as it happens to be, and one of those given its natural width pushes the label
			// out of the row -- so beside a label it is capped and truncates instead.
			//
			// The cap is for sharing a line and nothing else. A segmented control has the line to
			// itself, and holding it to six tenths of one cut `No proxy` to `No`.
			.child(
				div()
					.when(fixed || stacked, |s| s.flex_none())
					.when(!fixed && !stacked, |s| s.min_w_0().max_w(gpui::relative(0.6)).truncate())
					.child(right),
			);
		div()
			.debug_selector(move || format!("setting:{label}"))
			.flex()
			.flex_col()
			.gap_1()
			.py_1p5()
			.child(line)
			// The note runs the whole width under the row rather than beside the label, which is
			// the only place it fits: a sentence given the label's column wraps into a gutter,
			// and given the control's it is cut at four words. See spec/ui.md.
			.when(!note.is_empty(), |s| s.child(div().text_xs().text_color(p.muted).child(note)))
	}
}

/// Whether a choice's options can be drawn side by side as a segmented control, or want a
/// dropdown instead. What decides is how much room the words ask for, measured in the columns
/// they draw in rather than in characters: a CJK glyph is twice the width of a Latin one, so
/// `简体中文` is four characters and eight columns, and counting characters would call the
/// Japanese and Chinese windows narrow when they are not.
///
/// It is a count and not a measurement because a width can only be had after the frame it would
/// decide, and a control that changed shape one frame late would flicker on every language
/// change. Five options are a dropdown whatever they say, a row of five being a list.
pub(super) fn segments_fit(options: &[&str]) -> bool {
	let columns: usize = options
		.iter()
		.map(|option| option.chars().map(|c| if wide(c) { 2 } else { 1 }).sum::<usize>())
		.sum();
	options.len() <= 4 && columns <= 52
}

/// The ranges a font draws at two columns: the CJK blocks, the kana, Hangul and the full-width
/// forms. Enough for the three languages the window is read in.
pub(super) fn wide(c: char) -> bool {
	matches!(c as u32,
		0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
		| 0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD)
}

/// A heading within a section: smaller than the section's own and set off above the rows it
/// gathers, so the eye can skip a group whole rather than reading every label in it.
pub(super) fn group_title(p: crate::ui::theme::Palette, name: &'static str) -> gpui::Div {
	div()
		.debug_selector(move || format!("group:{name}"))
		.pt_3()
		.pb_0p5()
		.text_xs()
		.text_color(p.muted)
		.child(name)
}

pub(super) fn section_title(p: crate::ui::theme::Palette, name: &'static str) -> gpui::Div {
	div().text_xs().text_color(p.muted).pb_1().child(name)
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The two sets the rule was written between: the languages, which fit and read better as
	/// segments, and the disguises, whose fourth option ran off the pane.
	#[test]
	fn a_choice_is_segments_while_its_words_fit_and_a_dropdown_after_that() {
		assert!(segments_fit(&["System", "English", "简体中文", "日本語"]));
		assert!(!segments_fit(&[
			"This application",
			"Chrome on Windows",
			"Chrome on Linux",
			"Something else",
		]));
		// The widest set that still fits, and the one the budget was measured against.
		assert!(segments_fit(&["Ignore them", "Show what is inside", "Keep them as folders"]));
	}

	/// A CJK glyph draws in two columns, so the same sentence is half as many characters and the
	/// same width. Counting characters would have called this set narrow and drawn it off the pane.
	#[test]
	fn a_cjk_glyph_counts_as_the_two_columns_it_draws_in() {
		assert_eq!("简体中文".chars().count(), 4);
		assert!(wide('简') && wide('日') && wide('ア') && wide('한'));
		assert!(!wide('a') && !wide('/') && !wide('1'));
		let latin = ["aaaaaaaaa", "aaaaaaaaa", "aaaaaaaaa"];
		let cjk = ["简体中文简体中文简", "简体中文简体中文简", "简体中文简体中文简"];
		assert!(segments_fit(&latin));
		assert!(!segments_fit(&cjk));
	}

	/// However short they are: a row of five words is a list, and a list gets a list's shape.
	#[test]
	fn five_options_are_a_dropdown_whatever_they_say() {
		assert!(segments_fit(&["a", "b", "c", "d"]));
		assert!(!segments_fit(&["a", "b", "c", "d", "e"]));
	}
}
