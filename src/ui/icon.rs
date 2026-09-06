use gpui::{Hsla, Svg, prelude::*, svg};

use crate::download::{Filter, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
	Plus,
	/// The window's own frame buttons, drawn where the system draws none. See src/ui/frame.rs.
	Minimize,
	Maximize,
	Restore,
	Close,
	Pause,
	Play,
	Trash,
	Film,
	Music,
	FileText,
	Archive,
	Package,
	File,
	/// The five rings, one a status, worn as a mark in the list's Status column.
	CircleCheck,
	CircleX,
	CirclePause,
	CircleArrowDown,
	Clock,
	/// And their bare cousins, worn where a status is something to filter by rather than
	/// something a row is in. The bare pause is `Pause` above, which the toolbar already had,
	/// and the bare cross is `X` below, with the other closes.
	Check,
	ArrowDown,
	Hourglass,
	/// Unfinished in the sidebar, and the exception to the rule below it: the ring is dashed, so
	/// it reads as an outline not yet closed rather than as a mark in a ring.
	CircleDashed,
	LayoutList,
	LayoutGrid,
	ChevronUp,
	ChevronDown,
	Settings,
	/// All Tasks in the sidebar, and the funnel menu's All, which is the same thing.
	Pyramid,
	Funnel,
	/// The status bar's spinner, turned by an animation.
	Loader,
	X,
	Code,
	Image,
	BookOpen,
	Disc,
	Database,
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
	CircleQuestion,
	Search,
	SlidersHorizontal,
	Download,
	Palette,
	Bell,
	Magnet,
	Box,
	Cpu,
	FolderOpen,
	Table,
	Flag,
	FlagOff,
	Info,
}

impl Icon {
	fn path(self) -> &'static str {
		match self {
			Icon::Plus => "lucide/plus.svg",
			Icon::Minimize => "frame/minimize.svg",
			Icon::Maximize => "frame/maximize.svg",
			Icon::Restore => "frame/restore.svg",
			Icon::Close => "frame/close.svg",
			Icon::Search => "lucide/search.svg",
			Icon::SlidersHorizontal => "lucide/sliders-horizontal.svg",
			Icon::Download => "lucide/download.svg",
			Icon::Palette => "lucide/palette.svg",
			Icon::Bell => "lucide/bell.svg",
			Icon::Magnet => "lucide/magnet.svg",
			Icon::Box => "lucide/box.svg",
			Icon::Cpu => "lucide/cpu.svg",
			Icon::FolderOpen => "lucide/folder-open.svg",
			Icon::Table => "lucide/table.svg",
			Icon::Flag => "lucide/flag.svg",
			Icon::FlagOff => "lucide/flag-off.svg",
			Icon::Info => "lucide/info.svg",
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
			Icon::CircleArrowDown => "lucide/circle-arrow-down.svg",
			Icon::Clock => "lucide/clock.svg",
			Icon::Check => "lucide/check.svg",
			Icon::ArrowDown => "lucide/arrow-down.svg",
			Icon::Hourglass => "lucide/hourglass.svg",
			Icon::CircleDashed => "lucide/circle-dashed.svg",
			Icon::LayoutList => "lucide/layout-list.svg",
			Icon::LayoutGrid => "lucide/layout-grid.svg",
			Icon::ChevronUp => "lucide/chevron-up.svg",
			Icon::ChevronDown => "lucide/chevron-down.svg",
			Icon::Settings => "lucide/settings.svg",
			Icon::Pyramid => "lucide/pyramid.svg",
			Icon::Funnel => "lucide/funnel.svg",
			Icon::Loader => "lucide/loader-circle.svg",
			Icon::X => "lucide/x.svg",
			Icon::Code => "lucide/file-braces.svg",
			Icon::Image => "lucide/image.svg",
			Icon::BookOpen => "lucide/book-open.svg",
			Icon::Disc => "lucide/disc.svg",
			Icon::Database => "lucide/database.svg",
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
			Icon::Text => "lucide/file-type.svg",
			Icon::CircleQuestion => "lucide/circle-question-mark.svg",
		}
	}

	/// What a category may be drawn with: the defaults' icons and a few more file shapes. A short
	/// list on purpose; the whole of Lucide would be a picker nobody finishes scrolling.
	pub const CATEGORY_CHOICES: [Icon; 16] = [
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
			Icon::Code => "file-braces",
			Icon::Image => "image",
			Icon::BookOpen => "book-open",
			Icon::Disc => "disc",
			Icon::Database => "database",
			Icon::Gamepad => "gamepad-2",
			Icon::Globe => "globe",
			Icon::Presentation => "presentation",
			Icon::FileSpreadsheet => "file-spreadsheet",
			Icon::Text => "file-type",
			other => other.path().trim_start_matches("lucide/").trim_end_matches(".svg"),
		}
	}

	pub fn by_name(name: &str) -> Option<Icon> {
		// The name a config.json written before the glyph changed still reads as the same icon.
		let name = if name == "code" { "file-braces" } else { name };
		Icon::CATEGORY_CHOICES.into_iter().find(|i| i.name() == name)
	}

	/// The state filters' icons, bare; a category carries its own, which is bare too. See
	/// `for_status_filter` for why the legends go without the ring. Two of the four are not
	/// statuses and so have no ringed cousin: All Tasks is a pyramid, and Unfinished is a dashed
	/// circle, which is an outline left open rather than a ring closed around a mark.
	pub fn for_filter(filter: Filter) -> Icon {
		match filter {
			// A shape rather than a mark: a pyramid holds everything under it.
			Filter::All => Icon::Pyramid,
			Filter::Downloading => Icon::ArrowDown,
			Filter::Unfinished => Icon::CircleDashed,
			Filter::Completed => Icon::Check,
			Filter::Category(_) => Icon::File,
		}
	}

	/// The mark a row wears in the Status column to say where it got to: the glyph in a ring.
	/// The ring is what makes it a mark. It is read down a column, beside a word, at three
	/// points across, and at that size a bare tick and a bare cross are two strokes each; the
	/// ring gives every one of them the same outline, so the column reads as a column of marks
	/// rather than as scratches of different sizes. See spec/icons.md.
	pub fn for_status(status: Status) -> Icon {
		match status {
			Status::Queued => Icon::Clock,
			Status::Downloading => Icon::CircleArrowDown,
			Status::Paused => Icon::CirclePause,
			Status::Completed => Icon::CircleCheck,
			Status::Failed => Icon::CircleX,
		}
	}

	/// The same five where a status is something to filter by rather than something a row is in:
	/// bare, and each the bare cousin of the ring above it. The sidebar and the funnel's menu are
	/// a legend -- a glyph beside the word it stands for, one to a line, with room around it --
	/// and there a ring is a box drawn around a picture that did not need one. It also keeps the
	/// two apart at a glance: a ring in this window means a row's own state, never a filter.
	pub fn for_status_filter(status: Status) -> Icon {
		match status {
			Status::Queued => Icon::Hourglass,
			Status::Downloading => Icon::ArrowDown,
			Status::Paused => Icon::Pause,
			Status::Completed => Icon::Check,
			Status::Failed => Icon::X,
		}
	}
}

