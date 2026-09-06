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
pub const VERSION: i64 = 3;

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
					error TEXT,
					connections INTEGER,
					directory TEXT,
					mirrors TEXT,
					checksum TEXT,
					range TEXT,
					speed_limit INTEGER
				);",
			)?;
			connection.pragma_update(None, "user_version", VERSION)?;
		}
		// Each later version adds an arm here, from n to n + 1; the arms are never removed.
		if version == 1 {
			// How many connections a row was asked for at Add Task; NULL is the engine's own.
			connection.execute_batch("ALTER TABLE downloads ADD COLUMN connections INTEGER;")?;
			connection.pragma_update(None, "user_version", 2)?;
		}
		if version <= 2 && version != 0 {
			// The rest of what Add Task can ask for; mirrors as a JSON list of addresses.
			connection.execute_batch(
				"ALTER TABLE downloads ADD COLUMN directory TEXT;
				 ALTER TABLE downloads ADD COLUMN mirrors TEXT;
				 ALTER TABLE downloads ADD COLUMN checksum TEXT;
				 ALTER TABLE downloads ADD COLUMN range TEXT;
				 ALTER TABLE downloads ADD COLUMN speed_limit INTEGER;",
			)?;
			connection.pragma_update(None, "user_version", 3)?;
		}
		Ok(Store { connection })
	}

	/// Every row, oldest first.
	pub fn load(&self) -> Result<Vec<Download>> {
		let mut statement = self.connection.prepare(
			"SELECT id, name, url, source, size, received, status, added, path, error, connections,
			        directory, mirrors, checksum, range, speed_limit
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
				connections: row.get::<_, Option<i64>>(10)?.map(|n| n as u16),
				directory: row.get(11)?,
				mirrors: row
					.get::<_, Option<String>>(12)?
					.and_then(|text| serde_json::from_str(&text).ok())
					.unwrap_or_default(),
				checksum: row.get(13)?,
				range: row.get(14)?,
				speed_limit: row.get::<_, Option<i64>>(15)?.map(|n| n as u64),
			})
		})?;
		rows.map(|r| r.context("read a download")).collect()
	}

	/// Writes the row, new or changed. Speed is not kept: it is a number about now.
	pub fn save(&self, download: &Download) -> Result<()> {
		self.connection.execute(
			"INSERT INTO downloads (id, name, url, source, size, received, status, added, path, error, connections,
			                        directory, mirrors, checksum, range, speed_limit)
			 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
			 ON CONFLICT(id) DO UPDATE SET
				name = excluded.name, url = excluded.url, source = excluded.source,
				size = excluded.size, received = excluded.received, status = excluded.status,
				added = excluded.added, path = excluded.path, error = excluded.error,
				connections = excluded.connections, directory = excluded.directory,
				mirrors = excluded.mirrors, checksum = excluded.checksum, range = excluded.range,
				speed_limit = excluded.speed_limit",
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
				download.connections.map(|n| n as i64),
				download.directory,
				(!download.mirrors.is_empty()).then(|| serde_json::to_string(&download.mirrors).unwrap_or_default()),
				download.checksum,
				download.range,
				download.speed_limit.map(|n| n as i64),
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
		crate::testing::scratch(name).join("internal.sqlite")
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
			connections: Some(8),
			directory: None,
			mirrors: vec!["https://m/file.bin".into()],
			checksum: Some("md5:d41d8cd98f00b204e9800998ecf8427e".into()),
			range: Some("100-".into()),
			speed_limit: Some(4096),
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
	fn a_version_one_database_gains_the_connections_column_and_reads_none_for_old_rows() {
		let path = scratch("migrate");
		{
			let connection = Connection::open(&path).unwrap();
			connection
				.execute_batch(
					"CREATE TABLE downloads (id INTEGER PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL,
					 source TEXT, size INTEGER NOT NULL, received INTEGER NOT NULL, status TEXT NOT NULL,
					 added TEXT NOT NULL, path TEXT, error TEXT);
					 INSERT INTO downloads VALUES (1, 'a', 'https://h/a', NULL, 1, 0, 'Queued', '2026-01-01T00:00:00+00:00', NULL, NULL);",
				)
				.unwrap();
			connection.pragma_update(None, "user_version", 1).unwrap();
		}
		let store = Store::open(&path).unwrap();
		let rows = store.load().unwrap();
		assert_eq!((rows.len(), rows[0].connections), (1, None));
		store.save(&row(2, Status::Queued)).unwrap();
		let kept = &store.load().unwrap()[1];
		assert_eq!((kept.connections, kept.speed_limit), (Some(8), Some(4096)));
		assert_eq!((kept.mirrors.len(), kept.range.as_deref()), (1, Some("100-")));
		let version: i64 =
			store.connection.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
		assert_eq!(version, VERSION);
	}

	#[test]
	fn a_newer_database_is_refused() {
		let path = scratch("newer");
		{
			let connection = Connection::open(&path).unwrap();
			connection.pragma_update(None, "user_version", 99).unwrap();
		}
		assert!(Store::open(&path).is_err());
	}
}
