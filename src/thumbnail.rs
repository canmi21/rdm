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
///
/// It belongs to the reader below, and macOS is the only system with one; the cfg widens the day
/// Windows gets its `SHGetFileInfo`. See spec/ui.md.
#[cfg(target_os = "macos")]
const SIZE: usize = 128;

/// What the grid can put on a card, beyond the category's glyph.
#[derive(Clone)]
pub enum Preview {
	/// The file itself, scaled to fit: a picture of a picture.
	Picture(Arc<RenderImage>),
	/// The first few lines of it, as they are. A text file's contents are its own best icon, and
	/// a page of real words says more about what a file is than any glyph.
	Lines(Vec<String>),
	/// The system's icon for the kind, which is what everything else falls back to.
	Icon(Arc<RenderImage>),
}

/// How big a picture is drawn on a card, and the largest file worth opening to make one. A
/// hundred megapixels of RAW is not a card, it is a stall.
const CARD: u32 = 256;
const BIGGEST: u64 = 32 * 1024 * 1024;

/// How much of a text file is read and shown. Six lines of sixty is a paragraph's worth: enough
/// to tell a licence from a changelog from a stack trace, and no more than a card can hold.
const LINES: usize = 6;
const COLUMNS: usize = 60;

/// The pictures asked for so far, by path. `None` is a file the system had no picture for, kept
/// so it is not asked about again.
/// How many pictures a single frame will ask the system for. Asking is a trip to the window
/// server and a decode, and the list draws every row it has rather than only the visible ones,
/// so a folder of a thousand files would spend a minute in one frame with no limit -- which it
/// did, and the window answered nothing until it was over. With a limit the pictures arrive over
/// the next second or two and the window stays a window meanwhile.
const A_FRAME: u32 = 24;

/// How many made pictures the folder keeps. Each is a PNG of a card, tens of kilobytes; a
/// thousand of them is a folder somebody would not notice, and the oldest go first because the
/// newest are the files being looked at. One deleted is one made again the next time it is
/// wanted, so the count is a comfort rather than a promise.
const KEPT: usize = 1000;

#[derive(Default)]
pub struct Thumbnails {
	cache: HashMap<PathBuf, Option<Arc<RenderImage>>>,
	previews: HashMap<PathBuf, Option<Preview>>,
	/// Where a picture made from a file is kept between runs, if the platform gave us a place.
	/// Only pictures of files go here: an icon is 64 KB and a quarter of a millisecond to draw,
	/// while a picture is a whole file decoded, and it is the one worth not doing twice.
	kept: Option<PathBuf>,
	/// What is left of this frame's allowance, and whether the frame ran out. Running out is
	/// what asks for another frame: the rest of the pictures are waiting in it.
	budget: u32,
	starved: bool,
}

impl Thumbnails {
	/// Pictures made from files are kept in this folder between runs. Without one -- a platform
	/// that gave no directories, or a test -- everything is made afresh each run, which is the
	/// behaviour this had before the folder existed.
	pub fn keeping_pictures_in(folder: Option<PathBuf>) -> Thumbnails {
		Thumbnails { kept: folder, ..Thumbnails::default() }
	}

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

	/// What to put on a card for this file: the file itself where it can be shown, the first
	/// lines where it is text, and the system's icon otherwise. Under the same allowance as
	/// `of`, and for the same reason -- reading and decoding is work, and the list draws every
	/// row it has.
	pub fn preview(&mut self, path: &Path) -> Option<Preview> {
		if let Some(known) = self.previews.get(path) {
			return known.clone();
		}
		if self.budget == 0 {
			self.starved = true;
			return None;
		}
		self.budget -= 1;
		let made = match self.remembered(path) {
			Some(picture) => Some(Preview::Picture(picture)),
			None => match read_preview(path) {
				Some(Made::Picture(rgba)) => {
					self.keep(path, &rgba);
					drawable(rgba).map(Preview::Picture)
				}
				Some(Made::Lines(lines)) => Some(Preview::Lines(lines)),
				None => read(path).map(|image| Preview::Icon(Arc::new(image))),
			},
		};
		self.previews.insert(path.to_path_buf(), made.clone());
		made
	}

	/// The picture kept for this file, if one was made before and the file has not changed since.
	/// Changed is decided by the times: a picture older than the file it is of is a picture of
	/// something else, and is made again. A file touched without being edited costs one remake,
	/// which is the cheap way to be wrong.
	fn remembered(&self, path: &Path) -> Option<Arc<RenderImage>> {
		let file = self.kept_at(path)?;
		let made = std::fs::metadata(&file).ok()?.modified().ok()?;
		let changed = std::fs::metadata(path).ok()?.modified().ok()?;
		(made >= changed).then_some(())?;
		drawable(image::open(&file).ok()?.into_rgba8())
	}