/// The color is a parameter, not inherited: an untinted svg paints nothing. See spec/icons.md.
pub fn icon(icon: Icon, color: impl Into<Hsla>) -> Svg {
	svg().path(icon.path()).size_4().flex_none().text_color(color)
}

/// An icon inside a control that brightens it on hover. The svg carries its own color and
/// cannot inherit the control's, so it watches the control through the named group instead:
/// `color` at rest, `hover` while the pointer is on the group. None leaves it alone, for a
/// control that is chosen or disabled and has nothing to show for a hover.
pub fn hover_icon(icon: Icon, group: &'static str, color: Hsla, hover: Option<Hsla>) -> Svg {
	let svg = self::icon(icon, color);
	match hover {
		Some(hover) => svg.group_hover(group, move |s| s.text_color(hover)),
		None => svg,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// A status is drawn twice in this window and never the same way: ringed where a row states
	/// what it is, bare where it is a line in a legend. The ring is what tells the two apart at a
	/// glance, so nothing may wear one on the legend side.
	#[test]
	fn a_status_is_ringed_as_a_mark_and_bare_as_a_legend() {
		for status in Status::ALL {
			let mark = Icon::for_status(status);
			let legend = Icon::for_status_filter(status);
			assert_ne!(mark, legend, "{status:?} is drawn twice, differently, on purpose");
			assert!(!legend.path().contains("circle"), "a legend goes bare: {}", legend.path());
		}
		// Four rings and a clock face, which is the fifth: a circle either way.
		for status in [Status::Downloading, Status::Paused, Status::Completed, Status::Failed] {
			assert!(Icon::for_status(status).path().contains("circle-"), "{status:?}");
		}
		assert_eq!(Icon::for_status(Status::Queued), Icon::Clock);
	}

	/// The sidebar and the funnel's menu are one legend drawn in two places, so where they name
	/// the same thing they draw the same glyph. Two of the sidebar's four name something no
	/// status does, and those are the two that are neither.
	#[test]
	fn the_sidebar_and_the_menu_draw_a_shared_name_the_same_way() {
		assert_eq!(Icon::for_filter(Filter::Downloading), Icon::for_status_filter(Status::Downloading));
		assert_eq!(Icon::for_filter(Filter::Completed), Icon::for_status_filter(Status::Completed));
		assert_eq!(Icon::for_filter(Filter::All), Icon::Pyramid, "a shape, not a mark");
		assert_eq!(
			Icon::for_filter(Filter::Unfinished),
			Icon::CircleDashed,
			"an outline left open, which is why it keeps its circle"
		);
	}
}
