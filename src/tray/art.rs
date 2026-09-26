//! The tray's frames, drawn from SVG as they are asked for: the arrow of the application's glyph at
//! rest, falling while something downloads, with its base waiting while something queues, the
//! cloud turning while the rules sync, and a dot for something that wants a look. Everything is
//! laid out in the glyph's own 1024 space, `assets/icon-glyph.svg`, so the frames are the artwork
//! and not a second drawing of it. See spec/ui.md, "An icon in the tray".

use anyhow::{Context as _, Result};

/// What the icon says, from the most pressing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
	/// Something is moving: overall progress when every running size is known.
	Downloading {
		progress: Option<f32>,
	},
	/// The rules are being fetched.
	Syncing,
	/// Nothing moves, but something waits for a place.
	Queued,
	Idle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
	pub motion: Motion,
	/// A dot in the corner: a sync or a download failed and nobody has looked since.
	pub alert: bool,
}

impl Look {
	pub const IDLE: Look = Look { motion: Motion::Idle, alert: false };

	/// How many frames the motion takes before it repeats; one for a still one.
	pub fn frames(self) -> u32 {
		match self.motion {
			// Known progress moves as the bytes do and is redrawn when it does; unknown progress is
			// a short run sliding along the base.
			Motion::Downloading { progress: Some(_) } => 1,
			Motion::Downloading { progress: None } => SLIDE_FRAMES,
			Motion::Syncing => TURN_FRAMES,
			Motion::Queued => 3 * DOT_FRAMES,
			Motion::Idle => 1,
		}
	}
}

/// A frame decoded: straight RGBA bytes and the rectangle they make.
pub struct Artwork {
	pub width: u32,
	pub height: u32,
	pub rgba: Vec<u8>,
}

impl Artwork {
	/// The same pixels as ARGB32 in network byte order, which is what a StatusNotifierItem
	/// carries. Only Linux draws from it, but the test runs everywhere on purpose: a byte order
	/// put back to front is invisible until somebody opens the one desktop that reads it.
	#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
	pub fn argb32(&self) -> Vec<u8> {
		let mut data = self.rgba.clone();
		for pixel in data.as_chunks_mut::<4>().0 {
			pixel.rotate_right(1);
		}
		data
	}
}

/// How a system's tray draws an icon. macOS tints a template itself and sizes every icon to 18
/// points high, so the glyph is cropped to its own extent and drawn in black; the other two draw an
/// icon as it is, so the glyph sits white on the application's tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
	Template,
	Tile,
}

impl Style {
	pub fn native() -> Style {
		if cfg!(target_os = "macos") { Style::Template } else { Style::Tile }
	}

	fn pixels(self) -> u32 {
		match self {
			// 18 points at 2x, the height tray-icon gives every icon in the menu bar.
			Style::Template => 36,
			Style::Tile => 64,
		}
	}

	/// The square of the 1024 space the frame shows: the glyph's own extent, 178 to 846, and a
	/// point of room either side, or the whole tile.
	fn view(self) -> (f32, f32) {
		match self {
			Style::Template => (136.0, 752.0),
			Style::Tile => (0.0, 1024.0),
		}
	}

	fn stroke(self) -> f32 {
		match self {
			// Two points: the weight of the symbols beside it in the menu bar.
			Style::Template => 84.0,
			Style::Tile => 114.0,
		}
	}

	fn ink(self) -> &'static str {
		match self {
			Style::Template => "#000",
			Style::Tile => "#fff",
		}
	}
}

/// How many frames the run of an unknown download takes across the base and back.
const SLIDE_FRAMES: u32 = 24;
/// How long that run is, of the base's 424.
const RUN: f32 = 120.0;
const TURN_FRAMES: u32 = 12;
/// How long each of the three waiting dots stays lit.
pub const DOT_FRAMES: u32 = 4;
/// What the unlit part of a line or dot keeps.
const FAINT: f32 = 0.35;

/// The arrow: shaft and head, as the glyph has them.
const ARROW: &str = r#"<path d="M512 232V620"/><path d="M336 468L512 644L688 468"/>"#;

