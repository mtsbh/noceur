//! Windowed, sorted reads of a directory listing.

use crate::db::{Db, IndexError};

/// Column the list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Modified,
    Bpm,
    Key,
    Duration,
    Artist,
    Title,
    Album,
    AlbumArtist,
    Genre,
    Year,
    TrackNo,
    Comment,
    Isrc,
    SampleRate,
    BitDepth,
    Codec,
}

impl SortColumn {
    /// Every column, in the order the list shows them.
    pub fn all() -> [SortColumn; 18] {
        [
            SortColumn::Name,
            SortColumn::Bpm,
            SortColumn::Key,
            SortColumn::Duration,
            SortColumn::Artist,
            SortColumn::Title,
            SortColumn::Album,
            SortColumn::AlbumArtist,
            SortColumn::Genre,
            SortColumn::Year,
            SortColumn::TrackNo,
            SortColumn::Comment,
            SortColumn::Isrc,
            SortColumn::SampleRate,
            SortColumn::BitDepth,
            SortColumn::Codec,
            SortColumn::Size,
            SortColumn::Modified,
        ]
    }

    /// The name this column travels under on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            SortColumn::Name => "name",
            SortColumn::Size => "size",
            SortColumn::Modified => "modified",
            SortColumn::Bpm => "bpm",
            SortColumn::Key => "key",
            SortColumn::Duration => "duration",
            SortColumn::Artist => "artist",
            SortColumn::Title => "title",
            SortColumn::Album => "album",
            SortColumn::AlbumArtist => "albumArtist",
            SortColumn::Genre => "genre",
            SortColumn::Year => "year",
            SortColumn::TrackNo => "trackNo",
            SortColumn::Comment => "comment",
            SortColumn::Isrc => "isrc",
            SortColumn::SampleRate => "sampleRate",
            SortColumn::BitDepth => "bitDepth",
            SortColumn::Codec => "codec",
        }
    }

    /// Parses a wire name back, or `None` for anything unrecognised.
    ///
    /// The caller decides what to do with `None`; falling back to `Name` silently
    /// would hide a frontend that is asking for a column that does not exist.
    pub fn parse(s: &str) -> Option<Self> {
        SortColumn::all().into_iter().find(|c| c.as_str() == s)
    }

    /// SQL expression to sort by. Tag columns are NULL for directories, which
    /// SQLite orders first — acceptable because the group clause runs first.
    fn sql(self) -> &'static str {
        match self {
            SortColumn::Name => "f.name COLLATE NOCASE",
            SortColumn::Size => "f.size",
            SortColumn::Modified => "f.mtime",
            SortColumn::Bpm => "ct.bpm_tag",
            SortColumn::Key => "ct.key_norm",
            SortColumn::Duration => "c.duration_ms",
            SortColumn::Artist => "ct.artist COLLATE NOCASE",
            SortColumn::Title => "ct.title COLLATE NOCASE",
            SortColumn::Album => "ct.album COLLATE NOCASE",
            SortColumn::AlbumArtist => "ct.album_artist COLLATE NOCASE",
            SortColumn::Genre => "ct.genre COLLATE NOCASE",
            SortColumn::Year => "ct.year",
            SortColumn::TrackNo => "ct.track_no",
            SortColumn::Comment => "ct.comment COLLATE NOCASE",
            SortColumn::Isrc => "ct.isrc COLLATE NOCASE",
            SortColumn::SampleRate => "c.sample_rate",
            SortColumn::BitDepth => "c.bit_depth",
            SortColumn::Codec => "c.codec COLLATE NOCASE",
        }
    }
}

/// Which rows a listing includes.
///
/// Three states rather than an "audio only" flag, because hiding non-audio
/// files and hiding *everything* that is not audio are different requests and
/// only one of them leaves a folder you can navigate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Show {
    /// Every row.
    #[default]
    Everything,
    /// Audio, and the folders that lead to more of it. Drops the cover scans,
    /// cue sheets, `.nfo`s and stray archives a download leaves behind.
    Tracks,
    /// Audio alone — folders included in what is dropped. For a flat listing
    /// that is going to be read rather than walked.
    AudioOnly,
}

impl Show {
    /// The `WHERE` fragment this appends to a listing query.
    ///
    /// `f.kind` is [`EntryKind::group`]: 0 directory, 1 audio, 2 anything else.
    fn sql(self) -> &'static str {
        match self {
            Show::Everything => "",
            Show::Tracks => " AND f.kind != 2",
            Show::AudioOnly => " AND f.kind = 1",
        }
    }
}

/// How to slice and order a directory listing.
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub sort: SortColumn,
    pub desc: bool,
    pub show: Show,
    pub offset: i64,
    /// Rows to return. `-1` means no limit.
    pub limit: i64,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions {
            sort: SortColumn::Name,
            desc: false,
            show: Show::Everything,
            offset: 0,
            limit: -1,
        }
    }
}

