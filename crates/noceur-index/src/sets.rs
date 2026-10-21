//! Recorded sets and their tracklists.
//!
//! Only storage and matching. Where a tracklist comes from is the app's
//! business; this keeps it beside the set's path (see the V16 migration) and
//! answers the reverse question — which of the linked sets a track was played
//! in, and when.

use crate::db::{Db, IndexError};
use crate::query::{read_row, ListRow, ROW_COLUMNS};
use crate::scan::path_key;
use crate::volume::now;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

/// What a set is linked to.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetLink {
    pub source: String,
    pub slug: String,
    pub title: String,
    pub channel: String,
    pub url: String,
    pub duration_ms: Option<i64>,
    pub offset_ms: i64,
    pub artwork: String,
    /// `YYYY-MM-DD`, or empty.
    pub published: String,
}

/// One track heard in a set, at the times measured on the published copy.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTrack {
    pub start_ms: i64,
    pub end_ms: i64,
    pub artist: String,
    pub title: String,
    pub label: String,
    /// How many sets the track was heard in, as the service counts them.
    pub plays: i64,
}

/// A linked set with its tracklist, in order.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedSet {
    pub link: SetLink,
    pub tracks: Vec<SetTrack>,
}

/// A track played in one of the linked sets.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// The set's file.
    pub path: String,
    pub set_title: String,
    /// Where it starts in this copy of the set: the offset already applied.
    pub start_ms: i64,
    pub end_ms: i64,
    /// The indexed file at `path`, or `None` when the index has no row for it.
    pub row: Option<ListRow>,
}

