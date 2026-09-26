//! New Task drawn: the sheet, and the notice each outcome of a look puts on it.

use super::*;

impl Rdm {
	/// Drawn over everything from the window root; a click outside the sheet closes it.
	pub(crate) fn add_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(sheet) = &self.adding else { return deferred(div()).priority(2) };
		let checking = sheet.checking.is_some();
		let typed = !sheet.input.read(cx).content.trim().is_empty();
		let second = sheet.found.is_some();
		let asking = sheet.asking;
		let checksum_typed = !sheet.checksum.read(cx).content.trim().is_empty();
		deferred(
			// The backdrop takes every mouse event, so nothing behind the sheet can be pressed through it.
			backdrop(p).child(
				div()
					.id("add-dialog")
					// A node that holds the sheet's own, so `ctl tree "New Task"` finds it whole.
					.role(gpui::Role::Dialog)
					.aria_label("New Task")
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
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_add(cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.flex()
									.items_center()
									.gap_1p5()
									.when(second, |s| {
										s.child(crate::ui::icon_button(
											p,
											"add-back",
											Icon::ArrowLeft,
											"Back",
											true,
											cx.listener(move |this, _, window, cx| {
												if asking { this.stop_asking(cx) } else { this.back_to_address(window, cx) }
											}),
										))
									})
									.child(
										div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(text!("New Task")),
									),
							)
							.child(crate::ui::icon_button(
								p,
								"add-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_add(cx)),
							)),
					)
					// The second screen does not show the address: arriving there means it was right.
					.map(|s| match &sheet.found {
						Some(_) if asking => s.child(self.mirror_notice(sheet, cx)),
						Some(found) => s.child(self.found_notice(found, sheet, cx)),
						None => s.child(sheet.input.clone()).when_some(sheet.confirm.as_ref(), |s, confirm| {
							s.child(self.confirm_notice(confirm, cx))
						}),
					})
					.when_some(sheet.problem.as_ref(), |s, problem| {
						s.child(self.problem_notice(problem, sheet.details, cx))
					})
					// One line for the end of the sheet: the look in progress or More options at the
					// left, the button at the right, rather than a line of its own for the button.
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.gap_3()
							.child(
								div()
									.flex()
									.items_center()
									.text_xs()
									.text_color(p.muted)
									.when(checking, |s| s.child(text!("Looking at the address")))
									.when(second && !asking, |s| {
										s.child(disclosure(
											p,
											"add-more",
											if sheet.more { "Fewer options" } else { "More options" },
											sheet.more,
											cx.listener(|this, _, _, cx| this.toggle_add_more(cx)),
										))
									}),
							)
							.map(|s| {
								if second {
									s.child(button_after(
										p,
										"add-confirm",
										Icon::CornerDownLeft,
										"Download",
										!asking || checksum_typed,
										cx.listener(|this, _, _, cx| this.submit_add(cx)),
									))
								} else {
									s.child(button(
										p,
										"add-confirm",
										Icon::ChevronRight,
										"Continue",
										typed && !checking,
										cx.listener(|this, _, _, cx| this.submit_add(cx)),
									))
								}
							}),
					),
			),
		)
		.priority(2)
	}

	/// The third screen: other servers have the file and there is no checksum to hold them to. The
	/// checksum field again, with what it is for behind a `?`, and three ways to go on without it.
	/// See spec/rules.md.
	pub(super) fn mirror_notice(
		&self,
		sheet: &AddSheet,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let mirrors = sheet.resolved.as_ref().map(|r| r.mirrors.clone()).unwrap_or_default();
		let heading = match mirrors.len() {
			1 => "Another server has this file".to_owned(),
			n => format!("{n} other servers have this file"),
		};
		let mut hosts: Vec<String> =
			mirrors.iter().filter_map(|m| m.host_str().map(str::to_owned)).collect();
		hosts.dedup();
		let why = "rdm downloads from other servers alongside the source only when the finished file \
			can be checked against a checksum, since a mirror could send anything. With the file's \
			checksum it uses them all; without one it goes to the source alone.";
		let choice = |id: &'static str, label: &'static str, choice: Option<crate::rules::Choice>| {
			div()
				.id(id)
				.role(gpui::Role::Button)
				.aria_label(label)
				.debug_selector(move || format!("button:{label}"))
				.px_2()
				.py_0p5()
				.rounded_sm()
				.text_xs()
				.text_color(p.muted)
				.cursor_pointer()
				.hover(move |s| s.bg(p.hover).text_color(p.text))
				.on_click(cx.listener(move |this, _, _, cx| this.decline_mirror(choice, cx)))
				.child(text!(id = (id, 0usize), label))
		};
		div()
			.flex()
			.flex_col()
			.gap_3()
			.child(
				div()
					.debug_selector(|| "add-mirrors".to_owned())
					.flex()
					.flex_col()
					.gap_1()
					.p_3()
					.rounded_md()
					.bg(p.hover)
					.text_xs()
					.child(div().font_weight(gpui::FontWeight::MEDIUM).child(heading))
					.child(div().text_color(p.muted).truncate().child(hosts.join(", "))),
			)
			.child(
				div()
					.flex()
					.flex_col()
					.gap_1()
					.child(
						div()
							.flex()
							.items_center()
							.gap_1()
							.text_xs()
							.text_color(p.muted)
							.child(text!("Checksum"))
							.child(
								div()
									.id("mirror-why")
									.role(gpui::Role::Button)
									.aria_label("Why a checksum")
									.tooltip(crate::ui::tooltip::tooltip(why))
									.child(icon(Icon::CircleQuestion, p.muted).size_3p5()),
							),
					)
					.child(sheet.checksum.clone()),
			)
			.child(
				div()
					.flex()
					.flex_wrap()
					.gap_1()
					.child(choice("mirror-no", "No thanks", None))
					.child(choice(
						"mirror-auto",
						"Use mirror when possible",
						Some(crate::rules::Choice::Auto),
					))
					.child(choice(
						"mirror-never",
						"Never ask for this source",
						Some(crate::rules::Choice::Never),
					)),
			)
	}

	/// What went wrong in a line, and behind Details the whole text: an error is long enough to push
	/// the sheet apart when it is shown as it comes.
	pub(super) fn problem_notice(
		&self,
		problem: &Problem,
		open: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		div()
			.flex()
			.flex_col()
			.gap_1p5()
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap_3()
					.child(
						div()
							.min_w_0()
							.text_xs()
							.text_color(p.failure)
							.debug_selector(|| "add-error".to_owned())
							.child(text!(problem.summary.clone())),
					)
					.when(problem.detail.is_some(), |s| {
						s.child(disclosure(
							p,
							"add-details",
							"Details",
							open,
							cx.listener(|this, _, _, cx| this.toggle_add_details(cx)),
						))
					}),
			)
			.when_some(problem.detail.clone().filter(|_| open), |s, detail| {
				s.child(
					div()
						.id("add-detail")
						.debug_selector(|| "add-detail".to_owned())
						.max_h(px(120.0))
						.overflow_y_scroll()
						.p_2()
						.rounded_md()
						.bg(p.hover)
						.text_xs()
						.text_color(p.muted)
						.child(text!(detail)),
				)
			})
	}

	/// A page, a script or a stylesheet: say what it is, offer Download anyway, and for a page the
	/// files it links to, each added when pressed.
	pub(super) fn confirm_notice(
		&self,
		confirm: &Confirm,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let rows: Vec<_> = confirm
			.links
			.iter()
			.enumerate()
			.map(|(index, link)| {
				let added = confirm.added.contains(&index);
				let name = link.name.clone();
				let address = link.url.to_string();
				div()
					.id(("link", index))
					.debug_selector(move || format!("link:{name}"))
					.flex()
					.items_center()
					.gap_2()
					.px_1p5()
					.py_0p5()
					.rounded_sm()
					.text_xs()
					.when(!added, move |s| s.cursor_pointer().hover(move |s| s.bg(p.hover)))
					.on_click(cx.listener(move |this, _, _, cx| this.add_link(index, cx)))
					.child(
						icon(
							if added { Icon::CircleCheck } else { Icon::File },
							if added { p.success } else { p.muted },
						)
						.size_3p5(),
					)
					.child(div().flex_none().child(text!(link.name.clone())))
					.child(div().flex_1().min_w_0().truncate().text_color(p.muted).child(text!(address)))
			})
			.collect();
		div()
			.flex()
			.flex_col()
			.gap_2()
			.debug_selector(|| "add-page".to_owned())
			.child(
				div()
					.flex()
					.items_center()
					.justify_between()
					.gap_3()
					.child(
						div()
							.min_w_0()
							.text_xs()
							.text_color(p.warning)
							.child(text!(format!("This address is a {}, not a file.", confirm.kind))),
					)
					.child(button(
						p,
						"add-anyway",
						Icon::Download,
						"Download anyway",
						true,
						cx.listener(|this, _, _, cx| this.download_anyway(cx)),
					)),
			)
			.when(!rows.is_empty(), |s| {
				s.child(div().text_xs().text_color(p.muted).child(text!("Or one of the files it links to")))
					.child(
						div()
							.id("add-links")
							.flex()
							.flex_col()
							.max_h(px(220.0))
							.overflow_y_scroll()
							.children(rows),
					)
			})
	}

	/// The second screen: a card of what the look turned up, each fact beside its label in a column
	/// for the file and one for its source, then the name it will be saved under, then More options
	/// when they are open. How many connections to open is not asked: the
	/// settings' default starts the download and its window changes the count while it runs.
	pub(super) fn found_notice(
		&self,
		found: &Found,
		sheet: &AddSheet,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let probe = &found.probe;
		// Filed by the name it will be saved under, so renaming it in the field refiles it here.
		let typed = sheet.name.read(cx).content.trim().to_owned();
		let name = if typed.is_empty() { probe.file_name.clone() } else { typed };
		let filed = self
			.categories
			.iter()
			.find(|c| c.matches_name(&name))
			.or_else(|| self.categories.iter().find(|c| c.is_catch_all()))
			.map(|c| c.name.clone());
		// The host after redirects, which is not always the one typed.
		let from = probe.url.host_str().unwrap_or_default().to_owned();
		let size =
			probe.size.map(crate::download::format_bytes).unwrap_or_else(|| "Unknown".to_owned());
		let resume = if probe.ranges { "Supported" } else { "Not supported" };
		let updated = probe
			.last_modified
			.as_deref()
			.and_then(|date| chrono::DateTime::parse_from_rfc2822(date).ok())
			.map(|date| date.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string());
		let server = match &probe.server {
			Some(server) => format!("{server} ({})", probe.version),
			None => probe.version.to_owned(),
		};
		// A fact beside its grey label; the labels share one width so the values line up.
		let fact = |label: &'static str, value: String| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.min_w_0()
				.child(div().w(px(52.0)).flex_none().text_color(p.muted).child(text!(id = label, label)))
				.child(div().min_w_0().truncate().child(text!(id = (label, 1usize), value)))
		};
		// What the file is, and where it comes from, a column each and no heading over either.
		let column = || div().flex().flex_col().flex_1().min_w_0().gap_1();
		let field = |label: &'static str, input: Entity<TextInput>| {
			div()
				.flex()
				.flex_col()
				.gap_1()
				.child(div().text_xs().text_color(p.muted).child(text!(id = label, label)))
				.child(input)
		};
		div()
			.flex()
			.flex_col()
			.gap_3()
			.child(
				div()
					.debug_selector(|| "add-found".to_owned())
					.flex()
					.gap_4()
					.p_3()
					.rounded_md()
					.bg(p.hover)
					.text_xs()
					.child(
						column()
							.when_some(filed, |s, filed| s.child(fact("Type", filed)))
							.child(fact("Size", size))
							.child(fact("Resume", resume.to_owned())),
					)
					.child(
						column()
							.child(fact("From", from))
							.child(fact("Server", server))
							.when_some(updated, |s, updated| s.child(fact("Updated", updated))),
					),
			)
			// The name on a line of its own under its label: a name can be long, and a field that
			// shares its line with the label shows less of it.
			.child(field("Save as", sheet.name.clone()))
			.when(sheet.more, |s| s.child(self.more_fields(found, sheet, cx)))
	}
}
