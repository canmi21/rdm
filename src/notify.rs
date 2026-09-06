//! Telling somebody that something happened, and where it is said. Four moments are worth
//! telling about -- a download that finished, one that failed, a queue that emptied, and a build
//! newer than this one -- and each carries its own choice, because what somebody wants said out
//! loud about a finished download is rarely what they want said about a failed one. See
//! spec/ui.md.
//!
//! The forms are separate places and not degrees of loudness: the system's notification centre
//! reaches somebody who is not looking at the window, a card in the corner reaches somebody who
//! is, and silence reaches nobody. Which is right is the user's to say, so none of them stands in
//! for another -- a form that cannot be shown is not quietly swapped for one that can, since a
//! notice arriving somewhere it was not asked for is worse than one that does not arrive.

use serde::{Deserialize, Serialize};

/// Where a notice is said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Style {
	/// The system's own notification centre, which reaches past the window and outlives it.
	System,
	/// A card in the window's corner, over the list and above the status bar, which reaches
	/// somebody already looking at it and nobody else.
	InApp,
	/// A window of its own, above the others, which needs no main window open.
	Window,
	/// Nothing is said.
	#[default]
	Silent,
}

impl Style {
	/// What Settings offers, in the order it offers it: the places a notice can go, then the
	/// choice to send it nowhere, since somebody opening the row is usually turning one down.
	pub const ALL: [Style; 4] =
		[Style::System, Style::InApp, Style::Window, Style::Silent];

	pub fn name(self) -> &'static str {
		match self {
			Style::System => crate::i18n::t("notice.style.system"),
			Style::InApp => crate::i18n::t("notice.style.in_app"),
			Style::Window => crate::i18n::t("notice.style.window"),
			Style::Silent => crate::i18n::t("notice.style.silent"),
		}
	}
}

/// What a notice says, and what it is about. The file is there when the notice is about one:
/// it is what the dialog's buttons act on and what its size and its time are read from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Notice {
	pub title: String,
	pub body: String,
	pub file: Option<Finished>,
}

/// The download a notice is about: where it landed, how big it turned out, and how long it took
/// from being added to being done.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
	pub path: std::path::PathBuf,
	pub size: u64,
	pub took: std::time::Duration,
}

impl Notice {
	pub fn new(title: impl Into<String>, body: impl Into<String>) -> Notice {
		Notice { title: title.into(), body: body.into(), file: None }
	}

	pub fn about(mut self, file: Finished) -> Notice {
		self.file = Some(file);
		self
	}
}

/// A moment worth telling about. Each has its own row in Settings and its own field in the
/// preferences, so one can be turned down without touching the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Occasion {
	Finished,
	Failed,
	Queue,
	Update,
}

impl Occasion {
	pub const ALL: [Occasion; 4] =
		[Occasion::Finished, Occasion::Failed, Occasion::Queue, Occasion::Update];

	/// The line under the row's label: what a notice about this moment would say, or what makes
	/// this moment different from the one above it.
	pub fn note(self) -> &'static str {
		match self {
			Occasion::Finished => crate::i18n::t("notice.occasion.finished.note"),
			Occasion::Failed => crate::i18n::t("notice.occasion.failed.note"),
			Occasion::Queue => crate::i18n::t("notice.occasion.queue.note"),
			Occasion::Update => crate::i18n::t("notice.occasion.update.note"),
		}
	}

	/// What Settings calls the row: the moment, said as it happens rather than as a setting.
	pub fn label(self) -> &'static str {
		match self {
			Occasion::Finished => crate::i18n::t("notice.occasion.finished.label"),
			Occasion::Failed => crate::i18n::t("notice.occasion.failed.label"),
			Occasion::Queue => crate::i18n::t("notice.occasion.queue.label"),
			Occasion::Update => crate::i18n::t("notice.occasion.update.label"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_style_is_offered_and_named() {
		for style in Style::ALL {
			assert!(!style.name().is_empty());
		}
		assert!(Style::ALL.contains(&Style::Window));
		assert_eq!(Style::default(), Style::Silent, "an unreadable choice says nothing");
	}

	#[test]
	fn every_occasion_is_named_and_they_are_all_different() {
		let labels: Vec<&str> = Occasion::ALL.iter().map(|o| o.label()).collect();
		let mut sorted = labels.clone();
		sorted.sort_unstable();
		sorted.dedup();
		assert_eq!(sorted.len(), labels.len(), "two rows with one label would be one row: {labels:?}");
	}
}
