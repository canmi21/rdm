//! The file on disk while it is being written: every connection writes its bytes at its own
//! offset into one preallocated file, so there is nothing to merge when the last one finishes
//! -- only a rename. Writes are positioned, `pwrite` underneath, so connections need no lock
//! between them and no shared cursor. See spec/engine.md.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bytes::Bytes;

use crate::engine::control::part_path;
use crate::engine::error::{Error, Result};

/// The partial file, shared by every connection of one download.
#[derive(Clone)]
pub struct Writer {
	target: PathBuf,
	part: PathBuf,
	file: Arc<File>,
}

impl Writer {
	/// Opens `target.part`, creating it, and grows it to `length` when asked to preallocate:
	/// a full disk then fails here, at the start, rather than at the last byte, and every
	/// segment can be written at its offset from the first moment. An existing partial file is
	/// kept -- that is what resuming is -- and only grown, never cut.
	pub fn open(target: &Path, length: Option<u64>, preallocate: bool) -> Result<Writer> {
		let part = part_path(target);
		let disk = |source| Error::Disk { path: part.clone(), source };
		if let Some(parent) = target.parent() {
			std::fs::create_dir_all(parent)
				.map_err(|source| Error::Disk { path: parent.to_path_buf(), source })?;
		}
		let file = OpenOptions::new()
			.read(true)
			.write(true)
			.create(true)
			.truncate(false)
			.open(&part)
			.map_err(disk)?;
		if let (Some(length), true) = (length, preallocate) {
			let current = file.metadata().map_err(disk)?.len();
			if current < length {
				file.set_len(length).map_err(disk)?;
			}
		}
		Ok(Writer { target: target.to_path_buf(), part, file: Arc::new(file) })
	}

	pub fn part_path(&self) -> &Path {
		&self.part
	}

	/// Writes the whole of `bytes` at `offset`. Positioned, so two connections writing at once
	/// do not disturb each other; on a thread of the runtime's blocking pool, so a slow disk
	/// does not stall the connections that are not waiting on it.
	pub async fn write_at(&self, offset: u64, bytes: Bytes) -> Result<()> {
		let file = self.file.clone();
		let part = self.part.clone();
		tokio::task::spawn_blocking(move || {
			file.write_all_at(&bytes, offset).map_err(|source| Error::Disk { path: part, source })
		})
		.await
		.map_err(|e| Error::Disk { path: self.part.clone(), source: io::Error::other(e) })?
	}

	/// Reads back a range, for verification and for tests.
	pub async fn read_at(&self, offset: u64, len: usize) -> Result<Vec<u8>> {
		let file = self.file.clone();
		let part = self.part.clone();
		tokio::task::spawn_blocking(move || {
			let mut buffer = vec![0u8; len];
			file
				.read_exact_at(&mut buffer, offset)
				.map_err(|source| Error::Disk { path: part, source })?;
			Ok(buffer)
		})
		.await
		.map_err(|e| Error::Disk { path: self.part.clone(), source: io::Error::other(e) })?
	}

	/// The download is complete: the file is flushed to disk, cut to `length` if it was grown
	/// past what arrived, and renamed to its final name. A file already there under that name
	/// is not overwritten; the new one takes the next free `name (n).ext`, as browsers do, and
	/// the path it ended up with is returned.
	pub async fn finish(self, length: Option<u64>) -> Result<PathBuf> {
		let file = self.file.clone();
		let part = self.part.clone();
		let target = self.target.clone();
		tokio::task::spawn_blocking(move || {
			let disk = |path: &Path| {
				let path = path.to_path_buf();
				move |source| Error::Disk { path, source }
			};
			if let Some(length) = length {
				file.set_len(length).map_err(disk(&part))?;
			}
			file.sync_all().map_err(disk(&part))?;
			drop(file);
			let destination = free_name(&target);
			std::fs::rename(&part, &destination).map_err(disk(&part))?;
			Ok(destination)
		})
		.await
		.map_err(|e| Error::Disk { path: self.part.clone(), source: io::Error::other(e) })?
	}
}

/// `movie.mkv`, or `movie (1).mkv`, `movie (2).mkv`... whichever is first not taken.
pub fn free_name(target: &Path) -> PathBuf {
	if !target.exists() {
		return target.to_path_buf();
	}
	let stem = target.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
	let extension =
		target.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
	(1..)
		.map(|n| target.with_file_name(format!("{stem} ({n}){extension}")))
		.find(|candidate| !candidate.exists())
		.expect("some number is free")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn scratch(name: &str) -> PathBuf {
		let dir = std::env::temp_dir().join(format!("rdm-writer-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		dir.join("out.bin")
	}

	#[tokio::test]
	async fn segments_land_at_their_offsets_and_the_file_is_renamed_once_done() {
		let target = scratch("offsets");
		let writer = Writer::open(&target, Some(10), true).await_free();
		assert_eq!(std::fs::metadata(writer.part_path()).unwrap().len(), 10, "preallocated");
		let a = writer.clone();
		let b = writer.clone();
		let (x, y) = tokio::join!(
			a.write_at(5, Bytes::from_static(b"56789")),
			b.write_at(0, Bytes::from_static(b"01234"))
		);
		x.unwrap();
		y.unwrap();
		assert_eq!(writer.read_at(0, 10).await.unwrap(), b"0123456789");
		let done = writer.finish(Some(10)).await.unwrap();
		assert_eq!(done, target);
		assert_eq!(std::fs::read(&target).unwrap(), b"0123456789");
		assert!(!part_path(&target).exists());
	}

	#[tokio::test]
	async fn an_existing_file_is_not_overwritten_and_a_partial_one_is_kept() {
		let target = scratch("collide");
		std::fs::create_dir_all(target.parent().unwrap()).unwrap();
		std::fs::write(&target, b"old").unwrap();
		std::fs::write(target.with_file_name("out (1).bin"), b"older").unwrap();
		let writer = Writer::open(&target, Some(3), true).await_free();
		writer.write_at(0, Bytes::from_static(b"new")).await.unwrap();
		let done = writer.finish(Some(3)).await.unwrap();
		assert_eq!(done, target.with_file_name("out (2).bin"));
		assert_eq!(std::fs::read(&target).unwrap(), b"old", "what was there stays");
		// A partial file left by an earlier run is what resuming continues from.
		std::fs::write(part_path(&target), b"abc").unwrap();
		let again = Writer::open(&target, Some(6), true).await_free();
		assert_eq!(again.read_at(0, 3).await.unwrap(), b"abc");
		assert_eq!(std::fs::metadata(again.part_path()).unwrap().len(), 6, "grown, not cut");
	}

	/// Tests only: unwrap with a clearer message than the default.
	trait Free {
		fn await_free(self) -> Writer;
	}

	impl Free for Result<Writer> {
		fn await_free(self) -> Writer {
			self.unwrap_or_else(|e| panic!("open the partial file: {e}"))
		}
	}
}
