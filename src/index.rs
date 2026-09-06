//! What an archive holds, read without unpacking it, for the categories to judge it by: a zip's
//! central directory, a 7z's header, a tar's headers skipped through by their sizes, and for a
//! small gzip tar the whole stream, since gzip has no directory to read. The rest -- rar, the
//! large stream-compressed ones, disk images -- is left alone: what cannot be read cheaply is
//! not read. Indexed in the background and kept in the store. See spec/state.md.

use std::io::Read;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// A gzip tar larger than this is not read: a stream has to be inflated to its end to name its
/// last file, and past this much the wait is not worth a category.
pub const STREAM_LIMIT: u64 = 64 * 1024 * 1024;

/// How many entries are kept per archive: enough to judge it, not the whole of a large one.
pub const ENTRY_LIMIT: usize = 4096;

/// One file or directory in an archive, by its path inside it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
	pub name: String,
	pub size: u64,
	pub dir: bool,
}

/// What was read of one archive, or why it could not be, with the file's stamp so a changed
/// file is read again and an unchanged one never is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Indexed {
	pub modified: i64,
	pub size: u64,
	pub entries: Vec<Entry>,
	pub error: Option<String>,
}

/// The kinds that can be listed cheaply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Zip,
	SevenZ,
	Tar,
	TarGz,
}

/// The kind a file name says it is, if it is one that can be listed. `jar`, `apk` and `ipa`
/// are zips and are listed as such, so an archive of a program is seen to hold one.
pub fn kind_of(name: &str) -> Option<Kind> {
	let lower = name.to_ascii_lowercase();
	if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
		return Some(Kind::TarGz);
	}
	match lower.rsplit_once('.').map(|(_, ext)| ext)? {
		"zip" | "zipx" | "jar" | "apk" | "ipa" | "xapk" | "aab" => Some(Kind::Zip),
		"7z" => Some(Kind::SevenZ),
		"tar" => Some(Kind::Tar),
		_ => None,
	}
}

/// The file's stamp, as the index keys on it: its modification time in seconds and its size.
pub fn stamp(path: &Path) -> Option<(i64, u64)> {
	let metadata = std::fs::metadata(path).ok()?;
	let modified = metadata
		.modified()
		.ok()
		.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
		.map_or(0, |d| d.as_secs() as i64);
	Some((modified, metadata.len()))
}

/// The archive's entries, by its kind, up to `ENTRY_LIMIT` of them.
pub fn list(path: &Path) -> Result<Vec<Entry>> {
	let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
	let kind = kind_of(name).context("not a kind that can be listed")?;
	let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
	match kind {
		Kind::Zip => list_zip(file),
		Kind::SevenZ => list_7z(path),
		Kind::Tar => list_tar_file(file),
		Kind::TarGz => {
			let size = file.metadata().map(|m| m.len()).unwrap_or(0);
			anyhow::ensure!(size <= STREAM_LIMIT, "a gzip tar over the stream limit is not read");
			list_tar_stream(flate2::read::GzDecoder::new(file))
		}
	}
}

fn list_zip(file: std::fs::File) -> Result<Vec<Entry>> {
	let mut archive = zip::ZipArchive::new(file).context("read the zip directory")?;
	let mut entries = Vec::new();
	for index in 0..archive.len().min(ENTRY_LIMIT) {
		let entry = archive.by_index_raw(index).with_context(|| format!("entry {index}"))?;
		entries.push(Entry { name: entry.name().to_owned(), size: entry.size(), dir: entry.is_dir() });
	}
	Ok(entries)
}

fn list_7z(path: &Path) -> Result<Vec<Entry>> {
	let archive = sevenz_rust2::Archive::open(path).context("read the 7z header")?;
	Ok(
		archive
			.files
			.iter()
			.take(ENTRY_LIMIT)
			.map(|entry| Entry {
				name: entry.name().to_owned(),
				size: entry.size(),
				dir: entry.is_directory(),
			})
			.collect(),
	)
}

/// A tar's headers, one every block: a file that can seek skips each entry's data, a stream
/// reads through it.
fn list_tar_file(file: std::fs::File) -> Result<Vec<Entry>> {
	let mut archive = tar::Archive::new(file);
	read_tar_entries(archive.entries_with_seek().context("read the tar")?)
}

fn list_tar_stream<R: Read>(reader: R) -> Result<Vec<Entry>> {
	let mut archive = tar::Archive::new(reader);
	read_tar_entries(archive.entries().context("read the tar")?)
}

fn read_tar_entries<R: Read>(iter: tar::Entries<'_, R>) -> Result<Vec<Entry>> {
	let mut entries = Vec::new();
	for entry in iter {
		let entry = entry.context("a tar header")?;
		let header = entry.header();
		entries.push(Entry {
			name: entry.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
			size: header.size().unwrap_or(0),
			dir: header.entry_type().is_dir(),
		});
		if entries.len() >= ENTRY_LIMIT {
			break;
		}
	}
	Ok(entries)
}

