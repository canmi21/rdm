//! The list: how it is drawn and sorted, which rows it shows and under which category, and the
//! columns' widths as they are dragged.

use super::*;

/// How the list is drawn. Detailed is the default and the first offered because it is the one
/// that shows progress, speed and size at once; the other two trade that for the file's own
/// picture, at one row each and then at a card each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub enum View {
	/// The table: type, name, size, progress, speed, status, added.
	Detailed,
	/// The system's own icon for the file, and its name. What a file manager shows.
	Thumbnails,
	/// A card with a large icon, or a picture of the file where one can be made.
	Grid,
}

impl View {
	/// In the order the switcher offers them, by how much of a row is words and how much is
	/// picture: the whole table, a row with a picture on it, then cards.
	pub const ALL: [View; 3] = [View::Detailed, View::Thumbnails, View::Grid];
}

/// A column the table can be ordered by. `Added` is the default: the order downloads arrived in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum SortKey {
	Added,
	Name,
	Size,
	Progress,
	Speed,
	Status,
}

/// A fixed-width column of the table; the name takes whatever is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
	Size,
	Progress,
	Speed,
	Status,
	Added,
}

impl Column {
	/// The width a column will not go below. It is a floor, not a taste: enough that the cell is
	/// still read -- "1.2G", a stub of a bar, a truncated word beside its mark -- and no wider,
	/// because a floor anyone would willingly stop at is one that a drag runs into. What every
	/// floor comes to, plus the name's and the chrome around them, is the window's own minimum
	/// width, so the table is never given less room than its floors need. See spec/ui.md.
	pub const MINS: [f32; 5] = [40.0, 40.0, 40.0, 40.0, 40.0];
	/// The widths the columns start with, and go back to on the header's reset.
	pub const DEFAULT_WIDTHS: [f32; 5] = [132.0, 150.0, 84.0, 112.0, 108.0];

	pub fn min(self) -> f32 {
		Self::MINS[self.index()]
	}

	pub(super) fn index(self) -> usize {
		self as usize
	}
}

/// A drag on a column's edge in progress: which column, where the pointer started, and every
/// width as it stood then. A move recomputes the whole row from that snapshot rather than from
/// the row it last left, so a drag back the way it came gives back exactly what it took.
#[derive(Clone, Copy, Debug)]
pub struct Resize {
	pub column: Column,
	pub from_x: gpui::Pixels,
	/// The row as it was drawn when the press landed, which is the geometry the drag works in.
	pub from_widths: [f32; 5],
	/// The widths as they were asked for, which at a narrow window is not the row that was drawn.
	/// A drag that comes to move nothing puts these back, so taking hold of a handle at a window
	/// too narrow to give anything cannot quietly spend what the window is holding back.
	pub asked: [f32; 5],
}

impl Rdm {
	/// Every row the lists are cut from: the downloads, and with the funnel lit, the folder's
	/// other files. A filter or a count reads these, so a file the funnel let in is under All
	/// Tasks, under Completed, and under whichever category its name fits, like a download
	/// that finished.
	pub(crate) fn rows(&self) -> impl Iterator<Item = &Download> {
		let folder = if self.folder_shown { &self.folder_files[..] } else { &[] };
		self.downloads.iter().chain(folder)
	}

	/// The rows the list shows, in the order it shows them: what the sidebar's filter and the
	/// status menu let through.
	pub(crate) fn shown(&self) -> Vec<&Download> {
		let mut rows: Vec<&Download> = self
			.rows()
			.filter(|d| {
				self.passes(self.filter, d)
					&& self.status.is_none_or(|s| d.status == s)
					&& self.worth_a_row(d)
					&& self.under_an_open_folder(d)
			})
			.collect();
		rows.sort_by(|a, b| {
			let order = match self.sort {
				SortKey::Added => a.added.cmp(&b.added),
				SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
				SortKey::Size => a.size.cmp(&b.size),
				SortKey::Progress => a.progress().partial_cmp(&b.progress()).unwrap_or(Ordering::Equal),
				SortKey::Speed => a.speed.cmp(&b.speed),
				SortKey::Status => (a.status as u8).cmp(&(b.status as u8)),
			};
			if self.ascending { order } else { order.reverse() }
		});
		rows
	}

