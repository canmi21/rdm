//! The picture the system draws for a file. Every desktop keeps one -- an icon per kind, and for
//! some kinds a picture of the file itself -- and it is the picture somebody already knows the
//! file by, so the thumbnails view and the grid's cards ask for it rather than inventing one.
//!
//! There is no picture to be had on every system or for every file, and the answer is then the
//! category's own glyph. That is not a failure: the glyph is what this application draws when it
//! is drawing for itself, and a file with no picture is drawn the way every file used to be.
//!
//! The cache is by path and never expires within a run. A system icon is the same until the file
//! changes kind, which is not a thing files do; the cost of being wrong is a stale picture until
//! the next launch, and the cost of not caching is a trip to the window server per row per frame.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::RenderImage;

/// How big the system is asked to draw. One size for every use: the thumbnails row draws it at
/// twenty points and the grid at forty-eight, and asking for the larger and letting the smaller
/// scale down is one trip to the system rather than two.
pub const SIZE: usize = 128;

/// The pictures asked for so far, by path. `None` is a file the system had no picture for, kept
/// so it is not asked about again.
/// How many pictures a single frame will ask the system for. Asking is a trip to the window
/// server and a decode, and the list draws every row it has rather than only the visible ones,
/// so a folder of a thousand files would spend a minute in one frame with no limit -- which it
/// did, and the window answered nothing until it was over. With a limit the pictures arrive over
/// the next second or two and the window stays a window meanwhile.
const A_FRAME: u32 = 24;

#[derive(Default)]
pub struct Thumbnails {
	cache: HashMap<PathBuf, Option<Arc<RenderImage>>>,
	/// What is left of this frame's allowance, and whether the frame ran out. Running out is
	/// what asks for another frame: the rest of the pictures are waiting in it.
	budget: u32,
	starved: bool,
}

impl Thumbnails {
	/// The system's picture for this file, or None where there is none to be had -- or where this
	/// frame has asked for as many as it will. The first call for a path asks the system; the
	/// rest read the answer.
	pub fn of(&mut self, path: &Path) -> Option<Arc<RenderImage>> {
		if let Some(known) = self.cache.get(path) {
			return known.clone();
		}
		if self.budget == 0 {
			// Not cached as absent: this file has not been asked about, only deferred.
			self.starved = true;
			return None;
		}
		self.budget -= 1;
		let made = read(path).map(Arc::new);
		self.cache.insert(path.to_path_buf(), made.clone());
		made
	}

	/// Called once at the top of a frame: this frame may ask for so many and no more.
	pub fn begin_frame(&mut self) {
		self.budget = A_FRAME;
		self.starved = false;
	}

	/// Whether the frame ran out of allowance, which is to say another frame is owed.
	pub fn starved(&self) -> bool {
		self.starved
	}

	/// Forgets a file's picture, for a file that has changed on disk.
	pub fn forget(&mut self, path: &Path) {
		self.cache.remove(path);
	}
}

/// Turns straight BGRA bytes into what gpui draws. gpui's `RenderImage` is documented as BGRA
/// while the type it is built from is an RGBA image: the bytes are handed over in the order the
/// renderer wants and the type is only a carrier, which is worth saying out loud because getting
/// it backwards shows as blue people.
fn render_image(width: u32, height: u32, bgra: Vec<u8>) -> Option<RenderImage> {
	let frame = image::RgbaImage::from_raw(width, height, bgra)?;
	Some(RenderImage::new([image::Frame::new(frame)]))
}

#[cfg(target_os = "macos")]
fn read(path: &Path) -> Option<RenderImage> {
	use objc2_app_kit::NSWorkspace;
	use objc2_foundation::NSString;

	// The window server answers this, and it answers on the main thread; every caller is drawing,
	// so every caller is on it.
	let path = NSString::from_str(path.to_str()?);
	let icon = NSWorkspace::sharedWorkspace().iconForFile(&path);
	// An NSImage is a set of representations rather than pixels. Asking for the TIFF is asking it
	// to become pixels, and is one call against the half-dozen that drawing it into a bitmap
	// context takes.
	let tiff = icon.TIFFRepresentation()?;
	let bytes = tiff.to_vec();
	let decoded = image::load_from_memory_with_format(&bytes, image::ImageFormat::Tiff).ok()?;
	let scaled = image::imageops::resize(
		&decoded.into_rgba8(),
		SIZE as u32,
		SIZE as u32,
		image::imageops::FilterType::CatmullRom,
	);
	let mut bgra = scaled.into_raw();
	// The decoder gives RGBA and the renderer wants BGRA; the two differ by a swap of the first
	// and third byte of every pixel. Getting it backwards shows as blue people.
	for pixel in bgra.as_chunks_mut::<4>().0 {
		pixel.swap(0, 2);
	}
	render_image(SIZE as u32, SIZE as u32, bgra)
}

/// Windows keeps one too, and asking for it is `SHGetFileInfo`; until that is written the
/// category's glyph stands in, which is what it did for every file before any of this.
#[cfg(not(target_os = "macos"))]
fn read(_path: &Path) -> Option<RenderImage> {
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The answer is kept whatever it was, so a file is asked about once; and a frame that has
	/// spent its allowance defers rather than answering, which is not the same as answering None
	/// -- a deferred file has not been asked about and must not be remembered as having none.
	#[test]
	fn a_picture_is_asked_for_once_a_file_and_no_more_than_an_allowance_a_frame() {
		let mut thumbnails = Thumbnails::default();
		let missing = Path::new("/nowhere/at/all/file.txt");
		// No frame has begun, so nothing may be asked and nothing is remembered.
		assert!(thumbnails.of(missing).is_none(), "outside a frame there is no allowance");
		assert!(thumbnails.starved(), "and the frame is owed one");
		assert!(thumbnails.cache.is_empty(), "a deferred file is not a file without a picture");

		thumbnails.begin_frame();
		let _ = thumbnails.of(missing);
		assert_eq!(thumbnails.cache.len(), 1, "asked, and the answer kept whatever it was");
		let _ = thumbnails.of(missing);
		assert_eq!(thumbnails.cache.len(), 1, "and not asked again");
		thumbnails.forget(missing);
		assert!(thumbnails.cache.is_empty(), "a file that changed is asked about again");

		// The allowance is a frame's, and a new frame has a new one.
		thumbnails.begin_frame();
		assert!(!thumbnails.starved());
		for n in 0..A_FRAME {
			let _ = thumbnails.of(Path::new(&format!("/nowhere/{n}.txt")));
		}
		assert_eq!(thumbnails.cache.len(), A_FRAME as usize);
		let _ = thumbnails.of(Path::new("/nowhere/one-too-many.txt"));
		assert_eq!(thumbnails.cache.len(), A_FRAME as usize, "the allowance is spent");
		assert!(thumbnails.starved(), "so another frame is owed");
	}
}
