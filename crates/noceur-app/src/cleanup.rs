//! Writing a previewed set of name and tag changes.
//!
//! The tools in [`crate::tools`] work out what would change and touch nothing;
//! this applies exactly what was previewed. They are narrow but they are still
//! guesses about someone's music, and a guess applied to four hundred files
//! unseen is not a feature.

use crate::dto::{WriteFailure, WriteReport};
use crate::state::AppState;
use std::path::{Path, PathBuf};
use tauri::State;

/// One tag field that would change.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CleanField {
    pub field: String,
    pub label: String,
    pub from: String,
    pub to: String,
}

/// One file's proposed cleanup.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanPreview {
    pub file_id: i64,
    /// The current file name, always — it is what identifies the row on screen
    /// even when only tags are changing.
    pub from: String,
    /// The proposed file name, or the current one when the name is untouched.
    pub to: String,
    pub fields: Vec<CleanField>,
    /// Why this one will be skipped, or empty when it will go ahead.
    pub problem: String,
    /// Whether the rules find nothing to do here.
    pub unchanged: bool,
}

/// What the front end sends back to be applied.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanApply {
    pub file_id: i64,
    /// The new file name, or empty to leave the name alone.
    pub name: String,
    /// The tag fields to set, as they were previewed.
    pub fields: Vec<(String, String)>,
}

/// Applies exactly what the preview showed.
///
/// Tags first, then the name. Both act on the path this file had when the loop
/// picked it up, so the rename cannot leave the tag write looking for a file
/// that has already moved.
#[tauri::command]
pub async fn apply_cleanup(
    state: State<'_, AppState>,
    items: Vec<CleanApply>,
) -> Result<WriteReport, String> {
    crate::undo::seal();
    let db = state.db.clone();
    // Read once per command rather than per file: it cannot change mid-write,
    // and reading it two hundred times would be two hundred file reads to
    // answer the same question.
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;

    tauri::async_runtime::spawn_blocking(move || {
        let targets: Vec<(CleanApply, PathBuf)> = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            items
                .into_iter()
                .filter_map(|item| {
                    noceur_index::thumb::locate(&db, item.file_id)
                        .ok()
                        .flatten()
                        .filter(|f| f.kind == 1)
                        .map(|f| (item, f.path))
                })
                .collect()
        };

        let mut report = WriteReport::default();
        let mut touched = Vec::new();

        for (item, path) in targets {
            let mut edit = noceur_tags::Edit::default();
            for (field, value) in &item.fields {
                // Every value here was previewed as a change, so it is a set —
                // there is no "leave alone" to express, because leaving alone
                // means not being in the list.
                let change = noceur_tags::Change::Set(value.trim().to_string());
                match field.as_str() {
                    "title" => edit.title = change,
                    "artist" => edit.artist = change,
                    "album" => edit.album = change,
                    "albumartist" => edit.album_artist = change,
                    // Only ever previewed as a date cut down to its year.
                    "date" => edit.date_to_year = true,
                    _ => {}
                }
            }

            let mut failed = false;
            if !edit.is_empty() {
                let written = crate::timestamp::preserving(&path, keep_dates, || {
                    noceur_tags::edit::write(&path, &edit)
                });
                if let Err(e) = written {
                    report.failures.push(WriteFailure {
                        name: file_name(&path),
                        reason: e.to_string(),
                    });
                    failed = true;
                }
            }

            // A file whose tags could not be written is not renamed either. A
            // half-applied cleanup is worse than none: the name says it was done
            // and the tags say it was not.
            if !failed && !item.name.is_empty() {
                if let Err(reason) = rename_to(&path, &item.name) {
                    report.failures.push(WriteFailure {
                        name: file_name(&path),
                        reason,
                    });
                    failed = true;
                }
            }

            if !failed {
                report.written += 1;
                touched.push(item.file_id);
            }
        }

        // The index holds the name and the tags, so anything changed here has to
        // be re-read or the listing keeps showing what used to be true.
        if !touched.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for id in touched {
                noceur_index::reprobe(&db, id).map_err(|e| e.to_string())?;
            }
        }

        Ok(report)
    })
    .await
    .map_err(|e| format!("cleanup task failed: {e}"))?
}

/// Renames one file, checking the name again first.
///
/// Checked here rather than trusted: the preview was computed before anyone read
/// it, and a name arriving from the front end has been nowhere safe in between.
fn rename_to(path: &Path, to: &str) -> Result<(), String> {
    let safe = noceur_fsmodel::rename::sanitise(to);
    if safe.is_empty() || safe != to || to.contains(['/', '\\']) {
        return Err("that is not a name this will write".into());
    }

    let destination = path.with_file_name(&safe);
    if destination == path {
        return Ok(());
    }
    // A change of case only is the same file to Windows, which `exists` would
    // report as being in the way.
    if destination.exists() && !same_but_case(path, &destination) {
        return Err(format!("{safe} already exists"));
    }

    crate::renamer::rename_in_shell(path, &destination)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Whether two paths differ only in letter case.
pub(crate) fn same_but_case(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}