	/// Writes the picture beside the others, as a PNG. A folder that cannot be written to is not
	/// an error worth a word on screen: the picture was made and is about to be drawn, and the
	/// only cost is making it again next run.
	fn keep(&self, path: &Path, rgba: &image::RgbaImage) {
		let Some(file) = self.kept_at(path) else { return };
		if std::fs::create_dir_all(file.parent().unwrap_or(&file)).is_ok() {
			let _ = rgba.save(&file);
		}
	}

	/// What this file's picture is called: a hash of the path, so a name of any length or shape
	/// becomes one a file system will take, and the same file finds the same picture next run.
	fn kept_at(&self, path: &Path) -> Option<PathBuf> {
		use sha2::Digest;
		let digest = sha2::Sha256::digest(path.as_os_str().as_encoded_bytes());
		let name: String = digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect();
		Some(self.kept.as_ref()?.join(format!("{name}.png")))
	}

	/// Forgets a file's picture, for a file that has changed on disk.
	pub fn forget(&mut self, path: &Path) {
		self.cache.remove(path);
		self.previews.remove(path);
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
	use objc2::AllocAnyThread;
	use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSWorkspace};
	use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

	// The window server answers this, and it answers on the main thread; every caller is drawing,
	// so every caller is on it.
	let path = NSString::from_str(path.to_str()?);
	let icon = NSWorkspace::sharedWorkspace().iconForFile(&path);
	// An NSImage is a set of representations rather than pixels, and the way to pixels is to draw
	// it into a bitmap of the size wanted. The obvious shortcut -- ask the icon for its TIFF and
	// decode that -- is one call rather than four, and it costs 35 MB and 12 ms where this costs
	// 64 KB and a quarter of a millisecond: `TIFFRepresentation` has the system encode all
	// thirty-two representations, 16 square to 1024, into one uncompressed file, which is then
	// decoded again and scaled down to this. A list of two hundred rows did that two hundred
	// times and spent gigabytes on it. See spec/ui.md.
	let side = SIZE as isize;
	let rep = unsafe {
		NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
			NSBitmapImageRep::alloc(),
			std::ptr::null_mut(),
			side,
			side,
			8,
			4,
			true,
			false,
			NSDeviceRGBColorSpace,
			side * 4,
			32,
		)?
	};
	let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
	NSGraphicsContext::saveGraphicsState_class();
	NSGraphicsContext::setCurrentContext(Some(&context));
	icon.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIZE as f64, SIZE as f64)));
	NSGraphicsContext::restoreGraphicsState_class();
	let drawn = rep.bitmapData();
	if drawn.is_null() {
		return None;
	}
	// The rep holds the bytes; they are copied out because it is about to be dropped and gpui
	// wants a Vec of its own anyway.
	let mut bgra = unsafe { std::slice::from_raw_parts(drawn, SIZE * SIZE * 4) }.to_vec();
	// AppKit draws RGBA and the renderer wants BGRA; the two differ by a swap of the first and
	// third byte of every pixel. Getting it backwards shows as blue people.
	for pixel in bgra.as_chunks_mut::<4>().0 {
		pixel.swap(0, 2);
	}
	render_image(SIZE as u32, SIZE as u32, bgra)
}

/// What a file turned out to hold, before it is anything gpui can draw. A picture stays straight
/// RGBA so that the same bytes can be written to the folder as a PNG and turned into an image to
/// draw, rather than drawn and then unpicked back into a picture.
enum Made {
	Picture(image::RgbaImage),
	Lines(Vec<String>),
}

/// A picture of the file, or the first lines of it, or nothing. Extension-led rather than
/// content-led: opening every file in a folder to find out what it is would be the very thing
/// the allowance exists to prevent, and a file named `.png` that is not one simply fails to
/// decode and falls back like everything else.
fn read_preview(path: &Path) -> Option<Made> {
	let extension = path.extension()?.to_str()?.to_ascii_lowercase();
	let size = std::fs::metadata(path).ok()?.len();
	if size > BIGGEST {
		return None;
	}
	const PICTURES: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
	if PICTURES.contains(&extension.as_str()) {
		return picture(path);
	}
	const TEXT: [&str; 26] = [
		"txt", "text", "md", "markdown", "rst", "log", "nfo", "json", "toml", "yaml", "yml", "xml",
		"csv", "tsv", "rs", "py", "js", "ts", "go", "c", "h", "sh", "sql", "html", "css", "ini",
	];
	if TEXT.contains(&extension.as_str()) {
		return lines(path);
	}
	None
}

/// The file scaled to fit a card, keeping its shape: a picture squashed to a square is a picture
/// somebody has to look at twice to recognise.
fn picture(path: &Path) -> Option<Made> {
	let decoded = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
	let scaled = decoded.resize(CARD, CARD, image::imageops::FilterType::Triangle).into_rgba8();
	Some(Made::Picture(scaled))
}

