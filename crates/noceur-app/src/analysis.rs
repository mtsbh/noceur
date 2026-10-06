//! Measuring tempo and key.
//!
//! Writes nothing to the files. The front end shows each estimate next to the
//! file's own tag and writes the ones the user ticks, through the tag editor's
//! own command, so a detected value reaches a file exactly the way a typed one
//! does. The estimates are kept in the index, where the list and the
//! background analysis (`background.rs`) find them.
//!
//! Each file is decoded once for both. About a second a track all told, most
//! of it decoding, so this takes a batch like the quality check does and the
//! front end walks its selection a few at a time.

use crate::state::AppState;
use noceur_audio::tempo::{self, Range};
use noceur_index::analysis::Detected;
use noceur_tags::{KeyNotation, MusicalKey};
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
    // The range is fixed now; the arguments stay so the call keeps its shape.
    let _ = (min, max);
    let range = Range {
        min: crate::background::TEMPO_MIN,
        max: crate::background::TEMPO_MAX,
    };

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
                        .map(|f| (*id, f.content_id, f.path))
                })
                .collect::<Vec<_>>()
        };

        let measured = std::thread::scope(|s| {
            let handles: Vec<_> = targets
                .iter()
                .map(|(file_id, content_id, path)| {
                    s.spawn(move || (*content_id, analyse(*file_id, path, range)))
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .collect::<Vec<_>>()
        });

        // Kept, so the list can show them and the background need not measure
        // these files again.
        let method = noceur_index::analysis::Method {
            version: crate::background::VERSION,
            range_min: min,
            range_max: max,
        };
        if let Ok(db) = db.lock() {
            for (content_id, (_, detected)) in &measured {
                if let Some(c) = content_id {
                    let _ = noceur_index::analysis::set(&db, *c, &method, detected.as_ref());
                }
            }
        }
        Ok(measured.into_iter().map(|(_, (row, _))| row).collect())
    })
    .await
    .map_err(|e| format!("analysis task failed: {e}"))?
}

/// Measures one file for the index: `None` when it would not decode.
pub fn measure_file(
    path: &std::path::Path,
    method: &noceur_index::analysis::Method,
) -> Option<Detected> {
    let d = noceur_audio::decode(path, noceur_audio::ANALYSIS_SECS).ok()?;
    let range = Range {
        min: method.range_min,
        max: method.range_max,
    };
    Some(measure(&d, range))
}

/// Tempo and key of decoded audio, as the index keeps them.
fn measure(d: &noceur_audio::Decoded, range: Range) -> Detected {
    let bpm = tempo::estimate(&d.samples, d.rate, d.channels, range).map(|e| tempo::rounded(e.bpm));
    let key = noceur_audio::key::estimate(&d.samples, d.rate, d.channels);
    Detected {
        bpm,
        key_norm: key
            .as_ref()
            .and_then(|k| MusicalKey::parse(&k.camelot))
            .map(|k| i64::from(k.canonical())),
        key_confidence: key.map(|k| f64::from(k.confidence)),
    }
}

fn analyse(file_id: i64, path: &std::path::Path, range: Range) -> (AnalysisRow, Option<Detected>) {
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
            let detected = measure(&d, range);
            row.bpm = detected.bpm;
            row.key = detected
                .key_norm
                .and_then(|k| MusicalKey::from_canonical(k as i32))
                .map(|k| k.format(KeyNotation::Camelot));
            row.key_confidence = detected.key_confidence.map(|c| c as f32);
            (row, Some(detected))
        }
        Err(e) => {
            row.error = Some(e.to_string());
            (row, None)
        }
    }
}
