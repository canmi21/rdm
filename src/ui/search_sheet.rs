//! Search, a sheet inside the main window like Settings: the field across the top, a chip for each
//! category holding a match with its count, three segmented filters -- where, how recent, how big
//! -- then the results, a row each, and a line of keys at the foot. See spec/ui.md, "Search".

use gpui::{
	Context, Hsla, IntoElement, Role, SharedString, deferred, div, prelude::*, px, uniform_list,
};

use crate::app::Rdm;
use crate::download::{format_added, format_bytes};
use crate::search::{Age, Filters, Place, Size};
use crate::ui::icon::{Icon, icon};
use crate::ui::theme::Palette;
use crate::ui::{LeavesFocus, backdrop, icon_button};

const SHEET_W: f32 = 760.0;
const SHEET_H: f32 = 520.0;
const ROW_H: f32 = 40.0;

/// A segmented control's segments: what each is called and the filters it sets.
type Segments = Vec<(&'static str, Filters)>;

impl Rdm {
	pub(crate) fn search_sheet(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let Some(sheet) = &self.search else { return div().into_any_element() };
		let found = self.search_results(cx);
		let filters = sheet.filters;
		let gathering = sheet.gathered.is_none();

		let header = div()
			.flex()
			.items_center()
			.gap_2()
			.flex_none()
			.px_3()
			.py_2()
			.border_b_1()
			.border_color(p.border)
			.child(div().flex_1().min_w_0().child(sheet.input.clone()))
			.child(icon_button(
				p,
				"search-close",
				Icon::X,
				"Close",
				true,
				cx.listener(|this, _, _, cx| this.close_search(cx)),
			));

		// The categories holding a match, each with its count, after All.
		let mut chips =
			vec![self.search_chip(p, None, "All".to_owned(), None, found.all, sheet.kind.is_none(), cx)];
		for (id, count) in &found.counts {
			let Some(category) = self.categories.iter().find(|c| c.id == *id) else { continue };
			let tint = p.hue(category.color);
			chips.push(self.search_chip(
				p,
				Some(*id),
				category.name.clone(),
				Some((category.icon, tint)),
				*count,
				sheet.kind == Some(*id),
				cx,
			));
		}
		let chip_row = div().flex().flex_wrap().gap_1().px_3().pt_2().children(chips);

		let place: Segments = vec![
			("Anywhere", Filters { place: Place::Anywhere, ..filters }),
			("Files", Filters { place: Place::Files, ..filters }),
			("In archives", Filters { place: Place::Archives, ..filters }),
		];
		let age: Segments = vec![
			("Any time", Filters { age: Age::Any, ..filters }),
			("Today", Filters { age: Age::Day, ..filters }),
			("Week", Filters { age: Age::Week, ..filters }),
			("Month", Filters { age: Age::Month, ..filters }),
			("Year", Filters { age: Age::Year, ..filters }),
		];
		let size: Segments = vec![
			("Any size", Filters { size: Size::Any, ..filters }),
			("< 1 MB", Filters { size: Size::Small, ..filters }),
			("1-100 MB", Filters { size: Size::Medium, ..filters }),
			("100 MB-1 GB", Filters { size: Size::Large, ..filters }),
			("> 1 GB", Filters { size: Size::Huge, ..filters }),
		];
		let filter_row = div()
			.flex()
			.flex_wrap()
			.gap_2()
			.px_3()
			.py_2()
			.border_b_1()
			.border_color(p.border)
			.child(segmented(p, "place", place, filters, cx))
			.child(segmented(p, "age", age, filters, cx))
			.child(segmented(p, "size", size, filters, cx));

		let count = found.shown.len();
		let results = if count == 0 {
			div()
				.flex_1()
				.flex()
				.items_center()
				.justify_center()
				.text_color(p.muted)
				.child(if gathering { "Reading the download folder…" } else { "Nothing matches" })
				.into_any_element()
		} else {
			uniform_list(
				"search-results",
				count,
				cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
					range.map(|at| this.search_row(at, cx).into_any_element()).collect::<Vec<_>>()
				}),
			)
			.track_scroll(&sheet.scroll)
			.flex_1()
			.min_h_0()
			.px_1p5()
			.py_1()
			.into_any_element()
		};

		let summary = match &sheet.gathered {
			None => "Reading the download folder…".to_owned(),
			Some(gathered) => {
				let files = gathered.catalog.items.iter().filter(|i| i.inside.is_none()).count();
				format!(
					"{count} of {} · {files} files, {} archives looked inside",
					found.all, gathered.catalog.archives
				)
			}
		};
		let foot = div()
			.flex()
			.items_center()
			.justify_between()
			.flex_none()
			.h(px(28.0))
			.px_3()
			.border_t_1()
			.border_color(p.border)
			.text_xs()
			.text_color(p.muted)
			.child(summary)
			.child("↑↓ Choose   ↵ Open   ⌘↵ Show in folder   esc Close");

		deferred(
			backdrop(p).child(
				div()
					.id("search-card")
					.role(Role::Dialog)
					.aria_label("Search")
					.flex()
					.flex_col()
					.w(px(SHEET_W))
					.h(px(SHEET_H))
					.max_h(gpui::relative(0.9))
					.max_w(gpui::relative(0.95))
					.rounded_lg()
					.border_1()
					.border_color(p.border)
					.bg(p.window)
					.shadow_lg()
					.overflow_hidden()
					.on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_search(cx)))
					// Up and down reach here from the field, which has no use for them; so does a
					// command-return, which the field's own return does not answer.
					.on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
						match event.keystroke.key.as_str() {
							"up" => this.move_search_selection(-1, cx),
							"down" => this.move_search_selection(1, cx),
							"enter" if event.keystroke.modifiers.platform => this.open_result(true, cx),
							"escape" => this.close_search(cx),
							_ => return,
						}
						cx.stop_propagation();
					}))
					.child(header)
					.child(chip_row)
					.child(filter_row)
					.child(results)
					.child(foot),
			),
		)
		.priority(2)
		.into_any_element()
	}

	#[allow(clippy::too_many_arguments)]
	fn search_chip(
		&self,
		p: Palette,
		kind: Option<u64>,
		name: String,
		glyph: Option<(Icon, Hsla)>,
		count: usize,
		on: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement + use<> {
		div()
			.id(SharedString::from(format!("search-kind:{}", kind.map_or(0, |k| k + 1))))
			.flex()
			.items_center()
			.gap_1()
			.px_2()
			.py_0p5()
			.rounded_full()
			.border_1()
			.border_color(if on { p.accent } else { p.border })
			.when(on, |s| s.bg(p.selection))
			.when(!on, move |s| s.hover(move |s| s.bg(p.hover)))
			.cursor_pointer()
			.text_xs()
			.leaves_focus()
			.on_click(cx.listener(move |this, _, _, cx| this.set_search_kind(kind, cx)))
			.when_some(glyph, |s, (glyph, tint)| s.child(icon(glyph, tint).size_3()))
			.child(name)
			.child(div().text_color(p.muted).child(count.to_string()))
	}

	fn search_row(&self, at: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let p = self.palette;
		let found = self.search_results(cx);
		let (Some(sheet), Some(index)) = (&self.search, found.shown.get(at)) else {
			return div().into_any_element();
		};
		let Some(gathered) = &sheet.gathered else { return div().into_any_element() };
		let item = &gathered.catalog.items[*index];
		let selected = sheet.selected == at;
		let (glyph, tint) =
			match gathered.kinds[*index].and_then(|id| self.categories.iter().find(|c| c.id == id)) {
				_ if item.dir => (Icon::Folder, p.muted),
				Some(category) => (category.icon, p.hue(category.shade(&item.name))),
				None => (Icon::File, p.muted),
			};
		let folder = self.paths.as_ref().map(|paths| paths.downloads.clone());
		let relative = |path: &std::path::Path| {
			folder
				.as_deref()
				.and_then(|f| path.strip_prefix(f).ok())
				.map_or_else(|| path.display().to_string(), |r| r.display().to_string())
		};
		// Where it is: its folder under the download folder, or the archive and the folder inside it.
		let place = match &item.inside {
			Some(inside) => {
				let within = inside.rsplit_once('/').map_or("", |(dir, _)| dir);
				let archive = relative(&item.path);
				if within.is_empty() { archive } else { format!("{archive} › {within}") }
			}
			None => item
				.path
				.parent()
				.map(relative)
				.filter(|r| !r.is_empty())
				.unwrap_or_else(|| "Download folder".to_owned()),
		};
		div()
			.id(("search-row", at))
			// The list's width, which a row under a uniform list has to be told.
			.w_full()
			.flex()
			.items_center()
			.gap_2p5()
			.h(px(ROW_H))
			.px_2()
			.rounded_sm()
			.cursor_pointer()
			.when(selected, |s| s.bg(p.selection))
			.when(!selected, move |s| s.hover(move |s| s.bg(p.hover)))
			.on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
				this.select_search_result(at, cx);
				if event.click_count() == 2 {
					this.open_result(false, cx);
				}
			}))
			.child(icon(glyph, tint).size_4().flex_none())
			.child(
				div()
					.flex_1()
					.min_w_0()
					.flex()
					.flex_col()
					.child(div().truncate().child(item.name.clone()))
					.child(
						div()
							.flex()
							.items_center()
							.gap_1()
							.text_xs()
							.text_color(p.muted)
							.when(item.inside.is_some(), |s| {
								s.child(icon(Icon::Archive, p.muted).size_3().flex_none())
							})
							.child(div().truncate().child(place)),
					),
			)
			.child(
				div()
					.flex_none()
					.flex()
					.flex_col()
					.items_end()
					.text_xs()
					.text_color(p.muted)
					.child(if item.dir { String::new() } else { format_bytes(item.size) })
					.child(
						chrono::DateTime::from_timestamp(item.modified, 0)
							.map(|t| format_added(t.with_timezone(&chrono::Local)))
							.unwrap_or_default(),
					),
			)
			.into_any_element()
	}
}

/// One track with its segments inside, the one matching the filters lit.
fn segmented(
	p: Palette,
	name: &'static str,
	segments: Segments,
	current: Filters,
	cx: &mut Context<Rdm>,
) -> impl IntoElement {
	div().flex().p_0p5().rounded_md().bg(p.track).children(segments.into_iter().enumerate().map(
		move |(i, (word, filters))| {
			let on = filters == current;
			div()
				.id(SharedString::from(format!("search-{name}:{i}")))
				.px_2()
				.py_0p5()
				.rounded_sm()
				.text_xs()
				.cursor_pointer()
				.text_color(if on { p.text } else { p.muted })
				.when(on, |s| s.bg(p.selection))
				.when(!on, move |s| s.hover(move |s| s.bg(p.hover).text_color(p.text)))
				.leaves_focus()
				.on_click(cx.listener(move |this, _, _, cx| this.set_search_filters(filters, cx)))
				.child(word)
		},
	))
}
