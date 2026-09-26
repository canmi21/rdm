//! A card in the grid: a face showing the file, its name, its progress and its size. The face is
//! the file itself where a picture can be made -- fitted whole, never cropped -- a document's first
//! blocks drawn as a page, source in a fixed-width face, an archive's contents, or an icon. See
//! spec/ui.md, "A card shows the file".

use gpui::{Context, IntoElement, div, prelude::*, px};

use super::{progress_bar, tinted_icon};
use crate::app::Rdm;
use crate::download::{Download, format_bytes};
use crate::thumbnail::{Block, BlockKind, Preview};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;

/// How tall a card's face is: room for a page's worth of a document's opening. Its width is the
/// card's less the card's padding.
const FACE_H: f32 = 92.0;
const FACE_W: f32 = super::CARD - 16.0;
/// How far a picture's shape may be from the face's and still fill it: a crop this small takes
/// nothing anybody would miss.
const SAME_SHAPE: f32 = 0.06;
/// The room kept around a picture of another shape, so the sides it would touch stand a little off
/// the edge rather than against it; the other two sides keep whatever the shape leaves them.
const INSET: f32 = 6.0;

/// How a picture sits on the face.
#[derive(Debug, PartialEq, Eq)]
enum Fit {
	/// Its shape is the face's, near enough: filling the face, cut by no more than `SAME_SHAPE`.
	Cover,
	/// Whole, inside the face less `INSET` all round.
	Inset,
}

fn fit((width, height): (f32, f32), (face_w, face_h): (f32, f32)) -> Fit {
	if width <= 0.0 || height <= 0.0 {
		return Fit::Inset;
	}
	let off = (width / height) / (face_w / face_h);
	if (off - 1.0).abs() <= SAME_SHAPE { Fit::Cover } else { Fit::Inset }
}
/// How many of an archive's names a face lists before it says how many more.
const ARCHIVE_ROWS: usize = 7;

/// The fixed-width face source is set in, as each system names its own.
fn mono() -> &'static str {
	if cfg!(target_os = "macos") {
		"Menlo"
	} else if cfg!(windows) {
		"Consolas"
	} else {
		"DejaVu Sans Mono"
	}
}

impl Rdm {
	fn card_face(&self, download: &Download) -> gpui::AnyElement {
		let p = self.palette;
		let path = download.path.as_deref().map(std::path::Path::new);
		// An archive already read by the index shows what it holds; nothing more is opened for it.
		if let Some(indexed) = self.archive_of(download)
			&& indexed.error.is_none()
			&& !indexed.entries.is_empty()
		{
			return archive_face(p, &crate::index::outline(&indexed.entries));
		}
		let preview = path.and_then(|path| self.thumbnails.borrow_mut().preview(path));
		match preview {
			// Fitted whole, with the panel showing either side: the card is the file at a glance,
			// Rounded itself, as the face is: gpui clips a child to a rectangle, so the face's corners
			// do not cut it, and the radius lands on the picture as fitted rather than on its box.
			Some(Preview::Picture(picture)) => {
				let size = picture.size(0);
				let shape = (i32::from(size.width) as f32, i32::from(size.height) as f32);
				let drawn = gpui::img(picture).rounded_sm();
				match fit(shape, (FACE_W, FACE_H)) {
					Fit::Cover => drawn.size_full().object_fit(gpui::ObjectFit::Cover).into_any_element(),
					// Sized outright and centered by the face: a percentage inside a padded box
					// resolves against the box, not what the padding leaves.
					Fit::Inset => drawn
						.w(px(FACE_W - 2.0 * INSET))
						.h(px(FACE_H - 2.0 * INSET))
						.object_fit(gpui::ObjectFit::Contain)
						.into_any_element(),
				}
			}
			Some(Preview::Lines(lines)) => text_face(p)
				.text_size(px(6.0))
				.children(lines.into_iter().map(|line| div().flex_none().truncate().child(line)))
				.into_any_element(),
			Some(Preview::Code(lines)) => text_face(p)
				.font_family(mono())
				.text_size(px(5.5))
				.line_height(px(8.0))
				// Leading spaces are the indentation, and a line is cut rather than wrapped.
				.children(
					lines
						.into_iter()
						.map(|line| div().flex_none().whitespace_nowrap().overflow_hidden().child(line)),
				)
				.into_any_element(),
			Some(Preview::Document(blocks)) => document_face(p, blocks),
			Some(Preview::Icon(icon)) => gpui::img(icon).size_12().into_any_element(),
			None => tinted_icon(self.category_icon(download)).size_8().into_any_element(),
		}
	}

