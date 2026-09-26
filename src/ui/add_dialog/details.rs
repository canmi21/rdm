//! The second screen's fields past the name and folder: connections, speed limit, range and
//! checksum.

use super::*;

impl Rdm {
	/// What most downloads never touch: the folder as a word that opens the system's picker, a limit
	/// of the download's own, the part of the file wanted when the server serves parts, and a
	/// checksum the finished file must match. The limit and the part each have a slider for setting
	/// them roughly and fields for setting them exactly.
	pub(super) fn more_fields(
		&self,
		found: &Found,
		sheet: &AddSheet,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		// Three columns: the label, the control, and a fixed one on the right for what a slider's
		// field or reading shows, so both sliders end on one line and every field on another. A
		// control taller than a line has its label beside its first line, which is `first` high.
		const LABEL: f32 = 72.0;
		const END: f32 = 128.0;
		const LINE: f32 = 30.0;
		let label = |label: &'static str, first: f32| {
			div()
				.w(px(LABEL))
				.h(px(first))
				.flex_none()
				.flex()
				.items_center()
				.text_color(p.muted)
				.child(text!(id = label, label))
		};
		let row = |name: &'static str, field: gpui::AnyElement| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.text_xs()
				.child(label(name, LINE))
				.child(div().flex_1().min_w_0().child(field))
		};
		// The box a field is drawn in, for what shows a value without being typed into.
		let boxed = || {
			div()
				.h(px(LINE))
				.px_2()
				.flex()
				.items_center()
				.gap_2()
				.rounded_md()
				.border_1()
				.border_color(p.border)
				.bg(p.window)
		};
		// The folder the file goes to, chosen or the download folder, always as its path.
		let chosen = sheet.folder.is_some();
		let folder = sheet
			.folder
			.clone()
			.or_else(|| self.paths.as_ref().map(|p| p.downloads.clone()))
			.map(|f| f.display().to_string())
			.unwrap_or_else(|| "Download folder".to_owned());
		// How much the two ends take in, so a part is read as a size rather than as two numbers, and
		// out of how much once it is less than the whole: `1.2 GB / 3.9 GB`.
		let parts = found.probe.size.filter(|_| found.probe.ranges).map(|size| {
			let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
			let start = read(&sheet.range_start).unwrap_or(0);
			let end = read(&sheet.range_end).unwrap_or(size).min(size);
			let whole = crate::download::format_bytes(size);
			match end.checked_sub(start).filter(|part| *part > 0) {
				Some(part) if part < size => (crate::download::format_bytes(part), Some(whole)),
				Some(_) => (whole, None),
				None => (String::new(), None),
			}
		});
		div()
			.debug_selector(|| "add-more".to_owned())
			.flex()
			.flex_col()
			.gap_2()
			.child(row(
				"Folder",
				boxed()
					.child(div().flex_1().min_w_0().truncate().child(text!(folder)))
					.when(chosen, |s| {
						s.child(
							div()
								.id("add-folder-clear")
								.role(gpui::Role::Button)
								.aria_label("Download folder")
								.flex_none()
								.text_color(p.muted)
								.cursor_pointer()
								.on_click(cx.listener(|this, _, _, cx| this.clear_add_folder(cx)))
								.child("Reset"),
						)
					})
					.child(
						div()
							.id("add-folder")
							.role(gpui::Role::Button)
							.aria_label("Choose folder")
							.debug_selector(|| "button:Choose folder".to_owned())
							.flex_none()
							.text_color(p.accent)
							.cursor_pointer()
							.on_click(cx.listener(|this, _, _, cx| this.choose_add_folder(cx)))
							.child("Choose"),
					)
					.into_any_element(),
			))
			.child(row(
				"Speed limit",
				div()
					.flex()
					.items_center()
					.gap_3()
					.child(div().flex_1().min_w_0().child(sheet.limit_slider.clone()))
					.child(div().w(px(END)).flex_none().child(sheet.limit.clone()))
					.into_any_element(),
			))
			.when_some(parts, |s, part| {
				s.child(
					div().flex().items_start().gap_2().text_xs().child(label("Range", LINE)).child(
						div()
							.flex_1()
							.min_w_0()
							.flex()
							.flex_col()
							.gap_2()
							.child(
								div()
									.h(px(LINE))
									.flex()
									.items_center()
									.gap_3()
									.child(div().flex_1().min_w_0().child(sheet.range_slider.clone()))
									.child(
										div()
											.w(px(END))
											.flex_none()
											.pl_2()
											.flex()
											.gap_1()
											.overflow_hidden()
											.whitespace_nowrap()
											.child(
												div()
													.text_color(if part.1.is_some() { p.text } else { p.muted })
													.child(text!(part.0)),
											)
											.when_some(part.1, |s, whole| {
												s.child(div().text_color(p.muted).child(text!(format!("/ {whole}"))))
											}),
									),
							)
							.child(
								div()
									.flex()
									.items_center()
									.gap_2()
									.child(div().flex_1().min_w_0().child(sheet.range_start.clone()))
									.child(div().flex_none().text_color(p.muted).child(text!("–")))
									.child(div().flex_1().min_w_0().child(sheet.range_end.clone())),
							),
					),
				)
			})
			// Last, under the range it cannot be used with: a checksum is of the whole file.
			.child(row("Checksum", sheet.checksum.clone().into_any_element()))
	}
}
