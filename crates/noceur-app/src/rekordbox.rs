//! Send to rekordbox: noceur's playlists into the `rekordbox.xml` rekordbox's
//! Imported Library reads. The merge itself, and the proof that it changed
//! nothing it should not, are `noceur_rekordbox`; this is the file handling
//! around it.
//!
//! The file is the one copy of every cue point Mixed In Key has written, so the
//! write is careful: the old file is backed up, the new one written beside it,
//! read back and compared, and only then renamed over it. Mixed In Key or a
//! rekordbox export may write the file at any moment; if it changed while the
//! merge was being made, the merge is made again from the new file.

use crate::config::Config;
use crate::format;
use crate::state::AppState;
use noceur_index::playlist;
use noceur_rekordbox::{self as rb, NewTrack};
use noceur_tags::{KeyNotation, MusicalKey};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::State;

/// Backups kept beside the file, newest first.
const BACKUPS: usize = 10;

/// What a send did, for the flash.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    playlists: usize,
    added: usize,
    /// Tracks left out: not on disk, or not readable.
    missing: usize,
    path: String,
}

/// The file playlists go to: the one set in Settings, else the one rekordbox's
/// Imported Library is set to.
pub fn target(config: &Config) -> Option<PathBuf> {
    let set = config.rekordbox_xml.trim();
    if !set.is_empty() {
        return Some(PathBuf::from(set));
    }
    imported_library()
}

/// `bridgeImportedLibraryFile` from rekordbox's own settings, which is what
/// Preferences → Advanced → Imported Library writes.
fn imported_library() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    for dir in ["rekordbox7", "rekordbox6", "rekordbox"] {
        let settings = Path::new(&appdata)
            .join("Pioneer")
            .join(dir)
            .join("rekordbox3.settings");
        let Ok(text) = std::fs::read_to_string(&settings) else {
            continue;
        };
        let Some(at) = text.find("name=\"bridgeImportedLibraryFile\"") else {
            continue;
        };
        let rest = &text[at..];
        let line = &rest[..rest.find('>').unwrap_or(rest.len())];
        if let Some(v) = line
            .split("val=\"")
            .nth(1)
            .and_then(|v| v.split('"').next())
        {
            if !v.trim().is_empty() {
                return Some(PathBuf::from(v.replace("&amp;", "&")));
            }
        }
    }
    None
}

/// Where Send to rekordbox will write, and whether it is the one set in
/// Settings or found in rekordbox's.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    path: Option<String>,
    /// The path typed in Settings, empty when rekordbox's is used.
    set: String,
    found: Option<String>,
}

#[tauri::command]
pub fn rekordbox_target() -> Target {
    let config = Config::load(&Config::default_path());
    Target {
        path: target(&config).map(|p| p.to_string_lossy().into_owned()),
        set: config.rekordbox_xml.clone(),
        found: imported_library().map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
pub fn set_rekordbox_xml(path: String) -> Result<Target, String> {
    let file = Config::default_path();
    let mut config = Config::load(&file);
    config.rekordbox_xml = path.trim().to_string();
    config.save(&file)?;
    Ok(rekordbox_target())
}

#[tauri::command]
pub async fn send_to_rekordbox(state: State<'_, AppState>) -> Result<Sent, String> {
    let path = target(&Config::load(&Config::default_path())).ok_or(
        "No rekordbox.xml: set one in rekordbox (Preferences → Advanced → Imported Library) or in Settings",
    )?;
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Read under the lock, then let it go: the file work can take a while.
        let (lists, stars) = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            gather(&db).map_err(|e| e.to_string())?
        };
        send(
            &path,
            &lists,
            &stars,
            &format::today(),
            &format::now_stamp(),
        )
    })
    .await
    .map_err(|e| format!("send failed: {e}"))?
}

/// Every playlist with its paths, and the stars the index has for them.
fn gather(
    db: &noceur_index::Db,
) -> Result<(Vec<rb::Playlist>, HashMap<String, u8>), noceur_index::IndexError> {
    let mut stars = HashMap::new();
    let mut lists = Vec::new();
    for p in playlist::all(db)? {
        let entries = playlist::entries(db, p.id)?;
        for e in &entries {
            if let Some(n) = e.row.as_ref().and_then(|r| r.rating) {
                stars.insert(e.path.to_lowercase(), n.clamp(0, 5) as u8);
            }
        }
        lists.push(rb::Playlist {
            name: p.name,
            paths: entries.into_iter().map(|e| e.path).collect(),
        });
    }
    Ok((lists, stars))
}

