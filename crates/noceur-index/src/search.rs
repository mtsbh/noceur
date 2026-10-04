//! Full-text search across the indexed library.

use crate::db::{Db, IndexError};
use rusqlite::params;

/// One search hit: the row that matched, and where it lives.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub file_id: i64,
    pub name: String,
    /// Directory holding the file — where the list must navigate to reveal it.
    pub dir_path: String,
    /// `EntryKind::group()`: 0 directory, 1 audio, 2 other.
    pub kind: i64,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub bpm: Option<f64>,
    pub key_norm: Option<i64>,
    pub key_tag: Option<String>,
    pub duration_ms: Option<i64>,
}

/// Turns what the user typed into an FTS5 query.
///
/// Every word becomes a prefix term, so results narrow as they type rather than
/// appearing only once a word is finished. Terms are quoted, which both escapes
/// the punctuation that litters track names — hyphens, ampersands, parentheses —
/// and stops a stray `OR` or `NEAR` in a title being read as an operator.
///
/// Returns `None` for input with nothing to match, so callers can skip the query
/// entirely rather than asking SQLite to match on emptiness.
pub fn to_fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

/// Adds a file to the search index, replacing any entry it already had.
///
/// Called after a scan learns a file exists and again after a probe reads its
/// tags, so a row is searchable by name immediately and by artist once known.
pub fn index_file(db: &Db, file_id: i64) -> Result<(), IndexError> {
    db.conn()
        .execute("DELETE FROM file_fts WHERE rowid = ?1", params![file_id])?;

    db.conn().execute(
        "INSERT INTO file_fts (rowid, name, artist, title, album, path)
         SELECT f.id,
                f.name,
                COALESCE(ct.artist, ''),
                COALESCE(ct.title, ''),
                COALESCE(ct.album, ''),
                d.path_lower
         FROM file f
         JOIN dir d ON d.id = f.dir_id
         LEFT JOIN content_tag ct ON ct.content_id = f.content_id
         WHERE f.id = ?1",
        params![file_id],
    )?;
    Ok(())
}

/// Drops a file from the search index.
pub fn unindex_file(db: &Db, file_id: i64) -> Result<(), IndexError> {
    db.conn()
        .execute("DELETE FROM file_fts WHERE rowid = ?1", params![file_id])?;
    Ok(())
}

/// Indexes every file of a directory that is not in the search index yet.
///
/// Cheap to call on every scan: files already indexed are skipped, so reopening a
/// folder costs one query rather than a rewrite of its whole listing.
pub fn index_new_in_dir(db: &Db, dir_id: i64) -> Result<usize, IndexError> {
    let n = db.conn().execute(
        "INSERT INTO file_fts (rowid, name, artist, title, album, path)
         SELECT f.id,
                f.name,
                COALESCE(ct.artist, ''),
                COALESCE(ct.title, ''),
                COALESCE(ct.album, ''),
                d.path_lower
         FROM file f
         JOIN dir d ON d.id = f.dir_id
         LEFT JOIN content_tag ct ON ct.content_id = f.content_id
         WHERE f.dir_id = ?1
           AND f.id NOT IN (SELECT rowid FROM file_fts)",
        params![dir_id],
    )?;
    Ok(n)
}

