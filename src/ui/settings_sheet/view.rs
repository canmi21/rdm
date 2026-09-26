//! The sheet drawn: its header, the rail of sections, the pane of rows and the menus a row opens.

use super::*;

impl Rdm {
	pub(crate) fn settings_body(
		&self,
		p: crate::ui::theme::Palette,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let Some(sheet) = &self.settings else { return div().into_any_element() };
		let query = sheet.search.read(cx).content.trim().to_lowercase();
		let searching = !query.is_empty();
		let rows = self.settings_rows();

		// The rail: the search field, then one row per section, lit while it is the one shown.
		// While a search is on, no section is lit, since the pane shows every section's matches.
		let sections = Section::ALL.into_iter().map(|section| {
			let on = !searching && sheet.section == section;
			let name = section.name();
			div()
				.id(SharedString::from(format!("settings-section:{name}")))
				.role(Role::Tab)
				.aria_label(format!("Settings: {name}"))
				.aria_selected(on)
				.debug_selector(move || format!("section:{name}"))
				.flex()
				.items_center()
				.gap_2()
				.px_2()
				.py_1()
				.rounded_sm()
				.cursor_pointer()
				.group("settings-section")
				.text_color(if on { p.text } else { p.muted })
				.when(on, |s| s.bg(p.selection))
				.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
				.on_click(cx.listener(move |this, _, _, cx| this.set_settings_section(section, cx)))
				.child(
					hover_icon(
						section.icon(),
						"settings-section",
						if on { p.text } else { p.muted },
						(!on).then_some(p.text),
					)
					.size_3p5(),
				)
				.child(name)
		});
		let rail = div()
			.flex()
			.flex_col()
			.gap_0p5()
			.w(px(176.0))
			.flex_none()
			.p_2()
			.border_r_1()
			.border_color(p.border)
			.child(div().mb_1p5().child(sheet.search.clone()))
			.children(sections);

		// The pane: the section's rows under its name, or every match under each section's name.
		let auto = self.preferences.auto_update;
		let mut shown: Vec<&Row> = rows
			.iter()
			.filter(|row| auto || row.label != "settings.label.when_a_build_is_found")
			.filter(|row| {
				if searching {
					// A search reads what is on screen -- the label, the line under it and the
					// heading -- rather than the keys behind them: somebody looking for "proxy"
					// is looking for what a setting does, and in the language they are reading.
					let seen = |key: &str| crate::i18n::t(key).to_lowercase();
					seen(row.label).contains(&query)
						|| row.title.is_some_and(|title| seen(title).contains(&query))
						|| seen(row.note).contains(&query)
						|| seen(row.group).contains(&query)
				} else {
					row.section == sheet.section
				}
			})
			.collect();
		// Rows of one group are gathered together, in the order their groups first appear. The
		// heading is emitted when the group changes, so a group split in two by a row from
		// another gets its heading twice -- which it did, and read as two lists of the same name.
		if !searching {
			let mut order: Vec<&'static str> = Vec::new();
			for row in &shown {
				if !order.contains(&row.group) {
					order.push(row.group);
				}
			}
			shown.sort_by_key(|row| order.iter().position(|g| *g == row.group).unwrap_or(0));
		}
		let complaint = sheet.complaint.clone();
		// The pane scrolls. A row is a label, a line saying what it does and a control, and a
		// section of a dozen of those is taller than the sheet; without this the rows past the
		// bottom were drawn outside it, where a press reaches the backdrop and closes the sheet.
		let mut pane = div()
			.id("settings-pane")
			.flex()
			.flex_col()
			.flex_1()
			.min_w_0()
			// Without this the pane is as tall as its rows and grows past the sheet, whatever the
			// sheet's own height says: a flex child does not shrink below its content unless it
			// is told it may.
			.min_h_0()
			.overflow_y_scroll()
			.p_4()
			.gap_1();
		if searching && shown.is_empty() {
			pane = pane.child(div().text_color(p.muted).child(format!("Nothing matches \"{query}\"")));
		} else if searching {
			let mut last: Option<Section> = None;
			for row in shown {
				if last != Some(row.section) {
					let first = last.is_none();
					last = Some(row.section);
					pane = pane.child(section_title(p, row.section.name()).when(!first, |s| s.mt_2()));
				}
				pane = pane.child(self.setting_row(p, row, cx));
			}
		} else {
			// No section title here: the rail two inches to the left already shows which section
			// is open, lit, and the word repeated at the top of the pane was the same answer to a
			// question nobody had asked twice. It stays under a search, where the rail is lit by
			// nothing and the section is the only thing saying where a match came from.
			//
			// The headings within a section, emitted as the rows walk past them: a dozen rows in
			// one run is a list to read, and three short lists is a page to use.
			let mut group: Option<&'static str> = None;
			for row in shown {
				if group != Some(row.group) && !row.group.is_empty() {
					pane = pane.child(group_title(p, crate::i18n::t(row.group)));
				}
				group = Some(row.group);
				pane = pane.child(self.setting_row(p, row, cx));
				if let Some((label, message)) = &complaint
					&& *label == row.label
				{
					pane = pane.child(
						div()
							.text_xs()
							.text_color(p.failure)
							.debug_selector(|| "settings-complaint".to_owned())
							.child(message.clone()),
					);
				}
			}
		}

		// The card, drawn the same whether it is a sheet in the main window or a window of its
		// own: its own strip at the top, the rail and the pane under it. The frame around it is
		// the only difference between the two, which is what makes dragging it out move nothing
		// but the frame.
		div()
			.id("settings-sheet")
			.debug_selector(|| "settings-sheet".to_owned())
			.flex()
			.flex_col()
			.size_full()
			.overflow_hidden()
			// Zed's density, the same as the main window's: this is the same application and not
			// a dialog with a face of its own.
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.child(self.settings_header(p, cx))
			.child(div().flex().flex_1().min_h_0().child(rail).child(pane))
			.into_any_element()
	}

