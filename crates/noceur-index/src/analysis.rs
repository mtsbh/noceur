//! Tempo and key as measured, beside what the tags say.
//!
//! Only storage. Measuring is `noceur-audio`'s, and deciding what to do with a
//! measurement — write it, show it, leave it — is the app's.

use crate::db::{Db, IndexError};
use rusqlite::{params, OptionalExtension};

/// What was measured for one content.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Detected {
    pub bpm: Option<f64>,
    /// Canonical key, 0..23, as `MusicalKey::canonical` numbers it.
    pub key_norm: Option<i64>,
    pub key_confidence: Option<f64>,
}

/// How a measurement was taken. A stored row taken any other way is stale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Method {
    pub version: i64,
    pub range_min: f64,
    pub range_max: f64,
}

/// One stored row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stored {
    pub method: Method,
    /// `None` when the file would not decode.
    pub detected: Option<Detected>,
}

impl Stored {
    /// Whether this row answers for `method`, so the file need not be measured
    /// again.
    pub fn is_current(&self, method: &Method) -> bool {
        self.method == *method
    }
}

/// The stored measurement for one content, if any.
pub fn get(db: &Db, content_id: i64) -> Result<Option<Stored>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT version, range_min, range_max, bpm, key_norm, key_confidence, miss
             FROM analysis WHERE content_id = ?1",
            params![content_id],
            |r| {
                let miss: i64 = r.get(6)?;
                Ok(Stored {
                    method: Method {
                        version: r.get(0)?,
                        range_min: r.get(1)?,
                        range_max: r.get(2)?,
                    },
                    detected: (miss == 0)
                        .then(|| {
                            Ok::<_, rusqlite::Error>(Detected {
                                bpm: r.get(3)?,
                                key_norm: r.get(4)?,
                                key_confidence: r.get(5)?,
                            })
                        })
                        .transpose()?,
                })
            },
        )
        .optional()?)
}

/// Records a measurement, or with `None` that the file would not decode.
pub fn set(
    db: &Db,
    content_id: i64,
    method: &Method,
    detected: Option<&Detected>,
) -> Result<(), IndexError> {
    let d = detected.copied().unwrap_or_default();
    db.conn().execute(
        "INSERT INTO analysis
             (content_id, version, range_min, range_max, bpm, key_norm, key_confidence, miss)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(content_id) DO UPDATE SET
             version = excluded.version, range_min = excluded.range_min,
             range_max = excluded.range_max, bpm = excluded.bpm,
             key_norm = excluded.key_norm, key_confidence = excluded.key_confidence,
             miss = excluded.miss",
        params![
            content_id,
            method.version,
            method.range_min,
            method.range_max,
            d.bpm,
            d.key_norm,
            d.key_confidence,
            detected.is_none() as i64,
        ],
    )?;
    Ok(())
}

/// Moves a measurement to the content a tag write turned the file into.
///
/// Only for a write that touched tags and not audio. Leaves the destination
/// alone if it already has one.
pub fn carry(db: &Db, from: i64, to: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO analysis
             (content_id, version, range_min, range_max, bpm, key_norm, key_confidence, miss)
         SELECT ?2, version, range_min, range_max, bpm, key_norm, key_confidence, miss
         FROM analysis WHERE content_id = ?1
         ON CONFLICT(content_id) DO NOTHING",
        params![from, to],
    )?;
    Ok(())
}

/// Integrated loudness and true peak for one content.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Loudness {
    pub lufs: Option<f64>,
    /// dBTP.
    pub true_peak: Option<f64>,
    /// The true peak is a lower bound: the decoder clipped at full scale.
    pub peak_floor: bool,
}

/// Records a loudness measurement.
pub fn set_loudness(db: &Db, content_id: i64, l: &Loudness) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO loudness (content_id, lufs, true_peak, peak_floor)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(content_id) DO UPDATE SET
             lufs = excluded.lufs, true_peak = excluded.true_peak,
             peak_floor = excluded.peak_floor",
        params![content_id, l.lufs, l.true_peak, l.peak_floor as i64],
    )?;
    Ok(())
}

/// The stored loudness of one content, if it has been measured.
pub fn loudness(db: &Db, content_id: i64) -> Result<Option<Loudness>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT lufs, true_peak, peak_floor FROM loudness WHERE content_id = ?1",
            params![content_id],
            |r| {
                Ok(Loudness {
                    lufs: r.get(0)?,
                    true_peak: r.get(1)?,
                    peak_floor: r.get::<_, i64>(2)? != 0,
                })
            },
        )
        .optional()?)
}

/// [`carry`], for loudness.
pub fn carry_loudness(db: &Db, from: i64, to: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO loudness (content_id, lufs, true_peak, peak_floor)
         SELECT ?2, lufs, true_peak, peak_floor FROM loudness WHERE content_id = ?1
         ON CONFLICT(content_id) DO NOTHING",
        params![from, to],
    )?;
    Ok(())
}

/// Records what Check quality found broken, empty when nothing was.
pub fn set_damage(db: &Db, content_id: i64, summary: &str) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO damage (content_id, summary) VALUES (?1, ?2)
         ON CONFLICT(content_id) DO UPDATE SET summary = excluded.summary",
        params![content_id, summary],
    )?;
    Ok(())
}

/// The stored damage summary of one content, if it has been checked.
pub fn damage(db: &Db, content_id: i64) -> Result<Option<String>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT summary FROM damage WHERE content_id = ?1",
            params![content_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// [`carry`], for damage.
pub fn carry_damage(db: &Db, from: i64, to: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO damage (content_id, summary)
         SELECT ?2, summary FROM damage WHERE content_id = ?1
         ON CONFLICT(content_id) DO NOTHING",
        params![from, to],
    )?;
    Ok(())
}