	/// Whether a row is worth showing at all in the list as it stands. Junk is not, while the
	/// preference says so -- except a kind that is filed rather than dropped, which is worth a
	/// row under the category it belongs to, since that is where somebody looking for one looks.
	/// See `download::junk`.
	pub(super) fn worth_a_row(&self, download: &Download) -> bool {
		if !self.preferences.hide_junk {
			return true;
		}
		match crate::download::junk(&download.name) {
			None => true,
			Some(crate::download::Junk::Noise) => false,
			Some(crate::download::Junk::Filed) => matches!(self.filter, Filter::Category(_)),
		}
	}

	/// The categories a row is in: by its name, and for an archive that has been read, by what
	/// it holds. See src/app/indexing.rs.
	pub(crate) fn categories_of(&self, download: &Download) -> Vec<&Category> {
		categories_with_contents(&self.categories, download, &self.contents_of(download))
	}

	/// Whether the sidebar's filter lets a row through, judging a category by `categories_of`.
	pub(crate) fn passes(&self, filter: Filter, download: &Download) -> bool {
		match filter {
			Filter::Category(id) => self.categories_of(download).iter().any(|c| c.id == id),
			other => other.matches(download, &self.categories),
		}
	}

	/// The category a row is drawn as: the filtered one when one is filtered and matches, else
	/// the first that matches. None when nothing does and there is no catch-all.
	pub(super) fn category_shown(&self, download: &Download) -> Option<&Category> {
		// An archive that is a program, or an album, by what it holds wears that icon in every
		// list, the Archives list included: the contents are what it is.
		if let Some(nature) = category::nature(&self.categories, download, &self.contents_of(download))
		{
			return Some(nature);
		}
		let matched = self.categories_of(download);
		if let Filter::Category(id) = self.filter
			&& let Some(c) = matched.iter().find(|c| c.id == id)
		{
			return Some(c);
		}
		matched.first().copied()
	}

	/// The icon a row shows, and the hue it is drawn in: its category's, so the list reads the
	/// way the sidebar does. A plain file, muted, when no category claims it.
	pub(crate) fn category_icon(&self, download: &Download) -> (Icon, gpui::Hsla) {
		match self.category_shown(download) {
			// The icon is the category's; the colour may be the extension's, where the category
			// draws one of its own apart. See `Category::shade` and spec/ui.md.
			Some(c) => (c.icon, self.palette.hue(c.shade(&download.name))),
			None => (Icon::File, self.palette.muted),
		}
	}
}

