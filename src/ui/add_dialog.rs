//! Adding a download is a sheet inside the main window: one field over a dimmed list, filled
//! from the clipboard when what is there reads as an address. Enter or Add has the engine look
//! at the address first; a file is queued at once, a web page is said to be one, with the files
//! it links to offered instead. See spec/ui.md.

use std::sync::mpsc::Receiver;

use gpui::{Context, Entity, IntoElement, Window, deferred, div, prelude::*, px, text};
use reqwest::Url;

use crate::app::Rdm;
use crate::engine::{Inspection, Link};
use crate::ui::backdrop;
use crate::ui::button;
use crate::ui::icon::{Icon, icon};
use crate::ui::text_input::TextInput;

/// The clipboard is read only up to this length: an address is never longer, and a document
/// that happens to be on the clipboard is not worth parsing.
const CLIPBOARD_LIMIT: usize = 1000;

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
	/// The rest of what can be asked for, behind More: the name to save under, the folder,
	/// other addresses of the same file, a checksum, a range and a limit of its own.
	pub more: bool,
	pub name: Entity<TextInput>,
	pub folder: Option<std::path::PathBuf>,
	pub mirrors: Entity<TextInput>,
	pub checksum: Entity<TextInput>,
	pub range: Entity<TextInput>,
	pub limit: Entity<TextInput>,
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
				let (name, mirrors, checksum, range, limit) = (
					field("As the server names it"),
					field("Other addresses of the same file, apart by spaces"),
					field("sha256, sha512 or md5 hex; the length says which"),
					field("start-end, in bytes"),
					field("Off"),
				);
				self.adding = Some(AddSheet {
					input: input.clone(),
					checking: None,
					page: None,
					found: None,
					more: false,
					name,
					folder: None,
					mirrors,
					checksum,
					range,
					limit,
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

	/// Enter, or Add: the address is handed to the engine to look at; what happens next depends
	/// on its answer, which the pump collects. Once the address has been looked at and found to
	/// be a file, Enter or Add is the second step: the download, with the connections chosen.
	pub(crate) fn submit_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		if let Some(found) = &sheet.found
			&& found.url.as_str()
				== parse_address(&sheet.input.read(cx).content).map(|u| u.to_string()).unwrap_or_default()
		{
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
		let mirrors: Vec<String> = text(&sheet.mirrors).split_whitespace().map(str::to_owned).collect();
		for mirror in &mirrors {
			if parse_address(mirror).is_none() {
				return Err(format!("{mirror} is not a web address."));
			}
		}
		let checksum = text(&sheet.checksum);
		let checksum = if checksum.is_empty() {
			None
		} else {
			crate::engine::Checksum::parse(&checksum)
				.map(|_| checksum)
				.ok_or_else(|| {
					"A checksum is sha256, sha512 or md5 hex, its length saying which.".to_owned()
				})
				.map(Some)?
		};
		let range = text(&sheet.range);
		let range = if range.is_empty() {
			None
		} else {
			crate::download::parse_range(&range)?;
			Some(range)
		};
		let speed_limit = crate::download::parse_rate(&text(&sheet.limit))?;
		Ok(crate::app::Asked {
			connections,
			directory: sheet.folder.as_ref().map(|p| p.display().to_string()),
			mirrors,
			checksum,
			range,
			speed_limit,
		})
	}

	/// More, or less: the rest of the fields, shown or put away.
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
				// nothing was resolved.
				let name = inspection.probe.file_name.clone();
				sheet.found = Some(Found { url, probe: inspection.probe });
				let field = sheet.name.clone();
				field.update(cx, |input, cx| {
					if input.content.to_string().trim().is_empty() {
						input.set_content(&name, cx);
					}
				});
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
		deferred(
			// The backdrop takes every mouse event, so nothing behind the sheet can be pressed through it.
			backdrop(p).child(
				div()
					.id("add-dialog")
					// A node that holds the sheet's own, so `ctl tree "Add Task"` finds it whole.
					.role(gpui::Role::Dialog)
					.aria_label("Add Task")
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
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(text!("Add Task")))
							.child(crate::ui::icon_button(
								p,
								"add-close",
								Icon::X,
								"Close",
								true,
								cx.listener(|this, _, _, cx| this.close_add(cx)),
							)),
					)
					.child(input)
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
					.when_some(sheet.found.as_ref(), |s, found| s.child(self.found_notice(found, sheet, cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.text_xs()
									.text_color(p.muted)
									.when(checking, |s| s.child(text!("Looking at the address"))),
							)
							.child(button(
								p,
								"add-confirm",
								Icon::Plus,
								"Add",
								ready,
								cx.listener(|this, _, _, cx| this.submit_add(cx)),
							)),
					),
			),
		)
		.priority(2)
	}

	/// The address is a file: what the look turned up beyond its name, which the address and Save as
	/// already say -- the category it will be filed under, where the bytes really come from, its
	/// size with a grey mark for whether the server lets it be split and resumed, and a line of
	/// what the server said of the file and of itself -- then the name it will be saved under. How
	/// many connections to open is not asked: the settings' default starts the download and its
	/// window changes the count while it runs. See spec/ui.md.
	fn found_notice(
		&self,
		found: &Found,
		sheet: &AddSheet,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let probe = &found.probe;
		let size =
			probe.size.map(crate::download::format_bytes).unwrap_or_else(|| "size unknown".to_owned());
		// A mark rather than a sentence: what it says matters once the download runs, and there it
		// is a setting of the download's window rather than a question here.
		let (mark, meaning) = if probe.ranges {
			(Icon::Split, "Resumable, can be split across connections")
		} else {
			(Icon::MoveRight, "One connection, cannot resume")
		};
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
		let host = probe.url.host_str().unwrap_or_default();
		let source = match (&filed, host.is_empty()) {
			(_, true) => String::new(),
			(Some(_), false) => format!("\u{b7} {host}"),
			(None, false) => host.to_owned(),
		};
		let updated = probe
			.last_modified
			.as_deref()
			.and_then(|date| chrono::DateTime::parse_from_rfc2822(date).ok())
			.map(|date| format!("Updated {}", date.with_timezone(&chrono::Local).format("%b %-d, %Y")));
		let details = [updated, Some(probe.version.to_owned()), probe.server.clone()]
			.into_iter()
			.flatten()
			.collect::<Vec<_>>()
			.join(" \u{b7} ");
		div()
			.debug_selector(|| "add-found".to_owned())
			.flex()
			.flex_col()
			.gap_2()
			.p_3()
			.rounded_md()
			.bg(p.hover)
			.child(
				div()
					.flex()
					.flex_col()
					.gap_0p5()
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.gap_3()
							.child(
								div()
									.flex()
									.min_w_0()
									.items_center()
									.gap_1p5()
									.when_some(filed, |s, (glyph, tint, filed)| {
										s.child(icon(glyph, tint).size_3p5())
											.child(div().flex_none().child(text!(filed)))
									})
									.when(!source.is_empty(), |s| {
										s.child(div().min_w_0().truncate().text_color(p.muted).child(text!(source)))
									}),
							)
							.child(
								div()
									.flex()
									.flex_none()
									.items_center()
									.gap_1p5()
									.text_color(p.muted)
									.child(
										div()
											.id("add-ranges")
											.role(gpui::Role::Image)
											.aria_label(meaning)
											.debug_selector(|| "add-ranges".to_owned())
											.child(icon(mark, p.muted).size_3p5()),
									)
									.child(text!(size)),
							),
					)
					.child(div().text_xs().text_color(p.muted).truncate().child(text!(details))),
			)
			// The name it will be saved under, on the face rather than behind More: it is filled
			// in from what the look turned up, and a name somebody may want to change is not a
			// thing to hide behind a word. Everything else behind More is a thing most people
			// never touch; this is not.
			.child(
				div()
					.flex()
					.items_center()
					.gap_2()
					.text_xs()
					.child(div().flex_none().text_color(p.muted).child(text!("Save as")))
					.child(div().flex_1().min_w_0().child(sheet.name.clone())),
			)
			.child(
				div()
					.id("add-more")
					.role(gpui::Role::Button)
					.aria_label(if sheet.more { "Less" } else { "More" })
					.debug_selector(|| "button:More".to_owned())
					.text_xs()
					.text_color(p.muted)
					.cursor_pointer()
					.hover(move |s| s.text_color(p.text))
					.on_click(cx.listener(|this, _, _, cx| this.toggle_add_more(cx)))
					.child(if sheet.more { "Less" } else { "More" }),
			)
			.when(sheet.more, |s| s.child(self.more_fields(sheet, cx)))
	}

	/// The rest of what can be asked for, one labelled field each, and the folder as a word
	/// that opens the system's picker.
	fn more_fields(&self, sheet: &AddSheet, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
		div()
			.debug_selector(|| "add-more".to_owned())
			.flex()
			.flex_col()
			.gap_1p5()
			.pt_1()
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
			.child(row("Mirrors", sheet.mirrors.clone().into_any_element()))
			.child(row("Checksum", sheet.checksum.clone().into_any_element()))
			.child(row("Range", sheet.range.clone().into_any_element()))
			.child(row("Speed limit", sheet.limit.clone().into_any_element()))
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
}
