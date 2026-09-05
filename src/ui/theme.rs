//! Nord, in two moods. The names say what a colour is for, never what it looks like.

use gpui::{Hsla, Rgba, rgb, rgba};

/// The colours one frame is drawn with, chosen once per render from the window's state.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
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
}

// Polar night, snow storm, frost and aurora, as Nord names them.
const NORD0: u32 = 0x2e3440;
const NORD1: u32 = 0x3b4252;
const NORD2: u32 = 0x434c5e;
const NORD3: u32 = 0x4c566a;
const NORD4: u32 = 0xd8dee9;
const NORD6: u32 = 0xeceff4;
const NORD8: u32 = 0x88c0d0;
const NORD11: u32 = 0xbf616a;
const NORD12: u32 = 0xd08770;
const NORD14: u32 = 0xa3be8c;

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
	}
}
