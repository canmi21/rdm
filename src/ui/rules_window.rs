//! The rules, merged from every layer, as a table in a window of their own, drawn the way the main
//! window's detailed list is: a header of column titles whose boundaries drag, dense rows that
//! select, and a foot of tabs and buttons that act on the selected row. A rule's row names the hosts
//! its checksum is read from and the hosts that mirror it, an authority marked where it appears;
//! the row's tooltip says how, step by step. Nothing here is typed: a rule is written in a file, and
//! the custom folder is a press away. See spec/rules.md.

use gpui::{
	Animation, AnimationExt, Context, ElementId, Entity, IntoElement, MouseButton, MouseDownEvent,
	MouseMoveEvent, Render, ScrollHandle, SharedString, Subscription, Transformation, Window, div,
	percentage, prelude::*, px,
};

use crate::app::Rdm;
use crate::rules::checksum::Source;
use crate::rules::{Choice, Compiled, Layer};
use crate::ui::download_window::{Title, chrome};
use crate::ui::icon::{Icon, hover_icon, icon};
use crate::ui::icon_button;
use crate::ui::theme::{self, Palette};
use crate::ui::tooltip::tooltip;

/// The columns whose width is set, in the order drawn; what a rule matches takes the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
	Name,
	Checksum,
	Mirrors,
	Layer,
	Priority,
}

const COLUMNS: [Column; 5] =
	[Column::Name, Column::Checksum, Column::Mirrors, Column::Layer, Column::Priority];
const WIDTHS: [f32; 5] = [128.0, 150.0, 150.0, 60.0, 50.0];
/// A column is never narrower than this, nor wider than the most.
const NARROWEST: f32 = 40.0;
const WIDEST: f32 = 480.0;
/// The space a boundary takes between two columns, as the main list's does.
const HANDLE: f32 = 12.0;
/// The main list's measures, so the two tables read alike.
const HEADER_H: f32 = 24.0;
const ROW_H: f32 = 26.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
	All,
	Rules,
	Families,
	Authorities,
	Choices,
	Problems,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
	Entry,
	Family,
	Authority,
	Choice,
	Problem,
}

/// A host a rule reaches, and whether it is an authority -- trusted to answer for what it mirrors.
#[derive(Clone)]
struct Host {
	name: String,
	authority: bool,
}

/// One row of the table, whatever kind of thing it stands for.
struct Line {
	kind: Kind,
	/// Unique within the kind: what the selection remembers.
	key: String,
	glyph: Icon,
	name: String,
	matched: String,
	/// Where the checksum is read from, in the order tried, and where else the file is served.
	checksum: Vec<Host>,
	mirrors: Vec<Host>,
	/// The whole of it, for the tooltip: what the columns cut short, and how each step is taken.
	detail: String,
	layer: Option<Layer>,
	file: Option<String>,
	priority: Option<i32>,
	/// Where it stands among the rows of its kind, and how many there are, for moving.
	place: usize,
	of: usize,
}

fn layer_name(layer: Layer) -> &'static str {
	match layer {
		Layer::BuiltIn => "Built in",
		Layer::Synced => "Synced",
		Layer::Custom => "Custom",
	}
}

const NONE: &str = "\u{2014}";

/// A template as the match column shows it: without the scheme, which every rule shares and which
/// took the narrow column's first eight characters.
fn bare(template: &str) -> String {
	template.split_once("://").map_or(template, |(_, rest)| rest).to_owned()
}

/// The host of an address template, or of the address a rule matched when the template starts from
/// it, as a sum file beside the file does; None where the host is itself a part to fill.
fn host_of(template: &str, pattern: &str) -> Option<String> {
	let template = if template.starts_with("{url}") { pattern } else { template };
	let host = template.split("://").nth(1)?.split('/').next()?;
	(!host.contains('{') && !host.is_empty()).then(|| host.to_owned())
}

/// Each host once, in the order first met.
fn hosts(names: impl IntoIterator<Item = String>, rules: &Compiled) -> Vec<Host> {
	let mut out: Vec<Host> = Vec::new();
	for name in names {
		if !out.iter().any(|h| h.name == name) {
			out.push(Host { authority: rules.is_authority(&name), name });
		}
	}
	out
}

