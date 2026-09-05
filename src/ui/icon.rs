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
	ArrowLeft,
	GripVertical,
	Pencil,
	CaseSensitive,
	Space,
	Gamepad,
	Globe,
	Presentation,
	FileSpreadsheet,
	Text,
}

impl Icon {
	fn path(self) -> &'static str {
		match self {
			Icon::Plus => "lucide/plus.svg",
			Icon::Pause => "lucide/pause.svg",
			Icon::Play => "lucide/play.svg",
			Icon::Trash => "lucide/trash.svg",
			Icon::Film => "lucide/film.svg",
			Icon::Music => "lucide/music.svg",
			Icon::FileText => "lucide/file-text.svg",
			Icon::Archive => "lucide/archive.svg",
			Icon::Package => "lucide/package.svg",
			Icon::File => "lucide/file.svg",
			Icon::CircleCheck => "lucide/circle-check.svg",
			Icon::CircleX => "lucide/circle-x.svg",
			Icon::CirclePause => "lucide/circle-pause.svg",
			Icon::Clock => "lucide/clock.svg",
			Icon::ArrowDown => "lucide/arrow-down.svg",
			Icon::LayoutList => "lucide/layout-list.svg",
			Icon::Rows => "lucide/rows-3.svg",
			Icon::LayoutGrid => "lucide/layout-grid.svg",
			Icon::ChevronUp => "lucide/chevron-up.svg",
			Icon::ChevronDown => "lucide/chevron-down.svg",
			Icon::Settings => "lucide/settings.svg",
			Icon::List => "lucide/list.svg",
			Icon::CircleDashed => "lucide/circle-dashed.svg",
			Icon::Funnel => "lucide/funnel.svg",
			Icon::X => "lucide/x.svg",
			Icon::Code => "lucide/code.svg",
			Icon::Image => "lucide/image.svg",
			Icon::BookOpen => "lucide/book-open.svg",
			Icon::Disc => "lucide/disc.svg",
			Icon::Database => "lucide/database.svg",
			Icon::Terminal => "lucide/terminal.svg",
			Icon::ChevronRight => "lucide/chevron-right.svg",
			Icon::ArrowLeft => "lucide/arrow-left.svg",
			Icon::GripVertical => "lucide/grip-vertical.svg",
			Icon::Pencil => "lucide/pencil.svg",
			Icon::CaseSensitive => "lucide/case-sensitive.svg",
			Icon::Space => "lucide/space.svg",
			Icon::Gamepad => "lucide/gamepad-2.svg",
			Icon::Globe => "lucide/globe.svg",
			Icon::Presentation => "lucide/presentation.svg",
			Icon::FileSpreadsheet => "lucide/file-spreadsheet.svg",
			Icon::Text => "lucide/text-initial.svg",
		}
	}

	/// What a category may be drawn with: the defaults' icons and a few more file shapes. A short
	/// list on purpose; the whole of Lucide would be a picker nobody finishes scrolling.
	pub const CATEGORY_CHOICES: [Icon; 17] = [
		Icon::Film,
		Icon::Music,
		Icon::FileText,
		Icon::Text,
		Icon::Presentation,
		Icon::FileSpreadsheet,
		Icon::Archive,
		Icon::Package,
		Icon::File,
		Icon::Code,
		Icon::Image,
		Icon::BookOpen,
		Icon::Disc,
		Icon::Database,
		Icon::Terminal,
		Icon::Gamepad,
		Icon::Globe,
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
			Icon::Gamepad => "gamepad-2",
			Icon::Globe => "globe",
			Icon::Presentation => "presentation",
			Icon::FileSpreadsheet => "file-spreadsheet",
			Icon::Text => "text-initial",
			other => other.path().trim_start_matches("lucide/").trim_end_matches(".svg"),
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

/// The color is a parameter, not inherited: an untinted svg paints nothing. See spec/icons.md.
pub fn icon(icon: Icon, color: impl Into<Hsla>) -> Svg {
	svg().path(icon.path()).size_4().flex_none().text_color(color)
}
