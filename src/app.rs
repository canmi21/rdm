//! The root view: the downloads, how they are filtered and ordered, and which one is selected.

use std::cmp::Ordering;
use std::time::Duration;

use gpui::{Context, IntoElement, Render, Task, Window, div, prelude::*, px};

use crate::download::{self, Download, Filter, Status};
use crate::ui::theme::{self, Palette};

/// How the list is drawn. Detailed is the default because it is the one that shows progress,
/// speed and size at once; the others trade that for density or for a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
	Detailed,
	Compact,
	Grid,
}

impl View {
	pub const ALL: [View; 3] = [View::Detailed, View::Compact, View::Grid];
}

/// A column the table can be ordered by. `Added` is the order downloads arrived in and the
/// default; it has no column of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
	Added,
	Name,
	Size,
	Progress,
	Speed,
	Status,
}

pub struct Rdm {
	pub(crate) downloads: Vec<Download>,
	pub(crate) filter: Filter,
	/// A second cut within the sidebar's filter, from the chips above the list.
	pub(crate) status: Option<Status>,
	pub(crate) sort: SortKey,
	pub(crate) ascending: bool,
	pub(crate) view: View,
	pub(crate) selected: Option<u64>,
	/// Set at the top of every render from the window's state, read by everything below it.
	pub(crate) palette: Palette,
	_tick: Task<()>,
}

impl Rdm {
	pub fn new(cx: &mut Context<Self>) -> Self {
		// Drives the mock rows forward so the list moves while there is no transfer engine
		// behind it. The real engine will push state changes instead of being polled.
		let tick = cx.spawn(async move |this, cx| {
			loop {
				cx.background_executor().timer(Duration::from_millis(500)).await;
				let alive = this.update(cx, |this, cx| {
					this.advance();
					cx.notify();
				});
				if alive.is_err() {
					break;
				}
			}
		});
		Self {
			downloads: download::sample(),
			filter: Filter::All,
			status: None,
			sort: SortKey::Added,
			ascending: true,
			view: View::Detailed,
			selected: None,
			palette: theme::palette(true),
			_tick: tick,
		}
	}

	/// The rows the list shows, in the order it shows them.
	pub(crate) fn shown(&self) -> Vec<&Download> {
		let mut rows: Vec<&Download> = self
			.downloads
			.iter()
			.filter(|d| self.filter.matches(d) && self.status.is_none_or(|s| d.status == s))
			.collect();
		rows.sort_by(|a, b| {
			let order = match self.sort {
				SortKey::Added => a.id.cmp(&b.id),
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

	pub(crate) fn selected(&self) -> Option<&Download> {
		self.selected.and_then(|id| self.downloads.iter().find(|d| d.id == id))
	}

	fn selected_mut(&mut self) -> Option<&mut Download> {
		self.selected.and_then(|id| self.downloads.iter_mut().find(|d| d.id == id))
	}

	pub(crate) fn set_filter(&mut self, filter: Filter, cx: &mut Context<Self>) {
		self.filter = filter;
		cx.notify();
	}

	/// Clicking the active chip clears it, so the chips need no separate "all".
	pub(crate) fn toggle_status(&mut self, status: Status, cx: &mut Context<Self>) {
		self.status = if self.status == Some(status) { None } else { Some(status) };
		cx.notify();
	}

	/// A second click on the same column flips the direction; a click on another starts ascending.
	pub(crate) fn sort_by(&mut self, key: SortKey, cx: &mut Context<Self>) {
		if self.sort == key {
			self.ascending = !self.ascending;
		} else {
			self.sort = key;
			self.ascending = true;
		}
		cx.notify();
	}

	pub(crate) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
		self.view = view;
		cx.notify();
	}

	pub(crate) fn select(&mut self, id: u64, cx: &mut Context<Self>) {
		self.selected = if self.selected == Some(id) { None } else { Some(id) };
		cx.notify();
	}

	// TODO: opens a dialog asking for a URL once there is an input to type it into.
	pub(crate) fn add(&mut self, cx: &mut Context<Self>) {
		let id = self.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
		self.downloads.push(Download {
			id,
			name: format!("download-{id}.bin"),
			url: format!("https://example.org/files/download-{id}.bin"),
			size: 250_000_000,
			received: 0,
			speed: 0,
			status: Status::Queued,
		});
		self.selected = Some(id);
		cx.notify();
	}

	pub(crate) fn pause_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(download) = self.selected_mut() {
			download.status = Status::Paused;
			download.speed = 0;
			cx.notify();
		}
	}

	pub(crate) fn resume_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(download) = self.selected_mut() {
			download.status = Status::Downloading;
			download.speed = 12_000_000;
			cx.notify();
		}
	}

	pub(crate) fn remove_selected(&mut self, cx: &mut Context<Self>) {
		if let Some(id) = self.selected.take() {
			self.downloads.retain(|d| d.id != id);
			cx.notify();
		}
	}

	fn advance(&mut self) {
		for download in self.downloads.iter_mut().filter(|d| d.status == Status::Downloading) {
			download.received = (download.received + download.speed / 2).min(download.size);
			if download.received == download.size {
				download.status = Status::Completed;
				download.speed = 0;
			}
		}
	}
}

impl Render for Rdm {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		self.palette = theme::palette(window.is_window_active());
		let p = self.palette;
		div()
			.flex()
			.flex_col()
			.size_full()
			// Zed's density: a 13px UI face, and everything else in rems of it.
			.text_size(px(13.0))
			.bg(p.window)
			.text_color(p.text)
			.child(self.render_toolbar(cx))
			.child(
				div().flex().flex_1().min_h_0().child(self.render_sidebar(cx)).child(
					div()
						.flex()
						.flex_col()
						.flex_1()
						.min_w_0()
						.child(self.render_list(cx))
						.child(self.render_detail(cx)),
				),
			)
	}
}
