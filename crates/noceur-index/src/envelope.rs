//! Persistent cache of waveform envelopes.
//!
//! An envelope is the most expensive thing derived from a file: computing one
//! decodes the track from end to end, which for a long lossless file is close to
//! a second of work. A transport bar that recomputed it every time a track was
//! loaded would make re-auditioning a track feel worse than playing it the first
//! time.

use crate::db::{Db, IndexError};
use rusqlite::{params, OptionalExtension};

/// What the cache knows about one content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cached {
    /// An envelope: interleaved bands, laid out bucket by bucket.
    Envelope(Vec<u8>),
    /// Extraction was tried and failed — a file the decoder cannot read.
    /// Recorded so the slowest outcome is not the one paid for repeatedly.
    Miss,
    /// Never attempted, or attempted at a different resolution.
    Unknown,
}

/// Reads what the cache holds for one content at one resolution.
///
/// A row stored at some other bucket count reads as `Unknown` rather than being
/// rescaled: downsampling to a coarser envelope would be defensible, but
/// stretching a coarse one onto a finer grid would invent detail that was never
/// in the audio.
pub fn get(db: &Db, content_id: i64, buckets: usize, bands: usize) -> Result<Cached, IndexError> {
    let row: Option<(i64, Option<Vec<u8>>, i64)> = db
        .conn()
        .query_row(
            "SELECT buckets, data, miss FROM envelope WHERE content_id = ?1",
            params![content_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;

    Ok(match row {
        Some((_, _, miss)) if miss != 0 => Cached::Miss,
        // The length is checked as well as the bucket count, so a row written by
        // a build with a different number of bands is missed rather than drawn
        // with its bands read in the wrong places.
        Some((n, Some(data), _)) if n == buckets as i64 && data.len() == buckets * bands => {
            Cached::Envelope(data)
        }
        _ => Cached::Unknown,
    })
}

/// Records an envelope, or the fact that one could not be extracted.
pub fn put(
    db: &Db,
    content_id: i64,
    buckets: usize,
    data: Option<&[u8]>,
) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO envelope (content_id, buckets, data, miss) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(content_id) DO UPDATE SET
             buckets = excluded.buckets, data = excluded.data, miss = excluded.miss",
        params![content_id, buckets as i64, data, i64::from(data.is_none())],
    )?;
    Ok(())
}

/// Drops the cached envelope for one content, so an edited file re-extracts.
pub fn invalidate(db: &Db, content_id: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "DELETE FROM envelope WHERE content_id = ?1",
        params![content_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two bands, matching `noceur_audio::ENVELOPE_BANDS`. Spelled out here so
    /// these tests describe the layout rather than depending on the audio crate.
    const BANDS: usize = 2;

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
        assert_eq!(get(&db(), 1, 2, BANDS).unwrap(), Cached::Unknown);
    }

    #[test]
    fn a_stored_envelope_comes_back_unchanged() {
        let db = db();
        put(&db, 1, 2, Some(&[0, 128, 255, 7])).unwrap();
        assert_eq!(
            get(&db, 1, 2, BANDS).unwrap(),
            Cached::Envelope(vec![0, 128, 255, 7])
        );
    }

    /// The point of recording a miss: an undecodable file must not cost a full
    /// decode attempt every time it is selected.
    #[test]
    fn a_recorded_miss_is_distinguishable_from_never_having_looked() {
        let db = db();
        put(&db, 1, 2, None).unwrap();
        assert_eq!(get(&db, 1, 2, BANDS).unwrap(), Cached::Miss);
        assert_eq!(get(&db, 2, 2, BANDS).unwrap(), Cached::Unknown);
    }

    /// A miss is a property of the file, not of the resolution asked for: a
    /// track that will not decode will not decode at any bucket count.
    #[test]
    fn a_miss_reads_as_a_miss_at_any_resolution() {
        let db = db();
        put(&db, 1, 2, None).unwrap();
        assert_eq!(get(&db, 1, 2000, BANDS).unwrap(), Cached::Miss);
    }

    /// Changing `ENVELOPE_BUCKETS` must not draw old envelopes at the new scale.
    #[test]
    fn an_envelope_stored_at_another_resolution_is_not_returned() {
        let db = db();
        put(&db, 1, 2, Some(&[1, 2, 3, 4])).unwrap();
        assert_eq!(get(&db, 1, 8, BANDS).unwrap(), Cached::Unknown);
    }

    /// Adding or removing a band changes what every byte means, so old rows must
    /// miss rather than be drawn with rms read as peak.
    #[test]
    fn an_envelope_stored_with_another_band_count_is_not_returned() {
        let db = db();
        put(&db, 1, 2, Some(&[1, 2, 3, 4])).unwrap();
        assert_eq!(get(&db, 1, 2, 1).unwrap(), Cached::Unknown);
        assert_eq!(get(&db, 1, 2, 3).unwrap(), Cached::Unknown);
    }

    #[test]
    fn re_extracting_replaces_a_recorded_miss() {
        let db = db();
        put(&db, 1, 1, None).unwrap();
        put(&db, 1, 1, Some(&[5, 6])).unwrap();
        assert_eq!(get(&db, 1, 1, BANDS).unwrap(), Cached::Envelope(vec![5, 6]));
    }

    #[test]
    fn invalidate_affects_one_content_only() {
        let db = db();
        put(&db, 1, 1, Some(&[1, 1])).unwrap();
        put(&db, 2, 1, Some(&[2, 2])).unwrap();
        invalidate(&db, 1).unwrap();
        assert_eq!(get(&db, 1, 1, BANDS).unwrap(), Cached::Unknown);
        assert_eq!(get(&db, 2, 1, BANDS).unwrap(), Cached::Envelope(vec![2, 2]));
    }
}
