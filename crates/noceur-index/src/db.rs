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
    Scan(String),
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
        for expected in ["volume", "dir", "file", "content", "content_tag"] {
            assert!(
                t.contains(&expected.to_string()),
                "missing table {expected}"
            );
        }
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
}
