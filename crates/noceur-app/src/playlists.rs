//! Playlists: the commands behind the Playlists dialog, the P key and the
//! right-click ticks. Storage is `noceur_index::playlist`.

use crate::format::{self, RowDisplay};
use crate::state::AppState;
use noceur_index::playlist::{self, Playlist};
use noceur_index::Db;
use noceur_tags::KeyNotation;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::State;

/// The index, for [`follow`], which is called from the rename and move code
/// that has no `AppState` to hand.
static INDEX: OnceLock<Arc<Mutex<Db>>> = OnceLock::new();

/// Called once at startup.
pub fn init(state: &AppState) {
    let _ = INDEX.set(state.db.clone());
}

/// Keeps playlists, and linked sets, pointing at files the app renamed or moved.
///
/// Applied on a thread of its own: some callers rename while holding the index
/// lock, and waiting for it here would wait on them forever.
pub fn follow(from: PathBuf, to: PathBuf) {
    let Some(db) = INDEX.get().cloned() else {
        return;
    };
    std::thread::spawn(move || {
        if let Ok(db) = db.lock() {
            let _ = playlist::repath(&db, &from, &to);
            let _ = noceur_index::sets::repath(&db, &from, &to);
        }
    });
}

/// After a shell move, follows each source that is gone and whose name is now
/// in `into`. A source the user skipped at a name collision is still where it
/// was, and is left alone.
pub fn follow_moved(sources: &[PathBuf], into: &Path) {
    for source in sources {
        if let Some(name) = source.file_name() {
            let landed = into.join(name);
            if !source.exists() && landed.exists() {
                follow(source.clone(), landed);
            }
        }
    }
}

async fn with_db<T: Send + 'static>(
    state: &State<'_, AppState>,
    job: impl FnOnce(&Db) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        job(&db)
    })
    .await
    .map_err(|e| format!("playlist task failed: {e}"))?
}

/// The paths of indexed files, skipping folders and rows that are gone.
fn paths_of(db: &Db, file_ids: &[i64]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for &id in file_ids {
        if let Some(found) = noceur_index::thumb::locate(db, id).map_err(|e| e.to_string())? {
            if found.kind != 0 {
                out.push(found.path);
            }
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn playlists(state: State<'_, AppState>) -> Result<Vec<Playlist>, String> {
    with_db(&state, |db| playlist::all(db).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn playlist_create(state: State<'_, AppState>, name: String) -> Result<i64, String> {
    with_db(&state, move |db| {
        playlist::create(db, &name).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn playlist_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> Result<(), String> {
    with_db(&state, move |db| {
        playlist::rename(db, id, &name).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn playlist_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    with_db(&state, move |db| {
        playlist::delete(db, id).map_err(|e| e.to_string())
    })
    .await
}

/// One track of a playlist, as the dialog lists it.
#[derive(serde::Serialize)]
pub struct Track {
    path: String,
    /// `None` when the index has no file at `path`.
    row: Option<RowDisplay>,
}

#[tauri::command]
pub async fn playlist_tracks(state: State<'_, AppState>, id: i64) -> Result<Vec<Track>, String> {
    with_db(&state, move |db| {
        let entries = playlist::entries(db, id).map_err(|e| e.to_string())?;
        Ok(entries
            .into_iter()
            .map(|e| Track {
                row: e
                    .row
                    .as_ref()
                    .map(|r| format::display(r, KeyNotation::Camelot)),
                path: e.path,
            })
            .collect())
    })
    .await
}

/// Adds the files to the playlist, or takes them all out when every one is
/// already in it: one key, or one click on a tick, does both.
#[tauri::command]
pub async fn playlist_toggle(
    state: State<'_, AppState>,
    id: i64,
    file_ids: Vec<i64>,
) -> Result<Toggled, String> {
    with_db(&state, move |db| {
        let paths = paths_of(db, &file_ids)?;
        let held = playlist::holding_all(db, &paths).map_err(|e| e.to_string())?;
        if !paths.is_empty() && held.contains(&id) {
            let n = playlist::remove(db, id, &paths).map_err(|e| e.to_string())?;
            Ok(Toggled {
                added: 0,
                removed: n,
            })
        } else {
            let n = playlist::add(db, id, &paths).map_err(|e| e.to_string())?;
            Ok(Toggled {
                added: n,
                removed: 0,
            })
        }
    })
    .await
}

#[derive(serde::Serialize)]
pub struct Toggled {
    added: usize,
    removed: usize,
}

#[tauri::command]
pub async fn playlist_remove(
    state: State<'_, AppState>,
    id: i64,
    paths: Vec<String>,
) -> Result<usize, String> {
    with_db(&state, move |db| {
        playlist::remove(db, id, &paths).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn playlist_reorder(
    state: State<'_, AppState>,
    id: i64,
    paths: Vec<String>,
    to: usize,
) -> Result<(), String> {
    with_db(&state, move |db| {
        playlist::reorder(db, id, &paths, to).map_err(|e| e.to_string())
    })
    .await
}

/// Which playlists already hold the whole selection, for the menu's ticks.
#[tauri::command]
pub async fn playlists_holding(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<Vec<i64>, String> {
    with_db(&state, move |db| {
        let paths = paths_of(db, &file_ids)?;
        playlist::holding_all(db, &paths).map_err(|e| e.to_string())
    })
    .await
}
