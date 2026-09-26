//! The rules, merged from every layer, in a window of their own: each rule with the layer it came
//! from and its place, which can be moved; the mirror families, the authorities, and the choices
//! made in New Task, which can be forgotten. Nothing here is typed: a rule is written in a file,
//! and the custom folder is a press away. See spec/rules.md.

use gpui::{
	Context, ElementId, Entity, IntoElement, Render, ScrollHandle, SharedString, Subscription,
	Window, div, prelude::*, px,
};

use crate::app::Rdm;
use crate::rules::{Choice, Layer};
use crate::ui::download_window::{Title, chrome};
use crate::ui::icon::Icon;
use crate::ui::theme::{self, Palette};
use crate::ui::{button, icon_button};

pub struct RulesWindow {
	rdm: Entity<Rdm>,
	scroll: ScrollHandle,
	_follow: Subscription,
}

impl RulesWindow {
	pub fn new(rdm: Entity<Rdm>, cx: &mut Context<Self>) -> Self {
		// The rules are the main view's; this window redraws when that view changes.
		let follow = cx.observe(&rdm, |_, _, cx| cx.notify());
		RulesWindow { rdm, scroll: ScrollHandle::new(), _follow: follow }
	}
}

fn layer_name(layer: Layer) -> &'static str {
	match layer {
		Layer::BuiltIn => "Built in",
		Layer::Synced => "Synced",
		Layer::Custom => "Custom",
	}
}

/// A group's heading, with how many it holds.
fn heading(p: Palette, title: &'static str, count: usize) -> impl IntoElement {
	div()
		.flex()
		.items_center()
		.gap_1p5()
		.px_2()
		.pb_1()
		.text_xs()
		.font_weight(gpui::FontWeight::MEDIUM)
		.text_color(p.muted)
		.child(title)
		.child(div().text_color(p.border).child(count.to_string()))
}

/// A row's name, the line under it, and what sits at its right.
fn row(p: Palette, name: String, detail: String) -> gpui::Div {
	div()
		.flex()
		.items_center()
		.gap_2()
		.px_2()
		.py_1()
		.rounded_sm()
		.hover(move |s| s.bg(p.hover))
		.child(
			div()
				.flex()
				.flex_col()
				.flex_1()
				.min_w_0()
				.child(div().truncate().child(name))
				.child(div().text_xs().text_color(p.muted).truncate().child(detail)),
		)
}

fn badge(p: Palette, text: &'static str) -> impl IntoElement {
	div().flex_none().px_1p5().rounded_sm().bg(p.hover).text_xs().text_color(p.muted).child(text)
}

impl Render for RulesWindow {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let p = theme::palette(window.is_window_active());
		let rules = self.rdm.read(cx).rules.clone();
		let rdm = self.rdm.clone();
		// Up and down for a rule of either kind, dimmed where there is nowhere to go.
		let moves = |id: &str, family: bool, at: usize, count: usize| {
			let (up, down) = (rdm.clone(), rdm.clone());
			let (up_id, down_id) = (id.to_owned(), id.to_owned());
			let tag = if family { "family" } else { "entry" };
			div()
				.flex()
				.flex_none()
				.child(icon_button(
					p,
					ElementId::Name(SharedString::from(format!("up:{tag}:{id}"))),
					Icon::ChevronUp,
					"Move up",
					at > 0,
					move |_, _, cx| up.update(cx, |rdm, cx| rdm.move_rule(&up_id, family, true, cx)),
				))
				.child(icon_button(
					p,
					ElementId::Name(SharedString::from(format!("down:{tag}:{id}"))),
					Icon::ChevronDown,
					"Move down",
					at + 1 < count,
					move |_, _, cx| down.update(cx, |rdm, cx| rdm.move_rule(&down_id, family, false, cx)),
				))
		};
		let priority = |value: i32| {
			div()
				.w(px(28.0))
				.flex_none()
				.text_xs()
				.text_color(p.muted)
				.text_right()
				.child(value.to_string())
		};

