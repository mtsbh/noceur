//! Persistent cache of rendered cover-art thumbnails.
//!
//! The index stores the rendered bytes rather than re-deriving them, because a
//! thumbnail costs a tag read plus a JPEG decode plus a resize, and a list that
//! scrolls asks for the same rows over and over.

use crate::db::{Db, IndexError};
use rusqlite::{params, OptionalExtension};
use std::path::PathBuf;

/// What the cache knows about one (content, size) pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// A rendered thumbnail.
    Png(Vec<u8>),
    /// Rendering was tried and produced nothing — no artwork, or artwork that
    /// would not decode. Recorded so the failure is not retried on every scroll.
    Miss,
    /// Never attempted.
    Unknown,
}

/// Where a row lives, and what it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// `None` for a directory, and for a file no probe has linked yet.
    pub content_id: Option<i64>,
    pub path: PathBuf,
    /// `EntryKind::group()`: 0 directory, 1 audio, 2 other.
    pub kind: i64,
    /// What the last probe saw. `false` means skip the decode entirely.
    pub has_artwork: bool,
}

/// Resolves a row to its path and what it carries.
///
/// Deliberately does not require a linked content: a directory has no content but
/// still has an icon to draw, so the caller decides what to do with each kind.
pub fn locate(db: &Db, file_id: i64) -> Result<Option<Located>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT f.content_id, d.path_lower, f.name, f.kind,
                    COALESCE(ct.has_artwork, 0)
             FROM file f
             JOIN dir d ON d.id = f.dir_id
             LEFT JOIN content_tag ct ON ct.content_id = f.content_id
             WHERE f.id = ?1",
            params![file_id],
            |r| {
                let dir: String = r.get(1)?;
                let name: String = r.get(2)?;
                let has: i64 = r.get(4)?;
                Ok(Located {
                    content_id: r.get(0)?,
                    path: PathBuf::from(dir).join(name),
                    kind: r.get(3)?,
                    has_artwork: has != 0,
                })
            },
        )
        .optional()?)
}

/// Reads what the cache holds for one content at one size.
pub fn get(db: &Db, content_id: i64, size: &str) -> Result<Cached, IndexError> {
    let row: Option<(Option<Vec<u8>>, i64)> = db
        .conn()
        .query_row(
            "SELECT png, miss FROM thumb WHERE content_id = ?1 AND size = ?2",
            params![content_id, size],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    Ok(match row {
        Some((Some(png), 0)) => Cached::Png(png),
        Some(_) => Cached::Miss,
        None => Cached::Unknown,
    })
}

/// Records a rendered thumbnail, or the fact that there is nothing to render.
///
/// Upserts rather than inserts so a file that gains artwork after a retag
/// replaces its recorded miss instead of colliding with it.
pub fn put(db: &Db, content_id: i64, size: &str, png: Option<&[u8]>) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO thumb (content_id, size, png, miss) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(content_id, size) DO UPDATE SET
             png = excluded.png, miss = excluded.miss",
        params![content_id, size, png, i64::from(png.is_none())],
    )?;
    Ok(())
}

/// Drops every cached size for one content, so a retagged file re-renders.
pub fn invalidate(db: &Db, content_id: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "DELETE FROM thumb WHERE content_id = ?1",
        params![content_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute(
                "INSERT INTO content (id, quick_key) VALUES (1, 'k1'), (2, 'k2')",
                [],
            )
            .unwrap();
        db
    }

    #[test]
    fn an_unseen_pair_is_unknown() {
        let db = db();
        assert_eq!(get(&db, 1, "small").unwrap(), Cached::Unknown);
    }

    #[test]
    fn stored_bytes_come_back_unchanged() {
        let db = db();
        put(&db, 1, "medium", Some(&[1, 2, 3, 4])).unwrap();
        assert_eq!(
            get(&db, 1, "medium").unwrap(),
            Cached::Png(vec![1, 2, 3, 4])
        );
    }

    /// The point of recording a miss: a file with no art must not be decoded
    /// again every time it scrolls past.
    #[test]
    fn a_recorded_miss_is_distinguishable_from_never_having_looked() {
        let db = db();
        put(&db, 1, "small", None).unwrap();
        assert_eq!(get(&db, 1, "small").unwrap(), Cached::Miss);
        assert_eq!(
            get(&db, 2, "small").unwrap(),
            Cached::Unknown,
            "another content must be unaffected"
        );
    }

    #[test]
    fn sizes_are_cached_independently() {
        let db = db();
        put(&db, 1, "small", Some(&[9])).unwrap();
        assert_eq!(get(&db, 1, "small").unwrap(), Cached::Png(vec![9]));
        assert_eq!(get(&db, 1, "large").unwrap(), Cached::Unknown);
    }

    /// A file that gains artwork after a retag must be able to replace its miss.
    #[test]
    fn re_rendering_replaces_a_recorded_miss() {
        let db = db();
        put(&db, 1, "small", None).unwrap();
        put(&db, 1, "small", Some(&[7, 7])).unwrap();
        assert_eq!(get(&db, 1, "small").unwrap(), Cached::Png(vec![7, 7]));
    }

    #[test]
    fn invalidate_clears_every_size_for_one_content_only() {
        let db = db();
        for size in ["small", "medium", "large"] {
            put(&db, 1, size, Some(&[1])).unwrap();
            put(&db, 2, size, Some(&[2])).unwrap();
        }
        invalidate(&db, 1).unwrap();

        for size in ["small", "medium", "large"] {
            assert_eq!(get(&db, 1, size).unwrap(), Cached::Unknown);
            assert_eq!(get(&db, 2, size).unwrap(), Cached::Png(vec![2]));
        }
    }
}
