//! Named playlists: sets gathered from anywhere in the library without copying a
//! file.
//!
//! Only storage and ordering. A track is held by its path (see the V14
//! migration), and resolved to whatever `file` row the index has for that path
//! when the playlist is read, so a track whose folder has not been scanned since
//! a restart is still in the playlist, only not playable until it is.

use crate::db::{Db, IndexError};
use crate::query::{read_row, ListRow, ROW_COLUMNS};
use crate::scan::path_key;
use crate::volume::now;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

/// One playlist, as the list of playlists shows it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub tracks: i64,
    /// Summed over the tracks the index knows a length for.
    pub duration_ms: i64,
}

/// One entry of a playlist, in order.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    /// The indexed file at `path`, or `None` when the index has no row for it:
    /// moved outside the app, deleted, or on a drive that is not attached.
    pub row: Option<ListRow>,
}

/// All playlists, by name.
pub fn all(db: &Db) -> Result<Vec<Playlist>, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT p.id, p.name, COUNT(t.path_key)
         FROM playlist p
         LEFT JOIN playlist_track t ON t.playlist_id = p.id
         GROUP BY p.id
         ORDER BY p.name COLLATE NOCASE",
    )?;
    let lists = stmt
        .query_map([], |r| {
            Ok(Playlist {
                id: r.get(0)?,
                name: r.get(1)?,
                tracks: r.get(2)?,
                duration_ms: 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    lists
        .into_iter()
        .map(|mut p| {
            p.duration_ms = entries(db, p.id)?
                .iter()
                .filter_map(|e| e.row.as_ref()?.duration_ms)
                .sum();
            Ok(p)
        })
        .collect()
}

/// Makes a playlist. A name already taken, in any case, is refused.
pub fn create(db: &Db, name: &str) -> Result<i64, PlaylistError> {
    let name = valid_name(name)?;
    if taken(db, name, None)? {
        return Err(PlaylistError::Taken(name.to_string()));
    }
    db.conn().execute(
        "INSERT INTO playlist (name, created_at) VALUES (?1, ?2)",
        params![name, now()],
    )?;
    Ok(db.conn().last_insert_rowid())
}

pub fn rename(db: &Db, id: i64, name: &str) -> Result<(), PlaylistError> {
    let name = valid_name(name)?;
    if taken(db, name, Some(id))? {
        return Err(PlaylistError::Taken(name.to_string()));
    }
    db.conn().execute(
        "UPDATE playlist SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

/// Deletes the playlist. The files are not touched.
pub fn delete(db: &Db, id: i64) -> Result<(), IndexError> {
    db.conn()
        .execute("DELETE FROM playlist WHERE id = ?1", [id])?;
    Ok(())
}

/// Appends `paths` in the order given, skipping any already in the playlist.
/// Returns how many were added.
pub fn add<P: AsRef<Path>>(db: &Db, id: i64, paths: &[P]) -> Result<usize, IndexError> {
    let tx = db.conn().unchecked_transaction()?;
    let mut next: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position) + 1, 0) FROM playlist_track WHERE playlist_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let at = now();
    let mut added = 0;
    for path in paths {
        let path = path.as_ref();
        let changed = tx.execute(
            "INSERT OR IGNORE INTO playlist_track (playlist_id, position, path, path_key, added_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, next, path.to_string_lossy(), path_key(path), at],
        )?;
        if changed > 0 {
            next += 1;
            added += 1;
        }
    }
    tx.commit()?;
    Ok(added)
}

/// Takes `paths` out of the playlist. Returns how many were in it.
pub fn remove<P: AsRef<Path>>(db: &Db, id: i64, paths: &[P]) -> Result<usize, IndexError> {
    let tx = db.conn().unchecked_transaction()?;
    let mut removed = 0;
    for path in paths {
        removed += tx.execute(
            "DELETE FROM playlist_track WHERE playlist_id = ?1 AND path_key = ?2",
            params![id, path_key(path.as_ref())],
        )?;
    }
    renumber(&tx, id)?;
    tx.commit()?;
    Ok(removed)
}

/// Moves `paths`, keeping their order among themselves, so the first of them
/// lands at `to` — an index into the playlist as it is without them.
pub fn reorder<P: AsRef<Path>>(db: &Db, id: i64, paths: &[P], to: usize) -> Result<(), IndexError> {
    let keys: Vec<String> = paths.iter().map(|p| path_key(p.as_ref())).collect();
    let tx = db.conn().unchecked_transaction()?;
    let order: Vec<String> = {
        let mut stmt = tx.prepare(
            "SELECT path_key FROM playlist_track WHERE playlist_id = ?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([id], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let moving: Vec<&String> = order.iter().filter(|k| keys.contains(k)).collect();
    let mut rest: Vec<&String> = order.iter().filter(|k| !keys.contains(k)).collect();
    let to = to.min(rest.len());
    rest.splice(to..to, moving);
    for (i, key) in rest.iter().enumerate() {
        tx.execute(
            "UPDATE playlist_track SET position = ?1 WHERE playlist_id = ?2 AND path_key = ?3",
            params![i as i64, id, key],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The playlist's tracks, in order, each with its indexed row when there is one.
pub fn entries(db: &Db, id: i64) -> Result<Vec<Entry>, IndexError> {
    let paths: Vec<String> = {
        let mut stmt = db
            .conn()
            .prepare("SELECT path FROM playlist_track WHERE playlist_id = ?1 ORDER BY position")?;
        let rows = stmt.query_map([id], |r| r.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    let sql = format!("{ROW_COLUMNS}\nJOIN dir d ON d.id = f.dir_id\nWHERE d.path_lower = ?1 AND f.name = ?2 COLLATE NOCASE");
    let mut stmt = db.conn().prepare(&sql)?;
    paths
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            let row = match (p.parent(), p.file_name()) {
                (Some(dir), Some(name)) => stmt
                    .query_row(params![path_key(dir), name.to_string_lossy()], read_row)
                    .optional()?,
                _ => None,
            };
            Ok(Entry { path, row })
        })
        .collect()
}

/// Which of the playlists hold every one of `paths`.
///
/// What the right-click menu ticks: a playlist is ticked when the whole
/// selection is in it, so clicking a tick takes the selection out and clicking
/// an empty box puts all of it in.
pub fn holding_all<P: AsRef<Path>>(db: &Db, paths: &[P]) -> Result<Vec<i64>, IndexError> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut stmt = db
        .conn()
        .prepare("SELECT playlist_id FROM playlist_track WHERE path_key = ?1")?;
    let mut common: Option<Vec<i64>> = None;
    for path in paths {
        let ids: Vec<i64> = stmt
            .query_map([path_key(path.as_ref())], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        common = Some(match common {
            None => ids,
            Some(have) => have.into_iter().filter(|i| ids.contains(i)).collect(),
        });
        if common.as_ref().is_some_and(Vec::is_empty) {
            break;
        }
    }
    Ok(common.unwrap_or_default())
}

/// Points entries at `from` to `to`, after the app renamed or moved it. `from`
/// may be a folder, in which case everything under it follows.
pub fn repath(db: &Db, from: &Path, to: &Path) -> Result<usize, IndexError> {
    let old = path_key(from);
    let new = to
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_string();
    let tx = db.conn().unchecked_transaction()?;
    let moved: Vec<(i64, String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT playlist_id, path_key, path FROM playlist_track
             WHERE path_key = ?1 OR substr(path_key, 1, length(?1) + 1) IN (?1 || '\\', ?1 || '/')",
        )?;
        let rows = stmt.query_map([&old], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (playlist, key, path) in &moved {
        // The part below `from` keeps the case it was stored with.
        let below = path.get(old.len()..).unwrap_or("");
        let path = format!("{new}{below}");
        let changed = tx.execute(
            "UPDATE OR IGNORE playlist_track SET path = ?1, path_key = ?2
             WHERE playlist_id = ?3 AND path_key = ?4",
            params![path, path_key(Path::new(&path)), playlist, key],
        )?;
        // Already in the playlist at the new path: the old entry is the copy.
        if changed == 0 {
            tx.execute(
                "DELETE FROM playlist_track WHERE playlist_id = ?1 AND path_key = ?2",
                params![playlist, key],
            )?;
        }
    }
    tx.commit()?;
    Ok(moved.len())
}

fn renumber(conn: &rusqlite::Connection, id: i64) -> Result<(), IndexError> {
    conn.execute(
        "UPDATE playlist_track SET position = (
             SELECT COUNT(*) FROM playlist_track o
             WHERE o.playlist_id = playlist_track.playlist_id
               AND o.position < playlist_track.position)
         WHERE playlist_id = ?1",
        [id],
    )?;
    Ok(())
}

fn valid_name(name: &str) -> Result<&str, PlaylistError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PlaylistError::Empty);
    }
    Ok(name)
}

fn taken(db: &Db, name: &str, except: Option<i64>) -> Result<bool, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT id FROM playlist WHERE name = ?1 COLLATE NOCASE",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some_and(|id| Some(id) != except))
}

#[derive(Debug, thiserror::Error)]
pub enum PlaylistError {
    #[error("a playlist needs a name")]
    Empty,
    #[error("{0} already exists")]
    Taken(String),
    #[error(transparent)]
    Index(#[from] IndexError),
}

impl From<rusqlite::Error> for PlaylistError {
    fn from(e: rusqlite::Error) -> Self {
        PlaylistError::Index(e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn paths(db: &Db, id: i64) -> Vec<String> {
        entries(db, id)
            .unwrap()
            .into_iter()
            .map(|e| e.path)
            .collect()
    }

    #[test]
    fn names_are_unique_whatever_the_case() {
        let db = Db::open_in_memory().unwrap();
        let a = create(&db, " Warmup ").unwrap();
        assert!(matches!(
            create(&db, "warmup"),
            Err(PlaylistError::Taken(_))
        ));
        assert!(matches!(create(&db, "  "), Err(PlaylistError::Empty)));
        let b = create(&db, "Peak").unwrap();
        assert!(matches!(
            rename(&db, b, "WARMUP"),
            Err(PlaylistError::Taken(_))
        ));
        rename(&db, a, "warmup").unwrap();
        let names: Vec<String> = all(&db).unwrap().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Peak", "warmup"]);
    }

    #[test]
    fn adding_appends_once_and_removing_closes_the_gap() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, "set").unwrap();
        assert_eq!(
            add(&db, id, &[p("C:\\m\\a.wav"), p("C:\\m\\b.wav")]).unwrap(),
            2
        );
        // The same file in another case is the same file.
        assert_eq!(
            add(&db, id, &[p("c:\\M\\A.wav"), p("C:\\m\\c.wav")]).unwrap(),
            1
        );
        assert_eq!(
            paths(&db, id),
            ["C:\\m\\a.wav", "C:\\m\\b.wav", "C:\\m\\c.wav"]
        );

        assert_eq!(remove(&db, id, &[p("C:\\m\\b.wav")]).unwrap(), 1);
        add(&db, id, &[p("C:\\m\\d.wav")]).unwrap();
        assert_eq!(
            paths(&db, id),
            ["C:\\m\\a.wav", "C:\\m\\c.wav", "C:\\m\\d.wav"]
        );
        assert_eq!(all(&db).unwrap()[0].tracks, 3);
    }

    #[test]
    fn reorder_moves_a_block_to_a_place() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, "set").unwrap();
        let all: Vec<PathBuf> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|n| p(&format!("C:\\m\\{n}.wav")))
            .collect();
        add(&db, id, &all).unwrap();
        // d and b to the front, in playlist order.
        reorder(&db, id, &[p("C:\\m\\d.wav"), p("C:\\m\\b.wav")], 0).unwrap();
        let names = |db: &Db| -> String {
            paths(db, id)
                .iter()
                .map(|s| s.chars().nth(5).unwrap())
                .collect()
        };
        assert_eq!(names(&db), "bdace");
        reorder(&db, id, &[p("C:\\m\\b.wav")], 99).unwrap();
        assert_eq!(names(&db), "daceb");
    }

    #[test]
    fn ticks_show_playlists_holding_the_whole_selection() {
        let db = Db::open_in_memory().unwrap();
        let x = create(&db, "x").unwrap();
        let y = create(&db, "y").unwrap();
        add(&db, x, &[p("C:\\a.wav"), p("C:\\b.wav")]).unwrap();
        add(&db, y, &[p("C:\\a.wav")]).unwrap();
        let mut both = holding_all(&db, &[p("C:\\a.wav")]).unwrap();
        both.sort();
        assert_eq!(both, [x, y]);
        assert_eq!(
            holding_all(&db, &[p("C:\\a.wav"), p("C:\\b.wav")]).unwrap(),
            [x]
        );
    }

    #[test]
    fn entries_follow_a_rename_and_a_folder_move() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, "set").unwrap();
        add(
            &db,
            id,
            &[
                p("C:\\in\\One.wav"),
                p("C:\\in\\sub\\Two.wav"),
                p("C:\\inside\\Three.wav"),
            ],
        )
        .unwrap();

        repath(&db, &p("c:\\IN\\one.wav"), &p("C:\\in\\Uno.wav")).unwrap();
        // A folder: what is under it moves, a folder that only starts the same does not.
        repath(&db, &p("C:\\in"), &p("D:\\crate\\")).unwrap();
        assert_eq!(
            paths(&db, id),
            [
                "D:\\crate\\Uno.wav",
                "D:\\crate\\sub\\Two.wav",
                "C:\\inside\\Three.wav"
            ]
        );
    }

    #[test]
    fn an_indexed_path_resolves_to_its_row_in_any_case() {
        use crate::volume::{self, VolumeKind};
        let db = Db::open_in_memory().unwrap();
        let d = tempfile::TempDir::new().unwrap();
        std::fs::write(d.path().join("Track.wav"), b"x").unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
        crate::scan::scan_dir(&db, vol, d.path()).unwrap();

        let id = create(&db, "set").unwrap();
        let upper = d.path().to_string_lossy().to_uppercase();
        add(&db, id, &[PathBuf::from(upper).join("TRACK.WAV")]).unwrap();
        let row = entries(&db, id).unwrap().remove(0).row.expect("indexed");
        assert_eq!(row.name, "Track.wav");
    }

    #[test]
    fn a_path_the_index_does_not_know_is_kept_without_a_row() {
        let db = Db::open_in_memory().unwrap();
        let id = create(&db, "set").unwrap();
        add(&db, id, &[p("Z:\\gone.wav")]).unwrap();
        let e = entries(&db, id).unwrap();
        assert_eq!(e.len(), 1);
        assert!(e[0].row.is_none());
    }
}
