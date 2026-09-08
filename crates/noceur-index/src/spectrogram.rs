//! Persistent cache of spectrograms.
//!
//! The same arrangement as the envelope next door, for something bigger and
//! costlier: a spectrogram is a full decode plus several thousand transforms,
//! and sixty kilobytes of result rather than eight. Worth caching for exactly
//! that reason.
//!
//! Keyed by the shape as well as the content, because a grid measured at one
//! size cannot be drawn at another without inventing detail it never had.

use crate::db::{Db, IndexError};
use rusqlite::{params, OptionalExtension};

/// What the cache knows about one content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// A grid: `columns * bands` bytes, column by column.
    Grid(Vec<u8>),
    /// Measurement was tried and failed — a file the decoder cannot read.
    /// Recorded so the slowest outcome is not the one paid for repeatedly.
    Miss,
    /// Never attempted, or attempted at a different resolution.
    Unknown,
}

/// Reads what the cache holds for one content at one resolution.
pub fn get(db: &Db, content_id: i64, columns: usize, bands: usize) -> Result<Cached, IndexError> {
    let row: Option<(i64, i64, Option<Vec<u8>>, i64)> = db
        .conn()
        .query_row(
            "SELECT columns, bands, data, miss FROM spectrogram WHERE content_id = ?1",
            params![content_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;

    Ok(match row {
        Some((_, _, _, miss)) if miss != 0 => Cached::Miss,
        // The length is checked as well as both dimensions, so a row written by
        // a build that changed either is missed rather than drawn with its
        // columns read as rows.
        Some((c, b, Some(data), _))
            if c == columns as i64 && b == bands as i64 && data.len() == columns * bands =>
        {
            Cached::Grid(data)
        }
        _ => Cached::Unknown,
    })
}

/// Records a spectrogram, or the fact that one could not be measured.
pub fn put(
    db: &Db,
    content_id: i64,
    columns: usize,
    bands: usize,
    data: Option<&[u8]>,
) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO spectrogram (content_id, columns, bands, data, miss)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(content_id) DO UPDATE SET
             columns = excluded.columns, bands = excluded.bands,
             data = excluded.data, miss = excluded.miss",
        params![
            content_id,
            columns as i64,
            bands as i64,
            data,
            i64::from(data.is_none())
        ],
    )?;
    Ok(())
}

/// Drops the cached grid for one content, so an edited file is measured again.
pub fn invalidate(db: &Db, content_id: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "DELETE FROM spectrogram WHERE content_id = ?1",
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
    fn an_unseen_content_is_unknown() {
        assert_eq!(get(&db(), 1, 2, 2).unwrap(), Cached::Unknown);
    }

    #[test]
    fn a_stored_grid_comes_back_unchanged() {
        let db = db();
        put(&db, 1, 2, 2, Some(&[1, 2, 3, 4])).unwrap();
        assert_eq!(get(&db, 1, 2, 2).unwrap(), Cached::Grid(vec![1, 2, 3, 4]));
    }

    /// The point of recording a miss: a file that will not decode must not cost
    /// a full decode attempt every time it is looked at.
    #[test]
    fn a_recorded_miss_is_distinguishable_from_never_having_looked() {
        let db = db();
        put(&db, 1, 2, 2, None).unwrap();
        assert_eq!(get(&db, 1, 2, 2).unwrap(), Cached::Miss);
        assert_eq!(get(&db, 2, 2, 2).unwrap(), Cached::Unknown);
    }

    /// Changing either dimension changes what every byte means, so an old row
    /// must miss rather than be drawn with its columns read as rows.
    #[test]
    fn a_grid_stored_at_another_shape_is_not_returned() {
        let db = db();
        put(&db, 1, 2, 2, Some(&[1, 2, 3, 4])).unwrap();
        assert_eq!(get(&db, 1, 4, 2).unwrap(), Cached::Unknown, "more columns");
        assert_eq!(get(&db, 1, 2, 4).unwrap(), Cached::Unknown, "more bands");
    }

    /// A row whose blob does not match the shape it claims is corrupt, and
    /// drawing it would produce a picture of nothing in particular.
    #[test]
    fn a_row_whose_size_disagrees_with_its_shape_is_ignored() {
        let db = db();
        put(&db, 1, 8, 8, Some(&[1, 2, 3])).unwrap();
        assert_eq!(get(&db, 1, 8, 8).unwrap(), Cached::Unknown);
    }

    #[test]
    fn re_measuring_replaces_a_recorded_miss() {
        let db = db();
        put(&db, 1, 1, 2, None).unwrap();
        put(&db, 1, 1, 2, Some(&[5, 6])).unwrap();
        assert_eq!(get(&db, 1, 1, 2).unwrap(), Cached::Grid(vec![5, 6]));
    }

    #[test]
    fn invalidate_affects_one_content_only() {
        let db = db();
        put(&db, 1, 1, 2, Some(&[1, 1])).unwrap();
        put(&db, 2, 1, 2, Some(&[2, 2])).unwrap();
        invalidate(&db, 1).unwrap();
        assert_eq!(get(&db, 1, 1, 2).unwrap(), Cached::Unknown);
        assert_eq!(get(&db, 2, 1, 2).unwrap(), Cached::Grid(vec![2, 2]));
    }
}
