//! The control file: what is written beside a partial download so it can be picked up again
//! by a later run -- the address, what the server said about the file, and the plan with every
//! segment's progress. The same idea as aria2's `.aria2` file, and JSON for the same reason
//! state.json is: small, rewritten whole, readable when something goes wrong. See spec/engine.md.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::error::{Error, Result};
use crate::engine::segments::Plan;

/// Bumped only when a file written before can no longer be read as it is; see spec/state.md
/// for the rule.
pub const VERSION: u64 = 1;

/// The suffix of a file still being written, and of the control file beside it. The first says
/// what it is to anyone who sees it in a folder; the second names the application.
pub const PART: &str = "downloading";
pub const CONTROL: &str = "rdm";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Control {
	pub version: u64,
	pub url: String,
	/// The whole file's size as the server declared it; None when it never said.
	pub size: Option<u64>,
	/// The ETag, or failing that Last-Modified, that every resumed request carries as If-Range.
	pub validator: Option<String>,
	pub plan: Plan,
}

impl Control {
	pub fn new(url: &str, size: Option<u64>, validator: Option<&str>, plan: Plan) -> Control {
		Control {
			version: VERSION,
			url: url.to_owned(),
			size,
			validator: validator.map(str::to_owned),
			plan,
		}
	}
}

/// `movie.mkv` -> `movie.mkv.downloading`: where the bytes go until the download is complete.
pub fn part_path(target: &Path) -> PathBuf {
	with_suffix(target, PART)
}

/// `movie.mkv` -> `movie.mkv.rdm`: where the plan lives meanwhile.
pub fn control_path(target: &Path) -> PathBuf {
	with_suffix(target, CONTROL)
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
	let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
	name.push(".");
	name.push(suffix);
	path.with_file_name(name)
}

/// Written whole to a sibling and renamed over the old file, so a crash mid-write leaves the
/// previous plan rather than half of the new one; a plan that is a step behind costs a few
/// bytes re-downloaded, a broken one costs the download.
pub fn save(target: &Path, control: &Control) -> Result<()> {
	let path = control_path(target);
	let disk = |source| Error::Disk { path: path.clone(), source };
	let temporary = with_suffix(&path, "tmp");
	let text = serde_json::to_string_pretty(control).expect("a control file serialises");
	std::fs::write(&temporary, text).map_err(disk)?;
	std::fs::rename(&temporary, &path).map_err(disk)?;
	Ok(())
}

/// The control file beside `target`, if there is one. A file this build cannot read -- newer,
/// or damaged -- is an error rather than a fresh start, so a download is not silently begun
/// again over a partial file somebody meant to keep.
pub fn load(target: &Path) -> Result<Option<Control>> {
	let path = control_path(target);
	let text = match std::fs::read_to_string(&path) {
		Ok(text) => text,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
		Err(source) => return Err(Error::Disk { path, source }),
	};
	let control: Control = serde_json::from_str(&text).map_err(|_| Error::Control)?;
	if control.version != VERSION {
		return Err(Error::Control);
	}
	Ok(Some(control))
}

pub fn remove(target: &Path) {
	let _ = std::fs::remove_file(control_path(target));
}

/// A download found on disk by its files alone: where it would land, its plan, and when the
/// plan was last written.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
	pub target: PathBuf,
	pub control: Control,
	pub modified: Option<std::time::SystemTime>,
}

/// Every download that left its two files in `directory` and can be continued: a control file
/// this build reads, a plan that holds together, and the partial file beside it at least as
/// long as the plan says was written. Anything else is left exactly as it is -- a plan from a
/// newer build, a damaged one, a plan whose partial file is gone -- because a file the user
/// meant to keep is not this code's to delete, and one it cannot read is one it cannot judge.
pub fn find(directory: &Path) -> Vec<Found> {
	let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
	// Both of a download's files name the same target, so the targets are gathered first.
	let targets: std::collections::BTreeSet<PathBuf> =
		entries.flatten().filter_map(|entry| target_of(&entry.path())).collect();
	targets.iter().filter_map(|target| find_one(target)).collect()
}

/// The download whose files these would be: `movie.mkv.rdm` and `movie.mkv.downloading` both
/// answer `movie.mkv`; any other name answers nothing.
pub fn target_of(path: &Path) -> Option<PathBuf> {
	let name = path.file_name()?.to_str()?;
	let stem =
		name.strip_suffix(&format!(".{CONTROL}")).or_else(|| name.strip_suffix(&format!(".{PART}")))?;
	(!stem.is_empty()).then(|| path.with_file_name(stem))
}

