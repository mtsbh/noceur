//! The library at a glance: counts and spreads over everything indexed.
//!
//! Read straight from the index, so it costs queries rather than decodes and
//! covers exactly the folders Noceur has opened. A measured BPM or key stands in
//! where the file has no tag, as it does in the list.

use crate::db::{Db, IndexError};
use rusqlite::params_from_iter;

/// One value and how many tracks carry it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    pub value: String,
    pub count: i64,
}

/// A folder and what is wrong in it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemFolder {
    pub path: String,
    pub tracks: i64,
    /// Check quality found it broken.
    pub damaged: i64,
    /// Could not be opened or read at all.
    pub unreadable: i64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub tracks: i64,
    pub folders: i64,
    pub seconds: i64,
    pub bytes: i64,
    /// Whole BPM and how many tracks sit on it, ascending.
    pub bpm: Vec<(i64, i64)>,
    /// Tracks per key, indexed by `MusicalKey::canonical`.
    pub keys: Vec<i64>,
    pub formats: Vec<Tally>,
    pub sample_rates: Vec<Tally>,
    /// Integrated loudness rounded to whole LUFS, ascending.
    pub lufs: Vec<(i64, i64)>,
    pub checked: i64,
    pub damaged: i64,
    pub unreadable: i64,
    pub no_artist: i64,
    pub no_title: i64,
    pub no_genre: i64,
    pub no_bpm: i64,
    pub no_key: i64,
    pub no_art: i64,
    pub labels: Vec<Tally>,
    pub genres: Vec<Tally>,
    pub artists: Vec<Tally>,
    pub problem_folders: Vec<ProblemFolder>,
}

/// How many entries the top lists keep.
const TOP: i64 = 12;

/// The tracks in scope, as a CTE every query below starts from.
///
/// One row per audio file, not per content: two copies of a track are two
/// tracks on two drives, and the counts should match what the list shows.
const TRACKS: &str = "
    WITH t AS (
        SELECT f.id, f.dir_id, f.state, f.size,
               c.duration_ms, c.sample_rate, c.codec, c.bit_depth,
               ct.artist, ct.title, ct.genre, ct.label, ct.has_artwork,
               COALESCE(ct.bpm_tag, a.bpm)       AS bpm,
               COALESCE(ct.key_norm, a.key_norm) AS key_norm,
               l.lufs,
               dm.summary AS damage,
               (l.content_id IS NOT NULL OR dm.content_id IS NOT NULL) AS checked
        FROM file f
        JOIN dir d ON d.id = f.dir_id
        LEFT JOIN content     c  ON c.id = f.content_id
        LEFT JOIN content_tag ct ON ct.content_id = f.content_id
        LEFT JOIN analysis    a  ON a.content_id = f.content_id AND a.miss = 0
        LEFT JOIN loudness    l  ON l.content_id = f.content_id
        LEFT JOIN damage      dm ON dm.content_id = f.content_id
        WHERE f.kind = 1 AND f.state <> 'missing' {scope}
    )";

