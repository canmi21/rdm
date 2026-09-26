//! A row of the middle view: the system's picture for the file, its name and status on the first
//! line, and on the second what matters for where it stands -- how far and how fast while it moves,
//! where from and when once it is done, why while it is failed -- with a bar under a download that
//! is part way. See spec/ui.md, "The middle view is a row of two lines".

use gpui::{Context, IntoElement, div, prelude::*, px};

use super::{percent, progress_bar, status_label, tinted_icon};
use crate::app::Rdm;
use crate::download::{
	Download, Status, format_added, format_bytes, format_duration, format_speed,
};

/// A row's height and its picture's: two lines of text and a bar, beside a picture that spans them.
const ROW_H: f32 = 50.0;
const PICTURE: f32 = 30.0;

impl Rdm {
	pub(super) fn thumbnail_row(
		&self,
		download: &Download,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		let p = self.palette;
		let tint = p.status(download.status);
		let category = self.categories_of(download).first().map(|c| c.name.clone());
		let part_way = matches!(download.status, Status::Downloading | Status::Paused | Status::Queued)
			&& download.received > 0;
		let right =
			if matches!(download.status, Status::Downloading | Status::Paused) && download.size > 0 {
				percent(download).to_string()
			} else {
				format_added(download.added)
			};
		self
			.item(download, cx)
			.flex()
			.items_center()
			.gap_2p5()
			.h(px(ROW_H))
			.px_2()
			.child(self.folder_indent(download))
			.child(self.thumbnail(download, PICTURE))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.gap_0p5()
					.child(
						div()
							.flex()
							.items_center()
							.gap_2()
							.child(div().flex_1().min_w_0().truncate().child(download.name.clone()))
							.child(div().flex_none().text_xs().child(status_label(download, tint))),
					)
					.child(
						div()
							.flex()
							.gap_2()
							.text_xs()
							.text_color(if download.status == Status::Failed { p.failure } else { p.muted })
							.child(
								div().flex_1().min_w_0().truncate().child(detail(download, category.as_deref())),
							)
							.child(div().flex_none().text_color(p.muted).child(right)),
					)
					.when(part_way, |s| s.child(progress_bar(p, download, tint))),
			)
	}

	/// The picture for a row: the system's own where there is one, the category's glyph where
	/// there is not. See src/thumbnail/.
	/// A picture file shows itself, cropped to the square; everything else the system's icon. The
	/// square is kept whatever fills it, so every row's words start at one edge.
	fn thumbnail(&self, download: &Download, size: f32) -> gpui::AnyElement {
		let path = download.path.as_deref().map(std::path::Path::new);
		let own = path.and_then(|path| match self.thumbnails.borrow_mut().preview(path) {
			Some(crate::thumbnail::Preview::Picture(picture)) => Some(picture),
			_ => None,
		});
		let system = || path.and_then(|path| self.thumbnails.borrow_mut().of(path));
		let inside = match (own, system()) {
			(Some(picture), _) => gpui::img(picture)
				.size_full()
				.object_fit(gpui::ObjectFit::Cover)
				.rounded_sm()
				.into_any_element(),
			(None, Some(icon)) => gpui::img(icon).size_full().into_any_element(),
			(None, None) => {
				tinted_icon(self.category_icon(download)).size(px(size * 0.7)).into_any_element()
			}
		};
		div()
			.flex()
			.flex_none()
			.size(px(size))
			.items_center()
			.justify_center()
			.child(inside)
			.into_any_element()
	}
}

/// The second line's words, by where the download stands. A finished file says where it came from,
/// or for one the folder holds with no address, the category it is filed under.
fn detail(download: &Download, category: Option<&str>) -> String {
	let so_far = || match download.size {
		0 => format_bytes(download.received),
		size => format!("{} of {}", format_bytes(download.received), format_bytes(size)),
	};
	match download.status {
		Status::Downloading => {
			let mut parts = vec![so_far(), format_speed(download.speed)];
			if let Some(left) = download.remaining() {
				parts.push(format!("{} left", format_duration(left)));
			}
			parts.join(" \u{b7} ")
		}
		Status::Paused => so_far(),
		Status::Queued => match download.size {
			0 => "Waiting for a place".to_owned(),
			size => format!("Waiting for a place \u{b7} {}", format_bytes(size)),
		},
		Status::Failed => download.error.clone().unwrap_or_else(|| "Failed".to_owned()),
		Status::Completed => {
			let mut parts = vec![format_bytes(download.size.max(download.received))];
			if let Some(host) =
				reqwest::Url::parse(&download.url).ok().and_then(|u| u.host_str().map(str::to_owned))
			{
				parts.push(host);
			} else if let Some(category) = category {
				parts.push(category.to_owned());
			}
			parts.join(" \u{b7} ")
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_second_line_says_what_matters_for_where_the_download_stands() {
		let mut rows = crate::download::sample();
		let row = &mut rows[0];
		row.url = "https://example.com/a.iso".to_owned();
		row.size = 4_000_000;
		row.received = 1_000_000;
		row.speed = 1_000_000;
		row.status = Status::Downloading;
		assert_eq!(detail(row, None), "1.0 MB of 4.0 MB \u{b7} 1.0 MB/s \u{b7} 3s left");
		row.status = Status::Completed;
		assert_eq!(detail(row, Some("Disk Images")), "4.0 MB \u{b7} example.com");
		row.url.clear();
		assert_eq!(detail(row, Some("Disk Images")), "4.0 MB \u{b7} Disk Images");
		row.status = Status::Failed;
		row.error = Some("The server said 404".to_owned());
		assert_eq!(detail(row, None), "The server said 404");
	}
}