/// One checksum source in words, for the tooltip.
fn step(source: &Source, pattern: &str) -> String {
	match source {
		Source::Json { url, pick, .. } => {
			format!("the field {pick} of {}", host_of(url, pattern).unwrap_or_else(|| url.clone()))
		}
		Source::Sidecar { url, .. } => {
			format!(
				"a sum file beside it, {}",
				url.replace("{url}", &host_of(url, pattern).unwrap_or_default())
			)
		}
		Source::Sums { url, like, .. } => format!(
			"a list of sums named {} among the files {} lists",
			like.join(" or "),
			host_of(url, pattern).unwrap_or_else(|| url.clone())
		),
	}
}

/// Every row the rules make, in the order they are tried within each kind.
fn lines(rules: &Compiled) -> Vec<Line> {
	let mut out = Vec::new();
	let blank = |kind: Kind, key: &str, glyph: Icon, name: String, matched: String| Line {
		kind,
		key: key.to_owned(),
		glyph,
		detail: format!("{name}\n{matched}"),
		name,
		matched,
		checksum: Vec::new(),
		mirrors: Vec::new(),
		layer: None,
		file: None,
		priority: None,
		place: 0,
		of: 0,
	};
	let trusted = |host: &Host| if host.authority { " (an authority)" } else { "" };
	for (place, entry) in rules.entries.iter().enumerate() {
		let checksum = hosts(
			entry.checksum.iter().filter_map(|s| host_of(s.first_url_template(), &entry.pattern)),
			rules,
		);
		let mirrors = hosts(entry.mirror.iter().filter_map(|m| host_of(m, &entry.pattern)), rules);
		let mut detail = format!("{}\nMatches {}", entry.id, entry.pattern);
		if entry.checksum.is_empty() {
			detail.push_str("\nNo checksum of its own");
		} else {
			detail.push_str("\nThe checksum, in the order tried:");
			for (index, source) in entry.checksum.iter().enumerate() {
				let host = host_of(source.first_url_template(), &entry.pattern)
					.and_then(|name| checksum.iter().find(|h| h.name == name))
					.map(trusted)
					.unwrap_or("");
				detail.push_str(&format!("\n  {}. {}{host}", index + 1, step(source, &entry.pattern)));
			}
		}
		if !entry.mirror.is_empty() {
			detail.push_str("\nAlso served at:");
			for mirror in &entry.mirror {
				detail.push_str(&format!("\n  {mirror}"));
			}
		}
		out.push(Line {
			checksum,
			mirrors,
			detail,
			layer: Some(entry.layer),
			file: Some(entry.file.clone()),
			priority: Some(entry.priority),
			place,
			of: rules.entries.len(),
			..blank(Kind::Entry, &entry.id, Icon::Globe, entry.id.clone(), bare(&entry.pattern))
		});
	}
	for (place, family) in rules.families.iter().enumerate() {
		let first = family.prefixes.first().cloned().unwrap_or_default();
		let mirrors = hosts(family.prefixes.iter().skip(1).filter_map(|p| host_of(p, p)), rules);
		out.push(Line {
			mirrors,
			detail: format!(
				"{}\nThe same tree under every prefix:\n  {}\nNo checksum of its own: New Task asks for one before a mirror is used",
				family.id,
				family.prefixes.join("\n  ")
			),
			layer: Some(family.layer),
			file: Some(family.file.clone()),
			priority: Some(family.priority),
			place,
			of: rules.families.len(),
			..blank(Kind::Family, &family.id, Icon::Server, family.id.clone(), format!("{}\u{2026}", bare(&first)))
		});
	}
	for authority in &rules.authorities {
		// What it answers for: the rules that read a checksum from it or send a download to it.
		let users: Vec<String> = rules
			.entries
			.iter()
			.filter(|e| {
				e.checksum
					.iter()
					.any(|s| host_of(s.first_url_template(), &e.pattern).as_deref() == Some(authority))
					|| e.mirror.iter().any(|m| host_of(m, &e.pattern).as_deref() == Some(authority))
			})
			.map(|e| e.id.clone())
			.collect();
		let matched = if users.is_empty() { NONE.to_owned() } else { users.join(", ") };
		out.push(Line {
			detail: format!(
				"{authority}\nMay answer for a source it mirrors: its checksum holds a download from any mirror\nRules that reach it: {matched}"
			),
			..blank(Kind::Authority, authority, Icon::CircleCheck, authority.clone(), matched.clone())
		});
	}
	for domain in &rules.domains {
		let (glyph, said) = match domain.mirror {
			Choice::Auto => (Icon::Flag, "Use mirror when possible"),
			Choice::Never => (Icon::FlagOff, "Never ask for this source"),
		};
		out.push(Line {
			layer: Some(Layer::Custom),
			file: Some("choices.toml".to_owned()),
			..blank(Kind::Choice, &domain.host, glyph, domain.host.clone(), said.to_owned())
		});
	}
	for (index, problem) in rules.problems.iter().enumerate() {
		out.push(blank(
			Kind::Problem,
			&index.to_string(),
			Icon::CircleX,
			"Could not be read".to_owned(),
			problem.clone(),
		));
	}
	out
}

