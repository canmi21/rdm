//! Search over the download folder: every file and folder in it, and every entry inside the archives
//! it holds, gathered once into a catalog when the search opens and matched in memory on every key.
//! The archives are read through the index -- what it already knows is taken as it is, and what it
//! does not is listed here and handed back for it to keep. See spec/ui.md, "Search".

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::index::{self, Indexed};

/// How deep the folder is walked, and how many of its files are taken: a download folder is not a
/// disk, and a repository checked out inside one should not hold the search open.
const DEEPEST: usize = 8;
const MOST_FILES: usize = 50_000;

/// One thing a search can find: a file or folder on disk, or an entry inside an archive on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
	/// What it is called: the file's name, or the last part of the entry's path.
	pub name: String,
	/// The file on disk; for an entry, the archive holding it.
	pub path: PathBuf,
	/// For an entry, its path inside the archive.
	pub inside: Option<String>,
	pub size: u64,
	/// When the file on disk last changed, in seconds; an entry takes its archive's.
	pub modified: i64,
	pub dir: bool,
	/// The name and the entry's path lowercased, which is what a query is matched against.
	folded: String,
}

impl Item {
	fn new(
		name: String,
		path: PathBuf,
		inside: Option<String>,
		size: u64,
		modified: i64,
		dir: bool,
	) -> Item {
		let folded = match &inside {
			Some(inside) => inside.to_lowercase(),
			None => name.to_lowercase(),
		};
		Item { name, path, inside, size, modified, dir, folded }
	}
}

/// Everything gathered, and the archives read along the way that the index did not know, for it
/// to keep.
#[derive(Debug, Default)]
pub struct Catalog {
	pub items: Vec<Item>,
	pub archives: usize,
	pub learned: Vec<(String, Indexed)>,
}

/// The folder walked and its archives opened. `known` is the index as it stands, by path.
pub fn gather(folder: &Path, known: &HashMap<String, Indexed>) -> Catalog {
	let mut catalog = Catalog::default();
	walk(folder, 0, known, &mut catalog);
	catalog
}

fn walk(directory: &Path, depth: usize, known: &HashMap<String, Indexed>, catalog: &mut Catalog) {
	let Ok(entries) = std::fs::read_dir(directory) else { return };
	for entry in entries.flatten() {
		if catalog.items.len() >= MOST_FILES {
			return;
		}
		let name = entry.file_name().to_string_lossy().into_owned();
		// Hidden files, and what a download in progress keeps beside itself, are nobody's search.
		if name.starts_with('.') || name.ends_with(".downloading") || name.ends_with(".rdm") {
			continue;
		}
		let path = entry.path();
		let Ok(metadata) = entry.metadata() else { continue };
		let modified = metadata
			.modified()
			.ok()
			.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
			.map_or(0, |d| d.as_secs() as i64);
		if metadata.is_dir() {
			catalog.items.push(Item::new(name, path.clone(), None, 0, modified, true));
			if depth < DEEPEST && !path.extension().is_some_and(|e| e == "app") {
				walk(&path, depth + 1, known, catalog);
			}
			continue;
		}
		catalog.items.push(Item::new(
			name.clone(),
			path.clone(),
			None,
			metadata.len(),
			modified,
			false,
		));
		if index::kind_of(&name).is_some() {
			entries_of(&path, (modified, metadata.len()), known, catalog);
		}
	}
}

/// An archive's entries into the catalog, from the index where it knows the file as it now is and
/// read here where it does not.
fn entries_of(
	path: &Path,
	stamp: (i64, u64),
	known: &HashMap<String, Indexed>,
	catalog: &mut Catalog,
) {
	let key = path.to_string_lossy().into_owned();
	let indexed = match known.get(&key).filter(|i| (i.modified, i.size) == stamp) {
		Some(indexed) => indexed.clone(),
		None => {
			let indexed = match index::list(path) {
				Ok(entries) => Indexed { modified: stamp.0, size: stamp.1, entries, error: None },
				Err(error) => Indexed {
					modified: stamp.0,
					size: stamp.1,
					entries: Vec::new(),
					error: Some(format!("{error:#}")),
				},
			};
			catalog.learned.push((key, indexed.clone()));
			indexed
		}
	};
	if indexed.entries.is_empty() {
		return;
	}
	catalog.archives += 1;
	for entry in &indexed.entries {
		let inside = entry.name.trim_end_matches('/').to_owned();
		let Some(name) = inside.rsplit('/').next().filter(|n| !n.is_empty()).map(str::to_owned) else {
			continue;
		};
		catalog.items.push(Item::new(
			name,
			path.to_path_buf(),
			Some(inside),
			entry.size,
			stamp.0,
			entry.dir,
		));
	}
}

/// Where a result lies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
	#[default]
	Anywhere,
	Files,
	Archives,
}

/// How recently it changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Age {
	#[default]
	Any,
	Day,
	Week,
	Month,
	Year,
}

impl Age {
	fn seconds(self) -> Option<i64> {
		match self {
			Age::Any => None,
			Age::Day => Some(86_400),
			Age::Week => Some(7 * 86_400),
			Age::Month => Some(31 * 86_400),
			Age::Year => Some(366 * 86_400),
		}
	}
}

/// How big it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
	#[default]
	Any,
	Small,
	Medium,
	Large,
	Huge,
}