	/// The options of an open dropdown, hung off the button they belong to. The wrapper covers
	/// the button exactly -- absolute, so it is out of the button's own flow and costs it no
	/// room -- and the panel is anchored a hundred percent down it, which is the button's bottom
	/// edge whatever the row turned out to be. Anchoring locally is what keeps the two together:
	/// the panel is laid out inside the pane like everything else, so the pane's scroll and the
	/// card's centring move it exactly as they move the button, and there is no point to carry
	/// on the sheet and go stale. Deferred above the sheet, which is itself deferred, or the card
	/// would paint over it; a deferred draw carries no clip, so the panel is free to fall past
	/// the pane's bottom edge.
	///
	/// A hundred percent of an element is its padding box, so the point of border the wrapper
	/// sits inside is given back on both axes, and four more points below the button leave the
	/// gap. `snap_to_window_with_margin` is what makes it usable near an edge: GPUI measures the
	/// panel and slides it to fit, so a row at the bottom of the card opens upward without
	/// anything here having to work out which way there is room.
	pub(super) fn choice_menu(
		&self,
		p: crate::ui::theme::Palette,
		label: &'static str,
		options: &[&'static str],
		chosen: usize,
		set: fn(&mut Rdm, usize, &mut Context<Rdm>),
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let panel = floating(p, SharedString::from(format!("menu:{label}")))
			.debug_selector(move || format!("menu:{label}"))
			.flex()
			.flex_col()
			.gap_px()
			.w(px(200.0))
			.p_1()
			// A press anywhere else closes it -- except on the button it belongs to, which would
			// otherwise close it here and open it again in the same press, and read as a menu
			// that will not shut. The button marks its press the way every control that keeps the
			// keyboard does, which is what `default_prevented` reports; `backdrop` reads the same
			// flag for the same reason. See src/ui/mod.rs.
			.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_settings_menu(cx)))
			.children(options.iter().enumerate().map(|(index, option)| {
				let on = index == chosen;
				let option = *option;
				div()
					.id(SharedString::from(format!("option:{label}:{option}")))
					.role(Role::RadioButton)
					.aria_label(option)
					.aria_selected(on)
					.debug_selector(move || format!("choice:{option}"))
					.flex()
					.items_center()
					.px_1p5()
					.py_0p5()
					.rounded_sm()
					.cursor_pointer()
					.leaves_focus()
					.text_color(if on { p.text } else { p.muted })
					.when(on, |s| s.bg(p.selection))
					.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
					.on_click(cx.listener(move |this, _, _, cx| {
						set(this, index, cx);
						this.close_settings_menu(cx);
					}))
					.child(option)
			}));
		div().absolute().inset_0().child(self.menu_watch(cx)).child(
			div().absolute().left_0().top(relative(1.0)).w_0().h_0().child(
				deferred(
					anchored()
						.position_mode(gpui::AnchoredPositionMode::Local)
						.anchor(gpui::Anchor::TopLeft)
						.offset(point(px(-1.0), px(5.0)))
						.snap_to_window_with_margin(px(8.0))
						.child(panel),
				)
				.priority(3),
			),
		)
	}

	/// The menu is glued to its button, and the button rides the pane's scroll. When the button
	/// leaves what the pane shows, the menu leaves with it rather than hanging over the card:
	/// this reads the pane's own clip where it is in force -- at prepaint, inside the pane's
	/// subtree -- and closes the menu the frame its button is no longer within it. The closing is
	/// deferred, a frame being laid out being no place to change what it says.
	pub(super) fn menu_watch(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let sheet = cx.entity().downgrade();
		canvas(
			move |bounds, window, cx| {
				if !window.content_mask().bounds.intersects(&bounds) {
					cx.defer(move |cx| {
						sheet.update(cx, |this, cx| this.close_settings_menu(cx)).ok();
					});
				}
			},
			|_, (), _, _| {},
		)
		.absolute()
		.inset_0()
	}

	/// The strip at the top of the card: its name and the one button that closes it, laid out as
	/// every other sheet's is -- the name at the left, the cross at the right, on every system.
	/// A sheet is not a window and its cross is not a window button, so there is nothing here for
	/// a system's own arrangement to be followed.
	pub(super) fn settings_header(
		&self,
		p: crate::ui::theme::Palette,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		div()
			.debug_selector(|| "settings-header".to_owned())
			.flex()
			.items_center()
			.justify_between()
			.flex_none()
			.h(px(HEADER_H))
			.px_3()
			.border_b_1()
			.border_color(p.border)
			.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Settings"))
			.child(icon_button(
				p,
				"settings-close",
				Icon::X,
				"Close",
				true,
				cx.listener(|this, _, _, cx| this.close_settings(cx)),
			))
	}

	/// Settings inside the main window, which is where it opens: the card over the dimmed list.
	/// Dragging its strip takes it out. See spec/ui.md.
	pub(crate) fn settings_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		deferred(
			backdrop(p).child(
				div()
					.id("settings-card")
					.w(px(SHEET_W))
					.h(px(SHEET_H))
					// Except where the window is shorter than the card, which the window's own
					// minimum height allows: fixed would overflow it and be clipped by the window,
					// taking the close button off the bottom with it. See `MIN_HEIGHT`.
					.max_h(gpui::relative(0.9))
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.shadow_lg()
					.overflow_hidden()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_settings(cx)))
					.child(self.settings_body(p, cx)),
			),
		)
		.priority(2)
	}
}