/// Which rows a tab shows. The authorities are not among all of them: each is marked where a rule
/// reaches it, and has a tab of its own saying which rules do.
fn shows(tab: Tab, kind: Kind) -> bool {
	match tab {
		Tab::All => !matches!(kind, Kind::Problem | Kind::Authority),
		Tab::Rules => kind == Kind::Entry,
		Tab::Families => kind == Kind::Family,
		Tab::Authorities => kind == Kind::Authority,
		Tab::Choices => kind == Kind::Choice,
		Tab::Problems => kind == Kind::Problem,
	}
}

/// A column being dragged: which, where the pointer went down, and how wide it was then.
#[derive(Clone, Copy)]
struct Resize {
	column: usize,
	from: f32,
	width: f32,
}

pub struct RulesWindow {
	rdm: Entity<Rdm>,
	tab: Tab,
	selected: Option<(Kind, String)>,
	widths: [f32; 5],
	resizing: Option<Resize>,
	scroll: ScrollHandle,
	_follow: Subscription,
}

impl RulesWindow {
	pub fn new(rdm: Entity<Rdm>, cx: &mut Context<Self>) -> Self {
		// The rules are the main view's; this window redraws when that view changes.
		let follow = cx.observe(&rdm, |_, _, cx| cx.notify());
		RulesWindow {
			rdm,
			tab: Tab::All,
			selected: None,
			widths: WIDTHS,
			resizing: None,
			scroll: ScrollHandle::new(),
			_follow: follow,
		}
	}

	fn width(&self, column: Column) -> f32 {
		self.widths[COLUMNS.iter().position(|c| *c == column).unwrap_or(0)]
	}

	/// Follows the pointer while a boundary is dragged. The name's boundary is at its right and
	/// widens it to the right; every other column's is at its left and widens it to the left.
	fn drag(&mut self, x: f32) {
		let Some(resize) = self.resizing else { return };
		let moved = x - resize.from;
		let width = if resize.column == 0 { resize.width + moved } else { resize.width - moved };
		self.widths[resize.column] = width.clamp(NARROWEST, WIDEST);
	}

	fn tabs(&self, all: &[Line], p: Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let count = |tab: Tab| all.iter().filter(|l| shows(tab, l.kind)).count();
		let mut tabs = vec![
			(Tab::All, "All"),
			(Tab::Rules, "Rules"),
			(Tab::Families, "Mirror families"),
			(Tab::Authorities, "Authorities"),
			(Tab::Choices, "Your choices"),
		];
		if count(Tab::Problems) > 0 {
			tabs.push((Tab::Problems, "Problems"));
		}
		// Bare words with a one-pixel slot before, between and after them -- one slot to a boundary,
		// shared by the tabs on either side -- and the two slots beside the tab showing drawn as dashed
		// lines. A slot of its own on each side of every tab put two between neighbours, and the line
		// moved by a pixel as the choice moved from one to the other.
		let showing = tabs.iter().position(|(tab, _)| *tab == self.tab).unwrap_or(0);
		let slot = move |index: usize| {
			let lit = index == showing || index == showing + 1;
			div().w(px(1.0)).h_full().flex_none().border_l_1().border_dashed().border_color(if lit {
				p.border
			} else {
				gpui::transparent_black()
			})
		};
		let mut row = div().flex().flex_none().items_center().h_full().child(slot(0));
		for (index, (tab, title)) in tabs.into_iter().enumerate() {
			let on = self.tab == tab;
			row = row
				.child(
					div()
						.id(SharedString::from(format!("tab:{title}")))
						.role(gpui::Role::Tab)
						.aria_label(title)
						.aria_selected(on)
						.debug_selector(move || format!("tab:{title}"))
						.flex()
						.items_center()
						.gap_1()
						.h_full()
						.px_2()
						.cursor_pointer()
						.when(on, |s| s.text_color(p.text))
						.when(!on, move |s| s.text_color(p.muted).hover(move |s| s.text_color(p.text)))
						.on_click(cx.listener(move |this, _, _, cx| {
							this.tab = tab;
							cx.notify();
						}))
						.child(title)
						.child(div().text_color(p.muted).child(count(tab).to_string())),
				)
				.child(slot(index + 1));
		}
		row
	}

