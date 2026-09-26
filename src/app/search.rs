//! The search sheet's side of the main view: opening it, gathering the download folder in the
//! background, matching on every key, and what a result does when it is chosen. See src/search.rs
//! and spec/ui.md, "Search".

use std::cell::RefCell;
use std::sync::{Arc, mpsc};

use gpui::{AppContext, Context, Entity, UniformListScrollHandle, Window};

use crate::app::Rdm;
use crate::search::{self, Catalog, Filters};
use crate::ui::icon::Icon;
use crate::ui::text_input::TextInput;

/// The catalog as the sheet uses it: the items, and each one's category by id -- judged on the
/// same thread that gathered them, since fifty thousand names against every category's pattern is
/// not a frame's work.
pub(crate) struct Gathered {
	pub catalog: Catalog,
	pub kinds: Vec<Option<u64>>,
}

/// What the sheet shows for a query and filters, kept until either changes.
#[derive(Clone, PartialEq)]
struct Asked {
	query: String,
	filters: Filters,
	kind: Option<u64>,
}

pub(crate) struct SearchSheet {
	pub input: Entity<TextInput>,
	pub gathered: Option<Arc<Gathered>>,
	receiver: Option<mpsc::Receiver<Gathered>>,
	pub filters: Filters,
	/// The category chip lit, by id; None for all of them.
	pub kind: Option<u64>,
	pub selected: usize,
	pub scroll: UniformListScrollHandle,
	/// The results for the last query, and every category's count among the matches before the
	/// category is applied.
	found: RefCell<Option<(Asked, Arc<Found>)>>,
}

#[derive(Default)]
pub(crate) struct Found {
	pub shown: Vec<usize>,
	pub counts: Vec<(u64, usize)>,
	pub all: usize,
}

impl Rdm {
	pub(crate) fn search_open(&self) -> bool {
		self.search.is_some()
	}

