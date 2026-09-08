//! Checking what files actually are.
//!
//! Reads nothing from the index but the path. Everything reported here is
//! measured from the audio itself and compared against what the container
//! claims, because the whole point is that the container may be lying.
//!
//! Analysis costs about half a second per track, so this takes a batch rather
//! than a selection: the front end walks its selection a few files at a time and
//! shows progress, which is the same shape the tag probe uses and for the same
//! reason — one command per folder would be a command that appears to hang.

use crate::state::AppState;
use noceur_audio::quality::{self, Claim};
use tauri::State;

/// What one file turned out to be.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityRow {
    pub file_id: i64,
    pub name: String,
    /// `genuine`, `upscaled`, `transcoded`, `unsure`, or `unreadable`.
    pub verdict: String,
    /// What the container says, in kbps. Absent for lossless files.
    pub claimed_kbps: Option<u32>,
    /// What the spectrum suggests it was encoded at.
    pub implied_kbps: Option<u32>,
    /// Highest frequency carrying signal, in kHz.
    pub cutoff_khz: f32,
    pub sample_rate: u32,
    pub lossless: bool,
    /// Portion of samples pinned to full scale, as a percentage.
    pub clip_percent: f32,
    /// Longest unbroken run of full-scale samples.
    pub clip_run: u32,
    pub reason: String,
}

/// Analyses a batch of files.
///
/// Failures are rows rather than errors, like every other batch here: one file
/// the decoder will not open should be named in the results and not cost the
/// rest of the selection its answer.
#[tauri::command]
pub async fn analyse_quality(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<Vec<QualityRow>, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        // Paths first, holding the index only for that. Analysis decodes whole
        // files, and keeping the lock across it would stall every listing and
        // thumbnail for as long as the batch runs.
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

        let mut out = Vec::with_capacity(targets.len());
        for (file_id, path) in targets {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();

            // What the file says about itself. A file whose header will not
            // parse is still analysed — it just cannot be held to a claim.
            let meta = noceur_tags::read(&path).ok();
            let lossless = meta.as_ref().map(|m| m.lossless).unwrap_or(false);
            let claimed = meta.as_ref().and_then(|m| m.bitrate_kbps);
            let claim = if lossless {
                Claim::Lossless
            } else {
                Claim::Lossy(claimed)
            };

            match quality::analyse(&path) {
                Ok(spectrum) => {
                    let report = quality::judge(&spectrum, claim);
                    out.push(QualityRow {
                        file_id,
                        name,
                        verdict: report.verdict.as_str().to_string(),
                        claimed_kbps: report.claimed_kbps,
                        implied_kbps: report.implied_kbps,
                        cutoff_khz: report.cutoff_hz / 1000.0,
                        sample_rate: report.sample_rate,
                        lossless,
                        clip_percent: report.clipping.percent,
                        clip_run: report.clipping.longest_run,
                        reason: report.reason,
                    });
                }
                Err(e) => out.push(QualityRow {
                    file_id,
                    name,
                    verdict: "unreadable".into(),
                    claimed_kbps: claimed,
                    implied_kbps: None,
                    cutoff_khz: 0.0,
                    sample_rate: meta.and_then(|m| m.sample_rate).unwrap_or(0),
                    lossless,
                    clip_percent: 0.0,
                    clip_run: 0,
                    reason: e.to_string(),
                }),
            }
        }

        Ok(out)
    })
    .await
    .map_err(|e| format!("quality task failed: {e}"))?
}
