//! Nord, in two moods. The names say what a colour is for, never what it looks like.

use gpui::{Hsla, Rgba, rgb, rgba};

/// A hue an icon may carry: the sidebar's filters and categories each own one, and a row's type
/// icon borrows its category's. Named for the colour, since here the colour is the point: a
/// column of hues reads faster than a column of glyphs, which is why status is colour-coded too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
	Red,
	Orange,
	Yellow,
	Green,
	Teal,
	Frost,
	Blue,
	Navy,
	Purple,
	/// Snow storm's brightest white: what All carries, and the catch-all.
	Snow,
}

impl Tint {
	/// The hues handed out in turn to categories that do not name one, so a custom rule gets a
	/// colour of its own without anyone choosing it.
	pub const CYCLE: [Tint; 9] = [
		Tint::Frost,
		Tint::Green,
		Tint::Orange,
		Tint::Purple,
		Tint::Yellow,
		Tint::Teal,
		Tint::Red,
		Tint::Blue,
		Tint::Navy,
	];

	pub fn cycle(index: u64) -> Tint {
		Tint::CYCLE[(index as usize) % Tint::CYCLE.len()]
	}
}

/// The colours one frame is drawn with, chosen once per render from the window's state.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
	/// The window has the keyboard; without it every hue below collapses to `muted`.
	pub active: bool,
	pub window: Hsla,
	pub sidebar: Hsla,
	pub panel: Hsla,
	pub border: Hsla,
	pub text: Hsla,
	pub muted: Hsla,
	pub accent: Hsla,
	pub selection: Hsla,
	pub hover: Hsla,
	pub track: Hsla,
	pub success: Hsla,
	pub warning: Hsla,
	pub failure: Hsla,
	/// Laid over the window while a sheet is up.
	pub dim: Hsla,
}

// Polar night, snow storm, frost and aurora, as Nord names them.
const NORD0: u32 = 0x2e3440;
const NORD1: u32 = 0x3b4252;
const NORD2: u32 = 0x434c5e;
const NORD3: u32 = 0x4c566a;
const NORD4: u32 = 0xd8dee9;
const NORD6: u32 = 0xeceff4;
const NORD7: u32 = 0x8fbcbb;
const NORD8: u32 = 0x88c0d0;
const NORD9: u32 = 0x81a1c1;
const NORD10: u32 = 0x5e81ac;
const NORD11: u32 = 0xbf616a;
const NORD12: u32 = 0xd08770;
const NORD13: u32 = 0xebcb8b;
const NORD14: u32 = 0xa3be8c;
const NORD15: u32 = 0xb48ead;

fn solid(hex: u32) -> Hsla {
	rgb(hex).into()
}

fn glass(hex: u32, alpha: f32) -> Hsla {
	Rgba { a: alpha, ..rgba(hex << 8) }.into()
}

/// Only the sidebar lets the blurred desktop through, and only a little: a native sidebar is a
/// material, not a transparency. An inactive window gives up every hue and keeps its greys. See
/// spec/ui.md.
pub fn palette(active: bool) -> Palette {
	let muted = glass(NORD4, 0.55);
	let hue = |color: u32| if active { solid(color) } else { muted };
	Palette {
		active,
		window: solid(NORD0),
		sidebar: glass(NORD0, 0.88),
		panel: solid(NORD1),
		border: glass(NORD3, 0.55),
		text: solid(if active { NORD6 } else { NORD4 }),
		muted,
		accent: hue(NORD8),
		selection: if active { solid(NORD2) } else { glass(NORD3, 0.5) },
		hover: glass(NORD3, 0.35),
		track: glass(NORD3, 0.6),
		success: hue(NORD14),
		warning: hue(NORD12),
		failure: hue(NORD11),
		dim: glass(NORD0, 0.55),
	}
}

impl Palette {
	/// A tint as this frame draws it: the hue while the window is active, the muted grey while it
	/// is not, so an inactive window goes monochrome the way its status colours already do.
	pub fn tint(&self, tint: Tint) -> Hsla {
		if !self.active {
			return self.muted;
		}
		match tint {
			Tint::Red => solid(NORD11),
			Tint::Orange => solid(NORD12),
			Tint::Yellow => solid(NORD13),
			Tint::Green => solid(NORD14),
			Tint::Teal => solid(NORD7),
			Tint::Frost => solid(NORD8),
			Tint::Blue => solid(NORD9),
			Tint::Navy => solid(NORD10),
			Tint::Purple => solid(NORD15),
			Tint::Snow => solid(NORD6),
		}
	}
}
