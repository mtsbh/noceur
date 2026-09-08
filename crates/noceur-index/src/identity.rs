//! Which files hold the same audio.
//!
//! The index identifies content by its bytes, so a track and a retagged copy of
//! it are two `content` rows. The audio hash sits on those rows and says which
//! of them are the same recording: tags are not part of the audio, so nothing
//! written into them can change it.
//!
//! Kept in its own table beside the thumbnail, envelope and spectrogram caches.
//! The `content.audio_hash` column reserved for this in the first migration is
//! `UNIQUE`, which is exactly wrong for a value copies are meant to share, and
//! rebuilding `content` to drop that constraint would mean dropping a table four
//! others cascade from.

use crate::db::{Db, IndexError};
use rusqlite::{params, OptionalExtension};

/// One file that shares its audio with another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub file_id: i64,
    pub path: String,
    pub name: String,
    pub size: i64,
    pub mtime: i64,
}

/// A set of files holding identical audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub hash: String,
    pub files: Vec<Copy>,
}

/// Reads the stored hash for one content, if it has been measured.
pub fn hash_of(db: &Db, content_id: i64) -> Result<Option<String>, IndexError> {
    Ok(db
        .conn()
        .query_row(
            "SELECT hash FROM audio_identity WHERE content_id = ?1",
            params![content_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Records the hash of one content's audio.
pub fn set_hash(db: &Db, content_id: i64, hash: &str) -> Result<(), IndexError> {
    db.conn().execute(
        "INSERT INTO audio_identity (content_id, hash) VALUES (?1, ?2)
         ON CONFLICT(content_id) DO UPDATE SET hash = excluded.hash",
        params![content_id, hash],
    )?;
    Ok(())
}

/// Every set of files that share audio, within the given directories.
///
/// Scoped rather than global, because "everything in the library" is not a
/// question anyone can act on — a duplicate matters when it is between two
/// places you are looking at. An empty scope means everywhere that has been
/// hashed.
///
/// Files that have not been hashed are absent rather than reported as unique:
/// not knowing and knowing there is no copy are different answers, and only one
/// of them is safe to act on.
pub fn duplicates(db: &Db, scopes: &[String]) -> Result<Vec<Group>, IndexError> {
    let mut sql = String::from(
        "SELECT i.hash, f.id, d.path_lower, f.name, f.size, f.mtime
         FROM audio_identity i
         JOIN file f ON f.content_id = i.content_id
         JOIN dir  d ON d.id = f.dir_id
         WHERE i.hash IN (
               SELECT i2.hash
               FROM audio_identity i2
               JOIN file f2 ON f2.content_id = i2.content_id
               GROUP BY i2.hash
               HAVING COUNT(f2.id) > 1
           )",
    );

    let lowered: Vec<String> = scopes.iter().map(|s| s.to_lowercase()).collect();
    if !lowered.is_empty() {
        let clauses: Vec<String> = (0..lowered.len())
            .map(|i| format!("d.path_lower LIKE ?{} || '%'", i + 1))
            .collect();
        sql.push_str(&format!(" AND ({})", clauses.join(" OR ")));
    }
    sql.push_str(" ORDER BY i.hash, d.path_lower, f.name");

    let mut stmt = db.conn().prepare(&sql)?;
    let bound: Vec<&dyn rusqlite::ToSql> =
        lowered.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

    let rows = stmt.query_map(bound.as_slice(), |r| {
        Ok((
            r.get::<_, String>(0)?,
            Copy {
                file_id: r.get(1)?,
                path: r.get(2)?,
                name: r.get(3)?,
                size: r.get(4)?,
                mtime: r.get(5)?,
            },
        ))
    })?;

    let mut groups: Vec<Group> = Vec::new();
    for row in rows {
        let (hash, copy) = row?;
        match groups.last_mut() {
            Some(group) if group.hash == hash => group.files.push(copy),
            _ => groups.push(Group {
                hash,
                files: vec![copy],
            }),
        }
    }

    // A scope can cut a group down to one file, which is then not a duplicate of
    // anything you can see.
    groups.retain(|g| g.files.len() > 1);
    Ok(groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An index holding two directories, with files that can be pointed at
    /// whichever content a test needs.
    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO volume (id, kind, fs_uuid, mount_path)
                 VALUES (1, 'fixed', 'u1', 'E:\\');

                 INSERT INTO dir (id, volume_id, name, path_lower)
                 VALUES (1, 1, 'music', 'e:\\music'),
                        (2, 1, 'ssd',   'e:\\ssd');

                 INSERT INTO content (id, quick_key) VALUES
                     (1, 'k1'), (2, 'k2'), (3, 'k3');

                 INSERT INTO file (id, dir_id, name, kind, content_id) VALUES
                     (1, 1, 'a.flac', 1, 1),
                     (2, 2, 'a.flac', 1, 2),
                     (3, 1, 'b.flac', 1, 3);",
            )
            .unwrap();
        db
    }

    #[test]
    fn a_hash_round_trips() {
        let db = db();
        assert_eq!(hash_of(&db, 1).unwrap(), None);

        set_hash(&db, 1, "abc123").unwrap();
        assert_eq!(hash_of(&db, 1).unwrap(), Some("abc123".into()));
    }

    /// The case the whole feature exists for: one track, copied and retagged, so
    /// two different sets of bytes holding one recording.
    #[test]
    fn two_copies_of_one_recording_are_a_group() {
        let db = db();
        set_hash(&db, 1, "same").unwrap();
        set_hash(&db, 2, "same").unwrap();
        set_hash(&db, 3, "other").unwrap();

        let groups = duplicates(&db, &[]).unwrap();
        assert_eq!(groups.len(), 1, "one group, not one per file");
        assert_eq!(groups[0].hash, "same");
        assert_eq!(groups[0].files.len(), 2);
        assert_eq!(groups[0].files[0].path, "e:\\music");
        assert_eq!(groups[0].files[1].path, "e:\\ssd");
    }

    #[test]
    fn a_recording_that_appears_once_is_not_a_group() {
        let db = db();
        set_hash(&db, 1, "alone").unwrap();
        assert!(duplicates(&db, &[]).unwrap().is_empty());
    }

    /// Not knowing and knowing there is no copy are different answers, and a
    /// file nobody has hashed must not be reported as either.
    #[test]
    fn files_that_have_not_been_hashed_are_absent() {
        let db = db();
        assert!(duplicates(&db, &[]).unwrap().is_empty());

        set_hash(&db, 1, "same").unwrap();
        assert!(
            duplicates(&db, &[]).unwrap().is_empty(),
            "one hashed file is not a duplicate of an unhashed one"
        );
    }

    /// A duplicate matters when it is between two places you are looking at.
    #[test]
    fn a_scope_narrows_the_answer() {
        let db = db();
        set_hash(&db, 1, "same").unwrap();
        set_hash(&db, 2, "same").unwrap();

        assert_eq!(duplicates(&db, &["E:\\".into()]).unwrap().len(), 1);
        // Cut to one visible file, the group is no longer a duplicate of
        // anything the user can see.
        assert!(duplicates(&db, &["E:\\music".into()]).unwrap().is_empty());
    }

    #[test]
    fn a_scope_is_matched_regardless_of_case() {
        let db = db();
        set_hash(&db, 1, "same").unwrap();
        set_hash(&db, 2, "same").unwrap();
        assert_eq!(duplicates(&db, &["e:\\".into()]).unwrap().len(), 1);
    }
}
