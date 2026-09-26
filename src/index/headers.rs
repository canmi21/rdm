//! The archives whose names can be read straight off their headers, without a library: RAR 4 and 5,
//! whose headers stand before each file's data; ISO 9660, whose directories are sectors near the
//! start; CAB, whose file list follows its header; and a zip read from the front, for one whose
//! directory at the end has not arrived. Each reads until the file or a hole ends it, and keeps
//! what it named. See spec/state.md, "An archive is listed as far as it has arrived".

use std::io::{Read, Seek, SeekFrom};

use anyhow::{Context as _, Result, bail};

use super::{ENTRY_LIMIT, Entry};

fn bytes<R: Read>(reader: &mut R, n: usize) -> std::io::Result<Vec<u8>> {
	let mut buf = vec![0; n];
	reader.read_exact(&mut buf)?;
	Ok(buf)
}

fn u16_le(b: &[u8], at: usize) -> u16 {
	u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_le(b: &[u8], at: usize) -> u32 {
	u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// What was named before an error, or the error when nothing was: a list cut short by a hole is
/// still a list.
fn so_far(entries: Vec<Entry>, error: anyhow::Error) -> Result<Vec<Entry>> {
	if entries.is_empty() { Err(error) } else { Ok(entries) }
}

pub fn rar<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let signature = bytes(&mut reader, 7).context("read the signature")?;
	anyhow::ensure!(&signature[..6] == b"Rar!\x1a\x07", "not a rar");
	match signature[6] {
		0 => rar4(reader),
		1 => {
			bytes(&mut reader, 1)?;
			rar5(reader)
		}
		_ => bail!("a rar of a version not read here"),
	}
}

/// A RAR 5 variable-length integer: seven bits a byte, low first, the high bit saying more follow.
fn vint<R: Read>(reader: &mut R) -> std::io::Result<(u64, u64)> {
	let (mut value, mut shift, mut length) = (0u64, 0, 0);
	loop {
		let byte = bytes(reader, 1)?[0];
		length += 1;
		value |= u64::from(byte & 0x7f) << shift;
		if byte & 0x80 == 0 || shift >= 63 {
			return Ok((value, length));
		}
		shift += 7;
	}
}

fn rar5<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let mut entries = Vec::new();
	let mut at = reader.stream_position()?;
	while entries.len() < ENTRY_LIMIT {
		let step = (|| -> Result<Option<u64>> {
			reader.seek(SeekFrom::Start(at))?;
			bytes(&mut reader, 4)?;
			let (header_size, size_length) = vint(&mut reader)?;
			let header = bytes(&mut reader, usize::try_from(header_size)?.min(1 << 20))?;
			let mut cursor = std::io::Cursor::new(&header);
			let (kind, _) = vint(&mut cursor)?;
			let (flags, _) = vint(&mut cursor)?;
			if flags & 1 != 0 {
				vint(&mut cursor)?;
			}
			let data = if flags & 2 != 0 { vint(&mut cursor)?.0 } else { 0 };
			match kind {
				// A file: its own flags, size, attributes, an optional time and CRC, the method,
				// the system, then the name.
				2 => {
					let (file_flags, _) = vint(&mut cursor)?;
					let (size, _) = vint(&mut cursor)?;
					vint(&mut cursor)?;
					if file_flags & 2 != 0 {
						bytes(&mut cursor, 4)?;
					}
					if file_flags & 4 != 0 {
						bytes(&mut cursor, 4)?;
					}
					vint(&mut cursor)?;
					vint(&mut cursor)?;
					let (length, _) = vint(&mut cursor)?;
					let name = bytes(&mut cursor, usize::try_from(length)?)?;
					entries.push(Entry {
						name: String::from_utf8_lossy(&name).into_owned(),
						size,
						dir: file_flags & 1 != 0,
					});
				}
				// Encrypted headers, which hide every name after them, and the end.
				4 | 5 => return Ok(None),
				_ => {}
			}
			Ok(Some(at + 4 + size_length + header_size + data))
		})();
		match step {
			Ok(Some(next)) if next > at => at = next,
			Ok(_) => break,
			Err(error) => return so_far(entries, error),
		}
	}
	Ok(entries)
}

