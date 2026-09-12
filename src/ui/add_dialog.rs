//! Adding a download is a sheet inside the main window, titled New Task: one field over a dimmed
//! list, filled from the clipboard when what is there reads as an address. Enter or Query has the
//! engine look at the address first; a file is shown and Enter or Download queues it, a web page
//! is said to be one, with the files it links to offered instead. See spec/ui.md.

use std::sync::mpsc::Receiver;

use gpui::{App, Context, Entity, IntoElement, Window, deferred, div, prelude::*, px, text};
use reqwest::Url;

use crate::app::Rdm;
use crate::engine::{Inspection, Link};
use crate::ui::LeavesFocus;
use crate::ui::backdrop;
use crate::ui::button;
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::text_input::TextInput;

/// The clipboard is read only up to this length: an address is never longer, and a document
/// that happens to be on the clipboard is not worth parsing.
const CLIPBOARD_LIMIT: usize = 1000;

/// The common rates More options offers for a download's own limit, the first no limit at all.
/// Each is written into the limit field as it reads here, which `parse_rate` takes exactly; the
/// size a rate is displayed with counts in thousands and would not read back as the same rate.
const LIMITS: [&str; 4] = ["Unlimited", "1 MB/s", "5 MB/s", "10 MB/s"];

/// The sheet while it is up.
pub struct AddSheet {
	pub input: Entity<TextInput>,
	/// The engine is looking at this address; its answer is polled with the events.
	pub checking: Option<(Url, Receiver<Result<Inspection, String>>)>,
	/// The address turned out to be a page; what it links to, and which of those were added.
	pub page: Option<Page>,
	/// The address is a file, looked at: what the server said of it. How it downloads is not
	/// asked here; the download's window changes that while it runs. See spec/ui.md.
	pub found: Option<Found>,
	/// The name to save under and a checksum, on the face of the sheet.
	pub name: Entity<TextInput>,
	pub checksum: Entity<TextInput>,
	/// More options: the folder, a limit of its own, and the part of the file wanted, as the
	/// first byte and the byte it stops before.
	pub more: bool,
	pub folder: Option<std::path::PathBuf>,
	pub limit: Entity<TextInput>,
	pub range_start: Entity<TextInput>,
	pub range_end: Entity<TextInput>,
	pub error: Option<String>,
}

pub struct Found {
	pub url: Url,
	pub probe: crate::engine::Probe,
}

pub struct Page {
	pub url: Url,
	pub links: Vec<Link>,
	pub added: Vec<usize>,
}

/// The number in the connections field: one to `Connections::MAX`, or why not.
pub fn parse_count(text: &str) -> Result<u16, String> {
	let max = crate::engine::Connections::MAX;
	match text.trim().parse::<u32>() {
		Ok(n) if (1..=max as u32).contains(&n) => Ok(n as u16),
		_ => Err(format!("Connections must be a number from 1 to {max}.")),
	}
}

/// A connections field as Settings and a download's window read it: empty or `auto` is the
/// engine's own judgement, anything else a count.
pub fn parse_connections(text: &str) -> Result<Option<u16>, String> {
	let text = text.trim();
	if text.is_empty() || text.eq_ignore_ascii_case("auto") { Ok(None) } else { parse_count(text).map(Some) }
}

/// The two range fields as the row keeps them: `start-end` in bytes, the end excluded, or None for
/// the whole file -- both empty, or a start of zero with the end empty or at the file's size.
pub fn part_of_file(start: &str, end: &str, size: Option<u64>) -> Result<Option<String>, String> {
	let number = |text: &str| -> Result<Option<u64>, String> {
		let text = text.trim().replace([' ', ','], "");
		if text.is_empty() {
			Ok(None)
		} else {
			text.parse().map(Some).map_err(|_| "A range is counted in whole bytes.".to_owned())
		}
	};
	let (start, end) = (number(start)?.unwrap_or(0), number(end)?);
	if let Some(size) = size
		&& (start >= size || end.is_some_and(|end| end > size))
	{
		return Err(format!("The file is {size} bytes; the range must lie inside it."));
	}
	if end.is_some_and(|end| end <= start) {
		return Err("A range ends after it starts.".to_owned());
	}
	let whole = start == 0 && end.is_none_or(|end| Some(end) == size);
	Ok((!whole).then(|| match end {
		Some(end) => format!("{start}-{end}"),
		None => format!("{start}-"),
	}))
}