fn send(
    path: &Path,
    lists: &[rb::Playlist],
    stars: &HashMap<String, u8>,
    today: &str,
    stamp: &str,
) -> Result<Sent, String> {
    let shown = path.display();
    for _ in 0..3 {
        let before = stat(path);
        let old = match std::fs::read(path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|_| format!("{shown} is not UTF-8, which rekordbox always writes"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => rb::EMPTY.to_string(),
            Err(e) => return Err(format!("cannot read {shown}: {e}")),
        };
        let merged =
            rb::merge(&old, lists, |p| describe(p, stars), today).map_err(|e| e.to_string())?;
        rb::check(&old, &merged).map_err(|e| e.to_string())?;

        if stat(path) != before {
            continue;
        }
        if before.is_some() {
            backup(path, stamp).map_err(|e| format!("cannot back up {shown}: {e}"))?;
        }
        let new = sibling(path, ".noceur-new");
        std::fs::write(&new, &merged.xml)
            .map_err(|e| format!("cannot write beside {shown}: {e}"))?;
        let read_back = std::fs::read(&new).unwrap_or_default();
        if read_back != merged.xml.as_bytes() {
            let _ = std::fs::remove_file(&new);
            return Err(format!(
                "the new {shown} did not read back as written; nothing was changed"
            ));
        }
        // Written to while this was being made: start again from what is there now.
        if stat(path) != before {
            let _ = std::fs::remove_file(&new);
            continue;
        }
        std::fs::rename(&new, path).map_err(|e| {
            let _ = std::fs::remove_file(&new);
            format!("cannot replace {shown}: {e}")
        })?;
        return Ok(Sent {
            playlists: merged.playlists,
            added: merged.added,
            missing: merged.missing.len(),
            path: path.to_string_lossy().into_owned(),
        });
    }
    Err(format!("{shown} keeps changing; try again in a moment"))
}

/// Size and modified time, to tell whether anything wrote the file.
fn stat(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Copies the file into `noceur-backups` beside it, keeping the newest few.
fn backup(path: &Path, stamp: &str) -> std::io::Result<PathBuf> {
    let dir = path
        .parent()
        .unwrap_or(Path::new("."))
        .join("noceur-backups");
    std::fs::create_dir_all(&dir)?;
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let to = dir.join(format!("{stem}-{stamp}.xml"));
    std::fs::copy(path, &to)?;

    let prefix = format!("{stem}-");
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".xml"))
        })
        .collect();
    // The stamp sorts by time.
    old.sort();
    let extra = old.len().saturating_sub(BACKUPS);
    for p in &old[..extra] {
        let _ = std::fs::remove_file(p);
    }
    Ok(to)
}

/// What rekordbox is told about a track it does not have, read from the file
/// itself so it is current.
fn describe(path: &Path, stars: &HashMap<String, u8>) -> Option<NewTrack> {
    let size = std::fs::metadata(path).ok()?.len();
    let meta = noceur_tags::read(path).ok()?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_default();
    let kind = match ext.as_str() {
        "AIF" | "AIFF" => "AIFF File".to_string(),
        "" => "File".to_string(),
        e => format!("{e} File"),
    };
    let seconds = meta.duration_ms.unwrap_or(0) / 1000;
    let bit_rate = match (meta.lossless, meta.sample_rate, meta.bit_depth) {
        (true, Some(sr), Some(bits)) if meta.codec.as_deref() != Some("FLAC") => {
            sr * u32::from(bits) * u32::from(meta.channels.unwrap_or(2)) / 1000
        }
        _ => meta
            .bitrate_kbps
            .unwrap_or_else(|| (size * 8 / seconds.max(1) / 1000) as u32),
    };
    let tonality = meta
        .key
        .map(|k| k.format(KeyNotation::Camelot))
        .or_else(|| {
            meta.key_tag
                .as_deref()
                .and_then(MusicalKey::parse)
                .map(|k| k.format(KeyNotation::Camelot))
        })
        .unwrap_or_default();
    let name = meta.title.clone().unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    Some(NewTrack {
        name,
        artist: meta.artist.unwrap_or_default(),
        album: meta.album.unwrap_or_default(),
        genre: meta.genre.unwrap_or_default(),
        label: meta.label.unwrap_or_default(),
        comments: meta.comment.unwrap_or_default(),
        kind,
        size,
        total_time: seconds,
        track_number: meta.track_no.unwrap_or(0),
        year: meta.year.unwrap_or(0),
        bpm: meta.bpm.map(f64::from),
        bit_rate,
        sample_rate: meta.sample_rate.unwrap_or(0),
        stars: stars
            .get(&path.to_string_lossy().to_lowercase())
            .copied()
            .unwrap_or(0),
        tonality,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lists(paths: &[&Path]) -> Vec<rb::Playlist> {
        vec![rb::Playlist {
            name: "set".into(),
            paths: paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        }]
    }

    #[test]
    fn a_send_makes_the_file_backs_it_up_and_keeps_ten_backups() {
        let dir = tempfile::TempDir::new().unwrap();
        let xml = dir.path().join("rekordbox.xml");
        let gone = dir.path().join("gone.wav");

        // No file yet: one is made, and nothing is backed up.
        let sent = send(
            &xml,
            &lists(&[&gone]),
            &HashMap::new(),
            "2026-10-04",
            "20261004-000000",
        )
        .unwrap();
        assert_eq!((sent.playlists, sent.added, sent.missing), (1, 0, 1));
        assert!(std::fs::read_to_string(&xml)
            .unwrap()
            .contains("Name=\"noceur\""));
        assert!(!dir.path().join("noceur-backups").exists());

        for i in 1..=12 {
            send(
                &xml,
                &lists(&[]),
                &HashMap::new(),
                "2026-10-04",
                &format!("20261004-0000{i:02}"),
            )
            .unwrap();
        }
        let mut kept: Vec<String> = std::fs::read_dir(dir.path().join("noceur-backups"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        kept.sort();
        assert_eq!(kept.len(), BACKUPS);
        assert_eq!(kept[0], "rekordbox-20261004-000003.xml");
        assert!(!dir.path().join("rekordbox.xml.noceur-new").exists());
    }

    #[test]
    fn a_file_that_is_not_rekordbox_xml_is_left_alone() {
        let dir = tempfile::TempDir::new().unwrap();
        let xml = dir.path().join("rekordbox.xml");
        std::fs::write(&xml, "<html></html>").unwrap();
        assert!(send(&xml, &lists(&[]), &HashMap::new(), "d", "s").is_err());
        assert_eq!(std::fs::read_to_string(&xml).unwrap(), "<html></html>");
        assert!(!dir.path().join("noceur-backups").exists());
    }
}