	/// Opens the sheet with its field focused and starts gathering the folder; a second press just
	/// refocuses the field.
	pub(crate) fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let input = self.begin_search(cx);
		window.focus(&input.read(cx).focus(), cx);
	}

	/// The sheet up, with no window to focus it in: what `open_search` does short of the focus, and
	/// what the debug build's control socket asks for. The field, for whoever focuses it.
	pub(crate) fn begin_search(&mut self, cx: &mut Context<Self>) -> Entity<TextInput> {
		if let Some(sheet) = &self.search {
			return sheet.input.clone();
		}
		let rdm = cx.entity();
		let input = cx.new(|cx| {
			TextInput::new("Search the download folder, archives included", cx)
				.with_leading(Icon::Search)
				.on_confirm(move |_, _, cx| rdm.update(cx, |this, cx| this.open_result(false, cx)))
		});
		cx.observe(&input, |this, _, cx| {
			if let Some(sheet) = &mut this.search {
				sheet.selected = 0;
			}
			cx.notify();
		})
		.detach();
		let receiver = self.gather(cx);
		self.search = Some(SearchSheet {
			input: input.clone(),
			gathered: None,
			receiver,
			filters: Filters::default(),
			kind: None,
			selected: 0,
			scroll: UniformListScrollHandle::new(),
			found: RefCell::new(None),
		});
		cx.notify();
		input
	}

	/// The folder walked on the engine's runtime, off the window.
	fn gather(&self, _cx: &mut Context<Self>) -> Option<mpsc::Receiver<Gathered>> {
		let folder = self.paths.as_ref()?.downloads.clone();
		let known = self.archives.clone();
		let categories = self.categories.clone();
		let (sender, receiver) = mpsc::channel();
		let _ = self.engine.run(async move {
			let _ = tokio::task::spawn_blocking(move || {
				let catalog = search::gather(&folder, &known);
				let catch_all = categories.iter().find(|c| c.is_catch_all()).map(|c| c.id);
				let kinds = catalog
					.items
					.iter()
					.map(|item| {
						if item.dir {
							return None;
						}
						categories
							.iter()
							.find(|c| !c.is_catch_all() && c.matches_name(&item.name))
							.map(|c| c.id)
							.or(catch_all)
					})
					.collect();
				let _ = sender.send(Gathered { catalog, kinds });
			})
			.await;
		});
		Some(receiver)
	}

	pub(crate) fn close_search(&mut self, cx: &mut Context<Self>) {
		self.search = None;
		cx.notify();
	}

	/// The catalog, once it has been gathered; the archives read along the way go to the index and
	/// the store. True when it arrived.
	pub(crate) fn poll_search(&mut self) -> bool {
		let Some(sheet) = &mut self.search else { return false };
		let Some(receiver) = &sheet.receiver else { return false };
		let Ok(mut gathered) = receiver.try_recv() else { return false };
		sheet.receiver = None;
		for (path, indexed) in std::mem::take(&mut gathered.catalog.learned) {
			if let Some(store) = &self.store
				&& let Err(error) = store.save_archive(&path, &indexed)
			{
				eprintln!("could not keep the index of {path}: {error:#}");
			}
			self.archives.insert(path, indexed);
		}
		sheet.gathered = Some(Arc::new(gathered));
		*sheet.found.borrow_mut() = None;
		true
	}

	/// The results for what the sheet asks now, worked out again only when the query, a filter or
	/// the catalog changed.
	pub(crate) fn search_results(&self, cx: &gpui::App) -> Arc<Found> {
		let Some(sheet) = &self.search else { return Arc::default() };
		let Some(gathered) = &sheet.gathered else { return Arc::default() };
		let asked = Asked {
			query: sheet.input.read(cx).content.to_string(),
			filters: sheet.filters,
			kind: sheet.kind,
		};
		if let Some((known, found)) = &*sheet.found.borrow()
			&& *known == asked
		{
			return found.clone();
		}
		let now = chrono::Local::now().timestamp();
		let matched = search::find(&gathered.catalog.items, &asked.query, asked.filters, now);
		let mut counts: Vec<(u64, usize)> = Vec::new();
		for at in &matched {
			if let Some(id) = gathered.kinds[*at] {
				match counts.iter_mut().find(|(each, _)| *each == id) {
					Some((_, count)) => *count += 1,
					None => counts.push((id, 1)),
				}
			}
		}
		// In the sidebar's order, so the chips read as the sidebar does.
		counts.sort_by_key(|(id, _)| {
			self.categories.iter().position(|c| c.id == *id).unwrap_or(usize::MAX)
		});
		let all = matched.len();
		let shown = match asked.kind {
			Some(kind) => matched.into_iter().filter(|at| gathered.kinds[*at] == Some(kind)).collect(),
			None => matched,
		};
		let found = Arc::new(Found { shown, counts, all });
		*sheet.found.borrow_mut() = Some((asked, found.clone()));
		found
	}

	pub(crate) fn set_search_filters(&mut self, filters: Filters, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.search {
			sheet.filters = filters;
			sheet.selected = 0;
		}
		cx.notify();
	}

	pub(crate) fn set_search_kind(&mut self, kind: Option<u64>, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.search {
			sheet.kind = kind;
			sheet.selected = 0;
		}
		cx.notify();
	}

	/// Up or down the results, scrolled into view.
	pub(crate) fn move_search_selection(&mut self, by: isize, cx: &mut Context<Self>) {
		let count = self.search_results(cx).shown.len();
		let Some(sheet) = &mut self.search else { return };
		if count == 0 {
			return;
		}
		sheet.selected = sheet.selected.saturating_add_signed(by).min(count - 1);
		sheet.scroll.scroll_to_item(sheet.selected, gpui::ScrollStrategy::Center);
		cx.notify();
	}

	pub(crate) fn select_search_result(&mut self, at: usize, cx: &mut Context<Self>) {
		if let Some(sheet) = &mut self.search {
			sheet.selected = at;
		}
		cx.notify();
	}

	/// The chosen result opened, or shown in its folder. An entry inside an archive cannot be
	/// opened where it is, so either shows the archive.
	pub(crate) fn open_result(&mut self, reveal: bool, cx: &mut Context<Self>) {
		let found = self.search_results(cx);
		let Some(sheet) = &self.search else { return };
		let (Some(gathered), Some(at)) = (&sheet.gathered, found.shown.get(sheet.selected)) else {
			return;
		};
		let item = &gathered.catalog.items[*at];
		if reveal || item.inside.is_some() {
			crate::reveal::show(&item.path, &self.preferences.file_manager);
		} else {
			crate::reveal::open(&item.path);
		}
	}
}
