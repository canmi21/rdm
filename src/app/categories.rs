//! The categories as the window edits them: added, switched in and out, their lists, icons
//! and colors changed, dragged into order. Every change is written to config.json at once.

use gpui::Context;

use crate::app::{CategorySheet, Rdm};
use crate::category::{self, Category, Overrides};
use crate::config::{self, Config};
use crate::download::Filter;
use crate::ui::icon::Icon;

impl Rdm {
	/// A new category goes before the catch-all, which stays last so it reads as the remainder.
	pub(crate) fn add_category(
		&mut self,
		name: &str,
		icon: Icon,
		color: Option<u32>,
		custom_color: Option<String>,
		pattern: &str,
		cx: &mut Context<Self>,
	) -> Result<(), String> {
		let name = name.trim();
		if name.is_empty() {
			return Err("a category needs a name".to_owned());
		}
		if pattern.trim().is_empty() {
			return Err("a category needs a pattern".to_owned());
		}
		if self.categories.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
			return Err(format!("there is already a category called {name}"));
		}
		let mut category = Category::new(self.next_category_id(), name, icon, pattern.trim())?;
		if let Some(color) = color {
			category.color = color;
		}
		category.custom_color = custom_color.filter(|t| crate::ui::theme::parse_color(t).is_some());
		self.insert_category(category, cx);
		Ok(())
	}

	fn next_category_id(&self) -> u64 {
		self.categories.iter().map(|c| c.id).max().unwrap_or(0) + 1
	}

	fn insert_category(&mut self, category: Category, cx: &mut Context<Self>) {
		let at =
			self.categories.iter().position(Category::is_catch_all).unwrap_or(self.categories.len());
		self.categories.insert(at, category);
		self.save_config();
		cx.notify();
	}

	/// A preset is added when absent and removed when present, so the sheet's preset row is a
	/// switch for what the sidebar shows. Removing one drops the user's changes to its list with
	/// it; the list is the preset's and goes where it goes.
	pub(crate) fn toggle_preset(&mut self, name: &str, cx: &mut Context<Self>) {
		if let Some(at) = self.categories.iter().position(|c| c.name == name) {
			let removed = self.categories.remove(at);
			if self.filter == Filter::Category(removed.id) {
				self.filter = Filter::All;
			}
			self.save_config();
			cx.notify();
		} else if let Some(preset) =
			Category::from_preset(self.next_category_id(), name, Overrides::default())
		{
			self.insert_category(preset, cx);
		}
	}

	/// A change to one category, written and drawn at once. Nothing happens for an id that is
	/// no longer there.
	fn edit_category(&mut self, id: u64, cx: &mut Context<Self>, change: impl FnOnce(&mut Category)) {
		if let Some(category) = self.categories.iter_mut().find(|c| c.id == id) {
			change(category);
			self.save_config();
			cx.notify();
		}
	}

	/// One extension of a preset's list switched on or off; see `Category::set_extension`.
	pub(crate) fn set_preset_extension(
		&mut self,
		id: u64,
		extension: &str,
		on: bool,
		cx: &mut Context<Self>,
	) {
		self.edit_category(id, cx, |c| c.set_extension(extension, on));
	}

	/// `rs, py` typed into a preset's editor: each switched on, whether built in or new.
	pub(crate) fn add_preset_extensions(&mut self, id: u64, text: &str, cx: &mut Context<Self>) {
		for extension in category::split_extensions(text) {
			self.set_preset_extension(id, &extension, true, cx);
		}
	}

	pub(crate) fn set_category_icon(&mut self, id: u64, icon: Icon, cx: &mut Context<Self>) {
		self.edit_category(id, cx, |c| c.icon = icon);
	}

	pub(crate) fn set_category_color(&mut self, id: u64, color: u32, cx: &mut Context<Self>) {
		self.edit_category(id, cx, |c| c.color = color);
	}

	/// A color the user wrote for a category: kept as written beside the named ones, and made
	/// the one in use. Text that is not a color is ignored.
	pub(crate) fn set_category_custom_color(&mut self, id: u64, text: &str, cx: &mut Context<Self>) {
		let Some(color) = crate::ui::theme::parse_color(text) else { return };
		self.edit_category(id, cx, |c| {
			c.custom_color = Some(text.trim().to_owned());
			c.color = color;
		});
	}

	pub(crate) fn reset_preset(&mut self, id: u64, cx: &mut Context<Self>) {
		self.edit_category(id, cx, Category::reset_preset);
	}

	/// The sidebar's categories are being dragged into order; the rows drag instead of filtering.
	pub(crate) fn reordering(&self) -> bool {
		matches!(self.category_sheet, Some(CategorySheet::Reorder))
	}

	/// Drops the dragged category at the target's position, the rest shifting to make room. The
	/// catch-all is neither dragged nor a target, so it stays last. Written at once, like an add.
	pub(crate) fn move_category(&mut self, dragged: u64, onto: u64, cx: &mut Context<Self>) {
		let position = |id: u64| self.categories.iter().position(|c| c.id == id);
		let (Some(from), Some(to)) = (position(dragged), position(onto)) else { return };
		if from == to || self.categories[from].is_catch_all() || self.categories[to].is_catch_all() {
			return;
		}
		let category = self.categories.remove(from);
		self.categories.insert(to, category);
		self.save_config();
		cx.notify();
	}

	/// Written at once, not debounced: a category is added once, and the file is the user's.
	fn save_config(&self) {
		if let Some(paths) = &self.paths
			&& let Err(error) =
				config::save(&paths.config, &Config::from_parts(&self.categories, &self.preferences))
		{
			eprintln!("could not write {}: {error:#}", paths.config.display());
		}
	}

	pub(crate) fn set_colorful_categories(&mut self, on: bool, cx: &mut Context<Self>) {
		self.preferences.colorful_categories = on;
		self.save_config();
		cx.notify();
	}
}
