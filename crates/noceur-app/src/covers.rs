//! Finding cover art online and embedding it.
//!
//! Nothing here runs on its own. Both commands are reached from a menu the user
//! opened, on a selection the user made — a file browser that quietly posted its
//! library's contents to a web service would be a different kind of program.
//! What leaves the machine is the artist and title of the track being looked up.

use crate::dto::{WriteFailure, WriteReport};
use crate::state::AppState;
use noceur_online::Candidate;
use std::path::PathBuf;
use tauri::State;

/// Searches for releases that might carry the artwork for a track.
///
/// Searches on what the file itself says, not on the index's copy of it, for the
/// same reason the tag editor does: the file is the truth, and looking up a
/// stale artist would return covers for the wrong record.
///
/// Returns candidates rather than a picture. Which pressing of a record a track
/// came from is a judgement the user makes — a compilation, a remaster and a
/// bootleg all match the same search and carry different sleeves — so the app
/// offers what it found and embeds nothing until asked.
///
/// Asks every source. Their catalogues barely overlap: the Cover Art Archive is
/// strongest on independent and obscure records, Apple on anything with a
/// distributor, and Deezer often has a twelve-inch neither of the others
/// carried.
#[tauri::command]
pub async fn find_covers(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Vec<Candidate>, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let path = locate(&db, file_id)?;
        let meta = noceur_tags::read(&path).map_err(|e| e.to_string())?;

        let artist = meta
            .album_artist
            .clone()
            .or_else(|| meta.artist.clone())
            .unwrap_or_default();
        // The album is the better search term when the file has one — it is what
        // the archive is keyed by. The track title is the fallback, and the
        // search matches it against recordings as well as release names.
        let title = meta
            .album
            .clone()
            .or_else(|| meta.title.clone())
            .unwrap_or_else(|| stem(&path));

        noceur_online::search_all(&artist, &title).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cover search task failed: {e}"))?
}

/// Embeds a chosen cover into a selection.
///
/// The image is downloaded once and written to every file, so choosing a cover
/// for an album of twelve tracks costs one request rather than twelve. Failures
/// are per file, for the same reason as in the tag editor: one locked file in a
/// selection should not cost the other eleven their artwork.
///
/// The URL came from a search, but this command cannot know that — so the fetch
/// is restricted to the sources' own hosts rather than trusted. Nothing is
/// written to disk as an image: the bytes go from the download straight into
/// the tag.
#[tauri::command]
pub async fn apply_cover(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    url: String,
) -> Result<WriteReport, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let image = noceur_online::fetch_image(&url).map_err(|e| e.to_string())?;

        let targets: Vec<(i64, PathBuf)> = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            file_ids
                .iter()
                .filter_map(|id| {
                    noceur_index::thumb::locate(&db, *id)
                        .ok()
                        .flatten()
                        .filter(|f| f.kind == 1)
                        .map(|f| (*id, f.path))
                })
                .collect()
        };

        let mut report = WriteReport::default();
        let mut written = Vec::new();
        // Content ids are captured before the writes, because embedding artwork
        // changes each file's identity and the thumbnails cached against the old
        // one would otherwise be left behind as the covers nobody sees again.
        let stale: Vec<i64> = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            targets
                .iter()
                .filter_map(|(id, _)| {
                    noceur_index::thumb::locate(&db, *id)
                        .ok()
                        .flatten()
                        .and_then(|f| f.content_id)
                })
                .collect()
        };

        for (file_id, path) in targets {
            match noceur_tags::edit::set_front_cover(&path, &image) {
                Ok(()) => {
                    report.written += 1;
                    written.push(file_id);
                }
                Err(e) => report.failures.push(WriteFailure {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    reason: e.to_string(),
                }),
            }
        }

        if !written.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for content_id in stale {
                noceur_index::thumb::invalidate(&db, content_id).map_err(|e| e.to_string())?;
            }
            for id in written {
                noceur_index::reprobe(&db, id).map_err(|e| e.to_string())?;
            }
        }

        Ok(report)
    })
    .await
    .map_err(|e| format!("cover apply task failed: {e}"))?
}

/// Resolves a row to the audio file it names.
fn locate(
    db: &std::sync::Arc<std::sync::Mutex<noceur_index::Db>>,
    file_id: i64,
) -> Result<PathBuf, String> {
    let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
    noceur_index::thumb::locate(&db, file_id)
        .map_err(|e| e.to_string())?
        .filter(|f| f.kind == 1)
        .map(|f| f.path)
        .ok_or_else(|| "that row is not an audio file".to_string())
}

/// The file name without its extension, for a track with no tags at all.
fn stem(path: &std::path::Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