/// One download by where it would land, if its two files are there and can be continued; the
/// rules `find` applies, for one path, so a change to a single file costs one look and not a
/// read of the folder.
pub fn find_one(target: &Path) -> Option<Found> {
	let control = load(target).ok()??;
	if !control.plan.is_consistent() {
		return None;
	}
	let written =
		control.plan.segments.iter().map(|s| s.position()).max().unwrap_or(0) - control.plan.span.start;
	let part = std::fs::metadata(part_path(target)).ok()?;
	if part.len() < written {
		return None;
	}
	let modified = std::fs::metadata(control_path(target)).ok().and_then(|m| m.modified().ok());
	Some(Found { target: target.to_path_buf(), control, modified })
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::engine::segments::Span;

	fn scratch(name: &str) -> PathBuf {
		crate::testing::scratch(name).join("movie.mkv")
	}

	#[test]
	fn the_paths_hang_off_the_target() {
		let target = Path::new("/downloads/movie.mkv");
		assert_eq!(part_path(target), Path::new("/downloads/movie.mkv.downloading"));
		assert_eq!(control_path(target), Path::new("/downloads/movie.mkv.rdm"));
	}

	#[test]
	fn a_plan_is_saved_read_back_and_removed() {
		let target = scratch("roundtrip");
		assert_eq!(load(&target).unwrap(), None, "nothing there yet");
		let mut plan = Plan::split(Span::new(0, 100), 2, 1);
		plan.segments[0].done = 40;
		let control = Control::new("https://h/movie.mkv", Some(100), Some("\"e\""), plan);
		save(&target, &control).unwrap();
		assert_eq!(load(&target).unwrap(), Some(control));
		assert!(!with_suffix(&control_path(&target), "tmp").exists(), "the temporary is renamed away");
		remove(&target);
		assert_eq!(load(&target).unwrap(), None);
	}

	#[test]
	fn plans_left_in_a_folder_are_found_when_they_can_be_continued() {
		let dir = crate::testing::scratch("control-find");
		let plan = |done: u64| {
			let mut plan = Plan::split(Span::new(0, 100), 2, 1);
			plan.segments[0].done = done;
			plan
		};
		// Whole: a plan and a partial file long enough for it.
		save(&dir.join("good.bin"), &Control::new("https://h/good.bin", Some(100), None, plan(40)))
			.unwrap();
		std::fs::write(part_path(&dir.join("good.bin")), vec![0; 100]).unwrap();
		// The partial file is shorter than the plan says was written.
		save(&dir.join("short.bin"), &Control::new("https://h/short.bin", Some(100), None, plan(40)))
			.unwrap();
		std::fs::write(part_path(&dir.join("short.bin")), vec![0; 10]).unwrap();
		// No partial file at all.
		save(&dir.join("alone.bin"), &Control::new("https://h/alone.bin", Some(100), None, plan(0)))
			.unwrap();
		// Not a control file this build reads.
		std::fs::write(control_path(&dir.join("newer.bin")), "{ \"version\": 99 }").unwrap();
		std::fs::write(part_path(&dir.join("newer.bin")), vec![0; 100]).unwrap();
		// A plan that does not hold together.
		let mut broken = plan(0);
		broken.segments[0].done = 999;
		save(&dir.join("broken.bin"), &Control::new("https://h/broken.bin", Some(100), None, broken))
			.unwrap();
		std::fs::write(part_path(&dir.join("broken.bin")), vec![0; 100]).unwrap();
		// A file with the suffix that is not ours at all.
		std::fs::write(dir.join("note.rdm"), "hello").unwrap();

		let found = find(&dir);
		let names: Vec<_> =
			found.iter().map(|f| f.target.file_name().unwrap().to_str().unwrap()).collect();
		assert_eq!(names, ["good.bin"]);
		assert_eq!(found[0].control.url, "https://h/good.bin");
		assert_eq!(found[0].control.plan.done(), 40);
		assert!(found[0].modified.is_some());
		// Nothing was touched, least of all the ones that were refused.
		assert!(
			control_path(&dir.join("newer.bin")).exists()
				&& control_path(&dir.join("broken.bin")).exists()
		);
		assert!(control_path(&dir.join("alone.bin")).exists() && dir.join("note.rdm").exists());
		// One at a time, from either of its files.
		assert_eq!(target_of(Path::new("/d/movie.mkv.rdm")), Some(PathBuf::from("/d/movie.mkv")));
		assert_eq!(
			target_of(Path::new("/d/movie.mkv.downloading")),
			Some(PathBuf::from("/d/movie.mkv"))
		);
		assert_eq!(target_of(Path::new("/d/movie.mkv")), None);
		assert_eq!(target_of(Path::new("/d/.rdm")), None);
		assert!(find_one(&dir.join("good.bin")).is_some());
		assert!(find_one(&dir.join("short.bin")).is_none());
	}

	#[test]
	fn a_file_from_another_build_or_a_damaged_one_is_refused_not_ignored() {
		let target = scratch("refused");
		std::fs::write(control_path(&target), "{ \"version\": 99 }").unwrap();
		assert!(matches!(load(&target), Err(Error::Control)));
		std::fs::write(control_path(&target), "not json").unwrap();
		assert!(matches!(load(&target), Err(Error::Control)));
		remove(&target);
	}
}