/// Whatever was typed or pasted, as an address if it can be one. With a scheme, it must be
/// http or https. Without one, `example.org/file.zip` is tried as https, which is what the
/// person meant; anything with whitespace or no dot in it is not tried at all.
pub fn parse_address(text: &str) -> Option<Url> {
	let text = text.trim();
	if text.is_empty() || text.len() > CLIPBOARD_LIMIT {
		return None;
	}
	if let Ok(url) = Url::parse(text)
		&& matches!(url.scheme(), "http" | "https")
		&& url.host().is_some()
	{
		return Some(url);
	}
	if text.contains(char::is_whitespace) || !text.contains('.') || text.contains("://") {
		return None;
	}
	Url::parse(&format!("https://{text}")).ok().filter(|u| u.host().is_some())
}

/// Whether what was found is for the address now in the field: then Enter downloads it, and
/// otherwise Enter looks again.
fn found_is_current(sheet: &AddSheet, cx: &App) -> bool {
	sheet.found.as_ref().is_some_and(|found| {
		parse_address(&sheet.input.read(cx).content).is_some_and(|url| url == found.url)
	})
}

impl Rdm {
	/// Opens the sheet with the field focused, filled from the clipboard when that reads as an
	/// address; a second press just refocuses it.
	pub(crate) fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let input = match &self.adding {
			Some(sheet) => sheet.input.clone(),
			None => {
				let rdm = cx.entity();
				let cancel = rdm.clone();
				let pasted = cx
					.read_from_clipboard()
					.and_then(|item| item.text())
					.filter(|t| t.len() <= CLIPBOARD_LIMIT)
					.and_then(|t| parse_address(&t));
				let input = cx.new(|cx| {
					let mut input = TextInput::new("https://", cx)
						.on_confirm(move |_, _, cx| rdm.update(cx, |this, cx| this.submit_add(cx)))
						.on_cancel(move |_, cx| cancel.update(cx, |this, cx| this.dismiss_add(cx)));
					if let Some(url) = &pasted {
						input.set_content(url.as_str(), cx);
					}
					input
				});
				cx.observe(&input, |_, _, cx| cx.notify()).detach();
				let mut field = |placeholder: &'static str| {
					let confirm = cx.entity();
					let field = cx.new(|cx| {
						TextInput::new(placeholder, cx)
							.on_confirm(move |_, _, cx| confirm.update(cx, |this, cx| this.submit_add(cx)))
					});
					cx.observe(&field, |_, _, cx| cx.notify()).detach();
					field
				};
				let (name, checksum, limit, range_start, range_end) = (
					field("As the server names it"),
					field("sha256, sha512 or md5"),
					field("Custom"),
					field("0"),
					field("End of file"),
				);
				self.adding = Some(AddSheet {
					input: input.clone(),
					checking: None,
					page: None,
					found: None,
					name,
					checksum,
					more: false,
					folder: None,
					limit,
					range_start,
					range_end,
					error: None,
				});
				input
			}
		};
		window.focus(&input.read(cx).focus(), cx);
		cx.notify();
	}

	/// A click outside closes the sheet only while nothing has been typed and nothing is being
	/// looked at; typed text is kept until the cross is pressed. See spec/ui.md.
	pub(crate) fn dismiss_add(&mut self, cx: &mut Context<Self>) {
		if self.guide.is_some() {
			return;
		}
		let clean = self.adding.as_ref().is_none_or(|sheet| {
			sheet.input.read(cx).content.trim().is_empty()
				&& sheet.checking.is_none()
				&& sheet.page.is_none()
				&& sheet.found.is_none()
		});
		if clean {
			self.close_add(cx);
		}
	}

	pub(crate) fn close_add(&mut self, cx: &mut Context<Self>) {
		self.adding = None;
		cx.notify();
	}

	/// Enter, or Query: the address is handed to the engine to look at; what happens next depends
	/// on its answer, which the pump collects. Once the address in the field has been looked at
	/// and found to be a file, Enter or Download is the second step: the download itself.
	pub(crate) fn submit_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		if found_is_current(sheet, cx) {
			// The settings' default; the download's window changes it while it runs.
			let connections = self.preferences.connections;
			let asked = match self.asked(connections, cx) {
				Ok(asked) => asked,
				Err(message) => {
					if let Some(sheet) = &mut self.adding {
						sheet.error = Some(message);
					}
					cx.notify();
					return;
				}
			};
			let Some(sheet) = &self.adding else { return };
			let Some(found) = &sheet.found else { return };
			let typed = sheet.name.read(cx).content.trim().to_owned();
			let name = if typed.is_empty() { found.probe.file_name.clone() } else { typed };
			let url = found.url.clone();
			self.add_request(url, Some(name), None, asked, cx);
			self.close_add(cx);
			return;
		}
		let text = sheet.input.read(cx).content.trim().to_owned();
		if text.is_empty() {
			return;
		}
		let Some(url) = parse_address(&text) else {
			sheet.error = Some("That is not a web address.".to_owned());
			cx.notify();
			return;
		};
		sheet.error = None;
		sheet.page = None;
		sheet.found = None;
		sheet.checking = Some((url.clone(), self.engine.inspect(url)));
		cx.notify();
	}

	/// What the sheet's fields ask for, read and checked: each empty when it was left so.
	fn asked(
		&self,
		connections: Option<u16>,
		cx: &Context<Self>,
	) -> Result<crate::app::Asked, String> {
		let Some(sheet) = &self.adding else { return Ok(crate::app::Asked::default()) };
		let text = |field: &Entity<TextInput>| field.read(cx).content.trim().to_owned();
		let checksum = text(&sheet.checksum);
		let checksum = if checksum.is_empty() {
			None
		} else {
			crate::engine::Checksum::parse(&checksum)
				.map(|_| checksum)
				.ok_or_else(|| "A checksum is sha256, sha512 or md5, written in hex.".to_owned())
				.map(Some)?
		};
		let size = sheet.found.as_ref().and_then(|found| found.probe.size);
		let range = part_of_file(&text(&sheet.range_start), &text(&sheet.range_end), size)?;
		// A checksum is checked against a whole file; a part of one has nothing to match.
		if range.is_some() && checksum.is_some() {
			return Err("A checksum is for the whole file; clear it to download a part.".to_owned());
		}
		let speed_limit = crate::download::parse_rate(&text(&sheet.limit))?;
		Ok(crate::app::Asked {
			connections,
			directory: sheet.folder.as_ref().map(|p| p.display().to_string()),
			mirrors: Vec::new(),
			checksum,
			range,
			speed_limit,
		})
	}

	/// More options, or fewer: the folder, the limit and the range, shown or put away.
	pub(crate) fn toggle_add_more(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.more = !sheet.more;
			cx.notify();
		}
	}

	/// The system's folder picker, for where the file goes; nothing chosen leaves the download
	/// folder.
	pub(crate) fn choose_add_folder(&mut self, cx: &mut Context<Self>) {
		let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
			files: false,
			directories: true,
			multiple: false,
			prompt: Some("Save here".into()),
		});
		cx.spawn(async move |this, cx| {
			if let Ok(Ok(Some(paths))) = receiver.await
				&& let Some(path) = paths.into_iter().next()
			{
				let _ = this.update(cx, |this, cx| {
					if let Some(sheet) = &mut this.adding {
						sheet.folder = Some(path);
						cx.notify();
					}
				});
			}
		})
		.detach();
	}

	pub(crate) fn clear_add_folder(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.folder = None;
			cx.notify();
		}
	}

	/// The pencil on a looked-at address: the field back, and what the look filled in forgotten,
	/// since another address is another file. What was typed by hand -- the checksum, the limit,
	/// the folder -- stays.
	pub(crate) fn edit_add_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		sheet.checking = None;
		sheet.found = None;
		sheet.page = None;
		sheet.error = None;
		sheet.more = false;
		let input = sheet.input.clone();
		let filled = [sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone()];
		for field in filled {
			field.update(cx, |field, cx| field.set_content("", cx));
		}
		window.focus(&input.read(cx).focus(), cx);
		cx.notify();
	}

	/// One of the common limits, written into the field that takes any other; no limit is the
	/// field left empty.
	pub(crate) fn set_add_limit(&mut self, choice: &'static str, cx: &mut Context<Self>) {
		let Some(sheet) = &self.adding else { return };
		let text = if choice == LIMITS[0] { "" } else { choice };
		sheet.limit.clone().update(cx, |input, cx| input.set_content(text, cx));
	}

	/// The engine's answer about the address, if it has arrived. Called by the event pump.
	pub(crate) fn poll_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		let Some((url, receiver)) = &sheet.checking else { return };
		let Ok(answer) = receiver.try_recv() else { return };
		let url = url.clone();
		sheet.checking = None;
		match answer {
			Ok(inspection) if inspection.is_page => {
				sheet.page = Some(Page { url, links: inspection.links, added: Vec::new() });
			}
			Ok(inspection) => {
				// A file: say what it is and what can be done with it, and wait for the second step.
				// The name it will be saved under is filled in from what the look turned up, so
				// the user is changing a name rather than being asked to invent one -- the field
				// was empty before, and an empty field beside a resolved address reads as though
				// nothing was resolved. The range starts as the whole file, in the server's bytes,
				// so a part is asked for by moving an end rather than by working out a number.
				let name = inspection.probe.file_name.clone();
				let whole = inspection.probe.size.filter(|_| inspection.probe.ranges);
				sheet.found = Some(Found { url, probe: inspection.probe });
				let fill = |field: &Entity<TextInput>, text: &str, cx: &mut Context<Self>| {
					field.update(cx, |input, cx| {
						if input.content.trim().is_empty() {
							input.set_content(text, cx);
						}
					});
				};
				let (field, start, end) =
					(sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone());
				fill(&field, &name, cx);
				if let Some(size) = whole {
					fill(&start, "0", cx);
					fill(&end, &size.to_string(), cx);
				}
			}
			Err(message) => sheet.error = Some(message),
		}
		cx.notify();
	}

	/// The page itself, saved as a file after all.
	fn add_page_anyway(&mut self, cx: &mut Context<Self>) {
		let Some(page) = self.adding.as_ref().and_then(|s| s.page.as_ref()) else { return };
		let url = page.url.clone();
		let asked =
			crate::app::Asked { connections: self.preferences.connections, ..Default::default() };
		self.add_request(url, None, None, asked, cx);
		self.close_add(cx);
	}

	/// One of the files the page links to. The sheet stays up so several can be taken.
	fn add_link(&mut self, index: usize, cx: &mut Context<Self>) {
		let Some(page) = self.adding.as_mut().and_then(|s| s.page.as_mut()) else { return };
		let Some(link) = page.links.get(index).cloned() else { return };
		if page.added.contains(&index) {
			return;
		}
		let source = page.url.to_string();
		page.added.push(index);
		let asked =
			crate::app::Asked { connections: self.preferences.connections, ..Default::default() };
		self.add_request(link.url, Some(link.name), Some(source), asked, cx);
	}

	/// Drawn over everything from the window root; a click outside the sheet closes it.
	pub(crate) fn add_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(sheet) = &self.adding else { return deferred(div()).priority(2) };
		let input = sheet.input.clone();
		let typed = !input.read(cx).content.trim().is_empty();
		let checking = sheet.checking.is_some();
		let ready = typed && !checking;
		let current = found_is_current(sheet, cx);
		let locked = checking || sheet.found.is_some() || sheet.page.is_some();
		let address = input.read(cx).content.to_string();
		// The button says what it does next: look at the address, or download what was found there.
		let (glyph, action) = if current { (Icon::Download, "Download") } else { (Icon::Search, "Query") };
		let options = if sheet.more { "Fewer options" } else { "More options" };
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
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(text!("New Task")))
							.child(crate::ui::icon_button(
								p,
								"add-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_add(cx)),
							)),
					)
					// Once looked at, the address is a line of text with a pencil that frees it again: a
					// field left open after the look invited an edit that could only start over.
					.map(|s| {
						if locked {
							s.child(
								div()
									.id("add-address")
									.debug_selector(|| "add-address".to_owned())
									.flex()
									.items_center()
									.gap_2()
									.h(px(30.0))
									.pl_2()
									.rounded_md()
									.bg(p.hover)
									.child(icon(Icon::Globe, p.muted).size_3p5())
									.child(div().flex_1().min_w_0().truncate().child(text!(address)))
									.child(crate::ui::icon_button(
										p,
										"add-edit",
										Icon::Pencil,
										"Edit address",
										true,
										cx.listener(|this, _, window, cx| this.edit_add_address(window, cx)),
									)),
							)
						} else {
							s.child(input)
						}
					})
					.when_some(sheet.error.clone(), |s, error| {
						s.child(
							div()
								.text_xs()
								.text_color(p.failure)
								.debug_selector(|| "add-error".to_owned())
								.child(text!(error)),
						)
					})
					.when_some(sheet.page.as_ref(), |s, page| s.child(self.page_notice(page, cx)))
					.when_some(sheet.found.as_ref().filter(|_| current), |s, found| {
						s.child(self.found_notice(found, sheet, cx))
					})
					// One line for the end of the sheet: the options or the look in progress at the
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
									.when(!checking && current, |s| {
										s.child(
											div()
												.id("add-more")
												.role(gpui::Role::Button)
												.aria_label(options)
												.aria_expanded(sheet.more)
												.debug_selector(move || format!("button:{options}"))
												.group("add-more")
												.flex()
												.items_center()
												.gap_1()
												.cursor_pointer()
												.hover(move |s| s.text_color(p.text))
												.on_click(cx.listener(|this, _, _, cx| this.toggle_add_more(cx)))
												.child(options)
												.child(
													hover_icon(
														if sheet.more { Icon::ChevronUp } else { Icon::ChevronDown },
														"add-more",
														p.muted,
														Some(p.text),
													)
													.size_3p5(),
												),
										)
									}),
							)
							// A page's own rows are its actions, so there is no button while one is shown.
							.when(sheet.page.is_none(), |s| {
								s.child(button(
									p,
									"add-confirm",
									glyph,
									action,
									ready,
									cx.listener(|this, _, _, cx| this.submit_add(cx)),
								))
							}),
					),
			),
		)
		.priority(2)
	}

	/// The address is a file: a card of what the look turned up, each fact beside its label, then
	/// the name it will be saved under and a checksum, then More options when they are open. The
	/// file's name is not in the card, since the address and Save as already say it. How many
	/// connections to open is not asked: the settings' default starts the download and its window
	/// changes the count while it runs. See spec/ui.md.
	fn found_notice(
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
			.map(|c| {
				let tint = if self.preferences.colorful_categories { p.hue(c.color) } else { p.muted };
				(c.icon, tint, c.name.clone())
			});
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
		// A fact beside its grey label; the labels share one width so the values line up down a
		// column.
		let fact = |label: &'static str, value: gpui::AnyElement| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.min_w_0()
				.child(div().w(px(52.0)).flex_none().text_color(p.muted).child(text!(id = label, label)))
				.child(div().flex().items_center().min_w_0().child(value))
		};
		let plain = |label: &'static str, value: String| {
			div().min_w_0().truncate().child(text!(id = (label, 1usize), value)).into_any_element()
		};
		// What the file is, and where it comes from, each under a heading of its own.
		let column = |heading: &'static str| {
			div().flex().flex_col().flex_1().min_w_0().gap_1().child(
				div()
					.text_size(px(10.0))
					.font_weight(gpui::FontWeight::MEDIUM)
					.text_color(p.muted)
					.child(text!(id = heading, heading)),
			)
		};
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
						column("FILE")
							.when_some(filed, |s, (glyph, tint, filed)| {
								s.child(fact(
									"Type",
									div()
										.flex()
										.items_center()
										.gap_1()
										.min_w_0()
										.child(icon(glyph, tint).size_3p5())
										.child(div().min_w_0().truncate().child(text!(filed)))
										.into_any_element(),
								))
							})
							.child(fact("Size", plain("Size", size)))
							.child(fact("Resume", plain("Resume", resume.to_owned()))),
					)
					.child(
						column("SOURCE")
							.child(fact("From", plain("From", from)))
							.child(fact("Server", plain("Server", server)))
							.when_some(updated, |s, updated| s.child(fact("Updated", plain("Updated", updated)))),
					),
			)
			// The name on a line of its own under its label: a name can be long, and a field that
			// shares its line with the label shows less of it.
			.child(field("Save as", sheet.name.clone()))
			.child(field("Checksum (optional)", sheet.checksum.clone()))
			.when(sheet.more, |s| s.child(self.more_fields(found, sheet, cx)))
	}

	/// What most downloads never touch: the folder as a word that opens the system's picker, a
	/// limit of the download's own, and the part of the file wanted when the server serves parts.
	fn more_fields(
		&self,
		found: &Found,
		sheet: &AddSheet,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let row = |label: &'static str, field: gpui::AnyElement| {
			div()
				.flex()
				.items_center()
				.gap_2()
				.text_xs()
				.child(div().w(px(72.0)).flex_none().text_color(p.muted).child(text!(id = label, label)))
				.child(div().flex_1().min_w_0().child(field))
		};
		let folder = sheet
			.folder
			.as_ref()
			.map(|f| f.display().to_string())
			.unwrap_or_else(|| "Download folder".to_owned());
		let limit = crate::download::parse_rate(&sheet.limit.read(cx).content);
		// How much the two ends take in, so a part is read as a size rather than as two numbers.
		let parts = found.probe.size.filter(|_| found.probe.ranges).map(|size| {
			let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
			let start = read(&sheet.range_start).unwrap_or(0);
			let end = read(&sheet.range_end).unwrap_or(size);
			if end > start { crate::download::format_bytes(end - start) } else { String::new() }
		});
		div()
			.debug_selector(|| "add-more".to_owned())
			.flex()
			.flex_col()
			.gap_1p5()
			.child(row(
				"Folder",
				div()
					.flex()
					.items_center()
					.gap_2()
					.child(div().min_w_0().truncate().text_color(p.muted).child(text!(folder)))
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
					.when(sheet.folder.is_some(), |s| {
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
					.into_any_element(),
			))
			// The common rates as choices and a field for any other; the field is the value, so a
			// choice writes into it and a typed rate that matches one lights it.
			.child(row(
				"Speed limit",
				div()
					.flex()
					.items_center()
					.gap_1()
					.children(LIMITS.iter().enumerate().map(|(index, &label)| {
						let on = limit == crate::download::parse_rate(label);
						div()
							.id(("limit", index))
							.role(gpui::Role::RadioButton)
							.aria_label(label)
							.aria_selected(on)
							.debug_selector(move || format!("limit:{label}"))
							.flex_none()
							.px_1p5()
							.py_0p5()
							.rounded_sm()
							.cursor_pointer()
							.leaves_focus()
							.text_color(if on { p.text } else { p.muted })
							.when(on, |s| s.bg(p.selection))
							.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
							.on_click(cx.listener(move |this, _, _, cx| this.set_add_limit(label, cx)))
							.child(label)
					}))
					.child(div().w(px(84.0)).flex_none().ml_1().child(sheet.limit.clone()))
					.into_any_element(),
			))
			.when_some(parts, |s, part| {
				s.child(row(
					"Range",
					div()
						.flex()
						.items_center()
						.gap_2()
						.child(div().w(px(104.0)).flex_none().child(sheet.range_start.clone()))
						.child(div().flex_none().text_color(p.muted).child(text!("to")))
						.child(div().w(px(104.0)).flex_none().child(sheet.range_end.clone()))
						.child(div().min_w_0().truncate().text_color(p.muted).child(text!(part)))
						.into_any_element(),
				))
			})
	}

	/// The address is a page: say so, offer the files it links to, and let the page itself be
	/// saved after all.
	fn page_notice(&self, page: &Page, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let rows: Vec<_> = page
			.links
			.iter()
			.enumerate()
			.map(|(index, link)| {
				let added = page.added.contains(&index);
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
						div().text_xs().text_color(p.warning).child(text!("This address is a web page, not a file.")),
					)
					.child(button(
						p,
						"add-page-anyway",
						Icon::FileText,
						"Save the page anyway",
						true,
						cx.listener(|this, _, _, cx| this.add_page_anyway(cx)),
					)),
			)
			.when(!rows.is_empty(), |s| {
				s.child(
					div().text_xs().text_color(p.muted).child(text!("Files the page links to; press one to add it")),
				)
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
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn an_address_is_read_with_or_without_its_scheme_and_junk_is_not() {
		let ok = |t: &str| parse_address(t).map(|u| u.to_string());
		assert_eq!(ok("https://a.example/x.zip"), Some("https://a.example/x.zip".into()));
		assert_eq!(ok("  http://a.example/x.zip \n"), Some("http://a.example/x.zip".into()));
		assert_eq!(ok("a.example/x.zip"), Some("https://a.example/x.zip".into()));
		assert_eq!(ok("a.example"), Some("https://a.example/".into()));
		assert_eq!(ok("ftp://a.example/x"), None, "not a scheme the engine speaks");
		assert_eq!(ok("hello world"), None);
		assert_eq!(ok("just words"), None);
		assert_eq!(ok("nodot"), None);
		assert_eq!(ok(""), None);
		assert_eq!(ok(&"x".repeat(1001)), None, "over the limit is not looked at");
	}

	#[test]
	fn a_connections_field_is_auto_when_empty_or_said_and_a_count_otherwise() {
		assert_eq!(parse_connections(""), Ok(None));
		assert_eq!(parse_connections(" Auto "), Ok(None));
		assert_eq!(parse_connections("8"), Ok(Some(8)));
		assert!(parse_connections("0").is_err());
		assert!(parse_connections("257").is_err());
		assert!(parse_connections("lots").is_err());
	}

	#[test]
	fn the_range_fields_are_no_range_at_all_until_they_leave_out_part_of_the_file() {
		assert_eq!(part_of_file("", "", Some(5000)), Ok(None));
		assert_eq!(part_of_file("0", "5000", Some(5000)), Ok(None), "the whole file, as prefilled");
		assert_eq!(part_of_file("0", "1000", Some(5000)), Ok(Some("0-1000".into())));
		assert_eq!(part_of_file("1,000", "", Some(5000)), Ok(Some("1000-".into())));
		assert_eq!(part_of_file("", "", None), Ok(None), "a file of unknown size, untouched");
		assert!(part_of_file("0", "6000", Some(5000)).is_err(), "past the end");
		assert!(part_of_file("300", "200", Some(5000)).is_err(), "backwards");
		assert!(part_of_file("a", "", Some(5000)).is_err());
	}
}
