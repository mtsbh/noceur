//! Detecting tempo.
//!
//! Only measures; nothing here writes. The front end shows each estimate next
//! to the file's own BPM and writes the ones the user ticks, through the tag
//! editor's own command, so a detected tempo reaches a file exactly the way a
//! typed one does.
//!
//! About a second a track, most of it decoding, so this takes a batch like the
//! quality check does and the front end walks its selection a few at a time.

use crate::state::AppState;
use noceur_audio::tempo::{self, Range};
use tauri::State;

/// What one file measured at.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoRow {
    pub file_id: i64,
    pub name: String,
    /// The BPM the file is tagged with, if any.
    pub tagged: Option<f32>,
    /// The estimate, already rounded the way it would be written. Absent when
    /// the file has no beat to find or would not decode.
    pub detected: Option<f64>,
    /// Why there is no estimate, when it is an error rather than silence.
    pub error: Option<String>,
}

/// Estimates the tempo of a batch of files, within `min`..`max` BPM.
///
/// The files are measured side by side: decoding is most of the cost, and one
/// core per file finishes a batch in the time of its longest track.
#[tauri::command]
pub async fn detect_tempo(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    min: f64,
    max: f64,
) -> Result<Vec<TempoRow>, String> {
    let db = state.db.clone();
    let range = Range { min, max };

    tauri::async_runtime::spawn_blocking(move || {
        // Paths first, holding the index only for that; see `analyse_quality`.
        let targets = {
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
                .collect::<Vec<_>>()
        };

        let rows = std::thread::scope(|s| {
            let handles: Vec<_> = targets
                .iter()
                .map(|(file_id, path)| {
                    s.spawn(move || {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let tagged = noceur_tags::read(path).ok().and_then(|m| m.bpm);
                        let (detected, error) = match tempo::detect(path, range) {
                            Ok(found) => (found.map(|e| tempo::rounded(e.bpm)), None),
                            Err(e) => (None, Some(e.to_string())),
                        };
                        TempoRow {
                            file_id: *file_id,
                            name,
                            tagged,
                            detected,
                            error,
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect::<Vec<_>>()
        });
        Ok(rows)
    })
    .await
    .map_err(|e| format!("tempo task failed: {e}"))?
}
