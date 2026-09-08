//! Full pipeline: build a tree, scan it, mutate it, rescan, verify convergence.

use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use noceur_index::query::{list, ListOptions, SortColumn};
use noceur_index::volume::{upsert, VolumeKind};
use noceur_index::{probe_dir, scan_dir, Db};
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

#[test]
fn scan_mutate_rescan_converges() {
    let db = Db::open_in_memory().unwrap();
    let d = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    fs::create_dir(d.path().join("Unsorted")).unwrap();
    make(d.path(), "a.wav", "Alice", "124", "5A");
    make(d.path(), "b.wav", "Bob", "126", "Am");
    fs::write(d.path().join("notes.txt"), b"x").unwrap();

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(&db, dir_id, ListOptions::default()).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "Unsorted", "directory first");
    assert_eq!(rows.last().unwrap().name, "notes.txt", "other last");

    // Mutate: delete one, add one.
    fs::remove_file(d.path().join("a.wav")).unwrap();
    make(d.path(), "c.wav", "Carl", "128", "11B");

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(&db, dir_id, ListOptions::default()).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Unsorted", "b.wav", "c.wav", "notes.txt"]);
}

#[test]
fn a_copy_in_another_folder_reuses_the_content_row() {
    let db = Db::open_in_memory().unwrap();
    let root = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    let promos = root.path().join("promos");
    let party = root.path().join("party");
    fs::create_dir(&promos).unwrap();
    fs::create_dir(&party).unwrap();

    make(&promos, "track.wav", "G76", "123", "8A");
    fs::copy(promos.join("track.wav"), party.join("track.wav")).unwrap();

    for dir in [&promos, &party] {
        let id = scan_dir(&db, vol, dir).unwrap();
        probe_dir(&db, id).unwrap();
    }

    let files: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM file WHERE kind = 1", [], |r| r.get(0))
        .unwrap();
    let contents: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM content", [], |r| r.get(0))
        .unwrap();

    assert_eq!(files, 2, "two paths");
    assert_eq!(contents, 1, "one piece of audio");
}

#[test]
fn key_sort_is_harmonic_not_alphabetical() {
    let db = Db::open_in_memory().unwrap();
    let d = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    make(d.path(), "x.wav", "X", "120", "10A");
    make(d.path(), "y.wav", "Y", "120", "2A");
    make(d.path(), "z.wav", "Z", "120", "9A");

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(
        &db,
        dir_id,
        ListOptions {
            sort: SortColumn::Key,
            ..Default::default()
        },
    )
    .unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // Alphabetically "10A" < "2A"; harmonically 2A < 9A < 10A.
    assert_eq!(names, ["y.wav", "z.wav", "x.wav"]);
}

/// The codec column was declared in the very first migration and never written
/// to, so the list offered a column that was empty in every library. A probe has
/// to fill it, and only an end-to-end run proves that it does.
#[test]
fn probing_records_what_each_file_actually_is() {
    let db = Db::open_in_memory().unwrap();
    let d = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
    make(d.path(), "a.wav", "Artist", "128", "8A");

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let codec: Option<String> = db
        .conn()
        .query_row("SELECT codec FROM content LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(codec.as_deref(), Some("WAV"));
}