/// A made picture as gpui takes it. The decoder gives RGBA and the renderer wants BGRA, which is
/// a swap of the first and third byte of every pixel; getting it backwards shows as blue people.
fn drawable(rgba: image::RgbaImage) -> Option<Arc<RenderImage>> {
	let (width, height) = (rgba.width(), rgba.height());
	let mut bgra = rgba.into_raw();
	for pixel in bgra.as_chunks_mut::<4>().0 {
		pixel.swap(0, 2);
	}
	Some(Arc::new(render_image(width, height, bgra)?))
}

/// Deletes all but the newest `KEPT` pictures. Run once at launch, off the window's thread: a
/// folder is read and some files are removed, and nothing on screen waits for either.
pub fn trim(folder: &Path) {
	let Ok(entries) = std::fs::read_dir(folder) else { return };
	let mut kept: Vec<(std::time::SystemTime, PathBuf)> = entries
		.flatten()
		.filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
		.collect();
	if kept.len() <= KEPT {
		return;
	}
	kept.sort_by_key(|(made, _)| std::cmp::Reverse(*made));
	for (_, file) in kept.drain(KEPT..) {
		let _ = std::fs::remove_file(file);
	}
}

/// The first lines of a text file, as they are. Read as bytes and lossily converted: a file that
/// is not UTF-8 still has readable words in it, and a card that showed nothing because of one
/// stray byte would be a card that lied about the file.
fn lines(path: &Path) -> Option<Made> {
	use std::io::Read;
	let mut head = vec![0; LINES * COLUMNS * 4];
	let mut file = std::fs::File::open(path).ok()?;
	let read = file.read(&mut head).ok()?;
	head.truncate(read);
	let text = String::from_utf8_lossy(&head);
	let lines: Vec<String> = text
		.lines()
		.take(LINES)
		.map(|line| line.chars().take(COLUMNS).collect::<String>())
		.collect();
	(!lines.iter().all(|line| line.trim().is_empty())).then_some(Made::Lines(lines))
}

/// Windows keeps one too, and asking for it is `SHGetFileInfo`; until that is written the
/// category's glyph stands in, which is what it did for every file before any of this.
#[cfg(not(target_os = "macos"))]
fn read(_path: &Path) -> Option<RenderImage> {
	None
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

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

	/// A picture made from a file is written beside the others and read back next run rather than
	/// made again -- and a file that has changed since is made again, which is what the times are
	/// compared for.
	#[test]
	fn a_picture_made_from_a_file_is_kept_and_a_changed_file_is_made_again() {
		let dir = crate::testing::scratch("thumbnails");
		let folder = dir.join("kept");
		let file = dir.join("picture.png");
		image::RgbaImage::from_pixel(64, 64, image::Rgba([9, 9, 9, 255])).save(&file).unwrap();

		let mut thumbnails = Thumbnails::keeping_pictures_in(Some(folder.clone()));
		thumbnails.begin_frame();
		let made = thumbnails.preview(&file).expect("a picture is made from a picture");
		assert!(matches!(made, Preview::Picture(_)));
		let kept = thumbnails.kept_at(&file).expect("a folder was given, so there is a name");
		assert!(kept.exists(), "and what was made is written to it");

        // A different picture under the same name: whatever comes back next is what was read.
		image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255])).save(&kept).unwrap();
		let mut next_run = Thumbnails::keeping_pictures_in(Some(folder));
		next_run.begin_frame();
		match next_run.preview(&file).expect("the kept picture answers") {
			Preview::Picture(picture) => assert_eq!(picture.size(0).width.0, 8, "read, not made"),
			_ => panic!("a picture was kept, so a picture comes back"),
		}

		// The file changes, so the picture of it is older than the file and is made again. Its
		// time is set rather than waited for: a second of sleep in a test is a second every run.
		image::RgbaImage::from_pixel(64, 64, image::Rgba([7, 7, 7, 255])).save(&file).unwrap();
		let touched = std::fs::File::options().write(true).open(&file).unwrap();
		touched.set_modified(std::time::SystemTime::now() + Duration::from_secs(10)).unwrap();
		let mut after = Thumbnails::keeping_pictures_in(Some(kept.parent().unwrap().to_path_buf()));
		after.begin_frame();
		match after.preview(&file).expect("the file is still a picture") {
			// A card holds 256 square, so the picture that comes from the file is that; the one
			// that was kept was 8, which is how the two are told apart.
			Preview::Picture(picture) => assert_eq!(picture.size(0).width.0, 256, "made again"),
			_ => panic!("a picture was made, so a picture comes back"),
		}
	}
}
