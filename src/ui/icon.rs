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
	/// A status, ringed: the shape every one of them shares, wherever a status is drawn.
	CircleCheck,
	CircleX,
	CirclePause,
	CircleArrowDown,
	/// Queued, and a circle too: a clock face is the ring drawn with hands in it.
	Clock,
	/// Unfinished in the sidebar, which is a ring left open rather than one closed around a mark.
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

	/// The state filters' icons: the same rings the Status column wears, since the sidebar is a
	/// legend to that column and a legend drawn in another shape is a second legend. A category
	/// carries its own icon, which is whatever it was given.
	pub fn for_filter(filter: Filter) -> Icon {
		match filter {
			// A shape rather than a mark: a pyramid holds everything under it.
			Filter::All => Icon::Pyramid,
			Filter::Downloading => Icon::for_status(Status::Downloading),
			// The one ring that is not a status: an outline not yet closed, which is what
			// unfinished looks like and what no single status means.
			Filter::Unfinished => Icon::CircleDashed,
			Filter::Completed => Icon::for_status(Status::Completed),
			Filter::Category(_) => Icon::File,
		}
	}

	/// A status, wherever it is drawn: as the mark down the list's Status column, as a row in the
	/// funnel's menu, and behind the two state filters that name one. Every one of them is a
	/// ring -- a tick, a cross, two bars, an arrow, and the clock, which is a ring with hands in
	/// it. The ring is what makes it a mark: read down a column at three points across, a bare
	/// tick and a bare cross are two strokes each, and the ring gives them all one outline, so
	/// the eye finds the column before it reads any of it. Drawn the same in the menu because the
	/// menu is the legend to that column. See spec/icons.md.
	pub fn for_status(status: Status) -> Icon {
		match status {
			Status::Queued => Icon::Clock,
			Status::Downloading => Icon::CircleArrowDown,
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

	/// Every status is a ring, and the two state filters that name one wear the same ring, since
	/// the sidebar and the funnel's menu are the legend to the column the marks are read down.
	#[test]
	fn a_status_is_a_ring_wherever_it_is_drawn() {
		for status in Status::ALL {
			let path = Icon::for_status(status).path();
			assert!(path.contains("circle-") || path.contains("clock"), "{status:?} is a ring: {path}");
		}
		assert_eq!(Icon::for_status(Status::Downloading), Icon::CircleArrowDown);
		assert_eq!(Icon::for_filter(Filter::Downloading), Icon::for_status(Status::Downloading));
		assert_eq!(Icon::for_filter(Filter::Completed), Icon::for_status(Status::Completed));
	}

	/// Two of the sidebar's four name what no status does, and those are the two drawn otherwise.
	#[test]
	fn the_states_that_are_not_statuses_keep_their_own_glyph() {
		assert_eq!(Icon::for_filter(Filter::All), Icon::Pyramid, "a shape, not a mark");
		assert_eq!(
			Icon::for_filter(Filter::Unfinished),
			Icon::CircleDashed,
			"a ring left open, which is what unfinished looks like"
		);
		let statuses: Vec<Icon> = Status::ALL.into_iter().map(Icon::for_status).collect();
		for filter in [Filter::All, Filter::Unfinished] {
			assert!(!statuses.contains(&Icon::for_filter(filter)), "{filter:?} is nobody's status");
		}
	}
}
