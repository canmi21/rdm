use gpui::{Hsla, Svg, prelude::*, svg};

use crate::download::{Kind, Status};

#[derive(Clone, Copy, Debug)]
pub enum Icon {
	Plus,
	Pause,
	Play,
	Trash,
	Film,
	Music,
	FileText,
	Archive,
	Package,
	File,
	CircleCheck,
	CircleX,
	CirclePause,
	Clock,
	ArrowDown,
	LayoutList,
	Rows,
	LayoutGrid,
}

impl Icon {
	fn path(self) -> &'static str {
		match self {
			Icon::Plus => "icons/plus.svg",
			Icon::Pause => "icons/pause.svg",
			Icon::Play => "icons/play.svg",
			Icon::Trash => "icons/trash.svg",
			Icon::Film => "icons/film.svg",
			Icon::Music => "icons/music.svg",
			Icon::FileText => "icons/file-text.svg",
			Icon::Archive => "icons/archive.svg",
			Icon::Package => "icons/package.svg",
			Icon::File => "icons/file.svg",
			Icon::CircleCheck => "icons/circle-check.svg",
			Icon::CircleX => "icons/circle-x.svg",
			Icon::CirclePause => "icons/circle-pause.svg",
			Icon::Clock => "icons/clock.svg",
			Icon::ArrowDown => "icons/arrow-down.svg",
			Icon::LayoutList => "icons/layout-list.svg",
			Icon::Rows => "icons/rows-3.svg",
			Icon::LayoutGrid => "icons/layout-grid.svg",
		}
	}

	pub fn for_kind(kind: Kind) -> Icon {
		match kind {
			Kind::Video => Icon::Film,
			Kind::Audio => Icon::Music,
			Kind::Document => Icon::FileText,
			Kind::Archive => Icon::Archive,
			Kind::Program => Icon::Package,
			Kind::Other => Icon::File,
		}
	}

	pub fn for_status(status: Status) -> Icon {
		match status {
			Status::Queued => Icon::Clock,
			Status::Downloading => Icon::ArrowDown,
			Status::Paused => Icon::CirclePause,
			Status::Completed => Icon::CircleCheck,
			Status::Failed => Icon::CircleX,
		}
	}
}

/// The colour is a parameter, not inherited: an untinted svg paints nothing. See spec/icons.md.
pub fn icon(icon: Icon, color: impl Into<Hsla>) -> Svg {
	svg().path(icon.path()).size_4().flex_none().text_color(color)
}
