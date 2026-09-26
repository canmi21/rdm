//! A document's first page as the system draws it, on macOS. A PDF is drawn at once: an NSImage of
//! a PDF is its first page, drawn by CoreGraphics. Word, Excel, PowerPoint, Pages, Numbers and
//! Keynote are asked of QuickLook, the service Finder's own thumbnails come from, which answers
//! later on a queue of its own; the answers wait here until the window's tick collects them.
//! Elsewhere there is no such service here yet, and these files keep what the rest of the preview
//! makes of them. See spec/ui.md, "A card shows the file".

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::CARD;

/// The kinds QuickLook is asked for a first page of.
pub const ASKED: [&str; 18] = [
	"doc", "docx", "docm", "dot", "dotx", "rtf", "xls", "xlsx", "xlsm", "xlsb", "ppt", "pptx",
	"pptm", "pps", "ppsx", "pages", "numbers", "key",
];

/// What QuickLook has answered and nobody has collected yet: the file, and its first page or
/// nothing when it had none to give.
pub type Answers = Arc<Mutex<Vec<(PathBuf, Option<image::RgbaImage>)>>>;

/// Whether a file of this extension is asked of the system rather than read here.
pub fn asked(extension: &str) -> bool {
	cfg!(target_os = "macos") && ASKED.contains(&extension)
}

/// Draws an NSImage onto white paper, scaled to fit a card and keeping its shape, as straight
/// RGBA. Paper because a page is transparent where nothing is printed, and a card is dark.
#[cfg(target_os = "macos")]
fn on_paper(page: &objc2_app_kit::NSImage, paper: bool) -> Option<image::RgbaImage> {
	use objc2::AllocAnyThread;
	use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext};
	use objc2_foundation::{NSPoint, NSRect, NSSize};

	let size = page.size();
	if size.width <= 0.0 || size.height <= 0.0 {
		return None;
	}
	let scale = (f64::from(CARD) / size.width).min(f64::from(CARD) / size.height);
	let (width, height) =
		((size.width * scale).round().max(1.0), (size.height * scale).round().max(1.0));
	let (w, h) = (width as isize, height as isize);
	let rep = unsafe {
		NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
			NSBitmapImageRep::alloc(),
			std::ptr::null_mut(),
			w,
			h,
			8,
			4,
			true,
			false,
			NSDeviceRGBColorSpace,
			w * 4,
			32,
		)?
	};
	let bytes = rep.bitmapData();
	if bytes.is_null() {
		return None;
	}
	let length = (w * h * 4) as usize;
	unsafe { std::ptr::write_bytes(bytes, if paper { 255 } else { 0 }, length) };
	let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&rep)?;
	NSGraphicsContext::saveGraphicsState_class();
	NSGraphicsContext::setCurrentContext(Some(&context));
	page.drawInRect(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)));
	NSGraphicsContext::restoreGraphicsState_class();
	let rgba = unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec();
	image::RgbaImage::from_raw(w as u32, h as u32, rgba)
}

/// A PDF's first page, white under it as paper is, scaled to fit a card.
#[cfg(target_os = "macos")]
pub fn pdf(path: &Path) -> Option<image::RgbaImage> {
	use objc2::AllocAnyThread;
	use objc2_app_kit::NSImage;
	use objc2_foundation::NSString;
	let page =
		NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path.to_str()?))?;
	on_paper(&page, true)
}

#[cfg(not(target_os = "macos"))]
pub fn pdf(_path: &Path) -> Option<image::RgbaImage> {
	None
}

/// Asks QuickLook for the file's first page; the answer, a page or nothing, lands in `answers`.
/// Only a thumbnail is asked for, not the icon it falls back to: the icon is had more cheaply, and
/// an answer of nothing is what sends the file back to being read here.
#[cfg(target_os = "macos")]
pub fn ask(path: &Path, answers: &Answers) {
	use objc2::AllocAnyThread;
	use objc2_foundation::{NSError, NSString, NSURL};
	use objc2_quick_look_thumbnailing::{
		QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
		QLThumbnailGenerator, QLThumbnailRepresentation,
	};

	let Some(text) = path.to_str() else {
		answers.lock().unwrap_or_else(|held| held.into_inner()).push((path.to_path_buf(), None));
		return;
	};
	let url = NSURL::fileURLWithPath(&NSString::from_str(text));
	let side = f64::from(CARD);
	let request = unsafe {
		QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
			QLThumbnailGenerationRequest::alloc(),
			&url,
			objc2_core_foundation::CGSize::new(side, side),
			1.0,
			QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
		)
	};
	let (answers, file) = (answers.clone(), path.to_path_buf());
	let handler =
		block2::RcBlock::new(move |representation: *mut QLThumbnailRepresentation, _: *mut NSError| {
			// A thumbnail comes drawn on its own paper, so nothing goes under it.
			let page = unsafe { representation.as_ref() }.and_then(|r| {
				let image = unsafe { r.NSImage() };
				on_paper(&image, false)
			});
			answers.lock().unwrap_or_else(|held| held.into_inner()).push((file.clone(), page));
		});
	unsafe {
		QLThumbnailGenerator::sharedGenerator()
			.generateBestRepresentationForRequest_completionHandler(&request, &handler);
	}
}

#[cfg(not(target_os = "macos"))]
pub fn ask(path: &Path, answers: &Answers) {
	answers.lock().unwrap_or_else(|held| held.into_inner()).push((path.to_path_buf(), None));
}
