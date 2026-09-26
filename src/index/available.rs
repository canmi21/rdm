//! A file read only where it has arrived. A download is written by several connections at once, so
//! a partial file has holes wherever a connection has not reached yet; a read that runs into one
//! ends as the file would end, and a lister stops there with what it has named. A whole file is one
//! range from start to end. See spec/state.md, "An archive is listed as far as it has arrived".

use std::io::{Read, Seek, SeekFrom};

pub struct Available {
	file: std::fs::File,
	len: u64,
	/// The byte ranges that are on disk, `start..end`, in order and apart.
	done: Vec<(u64, u64)>,
	at: u64,
}

impl Available {
	/// A whole file.
	pub fn whole(file: std::fs::File) -> std::io::Result<Available> {
		let len = file.metadata()?.len();
		Ok(Available { file, len, done: vec![(0, len)], at: 0 })
	}

	/// A partial one: `len` is the size the file will have, `done` what has arrived of it.
	pub fn partial(file: std::fs::File, len: u64, mut done: Vec<(u64, u64)>) -> Available {
		done.retain(|(start, end)| end > start);
		done.sort_unstable();
		// Neighbours joined, so a read across two segments that meet is one read.
		let mut joined: Vec<(u64, u64)> = Vec::with_capacity(done.len());
		for (start, end) in done {
			match joined.last_mut() {
				Some(last) if start <= last.1 => last.1 = last.1.max(end),
				_ => joined.push((start, end)),
			}
		}
		Available { file, len, done: joined, at: 0 }
	}
}

impl Read for Available {
	fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
		if self.at >= self.len || buf.is_empty() {
			return Ok(0);
		}
		let Some(&(_, end)) = self.done.iter().find(|(start, end)| *start <= self.at && self.at < *end)
		else {
			return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "not downloaded yet"));
		};
		let most = usize::try_from(end - self.at).unwrap_or(usize::MAX).min(buf.len());
		self.file.seek(SeekFrom::Start(self.at))?;
		let read = self.file.read(&mut buf[..most])?;
		self.at += read as u64;
		Ok(read)
	}
}

impl Seek for Available {
	fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
		let at = match to {
			SeekFrom::Start(at) => Some(at),
			SeekFrom::End(delta) => self.len.checked_add_signed(delta),
			SeekFrom::Current(delta) => self.at.checked_add_signed(delta),
		};
		self.at = at.ok_or_else(|| {
			std::io::Error::new(std::io::ErrorKind::InvalidInput, "a seek before the start")
		})?;
		Ok(self.at)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;

	#[test]
	fn a_read_ends_at_a_hole_and_goes_on_past_it() {
		let dir = crate::testing::scratch("available");
		let path = dir.join("part");
		std::fs::File::create(&path).unwrap().write_all(b"0123456789").unwrap();
		let mut file =
			Available::partial(std::fs::File::open(&path).unwrap(), 10, vec![(6, 10), (0, 2), (2, 4)]);
		let mut all = Vec::new();
		assert!(file.read_to_end(&mut all).is_err(), "the hole at four");
		assert_eq!(all, b"0123", "two segments that meet read as one");
		file.seek(SeekFrom::End(-3)).unwrap();
		let mut tail = String::new();
		file.read_to_string(&mut tail).unwrap();
		assert_eq!(tail, "789");
	}
}
