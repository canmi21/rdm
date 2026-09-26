//! Adding a download is a sheet inside the main window, titled New Task, in two screens. The first
//! is the address, filled from the clipboard when what is there reads as one; Continue has the
//! engine look at it, and only a file moves on, while a page, a script or a stylesheet is asked
//! about first and a failure is said there. The second is what was found and what to keep it as,
//! and Download queues it. See spec/ui.md.

use std::sync::mpsc::Receiver;

use gpui::{App, Context, Entity, IntoElement, Window, deferred, div, prelude::*, px, text};
use reqwest::Url;

use crate::app::Rdm;
use crate::engine::{Failure, Inspection, Link};
use crate::rules::resolve::Resolution;
use crate::ui::backdrop;
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::slider::Slider;
use crate::ui::text_input::TextInput;
use crate::ui::theme::Palette;
use crate::ui::{button, button_after};

mod details;
mod fields;
mod flow;
mod view;

pub use fields::*;

/// The clipboard is read only up to this length: an address is never longer, and a document
/// that happens to be on the clipboard is not worth parsing.
const CLIPBOARD_LIMIT: usize = 1000;

/// How much of the limit slider is the scale; the rest, past its high end, is no limit at all.
const SCALE: f32 = 0.9;

/// The sheet while it is up.
pub struct AddSheet {
	pub input: Entity<TextInput>,
	/// The engine is looking at this address; its answer is polled with the events.
	pub checking: Option<(Url, Receiver<Result<Inspection, Failure>>)>,
	/// What went wrong, said in a line, with the whole text behind Details when there is one.
	pub problem: Option<Problem>,
	pub details: bool,
	/// The address answered as a page, a script or a stylesheet: asked about on the first screen.
	pub confirm: Option<Confirm>,
	/// The file the second screen is about. How it downloads is not asked; the download's window
	/// changes that while it runs. See spec/ui.md.
	pub found: Option<Found>,
	/// The name to save under, on the face of the second screen.
	pub name: Entity<TextInput>,
	/// More options: the folder, a limit of its own, the part of the file wanted, as the first byte
	/// and the byte it stops before, each with a slider beside its field, and a checksum.
	pub more: bool,
	pub checksum: Entity<TextInput>,
	pub folder: Option<std::path::PathBuf>,
	pub limit: Entity<TextInput>,
	pub limit_slider: Entity<Slider>,
	pub range_start: Entity<TextInput>,
	pub range_end: Entity<TextInput>,
	pub range_slider: Entity<Slider>,
	/// What the rules say about the file on the second screen: being worked out, then the answer.
	/// See spec/rules.md.
	pub resolving: Option<Receiver<Resolution>>,
	pub resolved: Option<Resolution>,
	/// The checksum the rules filled in, so one the user typed over it is told apart: a typed
	/// checksum is the user's word and vouches for every mirror.
	pub filled_checksum: Option<String>,
	/// The third screen: a mirror was found and no checksum, and the user is asked.
	pub asking: bool,
}

/// A checksum as the field shows it, `sha256:` and the hex.
fn checksum_text(checksum: &crate::engine::Checksum) -> String {
	let algo = match checksum {
		crate::engine::Checksum::Sha256(_) => "sha256",
		crate::engine::Checksum::Sha512(_) => "sha512",
		crate::engine::Checksum::Md5(_) => "md5",
	};
	format!("{algo}:{}", checksum.expected())
}

pub struct Problem {
	pub summary: String,
	pub detail: Option<String>,
}

pub struct Found {
	pub url: Url,
	pub probe: crate::engine::Probe,
}

/// A page, a script or a stylesheet, waiting for Download anyway; a page's links beside it.
pub struct Confirm {
	pub found: Found,
	pub kind: &'static str,
	pub links: Vec<Link>,
	pub added: Vec<usize>,
}

