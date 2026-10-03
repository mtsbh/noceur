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

use crate::dto::{WriteFailure, WriteReport};
use crate::state::AppState;
use noceur_audio::quality::{self, Claim};
use noceur_index::analysis::Loudness;
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
    /// Portion of samples in a flat top, as a percentage.
    pub clip_percent: f32,
    /// Longest unbroken run of flat-topped samples.
    pub clip_run: u32,
    /// Where the flat tops sit, dBFS: 0 at full scale, below it when the track
    /// clipped and was then turned down.
    pub clip_db: f32,
    /// Integrated loudness, LUFS.
    pub lufs: Option<f32>,
    /// True peak, dBTP.
    pub true_peak: Option<f32>,
    /// The true peak is at least this: the decoder clipped a lossy file at
    /// full scale, so the overs above it were never seen.
    pub peak_floor: bool,
    /// What is broken, as opposed to lossy: `0:30 missing · 2 dropouts`.
    /// Empty when the file is sound.
    pub damage: String,
    pub reason: String,
}

/// Whether a lossy file's decode reached full scale. Symphonia clips MP3
/// output to ±1.0, so past this its true peak is only a lower bound.
fn peak_is_floor(lossless: bool, sample_peak: f32) -> bool {
    !lossless && sample_peak >= 0.9999
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
                        .map(|f| (*id, f.path, f.content_id))
                })
                .collect::<Vec<_>>()
        };

        let mut out = Vec::with_capacity(targets.len());
        let mut measured = Vec::new();
        let mut broken = Vec::new();
        for (file_id, path, content_id) in targets {
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
                    let damage = report
                        .damage
                        .summary(meta.as_ref().and_then(|m| m.bit_depth));
                    let loud = report.loudness;
                    // Only a file known to be lossy: one whose header would not
                    // parse is as likely a WAV, whose decoder does not clip.
                    let known_lossy = meta.as_ref().is_some_and(|m| !m.lossless);
                    let peak_floor = peak_is_floor(!known_lossy, loud.sample_peak);
                    if let Some(content_id) = content_id {
                        broken.push((content_id, damage.clone()));
                        measured.push((
                            content_id,
                            Loudness {
                                lufs: loud.lufs.map(f64::from),
                                true_peak: loud.true_peak_db.map(f64::from),
                                peak_floor,
                            },
                        ));
                    }
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
                        clip_db: report.clipping.level_db,
                        lufs: loud.lufs,
                        true_peak: loud.true_peak_db,
                        peak_floor,
                        damage,
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
                    clip_db: 0.0,
                    lufs: None,
                    true_peak: None,
                    peak_floor: false,
                    damage: String::new(),
                    reason: e.to_string(),
                }),
            }
        }

        // Kept, so the list's LUFS, Peak and Damage columns have something to
        // show.
        if !measured.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for (content_id, l) in &measured {
                noceur_index::analysis::set_loudness(&db, *content_id, l)
                    .map_err(|e| e.to_string())?;
            }
            for (content_id, summary) in &broken {
                noceur_index::analysis::set_damage(&db, *content_id, summary)
                    .map_err(|e| e.to_string())?;
            }
        }

        Ok(out)
    })
    .await
    .map_err(|e| format!("quality task failed: {e}"))?
}

/// The ReplayGain 2.0 fields for a measurement: gain to -18 LUFS, and the true
/// peak as a linear ratio. `None` when there is no loudness to go by.
pub fn replay_gain_fields(l: &Loudness) -> Option<[(String, String); 2]> {
    let lufs = l.lufs?;
    let gain = noceur_audio::loudness::replay_gain_db(lufs as f32);
    let peak = 10f64.powf(l.true_peak.unwrap_or(-120.0) / 20.0);
    Some([
        ("REPLAYGAIN_TRACK_GAIN".into(), format!("{gain:.2} dB")),
        ("REPLAYGAIN_TRACK_PEAK".into(), format!("{peak:.6}")),
    ])
}

/// Writes ReplayGain track gain and peak from what Check quality measured.
/// Files not measured yet are skipped and named.
#[tauri::command]
pub async fn write_replay_gain(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<WriteReport, String> {
    let db = state.db.clone();
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;

    tauri::async_runtime::spawn_blocking(move || {
        let targets = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            let mut targets = Vec::new();
            for id in &file_ids {
                let Some(f) = noceur_index::thumb::locate(&db, *id)
                    .map_err(|e| e.to_string())?
                    .filter(|f| f.kind == 1)
                else {
                    continue;
                };
                let loud = match f.content_id {
                    Some(c) => {
                        noceur_index::analysis::loudness(&db, c).map_err(|e| e.to_string())?
                    }
                    None => None,
                };
                targets.push((*id, f.path, loud));
            }
            targets
        };

        let mut report = WriteReport::default();
        let mut written = Vec::new();
        for (id, path, loud) in targets {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Some(fields) = loud.as_ref().and_then(replay_gain_fields) else {
                report.failures.push(WriteFailure {
                    name,
                    reason: "not measured yet".into(),
                });
                continue;
            };
            let edit = noceur_tags::Edit {
                custom: fields
                    .into_iter()
                    .map(|(k, v)| (k, noceur_tags::Change::Set(v)))
                    .collect(),
                ..Default::default()
            };
            match crate::timestamp::preserving(&path, keep_dates, || {
                noceur_tags::edit::write(&path, &edit)
            }) {
                Ok(()) => {
                    report.written += 1;
                    written.push(id);
                }
                Err(e) => report.failures.push(WriteFailure {
                    name,
                    reason: e.to_string(),
                }),
            }
        }
        if !written.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for id in written {
                noceur_index::reprobe(&db, id).map_err(|e| e.to_string())?;
            }
        }
        Ok(report)
    })
    .await
    .map_err(|e| format!("tag write task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_gain_is_measured_against_minus_18() {
        let [gain, peak] = replay_gain_fields(&Loudness {
            lufs: Some(-9.5),
            true_peak: Some(0.0),
            peak_floor: false,
        })
        .unwrap();
        assert_eq!(gain, ("REPLAYGAIN_TRACK_GAIN".into(), "-8.50 dB".into()));
        assert_eq!(peak, ("REPLAYGAIN_TRACK_PEAK".into(), "1.000000".into()));
        assert!(replay_gain_fields(&Loudness::default()).is_none());
    }

    #[test]
    fn only_a_clipped_lossy_decode_is_a_floor() {
        assert!(peak_is_floor(false, 1.0));
        assert!(!peak_is_floor(true, 1.0), "lossless decoders do not clip");
        assert!(!peak_is_floor(false, 0.95));
    }
}