/// Searches the library, optionally restricted to a set of directory subtrees.
///
/// `scopes` are lowercase path prefixes. An empty list searches everything, which
/// is what a library with nothing pinned should do rather than nothing at all.
///
/// Ordering is FTS5's own relevance (`bm25`), with directories after tracks: the
/// spec groups results that way, and a folder is rarely what you meant when you
/// typed an artist's name.
pub fn search(db: &Db, query: &str, scopes: &[String], limit: i64) -> Result<Vec<Hit>, IndexError> {
    let Some(fts) = to_fts_query(query) else {
        return Ok(Vec::new());
    };

    // One LIKE per scope, ORed. Prefix patterns like `e:\music%` use the index on
    // `dir(path_lower)`, so scoping narrows the work rather than adding to it.
    let scope_sql = if scopes.is_empty() {
        String::new()
    } else {
        let clauses: Vec<String> = (0..scopes.len())
            .map(|i| format!("d.path_lower LIKE ?{} ESCAPE '\\'", i + 3))
            .collect();
        format!(" AND ({})", clauses.join(" OR "))
    };

    let sql = format!(
        "SELECT f.id, f.name, d.path_lower, f.kind,
                ct.artist, ct.title, ct.bpm_tag, ct.key_norm, ct.key_tag,
                c.duration_ms
         FROM file_fts
         JOIN file f ON f.id = file_fts.rowid
         JOIN dir  d ON d.id = f.dir_id
         LEFT JOIN content     c  ON c.id  = f.content_id
         LEFT JOIN content_tag ct ON ct.content_id = f.content_id
         WHERE file_fts MATCH ?1{scope_sql}
         ORDER BY (f.kind = 0), bm25(file_fts), f.name COLLATE NOCASE
         LIMIT ?2"
    );

    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts), Box::new(limit)];
    for scope in scopes {
        values.push(Box::new(format!("{}%", like_prefix(scope))));
    }

    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), hit)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// A tag the sidebar lists the library by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagColumn {
    Artist,
    Genre,
    Label,
}

impl TagColumn {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "artist" => Some(Self::Artist),
            "genre" => Some(Self::Genre),
            "label" => Some(Self::Label),
            _ => None,
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Artist => "ct.artist",
            Self::Genre => "ct.genre",
            Self::Label => "ct.label",
        }
    }
}

/// Every track whose tag is exactly `value`, ignoring case and outer spaces.
///
/// Exact rather than full-text: "Techno" must not bring back "Hard Techno" or a
/// track called "Techno Bunker". Scoped like [`search`], ordered by name.
pub fn search_tag(
    db: &Db,
    column: TagColumn,
    value: &str,
    scopes: &[String],
    limit: i64,
) -> Result<Vec<Hit>, IndexError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let scope_sql = if scopes.is_empty() {
        String::new()
    } else {
        let clauses: Vec<String> = (0..scopes.len())
            .map(|i| format!("d.path_lower LIKE ?{} ESCAPE '\\'", i + 3))
            .collect();
        format!(" AND ({})", clauses.join(" OR "))
    };
    let col = column.column();
    let sql = format!(
        "SELECT f.id, f.name, d.path_lower, f.kind,
                ct.artist, ct.title, ct.bpm_tag, ct.key_norm, ct.key_tag,
                c.duration_ms
         FROM content_tag ct
         JOIN file f ON f.content_id = ct.content_id
         JOIN dir  d ON d.id = f.dir_id
         LEFT JOIN content c ON c.id = f.content_id
         WHERE f.kind = 1 AND TRIM({col}) = ?1 COLLATE NOCASE{scope_sql}
         ORDER BY f.name COLLATE NOCASE
         LIMIT ?2"
    );
    let mut values: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(value.to_string()), Box::new(limit)];
    for scope in scopes {
        values.push(Box::new(format!("{}%", like_prefix(scope))));
    }
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), hit)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn hit(r: &rusqlite::Row<'_>) -> rusqlite::Result<Hit> {
    Ok(Hit {
        file_id: r.get(0)?,
        name: r.get(1)?,
        dir_path: r.get(2)?,
        kind: r.get(3)?,
        artist: r.get(4)?,
        title: r.get(5)?,
        bpm: r.get(6)?,
        key_norm: r.get(7)?,
        key_tag: r.get(8)?,
        duration_ms: r.get(9)?,
    })
}