		let entries = rules.entries.iter().enumerate().map(|(at, entry)| {
			let mut offers = Vec::new();
			if !entry.checksum.is_empty() {
				offers.push("checksum".to_owned());
			}
			match entry.mirror.len() {
				0 => {}
				1 => offers.push("1 mirror".to_owned()),
				n => offers.push(format!("{n} mirrors")),
			}
			let detail = if offers.is_empty() {
				entry.pattern.clone()
			} else {
				format!("{}  \u{b7}  {}", offers.join(", "), entry.pattern)
			};
			row(p, entry.id.clone(), detail)
				.child(badge(p, layer_name(entry.layer)))
				.child(priority(entry.priority))
				.child(moves(&entry.id, false, at, rules.entries.len()))
		});
		let families = rules.families.iter().enumerate().map(|(at, family)| {
			let detail =
				format!("{} prefixes  \u{b7}  {}", family.prefixes.len(), family.prefixes.join("  "));
			row(p, family.id.clone(), detail)
				.child(badge(p, layer_name(family.layer)))
				.child(priority(family.priority))
				.child(moves(&family.id, true, at, rules.families.len()))
		});
		let authorities = rules.authorities.iter().map(|host| {
			row(p, host.clone(), "May give a checksum for the sources it mirrors".to_owned())
		});
		let choices = rules.domains.iter().map(|domain| {
			let forget = rdm.clone();
			let host = domain.host.clone();
			let said = match domain.mirror {
				Choice::Auto => "Use mirror when possible",
				Choice::Never => "Never ask for this source",
			};
			row(p, domain.host.clone(), said.to_owned()).child(icon_button(
				p,
				ElementId::Name(SharedString::from(format!("forget:{host}"))),
				Icon::X,
				"Forget",
				true,
				move |_, _, cx| forget.update(cx, |rdm, cx| rdm.forget_choice(&host, cx)),
			))
		});
		let problems = rules.problems.iter().map(|problem| {
			div()
				.px_2()
				.py_0p5()
				.text_xs()
				.text_color(p.status(crate::download::Status::Failed))
				.child(problem.clone())
		});
		let group = || div().flex().flex_col();

		let list = div()
			.id("rules-list")
			.flex()
			.flex_col()
			.flex_1()
			.min_h_0()
			.gap_4()
			.px_3()
			.py_3()
			.overflow_y_scroll()
			.track_scroll(&self.scroll)
			.child(group().child(heading(p, "Rules", rules.entries.len())).children(entries))
			.when(!rules.families.is_empty(), |s| {
				s.child(
					group().child(heading(p, "Mirror families", rules.families.len())).children(families),
				)
			})
			.when(!rules.authorities.is_empty(), |s| {
				s.child(
					group().child(heading(p, "Authorities", rules.authorities.len())).children(authorities),
				)
			})
			.when(!rules.domains.is_empty(), |s| {
				s.child(group().child(heading(p, "Your choices", rules.domains.len())).children(choices))
			})
			.when(!rules.problems.is_empty(), |s| {
				s.child(
					group().child(heading(p, "Could not be read", rules.problems.len())).children(problems),
				)
			});

		let (reload, open) = (self.rdm.clone(), self.rdm.clone());
		let footer = div()
			.flex()
			.flex_none()
			.items_center()
			.justify_between()
			.gap_3()
			.px_4()
			.py_2()
			.border_t_1()
			.border_color(p.border)
			.text_xs()
			.child(
				div()
					.text_color(p.muted)
					.child("Ordered by priority; a moved rule keeps its place in the custom layer."),
			)
			.child(
				div()
					.flex()
					.flex_none()
					.gap_1()
					.child(button(p, "rules-reload", Icon::RotateCcw, "Reload", true, move |_, _, cx| {
						reload.update(cx, |rdm, cx| rdm.reload_rules(cx))
					}))
					.child(button(
						p,
						"rules-custom",
						Icon::FolderOpen,
						"Open custom folder",
						true,
						move |_, _, cx| open.read(cx).open_custom_rules(),
					)),
			);
		let body = div().flex().flex_col().flex_1().min_h_0().text_sm().child(list).child(footer);
		chrome(p, window, Title { before: None, name: "Rules".to_owned() }, body)
	}
}
