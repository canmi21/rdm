//! The rules, merged from every layer, as a table in a window of their own, drawn the way the main
//! window's detailed list is: a header of column titles, dense rows that select, and a status bar
//! whose buttons act on the selected row. Tabs narrow it to one kind. Nothing here is typed: a rule
//! is written in a file, and the custom folder is a press away. See spec/rules.md.

use gpui::{
	Context, ElementId, Entity, IntoElement, Render, ScrollHandle, SharedString, Subscription,
	Window, div, prelude::*, px,
};

use crate::app::Rdm;
use crate::rules::{Choice, Compiled, Layer};
use crate::ui::download_window::{Title, chrome};
use crate::ui::icon::{Icon, icon};
use crate::ui::icon_button;
use crate::ui::theme::{self, Palette};
use crate::ui::tooltip::tooltip;

/// The fixed columns' widths; the name's is its own and the match takes the rest.
const NAME: f32 = 136.0;
const PROVIDES: f32 = 140.0;
const LAYER: f32 = 64.0;
const PRIORITY: f32 = 52.0;
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

/// One row of the table, whatever kind of thing it stands for.
struct Line {
	kind: Kind,
	/// Unique within the kind: what the selection remembers.
	key: String,
	glyph: Icon,
	name: String,
	matched: String,
	provides: String,
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

/// Every row the rules make, in the order they are tried within each kind.
fn lines(rules: &Compiled) -> Vec<Line> {
	let mut out = Vec::new();
	let blank = |kind: Kind, key: &str, glyph: Icon, name: String, matched: String| Line {
		kind,
		key: key.to_owned(),
		glyph,
		name,
		matched,
		provides: NONE.to_owned(),
		layer: None,
		file: None,
		priority: None,
		place: 0,
		of: 0,
	};
	for (place, entry) in rules.entries.iter().enumerate() {
		let mut provides = Vec::new();
		if !entry.checksum.is_empty() {
			provides.push("Checksum".to_owned());
		}
		match entry.mirror.len() {
			0 => {}
			1 => provides.push("1 mirror".to_owned()),
			n => provides.push(format!("{n} mirrors")),
		}
		out.push(Line {
			provides: if provides.is_empty() { NONE.to_owned() } else { provides.join(", ") },
			layer: Some(entry.layer),
			file: Some(entry.file.clone()),
			priority: Some(entry.priority),
			place,
			of: rules.entries.len(),
			..blank(Kind::Entry, &entry.id, Icon::Globe, entry.id.clone(), entry.pattern.clone())
		});
	}
	for (place, family) in rules.families.iter().enumerate() {
		out.push(Line {
			provides: format!("{} mirrors", family.prefixes.len()),
			layer: Some(family.layer),
			file: Some(family.file.clone()),
			priority: Some(family.priority),
			place,
			of: rules.families.len(),
			..blank(
				Kind::Family,
				&family.id,
				Icon::LayoutList,
				family.id.clone(),
				family.prefixes.join("  "),
			)
		});
	}
	for host in &rules.authorities {
		out.push(Line {
			provides: "Checksum".to_owned(),
			..blank(
				Kind::Authority,
				host,
				Icon::CircleCheck,
				host.clone(),
				"May give the checksum of a source it mirrors".to_owned(),
			)
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

fn shows(tab: Tab, kind: Kind) -> bool {
	match tab {
		Tab::All => kind != Kind::Problem,
		Tab::Rules => kind == Kind::Entry,
		Tab::Families => kind == Kind::Family,
		Tab::Authorities => kind == Kind::Authority,
		Tab::Choices => kind == Kind::Choice,
		Tab::Problems => kind == Kind::Problem,
	}
}

pub struct RulesWindow {
	rdm: Entity<Rdm>,
	tab: Tab,
	selected: Option<(Kind, String)>,
	scroll: ScrollHandle,
	_follow: Subscription,
}

impl RulesWindow {
	pub fn new(rdm: Entity<Rdm>, cx: &mut Context<Self>) -> Self {
		// The rules are the main view's; this window redraws when that view changes.
		let follow = cx.observe(&rdm, |_, _, cx| cx.notify());
		RulesWindow { rdm, tab: Tab::All, selected: None, scroll: ScrollHandle::new(), _follow: follow }
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
		// A dashed line along the top and the tabs bare words under it, the row as high as the status
		// bar below; the one showing is told by its colour.
		div()
			.flex()
			.flex_none()
			.items_center()
			.gap_0p5()
			.h(px(crate::ui::status_bar::HEIGHT))
			.px_3()
			.border_t_1()
			.border_dashed()
			.border_color(p.border)
			.text_xs()
			.children(tabs.into_iter().map(|(tab, title)| {
				let on = self.tab == tab;
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
					// The one showing ruled solid on its sides and over its stretch of the dashed line:
					// up by the line's width, so its top edge is the line.
					.when(on, |s| {
						s.mt(px(-1.0))
							.h(px(crate::ui::status_bar::HEIGHT))
							.border_t_1()
							.border_l_1()
							.border_r_1()
							.border_color(p.muted)
							.text_color(p.text)
					})
					.when(!on, move |s| s.text_color(p.muted).hover(move |s| s.text_color(p.text)))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.tab = tab;
						cx.notify();
					}))
					.child(title)
					.child(div().text_color(p.muted).child(count(tab).to_string()))
			}))
	}

	fn header(p: Palette) -> impl IntoElement {
		let cell = |width: f32, title: &'static str| {
			div().w(px(width)).flex_none().pl(px(12.0)).overflow_hidden().truncate().child(title)
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
			.child(cell(NAME, "Name"))
			.child(div().flex_1().min_w_0().pl(px(12.0)).truncate().child("Matches"))
			.child(cell(PROVIDES, "Provides"))
			.child(cell(LAYER, "Layer"))
			.child(cell(PRIORITY, "Priority").flex().justify_end())
	}

	fn row(&self, line: &Line, p: Palette, cx: &mut Context<Self>) -> impl IntoElement + use<> {
		let selected =
			self.selected.as_ref().is_some_and(|(kind, key)| *kind == line.kind && *key == line.key);
		let problem = line.kind == Kind::Problem;
		let tint = if problem { p.status(crate::download::Status::Failed) } else { p.muted };
		let cell = |width: f32| {
			div().w(px(width)).flex_none().pl(px(12.0)).overflow_hidden().truncate().text_xs()
		};
		let (kind, key) = (line.kind, line.key.clone());
		let whole = format!("{}\n{}", line.name, line.matched);
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
			.tooltip(tooltip(whole))
			.on_click(cx.listener(move |this, _, _, cx| {
				this.selected = Some((kind, key.clone()));
				cx.notify();
			}))
			.child(icon(line.glyph, tint).size_3p5())
			.child(div().w(px(NAME)).flex_none().pl(px(12.0)).truncate().child(line.name.clone()))
			.child(
				div()
					.flex_1()
					.min_w_0()
					.pl(px(12.0))
					.truncate()
					.text_xs()
					.text_color(tint)
					.child(line.matched.clone()),
			)
			.child(cell(PROVIDES).text_color(p.muted).child(line.provides.clone()))
			.child(cell(LAYER).text_color(p.muted).child(line.layer.map(layer_name).unwrap_or(NONE)))
			.child(
				cell(PRIORITY)
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

		// The status bar: what the rules are and where the last sync stands, then what acts on the
		// selected row, then what acts on the rules as a whole.
		let (syncing, synced) = {
			let rdm = self.rdm.read(cx);
			(rdm.rules_sync.running, rdm.rules_sync.status.clone())
		};
		let plural =
			|n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
		let counted = format!(
			"{}, {}",
			plural(rules.entries.len(), "rule", "rules"),
			plural(rules.families.len(), "mirror family", "mirror families")
		);
		let said = match (syncing, synced) {
			(true, _) => format!("{counted}  \u{b7}  Fetching the rules"),
			(false, Some(status)) => format!("{counted}  \u{b7}  {status}"),
			(false, None) => counted,
		};
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
		let status = div()
			.flex()
			.flex_none()
			.items_center()
			.justify_between()
			.gap_3()
			.h(px(crate::ui::status_bar::HEIGHT))
			.px_3()
			.border_t_1()
			.border_color(p.border)
			.text_xs()
			.text_color(p.muted)
			.child(div().min_w_0().truncate().child(said))
			.child(
				div()
					.flex()
					.flex_none()
					.items_center()
					.gap_0p5()
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
					.child(icon_button(
						p,
						"rules-sync",
						Icon::Download,
						"Sync the rules now",
						!syncing,
						on(&self.rdm, |rdm, cx| rdm.sync_rules(cx)),
					))
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

		let body = div()
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.text_size(px(13.0))
			.child(Self::header(p))
			.child(table)
			// The tabs over the status bar, the two one foot to the table, as the main window keeps its
			// view switch at the foot beside the status.
			.child(self.tabs(&all, p, cx))
			.child(status);
		chrome(p, window, Title { before: None, name: "Rules".to_owned() }, body)
	}
}