fn rar4<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let mut entries = Vec::new();
	let mut at = reader.stream_position()?;
	while entries.len() < ENTRY_LIMIT {
		let step = (|| -> Result<Option<u64>> {
			reader.seek(SeekFrom::Start(at))?;
			let head = bytes(&mut reader, 7)?;
			let (kind, flags, size) = (head[2], u16_le(&head, 3), u64::from(u16_le(&head, 5)));
			anyhow::ensure!(size >= 7, "a header shorter than a header");
			let rest = bytes(&mut reader, usize::try_from(size - 7)?)?;
			let mut data =
				if flags & 0x8000 != 0 && rest.len() >= 4 { u64::from(u32_le(&rest, 0)) } else { 0 };
			match kind {
				0x74 if rest.len() >= 25 => {
					let packed_high = flags & 0x100 != 0 && rest.len() >= 33;
					data = u64::from(u32_le(&rest, 0))
						| if packed_high { u64::from(u32_le(&rest, 25)) << 32 } else { 0 };
					let unpacked = u64::from(u32_le(&rest, 4))
						| if packed_high { u64::from(u32_le(&rest, 29)) << 32 } else { 0 };
					let name_at = if packed_high { 33 } else { 25 };
					let length = usize::from(u16_le(&rest, 19));
					let name = rest.get(name_at..name_at + length).unwrap_or_default();
					// A Unicode name follows the plain one after a zero byte; the plain one is kept.
					let name = name.split(|b| *b == 0).next().unwrap_or_default();
					entries.push(Entry {
						name: String::from_utf8_lossy(name).replace('\\', "/"),
						size: unpacked,
						dir: flags & 0xe0 == 0xe0,
					});
				}
				0x7b => return Ok(None),
				_ => {}
			}
			Ok(Some(at + size + data))
		})();
		match step {
			Ok(Some(next)) if next > at => at = next,
			Ok(_) => break,
			Err(error) => return so_far(entries, error),
		}
	}
	Ok(entries)
}

const SECTOR: u64 = 2048;
/// How deep an ISO's folders are walked.
const ISO_DEPTH: usize = 8;

/// An ISO 9660 image: the volume descriptors from sector 16, the Joliet one preferred for its long
/// names, then its directories walked from the root.
pub fn iso<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let mut root = None;
	let mut joliet = false;
	for sector in 16..32 {
		reader.seek(SeekFrom::Start(sector * SECTOR))?;
		let descriptor = bytes(&mut reader, 2048).context("read a volume descriptor")?;
		anyhow::ensure!(&descriptor[1..6] == b"CD001", "not an ISO 9660 image");
		let record = &descriptor[156..190];
		let place = (u64::from(u32_le(record, 2)), u64::from(u32_le(record, 10)));
		match descriptor[0] {
			1 if root.is_none() => root = Some(place),
			// A supplementary descriptor whose escape names UCS-2: Joliet.
			2 if descriptor[88] == b'%' && descriptor[89] == b'/' => {
				root = Some(place);
				joliet = true;
			}
			255 => break,
			_ => {}
		}
	}
	let (extent, length) = root.context("no primary volume descriptor")?;
	let mut entries = Vec::new();
	let mut folders = vec![(String::new(), extent, length, 0)];
	// A folder whose sectors have not arrived is passed over, and the walk goes on through the
	// others: an image's folders are scattered, and one hole is not the end of it.
	let mut first_error = None;
	while let Some((prefix, extent, length, depth)) = folders.pop() {
		if let Err(error) =
			iso_folder(&mut reader, (&prefix, extent, length), joliet, &mut entries, |name, e, l| {
				if depth < ISO_DEPTH {
					folders.push((name, e, l, depth + 1));
				}
			}) {
			first_error.get_or_insert(error);
		}
		if entries.len() >= ENTRY_LIMIT {
			break;
		}
	}
	match first_error {
		Some(error) => so_far(entries, error),
		None => Ok(entries),
	}
}

fn iso_folder<R: Read + Seek>(
	reader: &mut R,
	(prefix, extent, length): (&str, u64, u64),
	joliet: bool,
	entries: &mut Vec<Entry>,
	mut folder: impl FnMut(String, u64, u64),
) -> Result<()> {
	reader.seek(SeekFrom::Start(extent * SECTOR))?;
	let data = bytes(reader, usize::try_from(length.min(4 * 1024 * 1024))?)?;
	let mut at = 0;
	while at < data.len() && entries.len() < ENTRY_LIMIT {
		let size = usize::from(data[at]);
		// A record never crosses a sector; a zero length pads to the next one.
		if size == 0 {
			at = (at / SECTOR as usize + 1) * SECTOR as usize;
			continue;
		}
		let record = data.get(at..at + size).context("a record past its folder")?;
		at += size;
		let name_length = usize::from(*record.get(32).unwrap_or(&0));
		let raw = record.get(33..33 + name_length).unwrap_or_default();
		// The folder itself and its parent, as a single byte zero and one.
		if raw == [0] || raw == [1] {
			continue;
		}
		let name = if joliet {
			let units: Vec<u16> = raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
			String::from_utf16_lossy(&units)
		} else {
			String::from_utf8_lossy(raw).into_owned()
		};
		let name = name.split(';').next().unwrap_or_default().trim_end_matches('.').to_owned();
		let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
		let (child, size) = (u64::from(u32_le(record, 2)), u64::from(u32_le(record, 10)));
		let dir = record[25] & 2 != 0;
		entries.push(Entry {
			name: if dir { format!("{path}/") } else { path.clone() },
			size: if dir { 0 } else { size },
			dir,
		});
		if dir {
			folder(path, child, size);
		}
	}
	Ok(())
}

