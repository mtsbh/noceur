//! Measuring tempo and key.
//!
//! Only measures; nothing here writes. The front end shows each estimate next
//! to the file's own tag and writes the ones the user ticks, through the tag
//! editor's own command, so a detected value reaches a file exactly the way a
//! typed one does.
//!
//! Each file is decoded once for both. About a second a track all told, most
//! of it decoding, so this takes a batch like the quality check does and the
//! front end walks its selection a few at a time.

use crate::state::AppState;
use noceur_audio::tempo::{self, Range};
use noceur_tags::KeyNotation;
use tauri::State;

/// What one file measured at, beside what it is tagged with.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRow {
    pub file_id: i64,
    pub name: String,
    /// The BPM the file is tagged with, if any.
    pub tagged_bpm: Option<f32>,
    /// The tempo estimate, rounded the way it would be written.
    pub bpm: Option<f64>,
    /// The key the file is tagged with, in Camelot, if any.
    pub tagged_key: Option<String>,
    /// The key estimate, in Camelot.
    pub key: Option<String>,
    /// The model's probability for that key, 0..1.
    pub key_confidence: Option<f32>,
    /// Why there is nothing, when the file would not decode.
    pub error: Option<String>,
}

/// Measures tempo (within `min`..`max` BPM) and key for a batch of files.
///
/// The files are measured side by side: decoding is most of the cost, and one
/// core per file finishes a batch in the time of its longest track.
#[tauri::command]
pub async fn analyse_tracks(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    min: f64,
    max: f64,
) -> Result<Vec<AnalysisRow>, String> {
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
                .map(|(file_id, path)| s.spawn(move || analyse(*file_id, path, range)))
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect::<Vec<_>>()
        });
        Ok(rows)
    })
    .await
    .map_err(|e| format!("analysis task failed: {e}"))?
}

fn analyse(file_id: i64, path: &std::path::Path, range: Range) -> AnalysisRow {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = noceur_tags::read(path).ok();
    let mut row = AnalysisRow {
        file_id,
        name,
        tagged_bpm: meta.as_ref().and_then(|m| m.bpm),
        bpm: None,
        tagged_key: meta
            .as_ref()
            .and_then(|m| m.key.as_ref())
            .map(|k| k.format(KeyNotation::Camelot)),
        key: None,
        key_confidence: None,
        error: None,
    };
    match noceur_audio::decode(path, noceur_audio::ANALYSIS_SECS) {
        Ok(d) => {
            row.bpm = tempo::estimate(&d.samples, d.rate, d.channels, range)
                .map(|e| tempo::rounded(e.bpm));
            if let Some(k) = noceur_audio::key::estimate(&d.samples, d.rate, d.channels) {
                row.key = Some(k.camelot);
                row.key_confidence = Some(k.confidence);
            }
        }
        Err(e) => row.error = Some(e.to_string()),
    }
    row
}