impl Size {
	fn holds(self, bytes: u64) -> bool {
		const MB: u64 = 1_000_000;
		match self {
			Size::Any => true,
			Size::Small => bytes < MB,
			Size::Medium => (MB..100 * MB).contains(&bytes),
			Size::Large => (100 * MB..1000 * MB).contains(&bytes),
			Size::Huge => bytes >= 1000 * MB,
		}
	}
}

/// What narrows a search besides its words; the category is the caller's to judge, since the
/// categories are the user's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filters {
	pub place: Place,
	pub age: Age,
	pub size: Size,
}

/// The items every word of `query` is in, passing `filters`, best first: a name that starts with the
/// query, then one that holds it whole, then the rest; a file before an entry; a short name before a
/// long one. By index into `items`. An empty query matches everything.
pub fn find(items: &[Item], query: &str, filters: Filters, now: i64) -> Vec<usize> {
	let folded = query.trim().to_lowercase();
	let words: Vec<&str> = folded.split_whitespace().collect();
	let mut found: Vec<(u8, usize)> = items
		.iter()
		.enumerate()
		.filter(|(_, item)| match filters.place {
			Place::Anywhere => true,
			Place::Files => item.inside.is_none(),
			Place::Archives => item.inside.is_some(),
		})
		.filter(|(_, item)| filters.age.seconds().is_none_or(|within| now - item.modified <= within))
		.filter(|(_, item)| {
			item.dir && filters.size == Size::Any || !item.dir && filters.size.holds(item.size)
		})
		.filter(|(_, item)| words.iter().all(|word| item.folded.contains(word)))
		.map(|(at, item)| {
			let name = item.name.to_lowercase();
			// An empty query starts every name.
			let rank = if name.starts_with(&folded) {
				0
			} else if name.contains(&folded) {
				1
			} else {
				2
			};
			(rank * 2 + u8::from(item.inside.is_some()), at)
		})
		.collect();
	found
		.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| items[a.1].name.len().cmp(&items[b.1].name.len())));
	found.into_iter().map(|(_, at)| at).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	fn file(name: &str, size: u64, modified: i64) -> Item {
		Item::new(name.to_owned(), PathBuf::from(name), None, size, modified, false)
	}

	fn entry(archive: &str, inside: &str) -> Item {
		let name = inside.rsplit('/').next().unwrap().to_owned();
		Item::new(name, PathBuf::from(archive), Some(inside.to_owned()), 10, 0, false)
	}

	#[test]
	fn every_word_must_match_and_the_best_names_come_first() {
		let items = vec![
			file("my-report-final.pdf", 10, 0),
			file("report.pdf", 10, 0),
			entry("docs.zip", "2024/report.docx"),
			file("notes.txt", 10, 0),
		];
		let names =
			|found: Vec<usize>| found.into_iter().map(|at| items[at].name.clone()).collect::<Vec<_>>();
		assert_eq!(
			names(find(&items, "report", Filters::default(), 0)),
			["report.pdf", "report.docx", "my-report-final.pdf"]
		);
		assert_eq!(
			names(find(&items, "report pdf", Filters::default(), 0)),
			["report.pdf", "my-report-final.pdf"]
		);
		assert_eq!(
			names(find(&items, "2024", Filters::default(), 0)),
			["report.docx"],
			"an entry's path is searched"
		);
	}

	#[test]
	fn the_filters_narrow_by_place_age_and_size() {
		let day = 86_400;
		let items = vec![
			file("old.iso", 4_000_000_000, 0),
			file("new.txt", 100, 400 * day),
			entry("a.zip", "inner.txt"),
		];
		let only = |filters: Filters| find(&items, "", filters, 400 * day).len();
		assert_eq!(only(Filters { place: Place::Archives, ..Filters::default() }), 1);
		assert_eq!(only(Filters { place: Place::Files, ..Filters::default() }), 2);
		assert_eq!(only(Filters { age: Age::Week, ..Filters::default() }), 1);
		assert_eq!(only(Filters { size: Size::Huge, ..Filters::default() }), 1);
		assert_eq!(only(Filters { size: Size::Small, ..Filters::default() }), 2);
	}

	#[test]
	fn the_folder_is_walked_and_its_archives_opened() {
		let dir = crate::testing::scratch("search-gather");
		std::fs::create_dir_all(dir.join("sub")).unwrap();
		std::fs::write(dir.join("sub/deep.txt"), b"x").unwrap();
		std::fs::write(dir.join("partial.iso.downloading"), b"x").unwrap();
		{
			let mut writer = zip::ZipWriter::new(std::fs::File::create(dir.join("a.zip")).unwrap());
			writer.start_file("inside/hello.md", zip::write::SimpleFileOptions::default()).unwrap();
			std::io::Write::write_all(&mut writer, b"hi").unwrap();
			writer.finish().unwrap();
		}
		let catalog = gather(&dir, &HashMap::new());
		let mut names: Vec<&str> = catalog.items.iter().map(|i| i.name.as_str()).collect();
		names.sort_unstable();
		assert_eq!(names, ["a.zip", "deep.txt", "hello.md", "sub"]);
		assert_eq!(catalog.archives, 1);
		assert_eq!(catalog.learned.len(), 1, "the archive the index did not know is handed back");
	}
}