/// The frame `phase` of `look`, drawn for `style`.
pub fn frame(look: Look, phase: u32, style: Style) -> Result<Artwork> {
	render(&svg(look, phase, style), style.pixels())
}

/// The frame as SVG. Separate from the drawing so a test can read what a frame holds.
pub fn svg(look: Look, phase: u32, style: Style) -> String {
	let (origin, side) = style.view();
	let stroke = style.stroke();
	let ink = style.ink();
	let body = match look.motion {
		Motion::Idle => format!(r#"{ARROW}<path d="M300 792H724"/>"#),
		Motion::Downloading { progress } => filling(phase % SLIDE_FRAMES, progress),
		Motion::Queued => waiting(phase / DOT_FRAMES % 3, stroke, ink),
		Motion::Syncing => turning(phase % TURN_FRAMES, stroke),
	};
	let tile = match style {
		Style::Template => String::new(),
		Style::Tile => r##"<defs><linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#459bff"/><stop offset="1" stop-color="#3a86f7"/></linearGradient></defs><rect width="1024" height="1024" rx="230" fill="url(#bg)"/>"##.to_owned(),
	};
	let (mask, badge) = if look.alert { alert(style) } else { (String::new(), String::new()) };
	let masked = if look.alert { r#" mask="url(#alert)""# } else { "" };
	format!(
		r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{origin} {origin} {side} {side}">{tile}{mask}<g{masked} fill="none" stroke="{ink}" stroke-width="{stroke}" stroke-linecap="round" stroke-linejoin="round">{body}</g>{badge}</svg>"#
	)
}

/// The arrow still, and its base the progress bar: filled as far as the download has come, or
/// for one of no known size a short run sliding along it and back. The icon says what goes on by
/// what it is, and only the smallest part of it moves. See spec/ui.md.
fn filling(step: u32, progress: Option<f32>) -> String {
	let (from, to) = match progress {
		Some(done) => (300.0, 300.0 + 424.0 * done.clamp(0.0, 1.0)),
		None => {
			// There and back: a triangle over the loop.
			let half = SLIDE_FRAMES as f32 / 2.0;
			let along = 1.0 - (step as f32 - half).abs() / half;
			let start = 300.0 + (424.0 - RUN) * along;
			(start, start + RUN)
		}
	};
	format!(
		r#"{ARROW}<path d="M300 792H724" stroke-opacity="{FAINT}"/><path d="M{from:.0} 792H{to:.0}"/>"#
	)
}

/// The arrow still, and its base three dots lit one after another.
fn waiting(lit: u32, stroke: f32, ink: &str) -> String {
	let r = stroke * 0.6;
	let dots: String = [300.0, 512.0, 724.0]
		.iter()
		.enumerate()
		.map(|(i, x)| {
			let opacity = if i as u32 == lit { 1.0 } else { FAINT };
			format!(r#"<circle cx="{x}" cy="792" r="{r:.0}" stroke="none" fill-opacity="{opacity}"/>"#)
		})
		.collect();
	// The dots are filled, which the group around them is not.
	format!(r#"{ARROW}<g fill="{ink}">{dots}</g>"#)
}

/// Lucide's cloud-sync: the cloud still, and only the two arrows under it turning. The cloud is
/// cleared in a ring round the arrows, so they turn in a space of their own rather than across its
/// line. Its 24 space is put on the glyph's extent at the glyph's weight; see assets/sync/.
fn turning(step: u32, stroke: f32) -> String {
	// The icon spans 1 to 23 across and 2 to 22 down, around 12 and 12.
	let scale = 668.0 / 21.0;
	// The pair looks the same half a turn on, so a half turn is the whole cycle.
	let angle = 180.0 * step as f32 / TURN_FRAMES as f32;
	// The arrows at seven tenths about their own middle, 12 and 16, so they turn clear of the cloud
	// and inside the frame; the ring cleared round them worked out in the frame's own space, since a
	// mask in the space of a scaled group is read differently by resvg.
	let (cx, cy, r) = (512.0, 512.0 + 4.0 * scale, 6.6 * scale);
	let lucide = format!("translate(512 512) scale({scale:.3}) translate(-12 -12)");
	// Lucide's own weight: the cloud and the arrows together are more lines than the arrow glyph,
	// and at its weight they ran into each other.
	let width = (stroke * 0.75) / scale;
	format!(
		r##"<mask id="gap" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024"><rect width="1024" height="1024" fill="#fff"/><circle cx="{cx:.0}" cy="{cy:.0}" r="{r:.0}" fill="#000"/></mask><g stroke-width="{width:.2}"><g mask="url(#gap)"><path transform="{lucide}" d="M20.996 15.251A4.5 4.5 0 0 0 17.495 8h-1.79a7 7 0 1 0-12.709 5.607"/></g><g transform="{lucide} rotate({angle:.0} 12 16) translate(12 16) scale(0.7) translate(-12 -16)" stroke-width="{:.2}"><path d="m17 18-1.535 1.605a5 5 0 0 1-8-1.5"/><path d="M17 22v-4h-4"/><path d="M7 10v4h4"/><path d="m7 14 1.535-1.605a5 5 0 0 1 8 1.5"/></g></g>"##,
		width / 0.7
	)
}

/// The mask that clears a ring around the corner, and the dot drawn in it.
fn alert(style: Style) -> (String, String) {
	let (cx, cy, r) = match style {
		// Three points across, a point and a half in from the top right.
		Style::Template => (763.0, 282.0, 125.0),
		Style::Tile => (800.0, 224.0, 150.0),
	};
	let ring = r * 1.5;
	let mask = format!(
		r##"<mask id="alert"><rect x="-1024" y="-1024" width="3072" height="3072" fill="#fff"/><circle cx="{cx}" cy="{cy}" r="{ring}" fill="#000"/></mask>"##
	);
	let dot = match style {
		Style::Template => format!(r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="#000"/>"##),
		Style::Tile => format!(r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="#bf616a"/>"##),
	};
	(mask, dot)
}

/// SVG to straight RGBA, `pixels` on a side.
fn render(svg: &str, pixels: u32) -> Result<Artwork> {
	let tree =
		resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).context("read the frame")?;
	let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels).context("a frame's pixels")?;
	let scale = pixels as f32 / tree.size().width();
	resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
	Ok(Artwork { width: pixels, height: pixels, rgba: crate::thumbnail::straight(&pixmap) })
}

#[cfg(test)]
mod tests {
	use super::*;

	const EVERY: [Motion; 5] = [
		Motion::Idle,
		Motion::Queued,
		Motion::Syncing,
		Motion::Downloading { progress: None },
		Motion::Downloading { progress: Some(0.4) },
	];

	#[test]
	fn every_frame_of_every_look_draws_for_both_styles() {
		for style in [Style::Template, Style::Tile] {
			for motion in EVERY {
				for alert in [false, true] {
					let look = Look { motion, alert };
					for phase in 0..look.frames() {
						let art = frame(look, phase, style).unwrap();
						assert_eq!(art.width, style.pixels());
						assert_eq!(art.rgba.len(), (art.width * art.height * 4) as usize);
						assert!(art.rgba.chunks(4).any(|p| p[3] > 0), "{look:?} {phase} draws something");
					}
				}
			}
		}
	}

	/// The glyph fills the menu bar's height: its rows run from near the top to near the bottom,
	/// which the old artwork, a third of it margin, did not.
	#[test]
	fn the_template_glyph_reaches_the_edges_of_its_frame() {
		let art = frame(Look::IDLE, 0, Style::Template).unwrap();
		let rows: Vec<u32> = (0..art.height)
			.filter(|y| (0..art.width).any(|x| art.rgba[((y * art.width + x) * 4 + 3) as usize] > 128))
			.collect();
		assert!(rows.first().is_some_and(|y| *y <= 3), "the top of the arrow: {rows:?}");
		assert!(rows.last().is_some_and(|y| *y >= art.height - 4), "the base: {rows:?}");
	}

	#[test]
	fn the_indicators_pixels_are_argb_and_the_decoders_are_rgba() {
		let art = Artwork { width: 1, height: 1, rgba: vec![0x11, 0x22, 0x33, 0xff] };
		assert_eq!(art.argb32(), vec![0xff, 0x11, 0x22, 0x33], "alpha leads, then red, green, blue");
	}
}
