//! Database handle and migration application.

use crate::schema::{MIGRATIONS, SCHEMA_VERSION};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("index is newer than this build: found v{found}, support v{supported}")]
    TooNew { found: i32, supported: i32 },
    #[error("scan failed: {0}")]
    Scan(#[from] noceur_fsmodel::EnumerateError),
}

/// An open index database.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Opens (creating if needed) the index at `path` and applies migrations.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        // `PRAGMA journal_mode` returns a row, so it must go through
        // `execute_batch` — `pragma_update` errors on result-returning pragmas.
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Self::init(conn)
    }

    /// Opens a second, read-only connection to an index that [`Db::open`] has
    /// already created and migrated.
    ///
    /// The index is in WAL mode, so this connection reads the last committed
    /// state while the main one is busy writing: a search never queues behind a
    /// folder walk or a probe batch.
    pub fn open_reader(path: &Path) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "query_only", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Db { conn })
    }

    /// Opens a throwaway in-memory index. Used by tests.
    pub fn open_in_memory() -> Result<Self, IndexError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, IndexError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Current schema version stored in `user_version`.
    pub fn version(&self) -> Result<i32, IndexError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Applies any migrations the file has not seen yet.
    fn migrate(&self) -> Result<(), IndexError> {
        let current = self.version()?;
        if current > SCHEMA_VERSION {
            return Err(IndexError::TooNew {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = i as i32 + 1;
            if current < target {
                self.conn.execute_batch(sql)?;
                self.conn.pragma_update(None, "user_version", target)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tables(db: &Db) -> Vec<String> {
        let mut stmt = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    }

    #[test]
    fn creates_every_table() {
        let db = Db::open_in_memory().unwrap();
        let t = tables(&db);
        for expected in ["volume", "dir", "file", "content", "content_tag", "thumb"] {
            assert!(
                t.contains(&expected.to_string()),
                "missing table {expected}"
            );
        }
    }

    /// An index written by an earlier build must gain new tables in place, not
    /// send the app to the startup error box. There are already v1 files on disk.
    #[test]
    fn an_older_index_is_upgraded_in_place() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");

        // A v1 file: first migration only, version pinned where v1 left it.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(crate::schema::MIGRATIONS[0]).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute("INSERT INTO content (id, quick_key) VALUES (1, 'keep')", [])
                .unwrap();
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);
        assert!(tables(&db).contains(&"thumb".to_string()));

        let kept: String = db
            .conn()
            .query_row("SELECT quick_key FROM content WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(kept, "keep", "existing rows must survive the upgrade");
    }

    /// A library browsed before search existed must be searchable the moment the
    /// build that has search opens it — not only the folders reopened afterwards.
    #[test]
    fn upgrading_backfills_the_search_index() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");

        // A v3 file: everything up to and including the empty FTS table.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            for sql in &crate::schema::MIGRATIONS[..3] {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", 3).unwrap();
            conn.execute(
                "INSERT INTO volume (id, kind, fs_uuid, label, mount_path)
                 VALUES (1, 'fixed', 'u', 'l', 'e:\\')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO dir (id, volume_id, name, path_lower)
                 VALUES (1, 1, 'music', 'e:\\music')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO file (id, dir_id, name, kind) VALUES (1, 1, 'Kindness.wav', 1)",
                [],
            )
            .unwrap();

            let indexed: i64 = conn
                .query_row("SELECT COUNT(*) FROM file_fts", [], |r| r.get(0))
                .unwrap();
            assert_eq!(indexed, 0, "v3 leaves the search index empty");
        }

        let db = Db::open(&path).unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);

        let hits = crate::search::search(&db, "Kindness", &[], 10).unwrap();
        assert_eq!(hits.len(), 1, "the existing file must have been backfilled");
        assert_eq!(hits[0].row.name, "Kindness.wav");
    }

    #[test]
    fn records_the_schema_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_is_idempotent() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");

        let first = Db::open(&path).unwrap();
        let before = tables(&first);
        drop(first);

        let second = Db::open(&path).unwrap();
        assert_eq!(tables(&second), before);
        assert_eq!(second.version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn enables_foreign_keys() {
        let db = Db::open_in_memory().unwrap();
        let on: i32 = db
            .conn()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn refuses_an_index_from_a_newer_build() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");
        {
            let db = Db::open(&path).unwrap();
            db.conn()
                .pragma_update(None, "user_version", SCHEMA_VERSION + 5)
                .unwrap();
        }
        assert!(matches!(Db::open(&path), Err(IndexError::TooNew { .. })));
    }

    /// Search reads on its own connection; a write in progress on the main one
    /// must neither block it nor show it half-done work.
    #[test]
    fn the_reader_reads_while_the_writer_is_mid_transaction() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");
        let writer = Db::open(&path).unwrap();
        let reader = Db::open_reader(&path).unwrap();

        writer
            .conn()
            .execute_batch(
                "BEGIN IMMEDIATE;
                 INSERT INTO volume (id, kind, fs_uuid, mount_path)
                 VALUES (1, 'fixed', 'u1', 'E:\\');",
            )
            .unwrap();
        let count = || -> i64 {
            reader
                .conn()
                .query_row("SELECT count(*) FROM volume", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(count(), 0);
        writer.conn().execute_batch("COMMIT").unwrap();
        assert_eq!(count(), 1);

        assert!(
            reader.conn().execute("DELETE FROM volume", []).is_err(),
            "the reader never writes"
        );
    }
}
