//! Adding a download is a sheet inside the main window: one field over a dimmed list, filled
//! from the clipboard when what is there reads as an address. Enter or Add has the engine look
//! at the address first; a file is queued at once, a web page is said to be one, with the files
//! it links to offered instead. See spec/ui.md.

use std::sync::mpsc::Receiver;

use gpui::{Context, Entity, IntoElement, Window, deferred, div, prelude::*, px};
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
	pub error: Option<String>,
}

pub struct Page {
	pub url: Url,
	pub links: Vec<Link>,
	pub added: Vec<usize>,
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
				self.adding =
					Some(AddSheet { input: input.clone(), checking: None, page: None, error: None });
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
	/// on its answer, which the pump collects.
	pub(crate) fn submit_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
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
		sheet.checking = Some((url.clone(), self.engine.inspect(url)));
		cx.notify();
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
				let name = inspection.probe.file_name.clone();
				self.add_request(url, Some(name), None, cx);
				self.close_add(cx);
			}
			Err(message) => sheet.error = Some(message),
		}
		cx.notify();
	}

	/// The page itself, saved as a file after all.
	fn add_page_anyway(&mut self, cx: &mut Context<Self>) {
		let Some(page) = self.adding.as_ref().and_then(|s| s.page.as_ref()) else { return };
		let url = page.url.clone();
		self.add_request(url, None, None, cx);
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
		self.add_request(link.url, Some(link.name), Some(source), cx);
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
							.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child("Add Task"))
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
								.child(error),
						)
					})
					.when_some(sheet.page.as_ref(), |s, page| s.child(self.page_notice(page, cx)))
					.child(
						div()
							.flex()
							.items_center()
							.justify_between()
							.child(
								div()
									.text_xs()
									.text_color(p.muted)
									.when(checking, |s| s.child("Looking at the address")),
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
					.child(div().flex_none().child(link.name.clone()))
					.child(div().flex_1().min_w_0().truncate().text_color(p.muted).child(address))
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
						div().text_xs().text_color(p.warning).child("This address is a web page, not a file."),
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
					div().text_xs().text_color(p.muted).child("Files the page links to; press one to add it"),
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
}