/// Statistics over every indexed track, or only those under `scope`.
pub fn overview(db: &Db, scope: Option<&str>) -> Result<Overview, IndexError> {
    let (clause, bound): (&str, Vec<String>) = match scope {
        Some(path) => {
            let prefix = crate::search::like_prefix(path);
            (
                "AND (d.path_lower = ?1 OR d.path_lower LIKE ?2 ESCAPE '\\')",
                vec![
                    path.trim_end_matches(['\\', '/']).to_lowercase(),
                    format!("{prefix}\\\\%"),
                ],
            )
        }
        None => ("", Vec::new()),
    };
    let cte = TRACKS.replace("{scope}", clause);
    let conn = db.conn();

    // Every query is the CTE plus a SELECT over `t`.
    let rows = |select: &str| -> Result<Vec<(Option<String>, i64)>, IndexError> {
        let mut stmt = conn.prepare(&format!("{cte} {select}"))?;
        let out = stmt
            .query_map(params_from_iter(bound.iter()), |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(out)
    };
    let tally = |select: &str| -> Result<Vec<Tally>, IndexError> {
        Ok(rows(select)?
            .into_iter()
            .filter_map(|(v, count)| Some(Tally { value: v?, count }))
            .collect())
    };
    let numbers = |select: &str| -> Result<Vec<(i64, i64)>, IndexError> {
        Ok(rows(select)?
            .into_iter()
            .filter_map(|(v, count)| Some((v?.parse().ok()?, count)))
            .collect())
    };
    let top = |column: &str| {
        tally(&format!(
            "SELECT TRIM({column}), COUNT(*) FROM t
             WHERE TRIM(COALESCE({column}, '')) <> ''
             GROUP BY TRIM({column}) COLLATE NOCASE
             ORDER BY COUNT(*) DESC, TRIM({column}) COLLATE NOCASE
             LIMIT {TOP}"
        ))
    };

    let mut o = Overview::default();

    let mut stmt = conn.prepare(&format!(
        "{cte} SELECT COUNT(*), COUNT(DISTINCT dir_id),
                      COALESCE(SUM(duration_ms), 0) / 1000, COALESCE(SUM(size), 0),
                      SUM(checked), SUM(COALESCE(damage, '') <> ''),
                      SUM(state IN ('unreadable', 'undecodable')),
                      SUM(TRIM(COALESCE(artist, '')) = ''),
                      SUM(TRIM(COALESCE(title, '')) = ''),
                      SUM(TRIM(COALESCE(genre, '')) = ''),
                      SUM(bpm IS NULL OR bpm <= 0),
                      SUM(key_norm IS NULL),
                      SUM(COALESCE(has_artwork, 0) = 0)
               FROM t"
    ))?;
    stmt.query_row(params_from_iter(bound.iter()), |r| {
        let n = |i: usize| r.get::<_, Option<i64>>(i).map(|v| v.unwrap_or(0));
        o.tracks = n(0)?;
        o.folders = n(1)?;
        o.seconds = n(2)?;
        o.bytes = n(3)?;
        o.checked = n(4)?;
        o.damaged = n(5)?;
        o.unreadable = n(6)?;
        o.no_artist = n(7)?;
        o.no_title = n(8)?;
        o.no_genre = n(9)?;
        o.no_bpm = n(10)?;
        o.no_key = n(11)?;
        o.no_art = n(12)?;
        Ok(())
    })?;
    drop(stmt);

    o.bpm = numbers(
        "SELECT CAST(CAST(ROUND(bpm) AS INTEGER) AS TEXT), COUNT(*) FROM t
         WHERE bpm > 0 GROUP BY ROUND(bpm) ORDER BY ROUND(bpm)",
    )?;

    o.keys = vec![0; 24];
    for (key, count) in numbers(
        "SELECT CAST(key_norm AS TEXT), COUNT(*) FROM t
         WHERE key_norm BETWEEN 0 AND 23 GROUP BY key_norm",
    )? {
        o.keys[key as usize] = count;
    }

    o.formats = tally(
        "SELECT UPPER(COALESCE(NULLIF(codec, ''), '?')), COUNT(*) FROM t
         GROUP BY 1 ORDER BY COUNT(*) DESC",
    )?;
    o.sample_rates = tally(
        "SELECT CASE WHEN sample_rate > 0 THEN CAST(sample_rate AS TEXT) || '/' ||
                     COALESCE(CAST(NULLIF(bit_depth, 0) AS TEXT), '') ELSE '?' END,
                COUNT(*)
         FROM t GROUP BY 1 ORDER BY COUNT(*) DESC",
    )?;
    o.lufs = numbers(
        "SELECT CAST(CAST(ROUND(lufs) AS INTEGER) AS TEXT), COUNT(*) FROM t
         WHERE lufs IS NOT NULL GROUP BY ROUND(lufs) ORDER BY ROUND(lufs)",
    )?;

    o.labels = top("label")?;
    o.genres = top("genre")?;
    o.artists = top("artist")?;

    let mut stmt = conn.prepare(&format!(
        "{cte} SELECT d.path_lower, COUNT(*),
                      SUM(COALESCE(t.damage, '') <> '') AS damaged,
                      SUM(t.state IN ('unreadable', 'undecodable')) AS unreadable
               FROM t JOIN dir d ON d.id = t.dir_id
               GROUP BY t.dir_id
               HAVING damaged + unreadable > 0
               ORDER BY damaged + unreadable DESC, d.path_lower
               LIMIT {TOP}"
    ))?;
    o.problem_folders = stmt
        .query_map(params_from_iter(bound.iter()), |r| {
            Ok(ProblemFolder {
                path: r.get(0)?,
                tracks: r.get(1)?,
                damaged: r.get(2)?,
                unreadable: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two folders under `e:\music`, one beside it, and tracks with the tags,
    /// measurements and damage each test needs.
    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO volume (id, kind, fs_uuid, mount_path)
                 VALUES (1, 'fixed', 'u1', 'E:\\');

                 INSERT INTO dir (id, volume_id, name, path_lower) VALUES
                     (1, 1, 'music', 'e:\\music'),
                     (2, 1, 'techno', 'e:\\music\\techno'),
                     (3, 1, 'musicx', 'e:\\musicx');

                 INSERT INTO content (id, quick_key, duration_ms, sample_rate, bit_depth, codec) VALUES
                     (1, 'k1', 300000, 44100, 16, 'flac'),
                     (2, 'k2', 240000, 44100, NULL, 'mp3'),
                     (3, 'k3', 360000, 48000, 24, 'flac'),
                     (4, 'k4', 200000, 44100, NULL, 'mp3');

                 INSERT INTO content_tag (content_id, artist, title, genre, label, bpm_tag, key_norm, has_artwork) VALUES
                     (1, 'Burial', 'Archangel', 'Dubstep', 'Hyperdub', 138.0, 15, 1),
                     (2, 'Burial', 'Near Dark', 'dubstep', 'Hyperdub', NULL, NULL, 0),
                     (3, 'DVS1',   NULL,        'Techno',  ' Klockworks ', 130.4, 0, 1),
                     (4, NULL,     NULL,        NULL,      NULL, NULL, NULL, 0);

                 INSERT INTO analysis (content_id, version, range_min, range_max, bpm, key_norm)
                 VALUES (2, 1, 70, 180, 139.6, 16);

                 INSERT INTO loudness (content_id, lufs, true_peak) VALUES (1, -8.4, -0.1), (3, -9.6, 0.2);
                 INSERT INTO damage (content_id, summary) VALUES (1, ''), (3, '0:30 missing');

                 INSERT INTO file (id, dir_id, name, kind, size, content_id, state) VALUES
                     (1, 1, 'a.flac', 1, 100, 1, 'ok'),
                     (2, 1, 'b.mp3',  1, 50,  2, 'ok'),
                     (3, 2, 'c.flac', 1, 200, 3, 'ok'),
                     (4, 3, 'd.mp3',  1, 40,  4, 'ok'),
                     (5, 2, 'e.mp3',  1, 10,  NULL, 'undecodable'),
                     (6, 2, 'gone.mp3', 1, 10, NULL, 'missing'),
                     (7, 1, 'cover.jpg', 2, 10, NULL, 'ok');",
            )
            .unwrap();
        db
    }

    #[test]
    fn counts_every_audio_file_that_is_still_there() {
        let o = overview(&db(), None).unwrap();
        assert_eq!(o.tracks, 5, "the missing file and the cover are not tracks");
        assert_eq!(o.folders, 3);
        assert_eq!(o.seconds, 1100);
        assert_eq!(o.bytes, 400);
    }

    /// `e:\music` is a folder and its subfolders, not every path that starts
    /// with those letters.
    #[test]
    fn a_scope_takes_the_folder_and_what_is_under_it() {
        let o = overview(&db(), Some("E:\\Music\\")).unwrap();
        assert_eq!(o.tracks, 4);
        assert_eq!(o.folders, 2);
        assert_eq!(overview(&db(), Some("e:\\music\\techno")).unwrap().tracks, 2);
    }

    #[test]
    fn a_measured_bpm_and_key_stand_in_for_a_missing_tag() {
        let o = overview(&db(), None).unwrap();
        assert_eq!(o.bpm, vec![(130, 1), (138, 1), (140, 1)]);
        assert_eq!(o.keys[15], 1);
        assert_eq!(o.keys[16], 1, "measured key of the untagged track");
        assert_eq!(o.keys[0], 1);
        assert_eq!(o.no_bpm, 2);
        assert_eq!(o.no_key, 2);
    }

    #[test]
    fn top_lists_ignore_case_and_spacing() {
        let o = overview(&db(), None).unwrap();
        assert_eq!(o.genres[0].count, 2, "Dubstep and dubstep are one genre");
        assert_eq!(o.labels[0].value, "Hyperdub");
        assert_eq!(o.labels[1].value, "Klockworks");
        assert_eq!(o.artists[0].value, "Burial");
        assert_eq!(o.artists.len(), 2, "an empty artist is not an artist");
    }

    #[test]
    fn formats_and_rates_are_counted() {
        let o = overview(&db(), None).unwrap();
        let mut formats: Vec<_> = o.formats.iter().map(|t| (t.value.as_str(), t.count)).collect();
        formats.sort();
        assert_eq!(formats, vec![("?", 1), ("FLAC", 2), ("MP3", 2)]);
        assert!(o.sample_rates.iter().any(|t| t.value == "44100/16" && t.count == 1));
        assert!(o.sample_rates.iter().any(|t| t.value == "44100/" && t.count == 2));
    }

    #[test]
    fn quality_comes_from_what_check_quality_stored() {
        let o = overview(&db(), None).unwrap();
        assert_eq!(o.checked, 2);
        assert_eq!(o.damaged, 1);
        assert_eq!(o.unreadable, 1);
        assert_eq!(o.lufs, vec![(-10, 1), (-8, 1)]);
    }

    #[test]
    fn folders_with_problems_come_worst_first() {
        let db = db();
        // One problem in a folder that sorts first by name, so only the count
        // can put the techno folder, with two, ahead of it.
        db.conn()
            .execute_batch(
                "INSERT INTO dir (id, volume_id, name, path_lower) VALUES (4, 1, 'a', 'e:\\a');
                 INSERT INTO file (id, dir_id, name, kind, state) VALUES (8, 4, 'x.mp3', 1, 'unreadable');",
            )
            .unwrap();

        let o = overview(&db, None).unwrap();
        assert_eq!(o.problem_folders.len(), 2);
        let f = &o.problem_folders[0];
        assert_eq!(f.path, "e:\\music\\techno");
        assert_eq!((f.tracks, f.damaged, f.unreadable), (2, 1, 1));
        assert_eq!(o.problem_folders[1].path, "e:\\a");
    }

    #[test]
    fn an_empty_index_is_all_zeros() {
        let o = overview(&Db::open_in_memory().unwrap(), None).unwrap();
        assert_eq!(o.tracks, 0);
        assert_eq!(o.keys, vec![0; 24]);
        assert!(o.bpm.is_empty());
    }
}