/// Escapes the wildcards LIKE would otherwise read as pattern syntax.
///
/// Folder names really do contain `%` and `_`, and an unescaped one silently
/// widens the scope to places the user did not ask about.
pub(crate) fn like_prefix(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .to_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_becomes_a_prefix_term() {
        assert_eq!(
            to_fts_query("dark matter"),
            Some("\"dark\"* \"matter\"*".into())
        );
    }

    /// Track names are full of punctuation; none of it should reach FTS5 as syntax.
    #[test]
    fn punctuation_is_stripped_rather_than_passed_through() {
        assert_eq!(
            to_fts_query("A - Shcaa (Masquée) [2019]"),
            Some("\"A\"* \"Shcaa\"* \"Masquée\"* \"2019\"*".into())
        );
    }

    /// A bare `OR` in a title is a word the user typed, not an operator.
    #[test]
    fn fts_operators_in_the_query_are_treated_as_words() {
        assert_eq!(
            to_fts_query("love OR war"),
            Some("\"love\"* \"OR\"* \"war\"*".into())
        );
    }

    #[test]
    fn input_with_nothing_to_match_yields_no_query() {
        assert_eq!(to_fts_query(""), None);
        assert_eq!(to_fts_query("   "), None);
        assert_eq!(to_fts_query("-- ()"), None);
    }

    #[test]
    fn like_wildcards_in_a_scope_are_escaped() {
        assert_eq!(like_prefix("E:\\100%_mix\\"), "e:\\\\100\\%\\_mix");
    }

    /// A genre link lists that genre, whatever its case, and not the genres
    /// that merely contain the word.
    #[test]
    fn a_tag_search_matches_the_whole_value_only() {
        let db = Db::open_in_memory().unwrap();
        db.conn()
            .execute_batch(
                "INSERT INTO volume (id, kind, fs_uuid, mount_path)
                 VALUES (1, 'fixed', 'u1', 'E:\\');
                 INSERT INTO dir (id, volume_id, name, path_lower)
                 VALUES (1, 1, 'music', 'e:\\music'), (2, 1, 'other', 'f:\\other');
                 INSERT INTO content (id, quick_key) VALUES (1, 'k1'), (2, 'k2'), (3, 'k3'), (4, 'k4');
                 INSERT INTO content_tag (content_id, genre, label) VALUES
                     (1, 'Techno', 'Ilian Tape'), (2, ' techno ', NULL),
                     (3, 'Hard Techno', NULL), (4, 'Techno', NULL);
                 INSERT INTO file (id, dir_id, name, kind, content_id) VALUES
                     (1, 1, 'b.wav', 1, 1), (2, 1, 'a.wav', 1, 2),
                     (3, 1, 'c.wav', 1, 3), (4, 2, 'd.wav', 1, 4);",
            )
            .unwrap();

        let names = |hits: Vec<Hit>| hits.into_iter().map(|h| h.name).collect::<Vec<_>>();
        let all = search_tag(&db, TagColumn::Genre, "TECHNO", &[], 50).unwrap();
        assert_eq!(names(all), ["a.wav", "b.wav", "d.wav"]);

        let scoped =
            search_tag(&db, TagColumn::Genre, "Techno", &["E:\\music".into()], 50).unwrap();
        assert_eq!(names(scoped), ["a.wav", "b.wav"]);

        let label = search_tag(&db, TagColumn::Label, "ilian tape", &[], 50).unwrap();
        assert_eq!(names(label), ["b.wav"]);
        assert!(search_tag(&db, TagColumn::Artist, "  ", &[], 50)
            .unwrap()
            .is_empty());
    }
}

/// Tags of one file, for the transport bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tags {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub bpm: Option<f64>,
    pub key_norm: Option<i64>,
    pub key_tag: Option<String>,
    /// From the container rather than the tags, for the spectrogram's axis.
    pub sample_rate: Option<i64>,
}

/// Reads the tags of one file.
///
/// The transport shows artist, BPM and key beside what is playing, and reading
/// them once when a track is loaded is the alternative to re-querying them on
/// every position poll.
pub fn tags_of(db: &Db, file_id: i64) -> Result<Tags, IndexError> {
    use rusqlite::OptionalExtension;

    Ok(db
        .conn()
        .query_row(
            "SELECT ct.artist, ct.title, ct.bpm_tag, ct.key_norm, ct.key_tag,
                    c.sample_rate
             FROM file f
             LEFT JOIN content     c  ON c.id  = f.content_id
             LEFT JOIN content_tag ct ON ct.content_id = f.content_id
             WHERE f.id = ?1",
            params![file_id],
            |r| {
                Ok(Tags {
                    artist: r.get(0)?,
                    title: r.get(1)?,
                    bpm: r.get(2)?,
                    key_norm: r.get(3)?,
                    key_tag: r.get(4)?,
                    sample_rate: r.get(5)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}