/// A cabinet: its header says where the file list starts and how long it is.
pub fn cab<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let header = bytes(&mut reader, 36).context("read the header")?;
	anyhow::ensure!(&header[..4] == b"MSCF", "not a cabinet");
	let (first, count) = (u64::from(u32_le(&header, 16)), usize::from(u16_le(&header, 28)));
	reader.seek(SeekFrom::Start(first))?;
	let mut entries = Vec::new();
	for _ in 0..count.min(ENTRY_LIMIT) {
		let read = (|| -> Result<Entry> {
			let fixed = bytes(&mut reader, 16)?;
			let mut name = Vec::new();
			loop {
				let byte = bytes(&mut reader, 1)?[0];
				if byte == 0 || name.len() > 1024 {
					break;
				}
				name.push(byte);
			}
			Ok(Entry {
				name: String::from_utf8_lossy(&name).replace('\\', "/"),
				size: u64::from(u32_le(&fixed, 0)),
				dir: false,
			})
		})();
		match read {
			Ok(entry) => entries.push(entry),
			Err(error) => return so_far(entries, error),
		}
	}
	Ok(entries)
}

/// A zip read from the front, one local header after another, for a zip whose directory at its end
/// has not arrived. A header that leaves its size to a descriptor after the data cannot be stepped
/// over, and ends the list there.
pub fn zip_from_front<R: Read + Seek>(mut reader: R) -> Result<Vec<Entry>> {
	let mut entries = Vec::new();
	let mut at = 0u64;
	while entries.len() < ENTRY_LIMIT {
		let step = (|| -> Result<Option<u64>> {
			reader.seek(SeekFrom::Start(at))?;
			let head = bytes(&mut reader, 30)?;
			if u32_le(&head, 0) != 0x0403_4b50 {
				return Ok(None);
			}
			let flags = u16_le(&head, 6);
			let (mut packed, mut size) = (u64::from(u32_le(&head, 18)), u64::from(u32_le(&head, 22)));
			let (name_length, extra_length) =
				(usize::from(u16_le(&head, 26)), usize::from(u16_le(&head, 28)));
			let name = bytes(&mut reader, name_length)?;
			let extra = bytes(&mut reader, extra_length)?;
			// Zip64 keeps the real sizes in an extra field when the header's say all ones.
			let mut field = 0;
			while field + 4 <= extra.len() {
				let (id, length) = (u16_le(&extra, field), usize::from(u16_le(&extra, field + 2)));
				if id == 1 && length >= 16 && field + 20 <= extra.len() {
					size = u64::from_le_bytes(extra[field + 4..field + 12].try_into()?);
					packed = u64::from_le_bytes(extra[field + 12..field + 20].try_into()?);
				}
				field += 4 + length;
			}
			let name = String::from_utf8_lossy(&name).into_owned();
			let dir = name.ends_with('/');
			if flags & 8 != 0 && packed == 0 && !dir {
				entries.push(Entry { name, size, dir });
				return Ok(None);
			}
			entries.push(Entry { name, size, dir });
			Ok(Some(at + 30 + name_length as u64 + extra_length as u64 + packed))
		})();
		match step {
			Ok(Some(next)) if next > at => at = next,
			Ok(_) => break,
			Err(error) => return so_far(entries, error),
		}
	}
	anyhow::ensure!(!entries.is_empty(), "no local header at the front");
	Ok(entries)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Cursor;

	fn rar4_file(name: &str, size: u32, packed: &[u8]) -> Vec<u8> {
		let mut head = Vec::new();
		head.extend((packed.len() as u32).to_le_bytes());
		head.extend(size.to_le_bytes());
		head.push(0);
		head.extend([0u8; 4]);
		head.extend([0u8; 4]);
		head.push(29);
		head.push(0x30);
		head.extend((name.len() as u16).to_le_bytes());
		head.extend([0u8; 4]);
		head.extend(name.as_bytes());
		let mut block = vec![0, 0, 0x74];
		block.extend(0x8000u16.to_le_bytes());
		block.extend(((7 + head.len()) as u16).to_le_bytes());
		block.extend(head);
		block.extend(packed);
		block
	}

	#[test]
	fn a_rar4s_file_headers_are_read_in_turn() {
		let mut archive = b"Rar!\x1a\x07\x00".to_vec();
		archive.extend(rar4_file("docs\\a.txt", 5, b"hello"));
		archive.extend(rar4_file("b.bin", 3, b"xyz"));
		archive.extend([0, 0, 0x7b, 0, 0, 7, 0]);
		let names: Vec<String> =
			rar(Cursor::new(archive)).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(names, ["docs/a.txt", "b.bin"]);
	}

	fn vint_bytes(mut value: u64) -> Vec<u8> {
		let mut out = Vec::new();
		loop {
			let byte = (value & 0x7f) as u8;
			value >>= 7;
			if value == 0 {
				out.push(byte);
				return out;
			}
			out.push(byte | 0x80);
		}
	}

	#[test]
	fn a_rar5s_file_headers_are_read_in_turn() {
		let file = |name: &str, dir: bool, data: &[u8]| {
			let mut header = vint_bytes(2);
			header.extend(vint_bytes(2));
			header.extend(vint_bytes(data.len() as u64));
			header.extend(vint_bytes(u64::from(dir)));
			header.extend(vint_bytes(data.len() as u64));
			header.extend(vint_bytes(0));
			header.extend(vint_bytes(0));
			header.extend(vint_bytes(0));
			header.extend(vint_bytes(name.len() as u64));
			header.extend(name.as_bytes());
			let mut block = vec![0u8; 4];
			block.extend(vint_bytes(header.len() as u64));
			block.extend(header);
			block.extend(data);
			block
		};
		let mut archive = b"Rar!\x1a\x07\x01\x00".to_vec();
		archive.extend(file("src", true, b""));
		archive.extend(file("src/main.rs", false, b"fn main() {}"));
		let entries = rar(Cursor::new(archive)).unwrap();
		assert_eq!(entries[0], Entry { name: "src".to_owned(), size: 0, dir: true });
		assert_eq!(entries[1], Entry { name: "src/main.rs".to_owned(), size: 12, dir: false });
	}

	#[test]
	fn an_iso_is_walked_from_its_root() {
		let mut image = vec![0u8; 20 * SECTOR as usize];
		let record = |extent: u32, size: u32, dir: bool, name: &[u8]| {
			let mut r = vec![0u8; 33];
			r[2..6].copy_from_slice(&extent.to_le_bytes());
			r[10..14].copy_from_slice(&size.to_le_bytes());
			r[25] = if dir { 2 } else { 0 };
			r[32] = name.len() as u8;
			r.extend(name);
			if r.len() % 2 == 1 {
				r.push(0);
			}
			r[0] = r.len() as u8;
			r
		};
		let pvd = 16 * SECTOR as usize;
		image[pvd] = 1;
		image[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
		let root = record(18, SECTOR as u32, true, &[0]);
		image[pvd + 156..pvd + 156 + 34].copy_from_slice(&root[..34]);
		let end = 17 * SECTOR as usize;
		image[end] = 255;
		image[end + 1..end + 6].copy_from_slice(b"CD001");
		let mut folder = Vec::new();
		folder.extend(record(18, SECTOR as u32, true, &[0]));
		folder.extend(record(18, SECTOR as u32, true, &[1]));
		folder.extend(record(19, 5, false, b"README.TXT;1"));
		folder.extend(record(19, SECTOR as u32, true, b"BOOT"));
		let at = 18 * SECTOR as usize;
		image[at..at + folder.len()].copy_from_slice(&folder);
		let names: Vec<String> = iso(Cursor::new(image)).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(names, ["README.TXT", "BOOT/"]);
	}

	#[test]
	fn a_zip_is_read_from_the_front_when_its_end_is_missing() {
		let mut bytes = Vec::new();
		{
			let mut writer = zip::ZipWriter::new(Cursor::new(&mut bytes));
			let options =
				zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
			writer.start_file("a.txt", options).unwrap();
			std::io::Write::write_all(&mut writer, b"hello").unwrap();
			writer.start_file("b/c.txt", options).unwrap();
			std::io::Write::write_all(&mut writer, b"world").unwrap();
			writer.finish().unwrap();
		}
		// The directory at the end cut away, as a download that has not reached it.
		bytes.truncate(bytes.len() - 40);
		let names: Vec<String> =
			zip_from_front(Cursor::new(bytes)).unwrap().into_iter().map(|e| e.name).collect();
		assert_eq!(names, ["a.txt", "b/c.txt"]);
	}
}