	pub(super) fn card(
		&self,
		download: &Download,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = p.status(download.status);
		self
			.item(download, cx)
			.flex()
			.flex_col()
			.gap_1p5()
			.w(px(super::CARD))
			.p_2()
			.child(
				div()
					.flex()
					.h(px(FACE_H))
					.justify_center()
					.items_center()
					.rounded_sm()
					.overflow_hidden()
					.bg(p.panel)
					.child(self.card_face(download)),
			)
			.child(div().truncate().text_xs().child(download.name.clone()))
			.child(progress_bar(p, download, tint))
			.child(
				div()
					.flex()
					.justify_between()
					.items_center()
					.text_xs()
					.text_color(p.muted)
					.child(format_bytes(download.size.max(download.received)))
					.child(icon(Icon::for_status(download.status), tint).size_3()),
			)
	}
}

/// The face's text laid from the top left, cut at the bottom.
fn text_face(p: Palette) -> gpui::Div {
	div().size_full().flex().flex_col().px_1p5().py_1().overflow_hidden().text_color(p.muted)
}

/// A document's opening as a page: headings larger and in the text's color, paragraphs wrapped,
/// items bulleted, quotes ruled and code fixed-width.
fn document_face(p: Palette, blocks: Vec<Block>) -> gpui::AnyElement {
	text_face(p)
		.gap(px(1.5))
		.children(blocks.into_iter().map(|block| {
			let line = div().text_size(px(5.5)).line_height(px(7.0));
			let drawn = match block.kind {
				BlockKind::Heading(level) => div()
					.text_size(px(if level <= 1 { 8.5 } else { 7.0 }))
					.line_height(px(if level <= 1 { 10.0 } else { 8.5 }))
					.font_weight(gpui::FontWeight::SEMIBOLD)
					.text_color(p.text)
					.truncate()
					.child(block.text),
				BlockKind::Paragraph => line.max_h(px(21.0)).overflow_hidden().child(block.text),
				BlockKind::Item => line
					.flex()
					.gap_1()
					.child(div().flex_none().child("\u{2022}"))
					.child(div().min_w_0().truncate().child(block.text)),
				BlockKind::Quote => line
					.pl_1()
					.border_l_1()
					.border_color(p.border)
					.max_h(px(14.0))
					.overflow_hidden()
					.child(block.text),
				BlockKind::Code => {
					line.font_family(mono()).whitespace_nowrap().overflow_hidden().child(block.text)
				}
			};
			// Laid at their own heights and cut by the face, never squeezed to fit it.
			drawn.flex_none()
		}))
		.into_any_element()
}

/// An archive as somebody opening it would see it: its top names, folders first marked as folders,
/// each with its size, and how many more there are.
fn archive_face(p: Palette, tops: &[crate::index::Top]) -> gpui::AnyElement {
	let total: u64 = tops.iter().map(|t| t.size).sum();
	let shown = tops.len().min(ARCHIVE_ROWS);
	let more = tops.len() - shown;
	let mut ordered: Vec<&crate::index::Top> = tops.iter().collect();
	ordered.sort_by_key(|t| !t.dir);
	text_face(p)
		.text_size(px(6.0))
		.line_height(px(9.0))
		.child(
			div()
				.flex()
				.justify_between()
				.text_color(p.text)
				.child(match tops.len() {
					1 => "1 item".to_owned(),
					n => format!("{n} items"),
				})
				.child(format_bytes(total)),
		)
		.children(ordered.into_iter().take(shown).map(|top| {
			div()
				.flex_none()
				.flex()
				.items_center()
				.gap_1()
				.child(
					icon(if top.dir { Icon::Folder } else { Icon::File }, p.muted).size(px(6.5)).flex_none(),
				)
				.child(div().flex_1().min_w_0().truncate().child(top.name.clone()))
				.child(div().flex_none().child(format_bytes(top.size)))
		}))
		.when(more > 0, |s| s.child(div().child(format!("and {more} more"))))
		.into_any_element()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_picture_of_the_faces_shape_fills_it_and_any_other_sits_inset() {
		let face = (FACE_W, FACE_H);
		assert_eq!(fit((1400.0, 920.0), face), Fit::Cover, "the same shape");
		assert_eq!(fit((1500.0, 1000.0), face), Fit::Cover, "within a few percent");
		assert_eq!(fit((1920.0, 1080.0), face), Fit::Inset, "wider");
		assert_eq!(fit((1024.0, 1024.0), face), Fit::Inset, "square");
		assert_eq!(fit((0.0, 10.0), face), Fit::Inset, "no size to judge by");
	}
}