/// A word that opens or closes what is under it, with a chevron that turns: More options, Details.
fn disclosure(
	p: Palette,
	id: &'static str,
	label: &'static str,
	open: bool,
	on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
	div()
		.id(id)
		.role(gpui::Role::Button)
		.aria_label(label)
		.aria_expanded(open)
		.debug_selector(move || format!("button:{label}"))
		.group(id)
		.flex()
		.flex_none()
		.items_center()
		.gap_1()
		.text_xs()
		.text_color(p.muted)
		.cursor_pointer()
		.hover(move |s| s.text_color(p.text))
		.on_click(on_click)
		.child(label)
		.child(
			hover_icon(if open { Icon::ChevronUp } else { Icon::ChevronDown }, id, p.muted, Some(p.text))
				.size_3p5(),
		)
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
				let mut field = |placeholder: &'static str, unit: Option<&'static str>| {
					let confirm = cx.entity();
					cx.new(|cx| {
						let input = TextInput::new(placeholder, cx)
							.on_confirm(move |_, _, cx| confirm.update(cx, |this, cx| this.submit_add(cx)));
						match unit {
							Some(unit) => input.with_trailing(unit),
							None => input,
						}
					})
				};
				let (name, checksum, limit, range_start, range_end) = (
					field("As the server names it", None),
					field("sha256, sha512 or md5", None),
					field("Unlimited", Some("MB/s")),
					field("0", None),
					field("End of file", None),
				);
				cx.observe(&name, |_, _, cx| cx.notify()).detach();
				cx.observe(&checksum, |_, _, cx| cx.notify()).detach();
				// Each field and its slider follow each other: a drag writes the field, and the field,
				// typed or written, moves the slider.
				cx.observe(&limit, |this, _, cx| this.follow_limit(cx)).detach();
				cx.observe(&range_start, |this, _, cx| this.follow_range(cx)).detach();
				cx.observe(&range_end, |this, _, cx| this.follow_range(cx)).detach();
				let owner = cx.weak_entity();
				let limit_slider = cx.new(|_| {
					let scale = owner.clone();
					Slider::new("Speed limit", vec![1.0])
						.snap(move |level, at, cx| {
							let scale = scale.upgrade().map(|this| this.read(cx).limit_scale());
							scale.map(|scale| limit_snap(level, at, scale)).unwrap_or(at)
						})
						.on_change(move |_, at, _, _, cx| {
							let _ = owner.update(cx, |this, cx| this.slide_limit(at, cx));
						})
				});
				let owner = cx.weak_entity();
				let range_slider = cx.new(|_| {
					Slider::new("Range", vec![0.0, 1.0]).on_change(move |handle, at, level, _, cx| {
						let _ = owner.update(cx, |this, cx| this.slide_range(handle, at, level, cx));
					})
				});
				self.adding = Some(AddSheet {
					input: input.clone(),
					checking: None,
					problem: None,
					details: false,
					confirm: None,
					found: None,
					name,
					checksum,
					more: false,
					folder: None,
					limit,
					limit_slider,
					range_start,
					range_end,
					range_slider,
					resolving: None,
					resolved: None,
					filled_checksum: None,
					asking: false,
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
				&& sheet.confirm.is_none()
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

	/// Enter, Continue or Download. On the first screen the address is handed to the engine to
	/// look at, and what happens next depends on its answer, which the pump collects. On the
	/// second, the file is queued with what the fields ask for.
	pub(crate) fn submit_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		if sheet.found.is_some() {
			// The settings' default; the download's window changes it while it runs.
			let connections = self.preferences.connections;
			let asked = match self.asked(connections, cx) {
				Ok(asked) => asked,
				Err(message) => {
					if let Some(sheet) = &mut self.adding {
						sheet.problem = Some(Problem { summary: message, detail: None });
					}
					cx.notify();
					return;
				}
			};
			match self.mirrors_for(&asked, cx) {
				Some(mirrors) => self.download_found(asked, mirrors, cx),
				None => {
					if let Some(sheet) = &mut self.adding {
						sheet.asking = true;
						sheet.problem = None;
					}
					cx.notify();
				}
			}
			return;
		}
		let text = sheet.input.read(cx).content.trim().to_owned();
		if text.is_empty() {
			return;
		}
		sheet.details = false;
		sheet.confirm = None;
		let Some(url) = parse_address(&text) else {
			sheet.problem =
				Some(Problem { summary: "That is not a web address.".to_owned(), detail: None });
			cx.notify();
			return;
		};
		sheet.problem = None;
		sheet.checking = Some((url.clone(), self.engine.inspect(url)));
		cx.notify();
	}
}