/// The cue points stored for one content: the finder's version and its text.
pub fn cues(db: &Db, content_id: i64) -> Result<Option<(i64, String)>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT version, data FROM cues WHERE content_id = ?1",
            params![content_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Records the cue points found for one content.
pub fn set_cues(db: &Db, content_id: i64, version: i64, data: &str) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO cues (content_id, version, data) VALUES (?1, ?2, ?3)
         ON CONFLICT(content_id) DO UPDATE SET version = excluded.version, data = excluded.data",
        params![content_id, version, data],
    )?;
    Ok(())
}

/// [`carry`], for cue points.
pub fn carry_cues(db: &Db, from: i64, to: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO cues (content_id, version, data)
         SELECT ?2, version, data FROM cues WHERE content_id = ?1
         ON CONFLICT(content_id) DO NOTHING",
        params![from, to],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_round_trip_and_follow_a_tag_write() {
        let db = db();
        assert_eq!(cues(&db, 1).unwrap(), None);
        set_cues(&db, 1, 1, "128 0.2\n0.2 Intro").unwrap();
        set_cues(&db, 1, 2, "128 0.2\n0.2 Intro\n60.2 Drop").unwrap();
        assert_eq!(
            cues(&db, 1).unwrap(),
            Some((2, "128 0.2\n0.2 Intro\n60.2 Drop".to_string()))
        );
        carry_cues(&db, 1, 3).unwrap();
        assert_eq!(cues(&db, 3).unwrap().unwrap().0, 2);
    }

    const METHOD: Method = Method {
        version: 1,
        range_min: 70.0,
        range_max: 180.0,
    };

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO volume (id, kind, fs_uuid, mount_path)
                 VALUES (1, 'fixed', 'u1', 'E:\\');
                 INSERT INTO dir (id, volume_id, name, path_lower)
                 VALUES (1, 1, 'music', 'e:\\music');
                 INSERT INTO content (id, quick_key) VALUES (1, 'k1'), (2, 'k2'), (3, 'k3');
                 INSERT INTO content_tag (content_id, bpm_tag, key_norm) VALUES
                     (1, 124.0, 14), (2, NULL, NULL);
                 INSERT INTO file (id, dir_id, name, kind, content_id) VALUES
                     (1, 1, 'b.wav', 1, 1),
                     (2, 1, 'a.wav', 1, 2),
                     (3, 1, 'cover.jpg', 2, NULL),
                     (4, 1, 'c.wav', 1, NULL);",
            )
            .unwrap();
        db
    }

    #[test]
    fn a_measurement_round_trips() {
        let db = db();
        assert_eq!(get(&db, 1).unwrap(), None);
        let d = Detected {
            bpm: Some(125.0),
            key_norm: Some(15),
            key_confidence: Some(0.4),
        };
        set(&db, 1, &METHOD, Some(&d)).unwrap();
        let stored = get(&db, 1).unwrap().unwrap();
        assert_eq!(stored.detected, Some(d));
        assert!(stored.is_current(&METHOD));
    }

    /// Half and double time are settled by the range, so a row measured with
    /// another one does not answer for this one.
    #[test]
    fn another_range_or_model_is_stale() {
        let db = db();
        set(&db, 1, &METHOD, Some(&Detected::default())).unwrap();
        let stored = get(&db, 1).unwrap().unwrap();
        assert!(!stored.is_current(&Method {
            range_max: 200.0,
            ..METHOD
        }));
        assert!(!stored.is_current(&Method {
            version: 2,
            ..METHOD
        }));
    }

    #[test]
    fn a_file_that_would_not_decode_is_remembered_as_such() {
        let db = db();
        set(&db, 2, &METHOD, None).unwrap();
        let stored = get(&db, 2).unwrap().unwrap();
        assert_eq!(stored.detected, None);
        assert!(stored.is_current(&METHOD), "not measured again");
    }

    /// A tag write lands the file on a new content row; the measurement of
    /// audio it did not touch goes with it.
    #[test]
    fn a_measurement_follows_a_tag_write() {
        let db = db();
        let d = Detected {
            bpm: Some(128.0),
            ..Default::default()
        };
        set(&db, 1, &METHOD, Some(&d)).unwrap();
        carry(&db, 1, 3).unwrap();
        assert_eq!(get(&db, 3).unwrap().unwrap().detected, Some(d));
    }

    #[test]
    fn loudness_round_trips_and_follows_a_tag_write() {
        let db = db();
        assert_eq!(loudness(&db, 1).unwrap(), None);
        let l = Loudness {
            lufs: Some(-9.4),
            true_peak: Some(0.6),
            peak_floor: true,
        };
        set_loudness(&db, 1, &l).unwrap();
        assert_eq!(loudness(&db, 1).unwrap(), Some(l));
        carry_loudness(&db, 1, 3).unwrap();
        assert_eq!(loudness(&db, 3).unwrap(), Some(l));
    }

    #[test]
    fn damage_round_trips_and_follows_a_tag_write() {
        let db = db();
        assert_eq!(damage(&db, 1).unwrap(), None, "not checked");
        set_damage(&db, 1, "").unwrap();
        assert_eq!(
            damage(&db, 1).unwrap().as_deref(),
            Some(""),
            "checked, sound"
        );
        set_damage(&db, 1, "1 dropout").unwrap();
        carry_damage(&db, 1, 3).unwrap();
        assert_eq!(damage(&db, 3).unwrap().as_deref(), Some("1 dropout"));
    }
}