/// Links a set file to a tracklist, replacing whatever it was linked to.
///
/// An offset already set for this file is kept: relinking to a corrected
/// tracklist of the same recording should not lose an alignment that was made
/// by ear.
pub fn link(db: &Db, path: &Path, link: &SetLink, tracks: &[SetTrack]) -> Result<(), IndexError> {
    let key = path_key(path);
    let tx = db.conn().unchecked_transaction()?;
    let offset: i64 = tx
        .query_row(
            "SELECT offset_ms FROM set_link WHERE path_key = ?1 AND slug = ?2",
            params![key, link.slug],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(link.offset_ms);

    tx.execute("DELETE FROM set_link WHERE path_key = ?1", [&key])?;
    tx.execute(
        "INSERT INTO set_link
            (path_key, path, source, slug, title, channel, url, duration_ms, offset_ms, linked_at,
             artwork, published)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            key,
            path.to_string_lossy(),
            link.source,
            link.slug,
            link.title,
            link.channel,
            link.url,
            link.duration_ms,
            offset,
            now(),
            link.artwork,
            link.published,
        ],
    )?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO set_track
                (path_key, position, start_ms, end_ms, artist, title, label, artist_key, title_key,
                 plays)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?;
        for (i, t) in tracks.iter().enumerate() {
            insert.execute(params![
                key,
                i as i64,
                t.start_ms,
                t.end_ms,
                t.artist,
                t.title,
                t.label,
                artists(&t.artist).join(","),
                fold_title(&t.title),
                t.plays,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// The tracklist a set file is linked to, if any.
pub fn get(db: &Db, path: &Path) -> Result<Option<LinkedSet>, IndexError> {
    let key = path_key(path);
    let link = db
        .conn()
        .query_row(
            "SELECT source, slug, title, channel, url, duration_ms, offset_ms, artwork, published
             FROM set_link WHERE path_key = ?1",
            [&key],
            |r| {
                Ok(SetLink {
                    source: r.get(0)?,
                    slug: r.get(1)?,
                    title: r.get(2)?,
                    channel: r.get(3)?,
                    url: r.get(4)?,
                    duration_ms: r.get(5)?,
                    offset_ms: r.get(6)?,
                    artwork: r.get(7)?,
                    published: r.get(8)?,
                })
            },
        )
        .optional()?;
    let Some(link) = link else {
        return Ok(None);
    };

    let mut stmt = db.conn().prepare(
        "SELECT start_ms, end_ms, artist, title, label, plays
         FROM set_track WHERE path_key = ?1 ORDER BY position",
    )?;
    let tracks = stmt
        .query_map([&key], |r| {
            Ok(SetTrack {
                start_ms: r.get(0)?,
                end_ms: r.get(1)?,
                artist: r.get(2)?,
                title: r.get(3)?,
                label: r.get(4)?,
                plays: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(LinkedSet { link, tracks }))
}

/// Forgets a set's tracklist.
pub fn unlink(db: &Db, path: &Path) -> Result<(), IndexError> {
    db.conn()
        .execute("DELETE FROM set_link WHERE path_key = ?1", [path_key(path)])?;
    Ok(())
}

/// Moves this copy's tracklist by `offset_ms`. Does nothing to a set that is
/// not linked.
pub fn set_offset(db: &Db, path: &Path, offset_ms: i64) -> Result<(), IndexError> {
    db.conn().execute(
        "UPDATE set_link SET offset_ms = ?1 WHERE path_key = ?2",
        params![offset_ms, path_key(path)],
    )?;
    Ok(())
}

/// Keeps links pointing at set files the app renamed or moved, folders included.
pub fn repath(db: &Db, from: &Path, to: &Path) -> Result<usize, IndexError> {
    let old = path_key(from);
    let new = to
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_string();
    let tx = db.conn().unchecked_transaction()?;
    let moved: Vec<(String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT path_key, path FROM set_link
             WHERE path_key = ?1 OR substr(path_key, 1, length(?1) + 1) IN (?1 || '\\', ?1 || '/')",
        )?;
        let rows = stmt.query_map([&old], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (key, path) in &moved {
        let below = path.get(old.len()..).unwrap_or("");
        let path = format!("{new}{below}");
        let new_key = path_key(Path::new(&path));
        // Something already linked at the destination is the one to keep: the
        // file there is what plays now.
        tx.execute("DELETE FROM set_link WHERE path_key = ?1 AND ?1 <> ?2", params![new_key, key])?;
        tx.execute(
            "UPDATE set_link SET path = ?1, path_key = ?2 WHERE path_key = ?3",
            params![path, new_key, key],
        )?;
    }
    tx.commit()?;
    Ok(moved.len())
}

/// The linked sets a track was played in, and where.
///
/// Matched on the title, folded, and then on at least one artist in common:
/// a set lists `John Tejada, Arian Leviste` where a tag says `John Tejada &
/// Arian Leviste`, and either should find the other. A set never lists itself.
pub fn appearances(
    db: &Db,
    artist: &str,
    title: &str,
    except: Option<&Path>,
) -> Result<Vec<Appearance>, IndexError> {
    let title_key = fold_title(title);
    if title_key.is_empty() {
        return Ok(Vec::new());
    }
    let wanted = artists(artist);
    let except = except.map(path_key);

    let mut stmt = db.conn().prepare(
        "SELECT l.path, l.path_key, l.title, l.offset_ms, t.start_ms, t.end_ms, t.artist_key
         FROM set_track t
         JOIN set_link l ON l.path_key = t.path_key
         WHERE t.title_key = ?1
         ORDER BY l.title COLLATE NOCASE, t.start_ms",
    )?;
    let found: Vec<(String, String, String, i64, i64, i64, String)> = stmt
        .query_map([&title_key], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let sql = format!(
        "{ROW_COLUMNS}\nJOIN dir d ON d.id = f.dir_id\nWHERE d.path_lower = ?1 AND f.name = ?2 COLLATE NOCASE"
    );
    let mut row_at = db.conn().prepare(&sql)?;
    let mut out = Vec::new();
    for (path, key, set_title, offset, start, end, artist_key) in found {
        if except.as_deref() == Some(key.as_str()) {
            continue;
        }
        // No artist on either side leaves the title to decide alone.
        let theirs: Vec<String> = artist_key
            .split(',')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        if !wanted.is_empty() && !theirs.is_empty() && !wanted.iter().any(|a| theirs.contains(a)) {
            continue;
        }
        let p = Path::new(&path);
        let row = match (p.parent(), p.file_name()) {
            (Some(dir), Some(name)) => row_at
                .query_row(params![path_key(dir), name.to_string_lossy()], read_row)
                .optional()?,
            _ => None,
        };
        out.push(Appearance {
            path,
            set_title,
            start_ms: (start + offset).max(0),
            end_ms: (end + offset).max(0),
            row,
        });
    }
    Ok(out)
}

/// Lowercase letters and digits, everything else one space.
///
/// Accents are kept: `Mühle` and `Muhle` are different words to this, which
/// costs a match only where a tag and the service disagree on spelling.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = true;
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

/// A title folded, without the version that says nothing: `Original Mix`,
/// `Extended Mix` and the like are the same track to anyone looking for it.
pub fn fold_title(title: &str) -> String {
    let mut t = fold(title);
    for plain in [
        " original mix",
        " extended mix",
        " original version",
        " extended version",
        " extended",
        " radio edit",
    ] {
        if let Some(stripped) = t.strip_suffix(plain) {
            t = stripped.to_string();
        }
    }
    t
}

/// The names in an artist credit, folded: split at commas, `&`, `/`, `and`,
/// `x`, `vs` and `feat`.
fn artists(credit: &str) -> Vec<String> {
    credit
        .split([',', '&', '/', ';'])
        .flat_map(|part| {
            let part = fold(part);
            [" and ", " x ", " vs ", " feat ", " ft ", " featuring "]
                .iter()
                .fold(vec![part], |names, sep| {
                    names
                        .iter()
                        .flat_map(|n| n.split(sep).map(str::to_string).collect::<Vec<_>>())
                        .collect()
                })
        })
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(start: i64, artist: &str, title: &str) -> SetTrack {
        SetTrack {
            start_ms: start,
            end_ms: start + 180_000,
            artist: artist.into(),
            title: title.into(),
            label: String::new(),
            plays: 0,
        }
    }

    fn sample_link(slug: &str) -> SetLink {
        SetLink {
            source: "trackid".into(),
            slug: slug.into(),
            title: "All night".into(),
            channel: "Radio".into(),
            url: String::new(),
            duration_ms: Some(7_200_000),
            offset_ms: 0,
            artwork: String::new(),
            published: String::new(),
        }
    }

    #[test]
    fn a_tracklist_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let set = Path::new("D:\\Sets\\All night.flac");
        let tracks = vec![track(0, "A", "One"), track(200_000, "B", "Two")];
        link(&db, set, &sample_link("all-night"), &tracks).unwrap();

        let got = get(&db, set).unwrap().unwrap();
        assert_eq!(got.tracks, tracks);
        assert_eq!(got.link.slug, "all-night");
        // Paths are matched the way the rest of the index matches them.
        assert!(get(&db, Path::new("d:\\sets\\all night.flac")).unwrap().is_some());

        unlink(&db, set).unwrap();
        assert!(get(&db, set).unwrap().is_none());
    }

    /// Relinking the same recording keeps the alignment made by ear; linking a
    /// different one starts from none.
    #[test]
    fn an_offset_survives_relinking_the_same_set() {
        let db = Db::open_in_memory().unwrap();
        let set = Path::new("D:\\Sets\\a.mp3");
        link(&db, set, &sample_link("a"), &[]).unwrap();
        set_offset(&db, set, -4_000).unwrap();

        link(&db, set, &sample_link("a"), &[]).unwrap();
        assert_eq!(get(&db, set).unwrap().unwrap().link.offset_ms, -4_000);

        link(&db, set, &sample_link("b"), &[]).unwrap();
        assert_eq!(get(&db, set).unwrap().unwrap().link.offset_ms, 0);
    }

    #[test]
    fn a_renamed_set_keeps_its_tracklist() {
        let db = Db::open_in_memory().unwrap();
        let set = Path::new("D:\\Sets\\a.mp3");
        link(&db, set, &sample_link("a"), &[track(0, "A", "One")]).unwrap();

        repath(&db, Path::new("D:\\Sets"), Path::new("E:\\Mixes")).unwrap();
        assert!(get(&db, set).unwrap().is_none());
        let moved = get(&db, Path::new("E:\\Mixes\\a.mp3")).unwrap().unwrap();
        assert_eq!(moved.tracks.len(), 1);
    }

    /// The tag and the tracklist spell the credit and the version differently;
    /// the track is still found, at its time in this copy.
    #[test]
    fn a_track_is_found_in_the_sets_it_was_played_in() {
        let db = Db::open_in_memory().unwrap();
        let set = Path::new("D:\\Sets\\night.flac");
        link(
            &db,
            set,
            &sample_link("night"),
            &[
                track(60_000, "Robag Wruhme, Wighnomy Brothers", "Killerteppich (Original Mix)"),
                track(400_000, "Someone Else", "Killerteppich"),
            ],
        )
        .unwrap();
        set_offset(&db, set, 2_500).unwrap();

        let found = appearances(&db, "Wighnomy Brothers & Robag Wruhme", "Killerteppich", None).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start_ms, 62_500);
        assert_eq!(found[0].set_title, "All night");

        assert!(appearances(&db, "Nobody", "Killerteppich", None).unwrap().is_empty());
        assert!(appearances(&db, "Robag Wruhme", "Killerteppich", Some(set)).unwrap().is_empty());
    }

    #[test]
    fn folding_keeps_only_what_tells_titles_apart() {
        assert_eq!(fold("  Lost And... (LoSoul Remix) "), "lost and losoul remix");
        assert_eq!(fold_title("QuoVadis - Original Mix"), "quovadis");
        assert_eq!(fold_title("Lucy (Extended Mix)"), "lucy");
        assert_eq!(artists("Tanzmann & Stefanik"), vec!["tanzmann", "stefanik"]);
        assert_eq!(artists("A feat. B"), vec!["a", "b"]);
    }
}
