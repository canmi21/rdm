//! Nord, in two moods. The names say what a color is for, never what it looks like.

use gpui::{Hsla, Rgba, rgb, rgba};

/// A hue an icon may carry: the sidebar's filters and categories each own one, and a row's type
/// icon borrows its category's. Named for the color, since here the color is the point: a
/// column of hues reads faster than a column of glyphs, which is why status is color-coded too.
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
	/// color of its own without anyone choosing it.
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

	/// The tint as a color, for a category that keeps its color as a number so a hand-typed
	/// one is the same kind of thing as a named one.
	pub fn rgb(self) -> u32 {
		match self {
			Tint::Red => NORD11,
			Tint::Orange => NORD12,
			Tint::Yellow => NORD13,
			Tint::Green => NORD14,
			Tint::Teal => NORD7,
			Tint::Frost => NORD8,
			Tint::Blue => NORD9,
			Tint::Navy => NORD10,
			Tint::Purple => NORD15,
			Tint::Snow => NORD6,
		}
	}
}

/// A color as a user might write one, to `0xrrggbb`. The shapes are the ones this stack has
/// constructors for: hex in every common length -- `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
/// with or without the hash -- and the CSS functions `rgb()`, `rgba()`, `hsl()` and `hsla()`,
/// with channels as numbers or percentages. Alpha is read and dropped, since an icon is either
/// drawn or not. None for anything else, including named colors, which nothing here knows.
pub fn parse_color(text: &str) -> Option<u32> {
	let text = text.trim();
	if let Some((name, rest)) = text.split_once('(')
		&& let Some(inner) = rest.strip_suffix(')')
	{
		let parts: Vec<&str> = inner
			.split(|c: char| c == ',' || c == '/' || c.is_whitespace())
			.filter(|p| !p.is_empty())
			.collect();
		if parts.len() < 3 {
			return None;
		}
		let channel = |part: &str, scale: f32| -> Option<f32> {
			match part.strip_suffix('%') {
				Some(percent) => percent.parse::<f32>().ok().map(|v| v / 100.0 * scale),
				None => part.parse::<f32>().ok(),
			}
		};
		return match name.trim().to_ascii_lowercase().as_str() {
			"rgb" | "rgba" => {
				let r = channel(parts[0], 255.0)?;
				let g = channel(parts[1], 255.0)?;
				let b = channel(parts[2], 255.0)?;
				Some(pack(r, g, b))
			}
			"hsl" | "hsla" => {
				let h = parts[0].trim_end_matches("deg").parse::<f32>().ok()?;
				let s =
					channel(parts[1], 1.0).map(|v| if parts[1].ends_with('%') { v } else { v / 100.0 })?;
				let l =
					channel(parts[2], 1.0).map(|v| if parts[2].ends_with('%') { v } else { v / 100.0 })?;
				let (r, g, b) = hsl_to_rgb(h, s, l);
				Some(pack(r, g, b))
			}
			_ => None,
		};
	}
	parse_hex(text)
}

fn pack(r: f32, g: f32, b: f32) -> u32 {
	let byte = |v: f32| v.round().clamp(0.0, 255.0) as u32;
	(byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// Hue in degrees, saturation and lightness in 0..=1, to channels in 0..=255.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
	let s = s.clamp(0.0, 1.0);
	let l = l.clamp(0.0, 1.0);
	let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
	let h = h.rem_euclid(360.0) / 60.0;
	let x = c * (1.0 - (h % 2.0 - 1.0).abs());
	let (r, g, b) = match h as u32 {
		0 => (c, x, 0.0),
		1 => (x, c, 0.0),
		2 => (0.0, c, x),
		3 => (0.0, x, c),
		4 => (x, 0.0, c),
		_ => (c, 0.0, x),
	};
	let m = l - c / 2.0;
	((r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0)
}

/// The hex shapes alone, with or without the hash.
fn parse_hex(text: &str) -> Option<u32> {
	let digits = text.trim().trim_start_matches('#');
	if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
		return None;
	}
	let expand = |s: &str| -> Option<u32> {
		let wide: String = s.chars().flat_map(|c| [c, c]).collect();
		u32::from_str_radix(&wide, 16).ok()
	};
	match digits.len() {
		3 => expand(digits),
		4 => expand(&digits[..3]),
		6 => u32::from_str_radix(digits, 16).ok(),
		8 => u32::from_str_radix(&digits[..6], 16).ok(),
		_ => None,
	}
}

pub fn format_hex(rgb: u32) -> String {
	format!("#{:06x}", rgb & 0xffffff)
}

/// The colors one frame is drawn with, chosen once per render from the window's state.
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
	/// A color as this frame draws it: itself while the window is active, the muted grey while
	/// it is not, so an inactive window goes monochrome the way its status colors already do.
	pub fn hue(&self, rgb: u32) -> Hsla {
		if self.active { solid(rgb) } else { self.muted }
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn hex_in_every_common_length_with_alpha_dropped() {
		assert_eq!(parse_color("#b48ead"), Some(0xb48ead));
		assert_eq!(parse_color("B48EAD"), Some(0xb48ead));
		assert_eq!(parse_color("#abc"), Some(0xaabbcc));
		assert_eq!(parse_color("#abcf"), Some(0xaabbcc));
		assert_eq!(parse_color("#b48ead80"), Some(0xb48ead));
		assert_eq!(parse_color("#b48ea"), None);
		assert_eq!(parse_color("#ggg"), None);
		assert_eq!(format_hex(0xb48ead), "#b48ead");
	}

	#[test]
	fn the_css_functions_the_stack_has_constructors_for() {
		assert_eq!(parse_color("rgb(180, 142, 173)"), Some(0xb48ead));
		assert_eq!(parse_color("rgba(180 142 173 / 0.5)"), Some(0xb48ead));
		assert_eq!(parse_color("rgb(100%, 0%, 50%)"), Some(0xff0080));
		assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some(0xff0000));
		assert_eq!(parse_color("hsl(120deg 100% 25%)"), Some(0x008000));
		assert_eq!(parse_color("hsla(240, 100, 50, 1)"), Some(0x0000ff));
		assert_eq!(parse_color("rgb(1, 2)"), None);
		assert_eq!(parse_color("red"), None);
	}
}