/// One row as the UI sees it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
    pub file_id: i64,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub mtime: i64,
    /// `EntryKind::group()`: 0 directory, 1 audio, 2 other.
    pub kind: i64,
    pub state: String,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub bpm: Option<f64>,
    pub key_tag: Option<String>,
    pub key_norm: Option<i64>,
    pub duration_ms: Option<i64>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track_no: Option<i64>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub codec: Option<String>,
    /// Whether the file carries embedded cover art, from the last probe.
    pub has_artwork: bool,
}

const SELECT: &str = "
SELECT f.id, f.name, f.ext, f.size, f.mtime, f.kind, f.state,
       ct.artist, ct.title, ct.album, ct.bpm_tag, ct.key_tag, ct.key_norm,
       c.duration_ms, COALESCE(ct.has_artwork, 0),
       ct.album_artist, ct.genre, ct.year, ct.track_no, ct.comment, ct.isrc,
       c.sample_rate, c.bit_depth, c.codec
FROM file f
LEFT JOIN content     c  ON c.id  = f.content_id
LEFT JOIN content_tag ct ON ct.content_id = f.content_id
WHERE f.dir_id = ?1";

/// Reads a window of a sorted, filtered directory listing.
///
/// Grouping (directories, audio, other) is applied before the sort column and is
/// never reversed by `desc`.
pub fn list(db: &Db, dir_id: i64, opts: ListOptions) -> Result<Vec<ListRow>, IndexError> {
    let sql = format!(
        "{SELECT}{audio}
         ORDER BY f.kind ASC, {col} {dir}, f.name COLLATE NOCASE ASC
         LIMIT ?2 OFFSET ?3",
        audio = opts.show.sql(),
        col = opts.sort.sql(),
        dir = if opts.desc { "DESC" } else { "ASC" },
    );

    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![dir_id, opts.limit, opts.offset], |r| {
        Ok(ListRow {
            file_id: r.get(0)?,
            name: r.get(1)?,
            ext: r.get(2)?,
            size: r.get(3)?,
            mtime: r.get(4)?,
            kind: r.get(5)?,
            state: r.get(6)?,
            artist: r.get(7)?,
            title: r.get(8)?,
            album: r.get(9)?,
            bpm: r.get(10)?,
            key_tag: r.get(11)?,
            key_norm: r.get(12)?,
            duration_ms: r.get(13)?,
            has_artwork: r.get::<_, i64>(14)? != 0,
            album_artist: r.get(15)?,
            genre: r.get(16)?,
            year: r.get(17)?,
            track_no: r.get(18)?,
            comment: r.get(19)?,
            isrc: r.get(20)?,
            sample_rate: r.get(21)?,
            bit_depth: r.get(22)?,
            codec: r.get(23)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Total rows in the view, ignoring `offset` and `limit`.
pub fn count(db: &Db, dir_id: i64, opts: ListOptions) -> Result<i64, IndexError> {
    let sql = format!(
        "SELECT COUNT(*) FROM file f WHERE f.dir_id = ?1{}",
        opts.show.sql()
    );
    Ok(db.conn().query_row(&sql, [dir_id], |r| r.get(0))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{probe_dir, scan_dir};
    use crate::volume::{self, VolumeKind};
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_wav(path: &Path) {
        let sr = 44_100u32;
        let data_len = sr / 10 * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&sr.to_le_bytes());
        f.extend_from_slice(&(sr * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        fs::write(path, f).unwrap();
    }

    fn make(dir: &Path, name: &str, artist: &str, bpm: &str, key: &str) {
        let p = dir.join(name);
        write_wav(&p);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(name.to_string());
        // IntegerBpm, not Bpm — ID3v2 maps only TBPM and silently drops Bpm. See Amendment 4.
        tag.insert_text(ItemKey::IntegerBpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(&p, WriteOptions::default()).unwrap();
    }

    fn setup() -> (Db, TempDir, i64) {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Zebra folder")).unwrap();
        make(d.path(), "c.wav", "Carl", "128", "8A");
        make(d.path(), "a.wav", "Alice", "124", "5A");
        make(d.path(), "b.wav", "Bob", "126", "11B");
        fs::write(d.path().join("aaa.txt"), b"x").unwrap();

        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();
        (db, d, dir_id)
    }

    fn names(rows: &[ListRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn groups_directories_first_then_audio_then_other() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        assert_eq!(
            names(&rows),
            ["Zebra folder", "a.wav", "b.wav", "c.wav", "aaa.txt"]
        );
    }

    #[test]
    fn grouping_survives_descending_order() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions {
                desc: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows[0].name, "Zebra folder", "dir stays first");
        assert_eq!(names(&rows)[1..4], ["c.wav", "b.wav", "a.wav"]);
    }

    #[test]
    fn sorts_by_bpm() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions {
                sort: SortColumn::Bpm,
                ..Default::default()
            },
        )
        .unwrap();
        let audio: Vec<&str> = rows
            .iter()
            .filter(|r| r.kind == 1)
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(audio, ["a.wav", "b.wav", "c.wav"]);
    }

    #[test]
    fn sorts_by_normalised_key_not_alphabetically() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions {
                sort: SortColumn::Key,
                ..Default::default()
            },
        )
        .unwrap();
        let audio: Vec<&str> = rows
            .iter()
            .filter(|r| r.kind == 1)
            .map(|r| r.name.as_str())
            .collect();
        // 5A (canonical 8) < 8A (14) < 11B (21)
        assert_eq!(audio, ["a.wav", "c.wav", "b.wav"]);
    }

    /// The setting behind "hide files that are not audio". The folder has to
    /// survive it: a browser that filters away the thing you navigate with has
    /// filtered away the browser.
    #[test]
    fn tracks_hides_other_files_but_keeps_the_folders() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions {
                show: Show::Tracks,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(names(&rows), ["Zebra folder", "a.wav", "b.wav", "c.wav"]);
        assert_eq!(
            count(
                &db,
                dir_id,
                ListOptions {
                    show: Show::Tracks,
                    ..Default::default()
                }
            )
            .unwrap(),
            4,
            "the count has to agree with the rows, or the list scrolls past its end"
        );
    }

    #[test]
    fn audio_only_hides_directories_and_other_files() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions {
                show: Show::AudioOnly,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(names(&rows), ["a.wav", "b.wav", "c.wav"]);
    }

    /// The header sends a column name back over the wire; if the two spellings
    /// drift, clicking a column silently sorts by something else.
    #[test]
    fn every_sort_column_round_trips_through_its_wire_name() {
        for column in SortColumn::all() {
            assert_eq!(SortColumn::parse(column.as_str()), Some(column));
        }
        assert_eq!(SortColumn::parse("loudness"), None);
    }

    #[test]
    fn sort_column_names_are_distinct() {
        let mut names: Vec<&str> = SortColumn::all().iter().map(|c| c.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "two columns share a wire name");
    }

    #[test]
    fn windows_return_a_slice_of_the_same_global_order() {
        let (db, _d, dir_id) = setup();
        let all = list(&db, dir_id, ListOptions::default()).unwrap();
        let window = list(
            &db,
            dir_id,
            ListOptions {
                offset: 1,
                limit: 2,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(names(&window), names(&all)[1..3]);
    }

    /// The frontend caches windows by page and stitches them together, so
    /// consecutive windows must tile the listing exactly — no row repeated at a
    /// seam, none skipped, and a short final window where the rows run out.
    #[test]
    fn consecutive_windows_tile_the_listing_exactly() {
        let (db, _d, dir_id) = setup();
        let all = list(&db, dir_id, ListOptions::default()).unwrap();

        for page in [1, 2, 3, 5, 8] {
            let mut stitched: Vec<String> = Vec::new();
            let mut offset = 0;
            loop {
                let window = list(
                    &db,
                    dir_id,
                    ListOptions {
                        offset,
                        limit: page,
                        ..Default::default()
                    },
                )
                .unwrap();
                if window.is_empty() {
                    break;
                }
                assert!(
                    window.len() as i64 <= page,
                    "window of {page} returned {} rows",
                    window.len()
                );
                stitched.extend(window.iter().map(|r| r.name.clone()));
                offset += page;
            }
            assert_eq!(stitched, names(&all), "windows of {page} did not tile");
        }
    }

    #[test]
    fn rows_carry_tag_values() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let a = rows.iter().find(|r| r.name == "a.wav").unwrap();
        assert_eq!(a.artist.as_deref(), Some("Alice"));
        assert_eq!(a.bpm, Some(124.0));
        assert_eq!(a.key_tag.as_deref(), Some("5A"));
        assert!(a.duration_ms.unwrap_or(0) > 0);
    }

    #[test]
    fn directories_have_no_tag_values() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let dir = rows.iter().find(|r| r.kind == 0).unwrap();
        assert!(dir.bpm.is_none());
        assert!(dir.artist.is_none());
    }

    #[test]
    fn counts_the_full_view_not_the_window() {
        let (db, _d, dir_id) = setup();
        assert_eq!(count(&db, dir_id, ListOptions::default()).unwrap(), 5);
        assert_eq!(
            count(
                &db,
                dir_id,
                ListOptions {
                    show: Show::AudioOnly,
                    ..Default::default()
                }
            )
            .unwrap(),
            3
        );
    }

    #[test]
    fn list_rows_serialize_to_json() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let json = serde_json::to_string(&rows).expect("ListRow must serialize");
        assert!(json.contains("\"name\""));
        assert!(
            json.contains("\"keyNorm\""),
            "fields must be camelCase for the UI"
        );
    }
}
