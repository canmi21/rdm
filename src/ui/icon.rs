use gpui::{Hsla, Svg, prelude::*, svg};

use crate::download::{Filter, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
	ChevronUp,
	ChevronDown,
	Settings,
	List,
	CircleDashed,
	Funnel,
	X,
	Code,
	Image,
	BookOpen,
	Disc,
	Database,
	Terminal,
	ChevronRight,
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
			Icon::ChevronUp => "icons/chevron-up.svg",
			Icon::ChevronDown => "icons/chevron-down.svg",
			Icon::Settings => "icons/settings.svg",
			Icon::List => "icons/list.svg",
			Icon::CircleDashed => "icons/circle-dashed.svg",
			Icon::Funnel => "icons/funnel.svg",
			Icon::X => "icons/x.svg",
			Icon::Code => "icons/code.svg",
			Icon::Image => "icons/image.svg",
			Icon::BookOpen => "icons/book-open.svg",
			Icon::Disc => "icons/disc.svg",
			Icon::Database => "icons/database.svg",
			Icon::Terminal => "icons/terminal.svg",
			Icon::ChevronRight => "icons/chevron-right.svg",
		}
	}

	/// What a category may be drawn with: the defaults' icons and a few more file shapes. A short
	/// list on purpose; the whole of Lucide would be a picker nobody finishes scrolling.
	pub const CATEGORY_CHOICES: [Icon; 12] = [
		Icon::Film,
		Icon::Music,
		Icon::FileText,
		Icon::Archive,
		Icon::Package,
		Icon::File,
		Icon::Code,
		Icon::Image,
		Icon::BookOpen,
		Icon::Disc,
		Icon::Database,
		Icon::Terminal,
	];

	/// The icon's name, for the picker's accessibility label and the control socket.
	pub fn name(self) -> &'static str {
		match self {
			Icon::Film => "film",
			Icon::Music => "music",
			Icon::FileText => "file-text",
			Icon::Archive => "archive",
			Icon::Package => "package",
			Icon::File => "file",
			Icon::Code => "code",
			Icon::Image => "image",
			Icon::BookOpen => "book-open",
			Icon::Disc => "disc",
			Icon::Database => "database",
			Icon::Terminal => "terminal",
			other => other.path().trim_start_matches("icons/").trim_end_matches(".svg"),
		}
	}

	pub fn by_name(name: &str) -> Option<Icon> {
		Icon::CATEGORY_CHOICES.into_iter().find(|i| i.name() == name)
	}

	/// The state filters' icons; a category carries its own.
	pub fn for_filter(filter: Filter) -> Icon {
		match filter {
			Filter::All => Icon::List,
			Filter::Downloading => Icon::ArrowDown,
			Filter::Unfinished => Icon::CircleDashed,
			Filter::Completed => Icon::CircleCheck,
			Filter::Category(_) => Icon::File,
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
