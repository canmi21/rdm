//! After the last byte: is the file what it was supposed to be, and what kind of file is it.
//! A checksum the user supplied is checked against the finished file, and the file's type is
//! read from its first bytes, which is worth more than the extension the server chose.

use std::path::Path;

use md5::Md5;
use sha2::{Digest, Sha256, Sha512};

use crate::error::{Error, Result};

/// A digest the user expects, as hex. Compared without regard to case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Checksum {
	Sha256(String),
	Sha512(String),
	Md5(String),
}

impl Checksum {
	/// `sha256:abc...`, `sha-256=abc...`, `md5 abc...`: the name, a separator, the hex. A bare
	/// hex string is taken by its length -- 32 for MD5, 64 for SHA-256, 128 for SHA-512.
	pub fn parse(text: &str) -> Option<Checksum> {
		let text = text.trim();
		let (name, hex) = match text.split_once([':', '=', ' ']) {
			Some((name, hex)) => (name.trim().to_ascii_lowercase().replace('-', ""), hex.trim()),
			None => (String::new(), text),
		};
		if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
			return None;
		}
		let hex = hex.to_ascii_lowercase();
		match (name.as_str(), hex.len()) {
			("sha256", 64) | ("", 64) => Some(Checksum::Sha256(hex)),
			("sha512", 128) | ("", 128) => Some(Checksum::Sha512(hex)),
			("md5", 32) | ("", 32) => Some(Checksum::Md5(hex)),
			_ => None,
		}
	}

	pub fn expected(&self) -> &str {
		match self {
			Checksum::Sha256(hex) | Checksum::Sha512(hex) | Checksum::Md5(hex) => hex,
		}
	}
}

/// Reads the whole file and compares. On the blocking pool, since a large file takes seconds.
pub async fn verify(path: &Path, checksum: &Checksum) -> Result<()> {
	let path = path.to_path_buf();
	let checksum = checksum.clone();
	tokio::task::spawn_blocking(move || {
		let computed = digest(&path, &checksum)?;
		if computed == checksum.expected() {
			Ok(())
		} else {
			Err(Error::Checksum { expected: checksum.expected().to_owned(), computed })
		}
	})
	.await
	.map_err(|e| Error::Disk { path: std::path::PathBuf::new(), source: std::io::Error::other(e) })?
}

fn digest(path: &Path, checksum: &Checksum) -> Result<String> {
	let disk = |source| Error::Disk { path: path.to_path_buf(), source };
	let mut file = std::fs::File::open(path).map_err(disk)?;
	fn run<D: Digest>(file: &mut std::fs::File) -> std::io::Result<String> {
		use std::io::Read;
		let mut hasher = D::new();
		let mut buffer = vec![0u8; 1 << 16];
		loop {
			let n = file.read(&mut buffer)?;
			if n == 0 {
				break;
			}
			hasher.update(&buffer[..n]);
		}
		Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
	}
	match checksum {
		Checksum::Sha256(_) => run::<Sha256>(&mut file),
		Checksum::Sha512(_) => run::<Sha512>(&mut file),
		Checksum::Md5(_) => run::<Md5>(&mut file),
	}
	.map_err(disk)
}

/// The file's type from its first bytes -- `video/x-matroska`, `application/zip` -- or None
/// when the bytes say nothing that `infer` knows. Read after the download, from the disk.
pub fn kind(path: &Path) -> Option<&'static str> {
	use std::io::Read;
	let mut head = [0u8; 8192];
	let mut file = std::fs::File::open(path).ok()?;
	let n = file.read(&mut head).ok()?;
	infer::get(&head[..n]).map(|t| t.mime_type())
}

#[cfg(test)]
mod tests {
	use super::*;

	fn scratch(name: &str, content: &[u8]) -> std::path::PathBuf {
		let dir = std::env::temp_dir().join(format!("rdm-verify-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join(name);
		std::fs::write(&path, content).unwrap();
		path
	}

	#[test]
	fn a_checksum_is_read_in_the_ways_people_write_them() {
		let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
		assert_eq!(Checksum::parse(&format!("sha256:{sha}")), Some(Checksum::Sha256(sha.into())));
		assert_eq!(
			Checksum::parse(&format!("SHA-256={}", sha.to_uppercase())),
			Some(Checksum::Sha256(sha.into()))
		);
		assert_eq!(Checksum::parse(sha), Some(Checksum::Sha256(sha.into())));
		assert_eq!(
			Checksum::parse("md5 d41d8cd98f00b204e9800998ecf8427e"),
			Some(Checksum::Md5("d41d8cd98f00b204e9800998ecf8427e".into()))
		);
		assert_eq!(Checksum::parse("sha256:abc"), None, "the wrong length is not a digest");
		assert_eq!(Checksum::parse("zz"), None);
	}

	#[tokio::test]
	async fn the_file_is_checked_and_a_mismatch_says_both_digests() {
		let path = scratch("empty.bin", b"");
		let empty =
			Checksum::Sha256("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into());
		verify(&path, &empty).await.unwrap();
		verify(&path, &Checksum::Md5("d41d8cd98f00b204e9800998ecf8427e".into())).await.unwrap();
		let path = scratch("abc.bin", b"abc");
		match verify(&path, &empty).await {
			Err(Error::Checksum { expected, computed }) => {
				assert_eq!(expected, empty.expected());
				assert_eq!(computed, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
			}
			other => panic!("{other:?}"),
		}
	}

	#[test]
	fn the_kind_is_read_from_the_bytes() {
		let png = scratch("x.bin", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
		assert_eq!(kind(&png), Some("image/png"));
		let text = scratch("t.bin", b"hello");
		assert_eq!(kind(&text), None);
	}
}
