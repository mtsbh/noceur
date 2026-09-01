//! Matching a track against online sources.
//!
//! Finds candidates and ranks them; it does not write anything. Applying a match
//! goes through the tag editor like any other edit, pre-filled — so the fields
//! about to be written are visible and adjustable before they are, and one
//! locked file in a selection is reported the same way it always was.

use crate::state::AppState;
use noceur_online::tracks::TrackMatch;
use tauri::State;

/// Searches every source for what this file might be.
///
/// Reads the file rather than the index, like the tag editor does. Falls back to
/// the file name when the tags have nothing to search with, because the files
/// that most need this are exactly the ones with no tags — promos, rips, things
/// pulled off a stick — and `Artist - Title.wav` is what a DJ library calls
/// them.
///
/// The duration comes from the file's own header and is used only as a tiebreak.
#[tauri::command]
pub async fn match_track(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Vec<TrackMatch>, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?
        };
        let Some(found) = found.filter(|f| f.kind == 1) else {
            return Err("that row is not an audio file".into());
        };

        let meta = noceur_tags::read(&found.path).map_err(|e| e.to_string())?;
        let name = found
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let tagged_artist = meta.artist.clone().unwrap_or_default();
        let tagged_title = meta.title.clone().unwrap_or_default();

        // The name is only consulted for what the tags do not already say, so a
        // file with a good title and no artist still gets both.
        let guess = noceur_online::matching::from_file_name(&name);
        let artist = if tagged_artist.trim().is_empty() {
            guess.artist
        } else {
            tagged_artist
        };
        let title = if tagged_title.trim().is_empty() {
            guess.title
        } else {
            tagged_title
        };

        let mut matches = noceur_online::match_track(&artist, &title, meta.duration_ms)
            .map_err(|e| e.to_string())?;

        // The fields the search endpoint leaves out are the ones worth having,
        // and only for results the user might actually take.
        for m in matches.iter_mut().take(4) {
            if m.source == noceur_online::deezer::NAME {
                let _ = noceur_online::deezer::enrich(m);
            }
        }

        Ok(matches)
    })
    .await
    .map_err(|e| format!("match task failed: {e}"))?
}