	/// The boundary a column is dragged by: a line down the header, in the space between columns.
	fn handle(&self, column: usize, p: Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let dragging = self.resizing.is_some_and(|r| r.column == column);
		div()
			.id(("rules-resize", column))
			.w(px(HANDLE))
			.h_full()
			.flex()
			.flex_none()
			.justify_center()
			.cursor_col_resize()
			.child(div().w_px().h_full().bg(if dragging { p.accent } else { p.border }))
			.on_mouse_down(
				MouseButton::Left,
				cx.listener(move |this, event: &MouseDownEvent, _, cx| {
					cx.stop_propagation();
					let width = this.widths[column];
					this.resizing = Some(Resize { column, from: f32::from(event.position.x), width });
					cx.notify();
				}),
			)
	}

	fn header(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let title = |column: Column, text: &'static str| {
			div().w(px(self.width(column))).flex_none().overflow_hidden().truncate().child(text)
		};
		div()
			.flex()
			.flex_none()
			.items_center()
			.h(px(HEADER_H))
			.mx_1p5()
			.px_2()
			.text_xs()
			.text_color(p.muted)
			.border_b_1()
			.border_color(p.border)
			.child(div().w(px(14.0)).flex_none())
			.child(title(Column::Name, "Name").pl(px(HANDLE)).w(px(self.width(Column::Name))))
			.child(self.handle(0, p, cx))
			.child(div().flex_1().min_w_0().truncate().child("Matches"))
			.child(self.handle(1, p, cx))
			.child(title(Column::Checksum, "Checksum from"))
			.child(self.handle(2, p, cx))
			.child(title(Column::Mirrors, "Mirrors"))
			.child(self.handle(3, p, cx))
			.child(title(Column::Layer, "Layer"))
			.child(self.handle(4, p, cx))
			.child(title(Column::Priority, "Priority").flex().justify_end())
	}

	/// Hosts in a cell, each with a check before it when it is an authority, cut short at the cell's
	/// end; a dash for none.
	fn hosts_cell(&self, column: Column, hosts: &[Host], p: Palette) -> impl IntoElement + use<> {
		let cell = div()
			.w(px(self.width(column) + HANDLE))
			.pl(px(HANDLE))
			.flex_none()
			.flex()
			.items_center()
			.gap_2()
			.overflow_hidden()
			.whitespace_nowrap()
			.text_xs()
			.text_color(p.muted);
		if hosts.is_empty() {
			return cell.child(NONE);
		}
		cell.children(hosts.iter().map(|host| {
			div()
				.flex()
				.flex_none()
				.items_center()
				.gap_0p5()
				.when(host.authority, |s| s.child(icon(Icon::CircleCheck, p.muted).size_3()))
				.child(host.name.clone())
		}))
	}

	fn row(&self, line: &Line, p: Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let selected =
			self.selected.as_ref().is_some_and(|(kind, key)| *kind == line.kind && *key == line.key);
		let problem = line.kind == Kind::Problem;
		let tint = if problem { p.status(crate::download::Status::Failed) } else { p.muted };
		let cell = |column: Column| {
			div()
				.w(px(self.width(column) + HANDLE))
				.pl(px(HANDLE))
				.flex_none()
				.overflow_hidden()
				.truncate()
				.text_xs()
		};
		let (kind, key) = (line.kind, line.key.clone());
		div()
			.id(ElementId::Name(SharedString::from(format!("rule:{:?}:{}", line.kind, line.key))))
			.role(gpui::Role::Row)
			.aria_label(line.name.clone())
			.aria_selected(selected)
			.flex()
			.flex_none()
			.items_center()
			.h(px(ROW_H))
			.mx_1p5()
			.px_2()
			.rounded_sm()
			.cursor_pointer()
			.when(selected, |s| s.bg(p.selection))
			.when(!selected, move |s| s.hover(move |s| s.bg(p.hover)))
			.tooltip(tooltip(line.detail.clone()))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.selected = Some((kind, key.clone()));
				cx.notify();
			}))
			.child(icon(line.glyph, tint).size_3p5())
			.child(
				div()
					.w(px(self.width(Column::Name)))
					.pl(px(HANDLE))
					.flex_none()
					.truncate()
					.child(line.name.clone()),
			)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.pl(px(HANDLE))
					.truncate()
					.text_xs()
					.text_color(tint)
					.child(line.matched.clone()),
			)
			.child(self.hosts_cell(Column::Checksum, &line.checksum, p))
			.child(self.hosts_cell(Column::Mirrors, &line.mirrors, p))
			.child(
				cell(Column::Layer).text_color(p.muted).child(line.layer.map(layer_name).unwrap_or(NONE)),
			)
			.child(
				cell(Column::Priority)
					.flex()
					.justify_end()
					.text_color(p.muted)
					.child(line.priority.map(|n| n.to_string()).unwrap_or_else(|| NONE.to_owned())),
			)
	}
}