/// The names at the archive's top: the first path component of every entry, each once, in
/// order of appearance. A directory's children are folded into it, so `App.app/Contents/...`
/// is `App.app`, which is what the categories should judge. A lone wrapping folder is looked
/// through: `project-1.0/src/main.rs` and `project-1.0/README` are judged by what is under
/// `project-1.0`, since that folder is the archive's own name and says nothing.
pub fn top_level(entries: &[Entry]) -> Vec<String> {
	let names = |strip: usize| -> Vec<String> {
		let mut seen = Vec::new();
		for entry in entries {
			// A leading `./`, and an empty part from a doubled slash, are not names.
			let mut parts = entry
				.name
				.trim_end_matches('/')
				.split('/')
				.filter(|p| !p.is_empty() && *p != ".")
				.skip(strip);
			let Some(first) = parts.next() else { continue };
			if first == ".." {
				continue;
			}
			let name = first.to_owned();
			if !seen.contains(&name) {
				seen.push(name);
			}
		}
		seen
	};
	let top = names(0);
	if top.len() == 1
		&& entries.iter().any(|e| e.name.trim_end_matches('/').contains('/'))
		&& !looks_like_a_bundle(&top[0])
	{
		let inner = names(1);
		if !inner.is_empty() {
			return inner;
		}
	}
	top
}

/// A folder that is itself the thing -- a macOS bundle -- is not looked through.
fn looks_like_a_bundle(name: &str) -> bool {
	let lower = name.to_ascii_lowercase();
	lower.ends_with(".app") || lower.ends_with(".framework") || lower.ends_with(".bundle")
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	fn entries(names: &[&str]) -> Vec<Entry> {
		names.iter().map(|n| Entry { name: (*n).to_owned(), size: 1, dir: n.ends_with('/') }).collect()
	}

	#[test]
	fn the_kind_is_read_off_the_name() {
		assert_eq!(kind_of("a.ZIP"), Some(Kind::Zip));
		assert_eq!(kind_of("a.tar.gz"), Some(Kind::TarGz));
		assert_eq!(kind_of("a.tgz"), Some(Kind::TarGz));
		assert_eq!(kind_of("a.tar"), Some(Kind::Tar));
		assert_eq!(kind_of("a.7z"), Some(Kind::SevenZ));
		assert_eq!(kind_of("app.apk"), Some(Kind::Zip));
		assert_eq!(kind_of("a.rar"), None, "rar needs a library this build does not carry");
		assert_eq!(kind_of("a.tar.xz"), None, "xz is a stream with no cheap way through");
		assert_eq!(kind_of("a.dmg"), None);
	}

	#[test]
	fn the_top_level_folds_children_and_looks_through_a_wrapping_folder() {
		assert_eq!(
			top_level(&entries(&["Foo.app/", "Foo.app/Contents/Info.plist", "README"])),
			["Foo.app", "README"]
		);
		assert_eq!(
			top_level(&entries(&["proj-1.0/", "proj-1.0/src/main.rs", "proj-1.0/Cargo.toml"])),
			["src", "Cargo.toml"]
		);
		assert_eq!(top_level(&entries(&["Foo.app/", "Foo.app/Contents/MacOS/Foo"])), ["Foo.app"]);
		assert_eq!(top_level(&entries(&["setup.exe"])), ["setup.exe"]);
		assert_eq!(top_level(&entries(&["./a.txt", "./b.txt"])), ["a.txt", "b.txt"]);
	}

	#[test]
	fn a_zip_a_tar_and_a_gzip_tar_are_listed_without_being_unpacked() {
		let dir = crate::testing::scratch("index");
		let zip_path = dir.join("bundle.zip");
		{
			let file = std::fs::File::create(&zip_path).unwrap();
			let mut zip = zip::ZipWriter::new(file);
			let options = zip::write::SimpleFileOptions::default();
			zip.add_directory("Foo.app/", options).unwrap();
			zip.start_file("Foo.app/Contents/Info.plist", options).unwrap();
			zip.write_all(b"<plist/>").unwrap();
			zip.start_file("notes.txt", options).unwrap();
			zip.write_all(b"hello").unwrap();
			zip.finish().unwrap();
		}
		let listed = list(&zip_path).unwrap();
		let names: Vec<&str> = listed.iter().map(|e| e.name.as_str()).collect();
		assert_eq!(names, ["Foo.app/", "Foo.app/Contents/Info.plist", "notes.txt"]);
		assert!(listed[0].dir && !listed[2].dir && listed[2].size == 5);
		assert_eq!(top_level(&listed), ["Foo.app", "notes.txt"]);

		let tar_path = dir.join("src.tar");
		let tgz_path = dir.join("src.tar.gz");
		let mut header = tar::Header::new_gnu();
		header.set_size(3);
		header.set_mode(0o644);
		header.set_cksum();
		{
			let mut builder = tar::Builder::new(std::fs::File::create(&tar_path).unwrap());
			builder.append_data(&mut header.clone(), "proj/main.rs", &b"fn "[..]).unwrap();
			builder.append_data(&mut header.clone(), "proj/lib.rs", &b"mod"[..]).unwrap();
			builder.finish().unwrap();
		}
		{
			let gz = flate2::write::GzEncoder::new(
				std::fs::File::create(&tgz_path).unwrap(),
				flate2::Compression::fast(),
			);
			let mut builder = tar::Builder::new(gz);
			builder.append_data(&mut header.clone(), "proj/main.rs", &b"fn "[..]).unwrap();
			builder.into_inner().unwrap().finish().unwrap();
		}
		let tar_names: Vec<String> = list(&tar_path).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(tar_names, ["proj/main.rs", "proj/lib.rs"]);
		assert_eq!(top_level(&list(&tar_path).unwrap()), ["main.rs", "lib.rs"]);
		let tgz_names: Vec<String> = list(&tgz_path).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(tgz_names, ["proj/main.rs"]);
		assert!(list(&dir.join("none.rar")).is_err());
	}
}
