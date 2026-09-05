//! The downloads themselves, between launches: one row each in `internal.sqlite` -- the
//! address, where it was found, the name, the size and how much landed, the status, when it
//! was added, where it ended up. Rows are written as they change and read back at launch, so
//! the list a user left is the list they find. The plans beside partial files are the
//! engine's; this is the window's. See spec/state.md.

use std::path::Path;

use anyhow::{Context as _, Result};
use chrono::{DateTime, Local};
use rusqlite::{Connection, params};

use crate::download::{Download, Status};

/// The schema's version, in SQLite's `user_version`. Bumped only when a database written before
/// can no longer be read as it is; the same rule as state.json's.
pub const VERSION: i64 = 1;

pub struct Store {
	connection: Connection,
}

impl Store {
	/// Opens or creates the database and brings its schema to this version.
	pub fn open(path: &Path) -> Result<Store> {
		if let Some(parent) = path.parent() {
			std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
		}
		let connection = Connection::open(path).with_context(|| format!("open {}", path.display()))?;
		// A crash between two writes must not lose the rows written before it.
		connection.pragma_update(None, "journal_mode", "WAL")?;
		let version: i64 = connection.pragma_query_value(None, "user_version", |r| r.get(0))?;
		if version > VERSION {
			anyhow::bail!("{} is version {version}, newer than this build's {VERSION}", path.display());
		}
		if version == 0 {
			connection.execute_batch(
				"CREATE TABLE IF NOT EXISTS downloads (
					id INTEGER PRIMARY KEY,
					name TEXT NOT NULL,
					url TEXT NOT NULL,
					source TEXT,
					size INTEGER NOT NULL,
					received INTEGER NOT NULL,
					status TEXT NOT NULL,
					added TEXT NOT NULL,
					path TEXT,
					error TEXT
				);",
			)?;
			connection.pragma_update(None, "user_version", VERSION)?;
		}
		// Each later version adds an arm here, from n to n + 1; the arms are never removed.
		Ok(Store { connection })
	}

	/// Every row, oldest first.
	pub fn load(&self) -> Result<Vec<Download>> {
		let mut statement = self.connection.prepare(
			"SELECT id, name, url, source, size, received, status, added, path, error
			 FROM downloads ORDER BY id",
		)?;
		let rows = statement.query_map([], |row| {
			let status: String = row.get(6)?;
			let added: String = row.get(7)?;
			Ok(Download {
				id: row.get::<_, i64>(0)? as u64,
				name: row.get(1)?,
				url: row.get(2)?,
				source: row.get(3)?,
				size: row.get::<_, i64>(4)? as u64,
				received: row.get::<_, i64>(5)? as u64,
				speed: 0,
				status: Status::parse(&status).unwrap_or(Status::Failed),
				added: DateTime::parse_from_rfc3339(&added)
					.map(|t| t.with_timezone(&Local))
					.unwrap_or_else(|_| Local::now()),
				path: row.get(8)?,
				error: row.get(9)?,
			})
		})?;
		rows.map(|r| r.context("read a download")).collect()
	}

	/// Writes the row, new or changed. Speed is not kept: it is a number about now.
	pub fn save(&self, download: &Download) -> Result<()> {
		self.connection.execute(
			"INSERT INTO downloads (id, name, url, source, size, received, status, added, path, error)
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
			 ON CONFLICT(id) DO UPDATE SET
				name = excluded.name, url = excluded.url, source = excluded.source,
				size = excluded.size, received = excluded.received, status = excluded.status,
				added = excluded.added, path = excluded.path, error = excluded.error",
			params![
				download.id as i64,
				download.name,
				download.url,
				download.source,
				download.size as i64,
				download.received as i64,
				download.status.name(),
				download.added.to_rfc3339(),
				download.path,
				download.error,
			],
		)?;
		Ok(())
	}

	pub fn remove(&self, id: u64) -> Result<()> {
		self.connection.execute("DELETE FROM downloads WHERE id = ?1", params![id as i64])?;
		Ok(())
	}

	/// One more than the highest id ever used, so a removed row's id is not handed out again
	/// while its partial file might still be on disk under it.
	pub fn next_id(&self) -> Result<u64> {
		let max: Option<i64> =
			self.connection.query_row("SELECT MAX(id) FROM downloads", [], |r| r.get(0))?;
		Ok(max.unwrap_or(0) as u64 + 1)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn scratch(name: &str) -> std::path::PathBuf {
		let dir = std::env::temp_dir().join(format!("rdm-store-{}-{name}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		dir.join("internal.sqlite")
	}

	fn row(id: u64, status: Status) -> Download {
		Download {
			id,
			name: format!("file-{id}.bin"),
			url: format!("https://h/file-{id}.bin"),
			source: Some("https://h/downloads/".into()),
			size: 1000,
			received: 250,
			speed: 99,
			status,
			added: Local::now(),
			path: None,
			error: None,
		}
	}

	#[test]
	fn rows_are_written_read_back_and_removed() {
		let path = scratch("roundtrip");
		let store = Store::open(&path).unwrap();
		assert_eq!(store.load().unwrap().len(), 0);
		assert_eq!(store.next_id().unwrap(), 1);
		let mut a = row(1, Status::Downloading);
		let b = row(2, Status::Completed);
		store.save(&a).unwrap();
		store.save(&b).unwrap();
		a.received = 900;
		a.status = Status::Paused;
		a.error = Some("network".into());
		store.save(&a).unwrap();
		let again = Store::open(&path).unwrap();
		let rows = again.load().unwrap();
		assert_eq!(rows.len(), 2);
		assert_eq!((rows[0].received, rows[0].status), (900, Status::Paused), "the update held");
		assert_eq!(rows[0].error.as_deref(), Some("network"));
		assert_eq!(rows[0].speed, 0, "speed is about now and is not kept");
		assert_eq!(rows[0].source.as_deref(), Some("https://h/downloads/"));
		assert_eq!(rows[0].added.timestamp(), a.added.timestamp());
		again.remove(2).unwrap();
		assert_eq!(again.load().unwrap().len(), 1);
		assert_eq!(again.next_id().unwrap(), 2, "the highest id in the table plus one");
	}

	#[test]
	fn a_newer_database_is_refused() {
		let path = scratch("newer");
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		{
			let connection = Connection::open(&path).unwrap();
			connection.pragma_update(None, "user_version", 99).unwrap();
		}
		assert!(Store::open(&path).is_err());
	}
}
