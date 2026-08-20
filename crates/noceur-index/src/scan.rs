//! Scan stages: enumerate into rows, then probe tags.

use crate::db::{Db, IndexError};
use std::collections::HashSet;
use std::path::Path;

use crate::quickkey;
use crate::volume::now;
use noceur_fsmodel::{read_dir, EntryKind};
use rusqlite::params;

/// Canonical key for a directory within a volume.
///
/// Trailing separators are stripped so `C:\Music` and `C:\Music\` resolve to one
/// row. Case is folded only on Windows, whose filesystems are case-insensitive;
/// on Unix `/Music` and `/music` are genuinely different directories and must not
/// collapse into one.
fn path_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['/', '\\']);
    // A bare root such as `C:\` or `/` must not trim away to nothing.
    let trimmed = if trimmed.is_empty() {
        s.as_ref()
    } else {
        trimmed
    };
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

/// Stage 1: enumerate `path` and write `dir` and `file` rows.
///
/// Returns the `dir.id`. Entries that have vanished since the last scan are
/// removed; entries that are new are inserted; the rest are updated in place so
/// `content_id` links survive a rescan.
pub fn scan_dir(db: &Db, volume_id: i64, path: &Path) -> Result<i64, IndexError> {
    // `read_dir` returns a `DirListing` carrying both entries and any that were
    // skipped (see Amendment 1). Stage 1 ignores skips; the UI surfaces them later.
    let listing = read_dir(path)?;
    let entries = listing.entries;

    let path_lower = path_key(path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_lower.clone());

    let tx = db.conn().unchecked_transaction()?;

    tx.execute(
        "INSERT INTO dir (volume_id, parent_id, name, path_lower, mtime, scan_state)
         VALUES (?1, NULL, ?2, ?3, ?4, 'scanned')
         ON CONFLICT(volume_id, path_lower) DO UPDATE SET
             mtime = excluded.mtime, scan_state = 'scanned'",
        params![volume_id, name, path_lower, now()],
    )?;

    let dir_id: i64 = tx.query_row(
        "SELECT id FROM dir WHERE volume_id = ?1 AND path_lower = ?2",
        params![volume_id, path_lower],
        |r| r.get(0),
    )?;

    // What the shell hides never enters the index. `$RECYCLE.BIN` and
    // `System Volume Information` are not part of anybody's library, and a
    // listing that includes them is a listing you have to read past.
    let entries: Vec<_> = entries.into_iter().filter(|e| !e.hidden).collect();

    for e in &entries {
        tx.execute(
            "INSERT INTO file (dir_id, name, ext, size, mtime, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(dir_id, name) DO UPDATE SET
                 ext = excluded.ext, size = excluded.size,
                 mtime = excluded.mtime, kind = excluded.kind,
                 -- A file the probe gave up on is skipped from then on, so that
                 -- one corrupt file cannot be retried on every pass. Rewriting it
                 -- changes its mtime and earns it another look.
                 state = CASE WHEN excluded.mtime <> file.mtime
                              THEN 'ok' ELSE file.state END",
            params![
                dir_id,
                e.name,
                e.ext,
                e.size as i64,
                e.mtime,
                e.kind.group() as i64
            ],
        )?;
    }

    // Remove rows for entries that no longer exist.
    let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut stmt = tx.prepare("SELECT id, name FROM file WHERE dir_id = ?1")?;
    let stale: Vec<i64> = stmt
        .query_map([dir_id], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .filter_map(Result::ok)
        .filter(|(_, n)| !names.contains(n.as_str()))
        .map(|(id, _)| id)
        .collect();
    drop(stmt);

    for id in stale {
        tx.execute("DELETE FROM file WHERE id = ?1", [id])?;
    }

    tx.commit()?;
    Ok(dir_id)
}

/// Stage 2: probe tags for audio files in `dir_id` that have never been probed,
/// or whose mtime is newer than the last probe.
///
/// Files whose `content_id` is already linked and whose tags were last read at
/// or after their current mtime are skipped — they are unchanged since the last
/// probe. This keeps rescans of an SMB share incremental instead of re-hashing
/// and re-reading tags for every audio file every time.
///
/// Failures mark the individual row and continue — one corrupt file must never
/// cost the folder.
pub fn probe_dir(db: &Db, dir_id: i64) -> Result<(), IndexError> {
    while probe_batch(db, dir_id, PROBE_BATCH)? > 0 {}
    Ok(())
}

/// Files one batch reads before the caller gets a chance to release the database.
///
/// Small enough that a listing query or a thumbnail request waits milliseconds
/// rather than minutes, large enough that the per-batch query is noise.
pub const PROBE_BATCH: usize = 32;

/// Probes at most `limit` unprobed files, returning how many it attempted.
///
/// Exists so a caller can probe a large folder without holding the database for
/// the whole of it. A 667-file folder took thirty-five seconds to probe, and for
/// that entire time nothing else could read the index: not the listing that was
/// waiting to paint, not the cover art beside it. Batching turns one long
/// exclusive hold into many short ones.
///
/// Returns 0 when there is nothing left to do, which is the caller's cue to stop.
pub fn probe_batch(db: &Db, dir_id: i64, limit: usize) -> Result<usize, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT f.id, f.name, d.path_lower, f.mtime
         FROM file f
         JOIN dir d ON d.id = f.dir_id
         LEFT JOIN content_tag ct ON ct.content_id = f.content_id
         WHERE f.dir_id = ?1
           AND f.kind = ?2
           AND f.state NOT IN ('unreadable', 'undecodable')
           AND (f.content_id IS NULL OR ct.read_at IS NULL OR f.mtime > ct.read_at)
         LIMIT ?3",
    )?;
    let targets: Vec<(i64, String, String, i64)> = stmt
        .query_map(
            params![dir_id, EntryKind::Audio.group() as i64, limit as i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    let attempted = targets.len();

    for (file_id, name, dir_path, file_mtime) in targets {
        let full = Path::new(&dir_path).join(&name);

        let quick = match quickkey::compute(&full) {
            Ok(q) => q,
            Err(_) => {
                mark_state(db, file_id, "unreadable")?;
                continue;
            }
        };

        let meta = match noceur_tags::read(&full) {
            Ok(m) => m,
            Err(_) => {
                mark_state(db, file_id, "undecodable")?;
                continue;
            }
        };

        db.conn().execute(
            "INSERT INTO content (quick_key, byte_size, duration_ms, sample_rate, channels, bit_depth)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(quick_key) DO UPDATE SET
                 duration_ms = excluded.duration_ms,
                 sample_rate = excluded.sample_rate,
                 channels    = excluded.channels,
                 bit_depth   = excluded.bit_depth",
            params![
                quick,
                full.metadata().map(|m| m.len() as i64).unwrap_or(0),
                meta.duration_ms.map(|v| v as i64),
                meta.sample_rate.map(|v| v as i64),
                meta.channels.map(|v| v as i64),
                meta.bit_depth.map(|v| v as i64),
            ],
        )?;

        let content_id: i64 = db.conn().query_row(
            "SELECT id FROM content WHERE quick_key = ?1",
            [&quick],
            |r| r.get(0),
        )?;

        db.conn().execute(
            "INSERT INTO content_tag (content_id, artist, title, album, album_artist, genre,
                                      year, track_no, comment, isrc, bpm_tag, key_tag,
                                      key_norm, has_artwork, read_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(content_id) DO UPDATE SET
                 artist=excluded.artist, title=excluded.title, album=excluded.album,
                 album_artist=excluded.album_artist, genre=excluded.genre,
                 year=excluded.year, track_no=excluded.track_no, comment=excluded.comment,
                 isrc=excluded.isrc, bpm_tag=excluded.bpm_tag, key_tag=excluded.key_tag,
                 key_norm=excluded.key_norm, has_artwork=excluded.has_artwork,
                 read_at=excluded.read_at",
            params![
                content_id,
                meta.artist,
                meta.title,
                meta.album,
                meta.album_artist,
                meta.genre,
                meta.year.map(|v| v as i64),
                meta.track_no.map(|v| v as i64),
                meta.comment,
                meta.isrc,
                meta.bpm.map(|v| v as f64),
                meta.key_tag,
                meta.key_norm.map(|v| v as i64),
                meta.has_artwork as i64,
                // Never behind the mtime we just read the file at.
                //
                // Staleness is `f.mtime > ct.read_at`. Recording only the wall
                // clock leaves a file whose mtime is in the future — a bad clock,
                // an SMB share, a copy that preserved timestamps — permanently
                // stale, and a caller that probes until nothing is stale would
                // re-read it forever. Recording the mtime we actually read says
                // what is true: the tags are current as of that stamp.
                now().max(file_mtime),
            ],
        )?;

        db.conn().execute(
            "UPDATE file SET content_id = ?1, state = 'ok' WHERE id = ?2",
            params![content_id, file_id],
        )?;
    }
    Ok(attempted)
}

fn mark_state(db: &Db, file_id: i64, state: &str) -> Result<(), IndexError> {
    db.conn().execute(
        "UPDATE file SET state = ?1 WHERE id = ?2",
        params![state, file_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::{self, VolumeKind};
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};
    use std::fs;
    use tempfile::TempDir;

    fn write_wav(path: &Path) {
        let sample_rate = 44_100u32;
        let samples = sample_rate / 10;
        let data_len = samples * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&sample_rate.to_le_bytes());
        f.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        fs::write(path, f).unwrap();
    }

    fn tag_it(path: &Path, artist: &str, title: &str, bpm: &str, key: &str) {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(title.to_string());
        // ID3v2 only maps `IntegerBpm` to a frame (`TBPM`); `ItemKey::Bpm` has
        // no ID3v2 mapping at all and is silently dropped on save (see
        // Amendment 4 in the plan, and `noceur_tags::meta`'s own fixture).
        tag.insert_text(ItemKey::IntegerBpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    /// Builds a tree: 1 subdir, 2 tagged wavs, 1 untagged wav, 1 txt.
    fn tree() -> TempDir {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Unsorted")).unwrap();

        let a = d.path().join("a.wav");
        write_wav(&a);
        tag_it(&a, "G76", "it takes time", "123", "8A");

        let b = d.path().join("b.wav");
        write_wav(&b);
        tag_it(&b, "Lizz", "Codex", "124.5", "Am");

        write_wav(&d.path().join("bare.wav"));
        fs::write(d.path().join("notes.txt"), b"hello").unwrap();
        d
    }

    fn setup() -> (Db, TempDir, i64) {
        let db = Db::open_in_memory().unwrap();
        let d = tree();
        let vol = volume::upsert(&db, "uuid-test", VolumeKind::Fixed, "test", "/").unwrap();
        (db, d, vol)
    }

    fn count(db: &Db, sql: &str) -> i64 {
        db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn stage_one_writes_a_row_per_entry() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        assert!(dir_id > 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 5);
    }

    #[test]
    fn stage_one_records_the_entry_kind() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM file WHERE kind = 0"),
            1,
            "dirs"
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM file WHERE kind = 1"),
            3,
            "audio"
        );
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM file WHERE kind = 2"),
            1,
            "other"
        );
    }

    #[test]
    fn rescanning_does_not_duplicate_rows() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 5);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM dir"), 1);
    }

    #[test]
    fn rescanning_drops_entries_that_disappeared() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        fs::remove_file(d.path().join("notes.txt")).unwrap();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 4);
    }

    #[test]
    fn stage_two_links_audio_files_to_content() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();
        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM file WHERE content_id IS NOT NULL"
            ),
            3,
            "only audio files get content rows"
        );
    }

    #[test]
    fn stage_two_stores_normalised_tags() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let (artist, bpm, key_tag, key_norm): (String, f64, String, i64) = db
            .conn()
            .query_row(
                "SELECT ct.artist, ct.bpm_tag, ct.key_tag, ct.key_norm
                 FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'b.wav'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();

        assert_eq!(artist, "Lizz");
        assert!((bpm - 124.5).abs() < 0.001);
        assert_eq!(key_tag, "Am", "original text preserved");
        assert_eq!(key_norm, 14, "Am normalises to Camelot 8A");
    }

    #[test]
    fn identical_copies_share_one_content_row() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

        let src = d.path().join("original.wav");
        write_wav(&src);
        tag_it(&src, "G76", "it takes time", "123", "8A");
        fs::copy(&src, d.path().join("copy.wav")).unwrap();

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 2);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM content"),
            1,
            "one audio, one content row"
        );
    }

    #[test]
    fn untagged_audio_still_gets_a_content_row() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let state: String = db
            .conn()
            .query_row("SELECT state FROM file WHERE name = 'bare.wav'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(state, "ok");
    }

    #[test]
    fn a_file_that_is_not_really_audio_is_marked_undecodable() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
        fs::write(d.path().join("fake.wav"), b"this is not a wav file").unwrap();

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let state: String = db
            .conn()
            .query_row("SELECT state FROM file WHERE name = 'fake.wav'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(state, "undecodable");
    }

    #[test]
    fn one_bad_file_does_not_stop_the_others() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

        fs::write(d.path().join("fake.wav"), b"garbage").unwrap();
        let good = d.path().join("good.wav");
        write_wav(&good);
        tag_it(&good, "G76", "fine", "120", "8A");

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        assert_eq!(
            count(
                &db,
                "SELECT COUNT(*) FROM file WHERE state = 'ok' AND content_id IS NOT NULL"
            ),
            1
        );
    }

    #[test]
    fn probing_twice_does_not_reprobe_unchanged_files() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let read_at: i64 = db
            .conn()
            .query_row(
                "SELECT ct.read_at FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        probe_dir(&db, dir_id).unwrap();

        let read_at_again: i64 = db
            .conn()
            .query_row(
                "SELECT ct.read_at FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            read_at, read_at_again,
            "unchanged file must not be reprobed"
        );
    }

    #[test]
    fn a_file_modified_since_probing_is_reprobed() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let read_at: i64 = db
            .conn()
            .query_row(
                "SELECT ct.read_at FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        // Rekordbox-style re-tag: change the artist on disk (Rekordbox rewrites
        // ID3 at the head of the file, so the quick key changes too), then
        // simulate the later mtime the OS would report — without touching the
        // filesystem clock.
        tag_it(
            &d.path().join("a.wav"),
            "New Artist",
            "it takes time",
            "123",
            "8A",
        );
        let future_mtime = read_at + 1_000_000;
        db.conn()
            .execute(
                "UPDATE file SET mtime = ?1 WHERE name = 'a.wav'",
                params![future_mtime],
            )
            .unwrap();

        probe_dir(&db, dir_id).unwrap();

        let artist: String = db
            .conn()
            .query_row(
                "SELECT ct.artist FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'a.wav'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        assert_eq!(
            artist, "New Artist",
            "a file with a newer mtime must be reprobed and pick up its new tags"
        );
    }

    /// Probing must reach a fixed point even when a file's mtime is ahead of the
    /// clock, which happens with bad clocks, SMB shares and timestamp-preserving
    /// copies. `probe_dir` loops until nothing is stale, so a file that is
    /// permanently "newer than its last read" would spin forever.
    #[test]
    fn a_file_with_a_future_mtime_stops_being_stale_once_probed() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();

        db.conn()
            .execute(
                "UPDATE file SET mtime = ?1 WHERE name = 'a.wav'",
                params![now() + 1_000_000],
            )
            .unwrap();

        probe_dir(&db, dir_id).unwrap();

        assert_eq!(
            probe_batch(&db, dir_id, 32).unwrap(),
            0,
            "nothing should still be stale after a full probe"
        );
    }

    #[test]
    fn trailing_separator_resolves_to_the_same_dir_row() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("x.txt"), b"x").unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

        let with_sep = {
            let mut s = d.path().to_string_lossy().into_owned();
            s.push(std::path::MAIN_SEPARATOR);
            std::path::PathBuf::from(s)
        };

        let id1 = scan_dir(&db, vol, d.path()).unwrap();
        let id2 = scan_dir(&db, vol, &with_sep).unwrap();

        assert_eq!(id1, id2, "same physical folder must map to one dir row");
        assert_eq!(count(&db, "SELECT COUNT(*) FROM dir"), 1);
    }

    #[test]
    fn path_key_strips_trailing_separators_but_keeps_a_bare_root() {
        assert_eq!(path_key(Path::new("/a/b/")), path_key(Path::new("/a/b")));
        assert_eq!(
            path_key(Path::new("C:\\Music\\")),
            path_key(Path::new("C:\\Music"))
        );
        assert_eq!(path_key(Path::new("/")), "/");
    }

    #[cfg(windows)]
    #[test]
    fn path_key_folds_case_on_windows() {
        assert_eq!(
            path_key(Path::new("C:\\Music")),
            path_key(Path::new("C:\\MUSIC"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn path_key_does_not_fold_case_on_unix() {
        assert_ne!(path_key(Path::new("/Music")), path_key(Path::new("/music")));
    }
}
