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
use crate::ui::backdrop;
use crate::ui::button;
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::slider::Slider;
use crate::ui::text_input::TextInput;
use crate::ui::theme::Palette;

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

/// The limit field, in megabytes a second: empty or `unlimited` is no limit.
pub fn parse_limit(text: &str) -> Result<Option<f64>, String> {
	let text = text.trim();
	let text = text.strip_suffix("MB/s").or_else(|| text.strip_suffix("mb/s")).unwrap_or(text).trim();
	if text.is_empty() || text.eq_ignore_ascii_case("unlimited") {
		return Ok(None);
	}
	match text.parse::<f64>() {
		Ok(megabytes) if megabytes > 0.0 && megabytes.is_finite() => Ok(Some(megabytes)),
		_ => Err("A limit is a number of MB/s, or empty for none.".to_owned()),
	}
}

/// A limit as the field shows it: tenths where they matter, whole numbers otherwise.
pub fn format_limit(megabytes: f64) -> String {
	let text = format!("{megabytes:.1}");
	text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

/// Where along the limit slider a limit sits. The scale runs from the slider's low end to its high
/// end logarithmically, since a megabyte more matters at five and not at ninety; no limit is the
/// far right, past the scale.
pub fn limit_position(limit: Option<f64>, (low, high): (f64, f64)) -> f32 {
	let Some(megabytes) = limit else { return 1.0 };
	let span = (high / low).ln();
	let along = if span > 0.0 { ((megabytes.max(low) / low).ln() / span) as f32 } else { 0.0 };
	along.clamp(0.0, 1.0) * SCALE
}

/// The limit a place on the slider stands for, rounded to what a person would type: tenths below
/// ten, whole numbers above. None past the middle of the gap after the scale.
pub fn limit_at(position: f32, (low, high): (f64, f64)) -> Option<f64> {
	if position > (1.0 + SCALE) / 2.0 {
		return None;
	}
	let along = f64::from((position / SCALE).clamp(0.0, 1.0));
	let megabytes = low * (high / low).powf(along);
	Some(if megabytes < 10.0 { (megabytes * 10.0).round() / 10.0 } else { megabytes.round() })
}

/// Where a limit slider's position lands at a drag's level: on even steps of its log scale, five
/// a decade at the coarsest, then twenty, then a hundred, or on no limit. Five a decade is the
/// preferred numbers -- 1, 1.6, 2.5, 4, 6.3, 10 -- so the coarse places are both evenly spaced on
/// the track and round as the field writes them. See spec/ui.md, "A slider reads the hand's intent
/// from its pauses".
pub fn limit_snap(level: usize, position: f32, (low, high): (f64, f64)) -> f32 {
	if position > (1.0 + SCALE) / 2.0 {
		return 1.0;
	}
	let Some(per_decade) = [5.0, 20.0, 100.0].get(level) else { return position };
	let steps = ((high / low).log10() * per_decade).round().max(1.0) as f32;
	let step = SCALE / steps;
	((position / step).round() * step).min(SCALE)
}

/// The step a part of a file is rounded to at a drag's level: a tenth, a hundredth and a thousandth
/// of the file, each put on the nearest 1, 2 or 5 times a power of ten below it, so the bytes in
/// the field are a round number at every level. None at exact.
pub fn range_step(size: u64, level: usize) -> Option<u64> {
	let share = [10, 100, 1000].get(level)?;
	let rough = (size / share).max(1);
	let decade = 10u64.pow(rough.ilog10());
	Some([5, 2, 1].map(|m| m * decade).into_iter().find(|step| *step <= rough).unwrap_or(1))
}

/// Bytes on the nearest step, with the end of the file a place of its own rather than the step
/// before it.
pub fn round_bytes(bytes: u64, size: u64, step: u64) -> u64 {
	let near = (bytes + step / 2) / step * step;
	if size.saturating_sub(near) < step / 2 { size } else { near.min(size) }
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
		.child(hover_icon(if open { Icon::ChevronUp } else { Icon::ChevronDown }, id, p.muted, Some(p.text)).size_3p5())
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
					let size = owner.clone();
					Slider::new("Range", vec![0.0, 1.0])
						.snap(move |level, at, cx| {
							let size = size.upgrade().and_then(|this| {
								this.read(cx).adding.as_ref()?.found.as_ref()?.probe.size.filter(|s| *s > 0)
							});
							let Some(size) = size else { return at };
							let Some(step) = range_step(size, level) else { return at };
							let bytes = (f64::from(at) * size as f64).round() as u64;
							(round_bytes(bytes, size, step) as f64 / size as f64) as f32
						})
						.on_change(move |handle, at, level, _, cx| {
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
		sheet.details = false;
		sheet.confirm = None;
		let Some(url) = parse_address(&text) else {
			sheet.problem = Some(Problem { summary: "That is not a web address.".to_owned(), detail: None });
			cx.notify();
			return;
		};
		sheet.problem = None;
		sheet.checking = Some((url.clone(), self.engine.inspect(url)));
		cx.notify();
	}

	/// What the second screen's fields ask for, read and checked: each empty when it was left so.
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
		let speed_limit = parse_limit(&text(&sheet.limit))?.map(|megabytes| (megabytes * 1_048_576.0) as u64);
		Ok(crate::app::Asked {
			connections,
			directory: sheet.folder.as_ref().map(|p| p.display().to_string()),
			mirrors: Vec::new(),
			checksum,
			range,
			speed_limit,
		})
	}

	/// The file the second screen is about, and the fields a look can fill filled from it: the
	/// name the server gives, and the whole file as the range, so a part is asked for by moving an
	/// end rather than by working out a number.
	fn accept(&mut self, found: Found, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		let name = found.probe.file_name.clone();
		let whole = found.probe.size.filter(|_| found.probe.ranges);
		sheet.found = Some(found);
		sheet.confirm = None;
		sheet.problem = None;
		sheet.details = false;
		let (field, start, end) = (sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone());
		let fill = |field: &Entity<TextInput>, text: &str, cx: &mut Context<Self>| {
			field.update(cx, |input, cx| {
				if input.content.trim().is_empty() {
					input.set_content(text, cx);
				}
			});
		};
		fill(&field, &name, cx);
		if let Some(size) = whole {
			fill(&start, "0", cx);
			fill(&end, &size.to_string(), cx);
		}
		self.follow_limit(cx);
		cx.notify();
	}

	/// Download anyway, for a page, a script or a stylesheet: on to the second screen with it.
	fn download_anyway(&mut self, cx: &mut Context<Self>) {
		let Some(confirm) = self.adding.as_mut().and_then(|sheet| sheet.confirm.take()) else { return };
		self.accept(confirm.found, cx);
	}

	/// The arrow before the title: back to the address, forgetting the file and what was filled in
	/// from it. What was typed by hand -- the checksum, the limit, the folder -- stays.
	pub(crate) fn back_to_address(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		sheet.found = None;
		sheet.confirm = None;
		sheet.problem = None;
		sheet.more = false;
		let input = sheet.input.clone();
		let filled = [sheet.name.clone(), sheet.range_start.clone(), sheet.range_end.clone()];
		for field in filled {
			field.update(cx, |field, cx| field.set_content("", cx));
		}
		window.focus(&input.read(cx).focus(), cx);
		cx.notify();
	}

	/// More options, or fewer: the folder, the limit and the range, shown or put away.
	pub(crate) fn toggle_add_more(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.more = !sheet.more;
			cx.notify();
		}
	}

	pub(crate) fn toggle_add_details(&mut self, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.adding {
			sheet.details = !sheet.details;
			cx.notify();
		}
	}

	/// The low and high ends of the limit slider, in MB/s, as Settings has them.
	fn limit_scale(&self) -> (f64, f64) {
		self.preferences.limit_slider()
	}

	/// The limit field changed, by hand or by the slider: the slider follows it.
	fn follow_limit(&mut self, cx: &mut Context<Self>) {
		cx.notify();
		let Some(sheet) = &self.adding else { return };
		let Ok(limit) = parse_limit(&sheet.limit.read(cx).content) else { return };
		// A limit the slider wrote stays where the slider put it: its places are even on the track
		// and the field writes them rounded, and moving the handle to the rounded one would take it
		// off the place the next drag starts from.
		let scale = self.limit_scale();
		if sheet.limit_slider.read(cx).handles().first().is_some_and(|at| limit_at(*at, scale) == limit) {
			return;
		}
		let at = limit_position(limit, scale);
		sheet.limit_slider.clone().update(cx, |slider, cx| slider.set(vec![at], cx));
	}

	/// The limit slider moved, or `ctl slide` moved it: the field says the limit it stands for.
	pub(crate) fn slide_limit(&mut self, position: f32, cx: &mut Context<Self>) {
		let Some(sheet) = &self.adding else { return };
		let text = limit_at(position, self.limit_scale()).map(format_limit).unwrap_or_default();
		sheet.limit.clone().update(cx, |input, cx| input.set_content(&text, cx));
	}

	/// A range field changed: the slider's handles follow the two ends.
	fn follow_range(&mut self, cx: &mut Context<Self>) {
		cx.notify();
		let Some(sheet) = &self.adding else { return };
		let Some(size) = sheet.found.as_ref().and_then(|f| f.probe.size).filter(|size| *size > 0) else {
			return;
		};
		let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
		let start = read(&sheet.range_start).unwrap_or(0).min(size);
		let end = read(&sheet.range_end).unwrap_or(size).min(size);
		let along = |bytes: u64| (bytes as f64 / size as f64) as f32;
		let handles = vec![along(start.min(end)), along(end.max(start))];
		sheet.range_slider.clone().update(cx, |slider, cx| slider.set(handles, cx));
	}

	/// A range handle moved: its field says the byte it stands for, a byte short of the other end.
	/// At a drag's level the byte is put back on that level's step, which a position cannot hold
	/// exactly for a file of gigabytes.
	pub(crate) fn slide_range(&mut self, handle: usize, position: f32, level: usize, cx: &mut Context<Self>) {
		let Some(sheet) = &self.adding else { return };
		let Some(size) = sheet.found.as_ref().and_then(|f| f.probe.size) else { return };
		let read = |field: &Entity<TextInput>| field.read(cx).content.trim().parse::<u64>().ok();
		let (start, end) = (read(&sheet.range_start).unwrap_or(0), read(&sheet.range_end).unwrap_or(size));
		let at = (f64::from(position.clamp(0.0, 1.0)) * size as f64).round() as u64;
		let at = range_step(size, level).map_or(at, |step| round_bytes(at, size, step));
		let (field, bytes) = if handle == 0 {
			(sheet.range_start.clone(), at.min(end.saturating_sub(1)))
		} else {
			(sheet.range_end.clone(), at.max(start + 1).min(size))
		};
		field.update(cx, |input, cx| input.set_content(&bytes.to_string(), cx));
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

	/// The engine's answer about the address, if it has arrived. Called by the event pump. A file
	/// moves on to the second screen; a page, a script or a stylesheet is asked about; a failure
	/// stays on the first screen, said in a line.
	pub(crate) fn poll_add(&mut self, cx: &mut Context<Self>) {
		let Some(sheet) = &mut self.adding else { return };
		let Some((url, receiver)) = &sheet.checking else { return };
		let Ok(answer) = receiver.try_recv() else { return };
		let url = url.clone();
		sheet.checking = None;
		match answer {
			Ok(inspection) => {
				let found = Found { url, probe: inspection.probe };
				match crate::engine::inspect::confirmation(found.probe.content_type.as_deref()) {
					Some(kind) => {
						sheet.confirm = Some(Confirm { found, kind, links: inspection.links, added: Vec::new() })
					}
					None => self.accept(found, cx),
				}
			}
			Err(failure) => {
				sheet.problem = Some(Problem { summary: failure.summary, detail: Some(failure.detail) })
			}
		}
		cx.notify();
	}

	/// One of the files a page links to. The sheet stays up so several can be taken.
	fn add_link(&mut self, index: usize, cx: &mut Context<Self>) {
		let Some(confirm) = self.adding.as_mut().and_then(|s| s.confirm.as_mut()) else { return };
		let Some(link) = confirm.links.get(index).cloned() else { return };
		if confirm.added.contains(&index) {
			return;
		}
		let source = confirm.found.url.to_string();
		confirm.added.push(index);
		let asked =
			crate::app::Asked { connections: self.preferences.connections, ..Default::default() };
		self.add_request(link.url, Some(link.name), Some(source), asked, cx);
	}

	/// Drawn over everything from the window root; a click outside the sheet closes it.
	pub(crate) fn add_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(sheet) = &self.adding else { return deferred(div()).priority(2) };
		let checking = sheet.checking.is_some();
		let typed = !sheet.input.read(cx).content.trim().is_empty();
		let second = sheet.found.is_some();
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
											cx.listener(|this, _, window, cx| this.back_to_address(window, cx)),
										))
									})
									.child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).child(text!("New Task"))),
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
						Some(found) => s.child(self.found_notice(found, sheet, cx)),
						None => s
							.child(sheet.input.clone())
							.when_some(sheet.confirm.as_ref(), |s, confirm| s.child(self.confirm_notice(confirm, cx))),
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
									.when(second, |s| {
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
									s.child(button(
										p,
										"add-confirm",
										Icon::Download,
										"Download",
										true,
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

	/// What went wrong in a line, and behind Details the whole text: an error is long enough to push
	/// the sheet apart when it is shown as it comes.
	fn problem_notice(&self, problem: &Problem, open: bool, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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
	fn confirm_notice(&self, confirm: &Confirm, cx: &mut Context<Self>) -> impl IntoElement + use<> {
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

	/// What most downloads never touch: the folder as a word that opens the system's picker, a limit
	/// of the download's own, the part of the file wanted when the server serves parts, and a
	/// checksum the finished file must match. The limit and the part each have a slider for setting
	/// them roughly and fields for setting them exactly.
	fn more_fields(
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
		const END: f32 = 112.0;
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
			.gap_2()
			.child(row(
				"Folder",
				boxed()
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.child(text!(folder)),
					)
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
					div()
						.flex()
						.items_start()
						.gap_2()
						.text_xs()
						.child(label("Range", LINE))
						.child(
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
												.px_2()
												.text_color(p.muted)
												.truncate()
												.child(text!(part)),
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

	#[test]
	fn a_drag_lands_on_round_rates_and_round_bytes_level_by_level() {
		let scale = (1.0, 100.0);
		let rate = |level, mb: f64| limit_at(limit_snap(level, limit_position(Some(mb), scale), scale), scale);
		let coarse: Vec<Option<f64>> =
			(0..=10).map(|k| limit_at(limit_snap(0, k as f32 * SCALE / 10.0, scale), scale)).collect();
		let preferred = [1.0, 1.6, 2.5, 4.0, 6.3, 10.0, 16.0, 25.0, 40.0, 63.0, 100.0];
		assert_eq!(coarse, preferred.map(Some).to_vec(), "the coarsest is even and reads as round");
		assert_eq!(rate(0, 4.1), Some(4.0));
		assert_eq!(rate(0, 37.0), Some(40.0));
		assert_eq!(limit_snap(0, 0.97, scale), 1.0, "and no limit is a place of its own");
		assert_eq!(rate(1, 37.4), Some(35.0), "then twenty a decade");
		assert_eq!(rate(2, 3.46), Some(3.5), "then a hundred");
		let size = 3_888_513_024;
		assert_eq!(range_step(size, 0), Some(200_000_000));
		assert_eq!(range_step(size, 1), Some(20_000_000));
		assert_eq!(range_step(size, 2), Some(2_000_000));
		assert_eq!(range_step(size, 3), None, "exact is the byte");
		assert_eq!(range_step(1_033_297, 0), Some(100_000));
		assert_eq!(round_bytes(1_234_567_890, size, 200_000_000), 1_200_000_000);
		assert_eq!(round_bytes(3_850_000_000, size, 200_000_000), size, "near the end is the end");
		assert_eq!(round_bytes(0, size, 200_000_000), 0);
	}

	#[test]
	fn the_limit_slider_is_a_log_scale_with_no_limit_past_its_end() {
		let scale = (1.0, 100.0);
		assert_eq!(limit_position(None, scale), 1.0, "no limit is the far right");
		assert_eq!(limit_position(Some(1.0), scale), 0.0);
		assert!((limit_position(Some(10.0), scale) - 0.45).abs() < 1e-4, "ten is halfway along the scale");
		assert!((limit_position(Some(100.0), scale) - SCALE).abs() < 1e-6);
		assert_eq!(limit_at(0.0, scale), Some(1.0));
		assert_eq!(limit_at(0.45, scale), Some(10.0));
		assert_eq!(limit_at(SCALE, scale), Some(100.0));
		assert_eq!(limit_at(1.0, scale), None, "the far right is no limit");
		assert_eq!(limit_at(0.2, scale), Some(2.8), "tenths below ten");
		assert_eq!(parse_limit(""), Ok(None));
		assert_eq!(parse_limit("5"), Ok(Some(5.0)));
		assert_eq!(parse_limit("2.5 MB/s"), Ok(Some(2.5)));
		assert!(parse_limit("fast").is_err());
		assert_eq!(format_limit(2.8), "2.8");
		assert_eq!(format_limit(40.0), "40");
	}
}