impl Render for RulesWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let rules = self.rdm.read(cx).rules.clone();
		let all = lines(&rules);
		if self.tab == Tab::Problems && rules.problems.is_empty() {
			self.tab = Tab::All;
		}
		let shown: Vec<&Line> = all.iter().filter(|l| shows(self.tab, l.kind)).collect();
		// A selection that is no longer there -- forgotten, or filtered out -- is dropped.
		let chosen = self
			.selected
			.as_ref()
			.and_then(|(kind, key)| shown.iter().find(|l| l.kind == *kind && &l.key == key).copied());
		if chosen.is_none() {
			self.selected = None;
		}
		let rows: Vec<_> = shown.iter().map(|line| self.row(line, p, cx).into_any_element()).collect();
		let empty = rows.is_empty();
		let table = div()
			.id("rules-table")
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.py_1()
			.overflow_y_scroll()
			.track_scroll(&self.scroll)
			.children(rows)
			.when(empty, |s| {
				s.child(
					div()
						.flex_1()
						.flex()
						.items_center()
						.justify_center()
						.text_xs()
						.text_color(p.muted)
						.child("Nothing here"),
				)
			});

		// The foot, one row the status bar's height: the tabs at the left, and at the right what acts
		// on the selected row, then what acts on the rules as a whole. Where the last sync stands is
		// said by the sync button when the pointer rests on it; the counts are on the tabs.
		let (syncing, synced, succeeded) = {
			let rdm = self.rdm.read(cx);
			(rdm.rules_sync.running, rdm.rules_sync.status.clone(), rdm.rules_sync.succeeded)
		};
		// The button says where the sync stands and nothing else: a cloud to fetch from, and while it
		// fetches, the cloud with its arrows turning inside it.
		let sync_said = match (syncing, synced) {
			(true, _) => "Fetching the rules".to_owned(),
			(false, Some(status)) => status,
			(false, None) => "Not synced since rdm started".to_owned(),
		};
		let turning =
			div().relative().size_3p5().child(icon(Icon::SyncCloud, p.text).size_3p5()).child(
				// Down by the four units the arrows were moved up to centre their circle, of the
				// twenty-four the icon is drawn in. See spec/icons.md.
				div().absolute().top(px(14.0 * 4.0 / 24.0)).left_0().size_3p5().child(
					icon(Icon::SyncArrows, p.text).size_3p5().with_animation(
						"rules-sync-turn",
						Animation::new(std::time::Duration::from_secs(1)).repeat(),
						|svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
					),
				),
			);
		let sync_rdm = self.rdm.clone();
		let sync = div()
			.id("rules-sync")
			.role(gpui::Role::Button)
			.aria_label("Sync the rules now")
			.debug_selector(|| "button:Sync the rules now".to_owned())
			.flex()
			.items_center()
			.justify_center()
			.size_5()
			.group("rules-sync")
			.tooltip(tooltip(sync_said))
			.map(|s| {
				if syncing {
					return s.child(turning);
				}
				// How the last sync ended, until the next begins: none yet, done, or failed.
				let (glyph, tint) = match succeeded {
					None => (Icon::CloudDownload, p.muted),
					Some(true) => (Icon::CloudCheck, p.muted),
					Some(false) => (Icon::CloudAlert, p.status(crate::download::Status::Failed)),
				};
				s.child(hover_icon(glyph, "rules-sync", tint, Some(p.text)).size_3p5())
			})
			.when(!syncing, |s| {
				s.cursor_pointer()
					.on_click(move |_, _, cx| sync_rdm.update(cx, |rdm, cx| rdm.sync_rules(cx)))
			});
		let movable = chosen.filter(|l| matches!(l.kind, Kind::Entry | Kind::Family));
		let (up, down) =
			(movable.is_some_and(|l| l.place > 0), movable.is_some_and(|l| l.place + 1 < l.of));
		let family = movable.is_some_and(|l| l.kind == Kind::Family);
		let moved = movable.map(|l| l.key.clone());
		let forgettable = chosen.filter(|l| l.kind == Kind::Choice).map(|l| l.key.clone());
		let file = chosen
			.and_then(|l| Some((l.layer?, l.file.clone()?)))
			.filter(|(layer, file)| self.rdm.read(cx).rule_file(*layer, file).is_some());
		let separator = || div().w(px(1.0)).h(px(12.0)).mx_1().bg(p.border);
		let on = |rdm: &Entity<Rdm>, f: fn(&mut Rdm, &mut Context<Rdm>)| {
			let rdm = rdm.clone();
			move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut gpui::App| rdm.update(cx, f)
		};
		let (up_rdm, down_rdm, forget_rdm, file_rdm, custom_rdm) =
			(self.rdm.clone(), self.rdm.clone(), self.rdm.clone(), self.rdm.clone(), self.rdm.clone());
		let (up_id, down_id) = (moved.clone(), moved);
		let foot = div()
			.relative()
			.flex()
			.flex_none()
			.items_center()
			.h(px(crate::ui::status_bar::HEIGHT))
			.text_xs()
			.text_color(p.muted)
			// The dashed line along the top is an element under the tabs rather than the row's border,
			// which gpui paints over the children.
			.child(
				div()
					.absolute()
					.top_0()
					.left_0()
					.right_0()
					.h(px(1.0))
					.border_t_1()
					.border_dashed()
					.border_color(p.border),
			)
			.child(self.tabs(&all, p, cx))
			.child(div().flex_1())
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap_0p5()
					.pr_2()
					.child(icon_button(p, "rule-up", Icon::ChevronUp, "Move up", up, move |_, _, cx| {
						if let Some(id) = &up_id {
							up_rdm.update(cx, |rdm, cx| rdm.move_rule(id, family, true, cx));
						}
					}))
					.child(icon_button(
						p,
						"rule-down",
						Icon::ChevronDown,
						"Move down",
						down,
						move |_, _, cx| {
							if let Some(id) = &down_id {
								down_rdm.update(cx, |rdm, cx| rdm.move_rule(id, family, false, cx));
							}
						},
					))
					.child(icon_button(
						p,
						"rule-forget",
						Icon::X,
						"Forget this choice",
						forgettable.is_some(),
						move |_, _, cx| {
							if let Some(host) = &forgettable {
								forget_rdm.update(cx, |rdm, cx| rdm.forget_choice(host, cx));
							}
						},
					))
					.child(icon_button(
						p,
						"rule-file",
						Icon::FolderSearch,
						"Show the rule's file",
						file.is_some(),
						move |_, _, cx| {
							if let Some((layer, file)) = &file {
								file_rdm.read(cx).reveal_rule_file(*layer, file);
							}
						},
					))
					.child(separator())
					.child(sync)
					.child(icon_button(
						p,
						"rules-reload",
						Icon::RotateCcw,
						"Reload from disk",
						true,
						on(&self.rdm, |rdm, cx| rdm.reload_rules(cx)),
					))
					.child(icon_button(
						p,
						"rules-custom",
						Icon::FolderOpen,
						"Open the custom folder",
						true,
						move |_, _, cx| {
							custom_rdm.read(cx).open_custom_rules();
						},
					)),
			);

		// A drag that began on a boundary is followed across the whole window, and ends where the
		// button comes up.
		let body = div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.text_size(px(13.0))
			.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
				if this.resizing.is_some() {
					if event.pressed_button == Some(MouseButton::Left) {
						this.drag(f32::from(event.position.x));
					} else {
						this.resizing = None;
					}
					cx.notify();
				}
			}))
			.on_mouse_up(
				MouseButton::Left,
				cx.listener(|this, _, _, cx| {
					if this.resizing.take().is_some() {
						cx.notify();
					}
				}),
			)
			.child(self.header(p, cx))
			.child(table)
			.child(foot);
		chrome(p, window, Title { before: None, name: "Rules".to_owned() }, body)
	}
}
