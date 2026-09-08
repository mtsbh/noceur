//! Finding the same track twice.
//!
//! Two commands: one that measures what files are, and one that reports which of
//! them turn out to be the same. They are separate because measuring costs a
//! full decode per track and reporting costs a query — so the expensive half
//! runs on a selection, in batches, with the count moving, and the cheap half
//! runs whenever you want to look.
//!
//! A hash is stored against *content*, which is a set of bytes. Two copies of a
//! track that have been tagged differently are two contents and one recording,
//! which is exactly the case this exists for.

use crate::state::AppState;
use tauri::State;

/// One file in a group of copies.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Copy {
    pub file_id: i64,
    pub name: String,
    /// The folder it sits in.
    pub folder: String,
    pub size: String,
    /// `YYYY-MM-DD HH:MM`.
    pub modified: String,
}

/// A set of files holding identical audio.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub hash: String,
    pub files: Vec<Copy>,
    /// What could be reclaimed by keeping one of them.
    pub wasted: String,
}

/// Measures a batch of files and records what they are.
///
/// Returns how many were newly measured, so the caller can show progress. A file
/// already hashed is skipped rather than re-decoded: its bytes have not changed,
/// because a change to them would have given it a different content row.
#[tauri::command]
pub async fn identify(state: State<'_, AppState>, file_ids: Vec<i64>) -> Result<i64, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        // Paths and content ids first, holding the lock only for that. Hashing
        // decodes whole files, and keeping the index across it would stall every
        // listing and thumbnail for as long as the batch runs.
        let targets: Vec<(i64, std::path::PathBuf)> = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            file_ids
                .iter()
                .filter_map(|id| {
                    let found = noceur_index::thumb::locate(&db, *id).ok().flatten()?;
                    if found.kind != 1 {
                        return None;
                    }
                    let content_id = found.content_id?;
                    match noceur_index::identity::hash_of(&db, content_id) {
                        Ok(Some(_)) => None,
                        _ => Some((content_id, found.path)),
                    }
                })
                .collect()
        };

        let mut done = 0i64;
        for (content_id, path) in targets {
            let Ok(hash) = noceur_audio::identity::audio_hash(&path) else {
                // A file the decoder will not read has no identity to record.
                // Left unhashed rather than marked, so it is absent from the
                // answer instead of wrongly grouped with every other failure.
                continue;
            };
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::identity::set_hash(&db, content_id, &hash).map_err(|e| e.to_string())?;
            done += 1;
        }

        Ok(done)
    })
    .await
    .map_err(|e| format!("identify task failed: {e}"))?
}

/// Every set of measured files that hold the same audio.
#[tauri::command]
pub async fn duplicates(
    state: State<'_, AppState>,
    scopes: Vec<String>,
) -> Result<Vec<Group>, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let groups = noceur_index::identity::duplicates(&db, &scopes).map_err(|e| e.to_string())?;

        Ok(groups
            .into_iter()
            .map(|g| {
                // What keeping one of them would give back. Counted from the
                // largest, because that is the one anybody keeps.
                let largest = g.files.iter().map(|f| f.size).max().unwrap_or(0);
                let total: i64 = g.files.iter().map(|f| f.size).sum();

                Group {
                    hash: g.hash,
                    wasted: crate::format::size(total - largest),
                    files: g
                        .files
                        .into_iter()
                        .map(|f| Copy {
                            file_id: f.file_id,
                            name: f.name,
                            folder: f.path,
                            size: crate::format::size(f.size),
                            modified: crate::format::modified(f.mtime),
                        })
                        .collect(),
                }
            })
            .collect())
    })
    .await
    .map_err(|e| format!("duplicates task failed: {e}"))?
}
