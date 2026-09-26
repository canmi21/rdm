//! What an archive holds, read without unpacking it, for the categories to judge it by and a card
//! to list: a zip's central directory, a 7z's header, a tar's headers skipped through by their
//! sizes, a compressed tar's stream as far as its first megabytes go, and the names RAR, ISO 9660
//! and CAB keep in their headers. A download not yet whole is read where it has arrived. A disk
//! image is left alone: its names sit inside a file system inside compressed blocks. Indexed in the
//! background and kept in the store. See spec/state.md.

use std::io::Read;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

mod available;
mod headers;

pub use available::Available;

/// How much of a compressed tar is read: a stream has to be inflated to name what is in it, and
/// past this much the names already read are what the card and the categories get.
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

/// The kinds that can be listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
	Zip,
	SevenZ,
	Tar,
	TarGz,
	TarXz,
	TarBz2,
	TarZst,
	TarLz,
	Rar,
	Iso,
	Cab,
}

/// The kind a file name says it is, if it is one that can be listed. `jar`, `apk` and `ipa`
/// are zips and are listed as such, so an archive of a program is seen to hold one.
pub fn kind_of(name: &str) -> Option<Kind> {
	let lower = name.to_ascii_lowercase();
	for (ends, kind) in [
		(&[".tar.gz", ".tgz"][..], Kind::TarGz),
		(&[".tar.xz", ".txz"], Kind::TarXz),
		(&[".tar.bz2", ".tbz2", ".tbz"], Kind::TarBz2),
		(&[".tar.zst", ".tzst"], Kind::TarZst),
		(&[".tar.lz", ".tlz"], Kind::TarLz),
	] {
		if ends.iter().any(|end| lower.ends_with(end)) {
			return Some(kind);
		}
	}
	match lower.rsplit_once('.').map(|(_, ext)| ext)? {
		"zip" | "zipx" | "jar" | "apk" | "ipa" | "xapk" | "aab" => Some(Kind::Zip),
		"7z" => Some(Kind::SevenZ),
		"tar" => Some(Kind::Tar),
		"rar" => Some(Kind::Rar),
		"iso" => Some(Kind::Iso),
		"cab" => Some(Kind::Cab),
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

/// A whole archive's entries, by the kind its name says, up to `ENTRY_LIMIT` of them.
#[cfg(test)]
pub fn list(path: &Path) -> Result<Vec<Entry>> {
	let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
	let kind = kind_of(name).context("not a kind that can be listed")?;
	let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
	list_available(Available::whole(file)?, kind)
}

/// An archive's entries as far as the file has arrived: a whole file, or a partial one with its
/// holes, read the same way.
pub fn list_available(mut file: Available, kind: Kind) -> Result<Vec<Entry>> {
	let stream = |file: Available| file.take(STREAM_LIMIT);
	match kind {
		// The directory at the end, which a download split across connections often has early;
		// failing that, the headers from the front.
		Kind::Zip => match list_zip(&mut file) {
			Ok(entries) => Ok(entries),
			Err(error) => headers::zip_from_front(file).map_err(|_| error),
		},
		Kind::SevenZ => list_7z(file),
		Kind::Tar => list_tar(file),
		Kind::TarGz => list_tar(flate2::read::GzDecoder::new(stream(file))),
		Kind::TarXz => list_tar(lzma_rust2::XzReader::new(stream(file), true)),
		Kind::TarBz2 => list_tar(bzip2::read::MultiBzDecoder::new(stream(file))),
		Kind::TarZst => list_tar(
			ruzstd::decoding::StreamingDecoder::new(stream(file)).context("read the zstd frame")?,
		),
		Kind::TarLz => list_tar(lzma_rust2::LzipReader::new(stream(file))),
		Kind::Rar => headers::rar(file),
		Kind::Iso => headers::iso(file),
		Kind::Cab => headers::cab(file),
	}
}

fn list_zip(file: &mut Available) -> Result<Vec<Entry>> {
	let mut archive = zip::ZipArchive::new(file).context("read the zip directory")?;
	let mut entries = Vec::new();
	for index in 0..archive.len().min(ENTRY_LIMIT) {
		let entry = archive.by_index_raw(index).with_context(|| format!("entry {index}"))?;
		entries.push(Entry { name: entry.name().to_owned(), size: entry.size(), dir: entry.is_dir() });
	}
	Ok(entries)
}

fn list_7z(mut file: Available) -> Result<Vec<Entry>> {
	let archive = sevenz_rust2::Archive::read(&mut file, &sevenz_rust2::Password::empty())
		.context("read the 7z header")?;
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

/// A tar's headers, one every block, read through its data. What was named before an error is
/// kept: a stream cut at the limit, or a download at a hole, still named what came before.
fn list_tar<R: Read>(reader: R) -> Result<Vec<Entry>> {
	let mut archive = tar::Archive::new(reader);
	let mut entries = Vec::new();
	for entry in archive.entries().context("read the tar")? {
		let entry = match entry {
			Ok(entry) => entry,
			Err(error) if entries.is_empty() => return Err(error).context("a tar header"),
			Err(_) => break,
		};
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

/// One name at the top of an archive, as a card lists it: whether it is a folder, and the size of
/// everything under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Top {
	pub name: String,
	pub dir: bool,
	pub size: u64,
}

/// The names `top_level` gives, each with what it holds: the archive as somebody opening it would
/// see it, a single wrapping folder looked through.
pub fn outline(entries: &[Entry]) -> Vec<Top> {
	let names = top_level(entries);
	let parts = |entry: &Entry| -> Vec<String> {
		entry
			.name
			.trim_end_matches('/')
			.split('/')
			.filter(|p| !p.is_empty() && *p != ".")
			.map(str::to_owned)
			.collect()
	};
	// Looked through a wrapper when no entry starts with the first name at the top.
	let strip = usize::from(
		names.first().is_some_and(|first| !entries.iter().any(|e| parts(e).first() == Some(first))),
	);
	let mut tops: Vec<Top> =
		names.into_iter().map(|name| Top { name, dir: false, size: 0 }).collect();
	for entry in entries {
		let parts = parts(entry);
		let Some(first) = parts.get(strip) else { continue };
		if let Some(top) = tops.iter_mut().find(|t| &t.name == first) {
			top.dir |= entry.dir || parts.len() > strip + 1;
			top.size += entry.size;
		}
	}
	tops
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
		assert_eq!(kind_of("a.rar"), Some(Kind::Rar));
		assert_eq!(kind_of("a.tar.xz"), Some(Kind::TarXz));
		assert_eq!(kind_of("a.tbz2"), Some(Kind::TarBz2));
		assert_eq!(kind_of("a.tar.zst"), Some(Kind::TarZst));
		assert_eq!(kind_of("image.ISO"), Some(Kind::Iso));
		assert_eq!(kind_of("a.dmg"), None, "a disk image's names are inside a file system");
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

	#[test]
	fn the_outline_looks_through_a_wrapper_and_sums_what_each_name_holds() {
		let entry = |name: &str, size: u64, dir: bool| Entry { name: name.to_owned(), size, dir };
		let entries = [
			entry("pkg/", 0, true),
			entry("pkg/bin/tool", 100, false),
			entry("pkg/bin/other", 50, false),
			entry("pkg/README", 7, false),
		];
		assert_eq!(
			outline(&entries),
			[
				Top { name: "bin".to_owned(), dir: true, size: 150 },
				Top { name: "README".to_owned(), dir: false, size: 7 },
			]
		);
		let flat = [entry("a.txt", 3, false), entry("docs/b.md", 4, false)];
		assert_eq!(
			outline(&flat),
			[
				Top { name: "a.txt".to_owned(), dir: false, size: 3 },
				Top { name: "docs".to_owned(), dir: true, size: 4 },
			]
		);
	}

	fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
		let mut builder = tar::Builder::new(Vec::new());
		for (name, data) in files {
			let mut header = tar::Header::new_gnu();
			header.set_size(data.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder.append_data(&mut header, name, *data).unwrap();
		}
		builder.into_inner().unwrap()
	}

	#[test]
	fn a_bzip2_tar_is_listed_through_its_stream() {
		let dir = crate::testing::scratch("index-bz2");
		let path = dir.join("a.tar.bz2");
		let mut encoder = bzip2::write::BzEncoder::new(
			std::fs::File::create(&path).unwrap(),
			bzip2::Compression::fast(),
		);
		encoder.write_all(&tar_of(&[("one.txt", b"1"), ("two.txt", b"2")])).unwrap();
		encoder.finish().unwrap();
		let names: Vec<String> = list(&path).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(names, ["one.txt", "two.txt"]);
	}

	/// A tar downloaded as far as its second file's header: the first is named, and the list stops
	/// at the hole rather than failing.
	#[test]
	fn a_partial_tar_names_what_has_arrived() {
		let dir = crate::testing::scratch("index-partial");
		let path = dir.join("a.tar.downloading");
		let bytes = tar_of(&[("first.bin", &[7u8; 600][..]), ("second.bin", b"2")]);
		std::fs::write(&path, &bytes).unwrap();
		let file =
			Available::partial(std::fs::File::open(&path).unwrap(), bytes.len() as u64, vec![(0, 1100)]);
		let names: Vec<String> =
			list_available(file, Kind::Tar).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(names, ["first.bin"]);
	}
}