impl Rdm {
	pub(crate) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
		self.filter = filter;
		// The funnel cuts inside the state, so a status the new state does not hold is not a
		// narrower list but an empty one, under a funnel lit with a word that cannot match. It
		// is dropped rather than kept, and the menu stops offering it. See `Filter::statuses`.
		if self.status.is_some_and(|status| !filter.statuses().contains(&status)) {
			self.status = None;
		}
		cx.notify();
	}

	/// From the funnel's menu, which closes on a choice; `None` is its "All".
	pub(crate) fn set_status(&mut self, status: Option<Status>, cx: &mut Context<Self>) {
		self.status = status;
		self.filter_open = false;
		cx.notify();
	}

	pub(crate) fn toggle_filter_menu(&mut self, open: bool, cx: &mut Context<Self>) {
		self.filter_open = open;
		cx.notify();
	}

	/// The order nothing has been asked for: newest first.
	pub(crate) fn default_order(&self) -> bool {
		self.sort == SortKey::Added && !self.ascending
	}

	/// Three clicks on a title: ascending, descending, then back to the default order. A click on
	/// another title starts that one ascending.
	pub(crate) fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
		if self.sort != key || self.default_order() {
			self.sort = key;
			self.ascending = true;
		} else if self.ascending {
			self.ascending = false;
		} else {
			self.sort = SortKey::Added;
			self.ascending = false;
		}
		cx.notify();
	}

	/// What a column is drawn at. The stored width is what was asked for; this is what the table
	/// has room for, which is less whenever the window is too narrow to hold them all. The
	/// shortfall is shared out in proportion to what each column has to spare above its floor, so
	/// narrowing the window compresses the table evenly rather than crushing one column, and every
	/// column lands exactly on its floor at the window's own minimum width -- which is where that
	/// minimum comes from. The stored widths are untouched, so widening gives back what narrowing
	/// took, to the pixel.
	pub(crate) fn width(&self, column: Column) -> f32 {
		self.drawn()[column.index()]
	}

	pub(crate) fn drawn(&self) -> [f32; 5] {
		let mut widths = self.widths;
		let short = crate::ui::list::NAME_MIN - self.name_width(&widths);
		let spare: f32 = widths.iter().zip(Column::MINS).map(|(w, min)| (w - min).max(0.0)).sum();
		if short <= 0.0 || spare <= 0.0 {
			return widths;
		}
		let taken = short.min(spare);
		for (width, min) in widths.iter_mut().zip(Column::MINS) {
			*width -= (*width - min).max(0.0) / spare * taken;
		}
		widths
	}

	/// The drag starts from what is on screen, not from what was asked for: at a narrow window the
	/// two differ, and the boundary has to leave from under the pointer.
	pub(crate) fn begin_resize(&mut self, column: Column, at: gpui::Pixels) {
		self.resizing =
			Some(Resize { column, from_x: at, from_widths: self.drawn(), asked: self.widths });
	}

	/// What the name column is left once the fixed columns and their handles have taken theirs.
	pub(crate) fn name_width(&self, widths: &[f32; 5]) -> f32 {
		let table =
			f32::from(self.viewport.width) - crate::ui::sidebar::WIDTH - crate::ui::list::TABLE_CHROME;
		table - 5.0 * crate::ui::list::HANDLE_W - widths.iter().sum::<f32>()
	}

	/// Called for every pointer move over the window. The handle is a column's left edge and the
	/// table is anchored at its right, so moving the boundary left widens the column. A move with
	/// the button up ends the drag: the release happened outside the window, unseen.
	///
	/// What widening takes has to come from the left of the handle, and it is taken in the order
	/// the eye expects the squeeze to travel: the name column first, since it is the one holding
	/// the slack, then each fixed column between the name and the handle, nearest first, each down
	/// to its own floor and no further. The boundary only stops once everything left of it is on
	/// its floor -- there is no ceiling derived from any one column, so nothing to snap to when a
	/// press lands, and a floor small enough that the stop is rarely reached at all.
	pub(crate) fn resize_to(&mut self, at: gpui::Pixels, pressed: bool, cx: &mut Context<Self>) {
		let Some(resize) = self.resizing else { return };
		if !pressed {
			self.end_resize(cx);
			return;
		}
		let column = resize.column.index();
		let mut widths = resize.from_widths;
		widths[column] =
			(resize.from_widths[column] - f32::from(at - resize.from_x)).max(resize.column.min());
		// Narrowing owes nothing: the name column takes back what is given up. Widening owes the
		// difference, and asks for it leftwards until it is met or nobody has any left.
		let mut owed = widths[column] - resize.from_widths[column];
		owed -= (self.name_width(&resize.from_widths) - crate::ui::list::NAME_MIN).max(0.0);
		for other in (0..column).rev() {
			if owed <= 0.0 {
				break;
			}
			let spare = (resize.from_widths[other] - Column::MINS[other]).max(0.0).min(owed);
			widths[other] = resize.from_widths[other] - spare;
			owed -= spare;
		}
		// Asked for more than the row had: the boundary stops where the last of it was found.
		if owed > 0.0 {
			widths[column] = (widths[column] - owed).max(resize.column.min());
		}
		// A drag that has come to move nothing leaves the asked-for widths as they were, squeezed
		// or not, so that letting go where the press landed is the same as never having pressed.
		self.widths = if widths == resize.from_widths { resize.asked } else { widths };
		cx.notify();
	}

	pub(crate) fn end_resize(&mut self, cx: &mut Context<Self>) {
		if self.resizing.take().is_some() {
			self.schedule_save(cx);
			cx.notify();
		}
	}

	/// Every column back to the width it started with, from Reset under Appearance in Settings.
	pub(crate) fn reset_widths(&mut self, cx: &mut Context<Self>) {
		self.widths = Column::DEFAULT_WIDTHS;
		self.schedule_save(cx);
		cx.notify();
	}

	/// The toolbar's second button: what the selection can do next, by its state.
	pub(crate) fn act_on_selected(&mut self, cx: &mut Context<Self>) {
		let Some(download) = self.selected() else { return };
		match download.status {
			Status::Downloading => self.pause_selected(cx),
			Status::Completed => self.remove_selected(cx),
			Status::Paused | Status::Queued | Status::Failed => self.resume_selected(cx),
		}
	}

	pub(crate) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
		self.view = view;
		self.schedule_save(cx);
		cx.notify();
	}
}
