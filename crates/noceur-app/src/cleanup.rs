//! Cleaning up names and tags with a set of rules.
//!
//! The template renamer next door rebuilds a name out of tags. This does the
//! opposite job: it takes what is already there — in the file name, in the tags,
//! usually in both — and removes the debris a download, a rip or an old
//! filesystem left in it.
//!
//! Two commands, for the same reason renaming has two: one that works out every
//! change and touches nothing, and one that applies exactly what was previewed.
//! The rules are narrow but they are still guesses about someone's music, and a
//! guess applied to four hundred files unseen is not a feature.

use crate::dto::{WriteFailure, WriteReport};
use crate::state::AppState;
use noceur_fsmodel::cleanup::{self, Rule};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tauri::State;

/// A rule, as the interface lists it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CleanRule {
    pub slug: String,
    pub label: String,
    pub hint: String,
}

/// The rules this build knows about.
///
/// Sent from here rather than written out in the front end, so the list on
/// screen cannot come to disagree with the list that runs.
#[tauri::command]
pub fn cleanup_rules() -> Vec<CleanRule> {
    cleanup::CATALOGUE
        .iter()
        .map(|(slug, label, hint)| CleanRule {
            slug: (*slug).to_string(),
            label: (*label).to_string(),
            hint: (*hint).to_string(),
        })
        .collect()
}

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

/// The tag fields worth cleaning, and what to call them.
///
/// The four that carry `(Original Mix)` and underscores. Genre, comment and the
/// rest are either short enough to be right already or hold things — label
/// names, cue notes — that these rules would mangle.
const FIELDS: &[(&str, &str)] = &[
    ("title", "Title"),
    ("artist", "Artist"),
    ("album", "Album"),
    ("albumartist", "Album artist"),
];

/// Works out what the rules would do to the selection. Changes nothing.
#[tauri::command]
pub async fn preview_cleanup(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    rules: Vec<String>,
    name: bool,
    tags: bool,
) -> Result<Vec<CleanPreview>, String> {
    let db = state.db.clone();
    let rules: Vec<Rule> = rules.iter().filter_map(|s| Rule::from_slug(s)).collect();

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
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

        // Names already spoken for, so two tracks that would clean to the same
        // name are reported rather than one quietly landing on the other.
        let mut taken: HashSet<String> = HashSet::new();
        let mut out = Vec::with_capacity(found.len());

        for (file_id, path) in &found {
            let from = file_name(path);

            // Only the stem is cleaned. An extension is not a name, and a rule
            // that reached it would produce files nothing can open.
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();

            let mut to = from.clone();
            let mut problem = String::new();

            if name {
                let cleaned = noceur_fsmodel::rename::sanitise(&cleanup::clean(&stem, &rules));
                if cleaned.is_empty() {
                    problem = "the rules leave nothing to name it".into();
                } else {
                    to = if ext.is_empty() {
                        cleaned
                    } else {
                        format!("{cleaned}.{ext}")
                    };
                }
            }

            if problem.is_empty() && to != from {
                let key = to.to_lowercase();
                if taken.contains(&key) {
                    problem = "another selected track would take this name".into();
                } else if path.with_file_name(&to).exists() {
                    problem = "a file of that name is already here".into();
                } else {
                    taken.insert(key);
                }
            }

            // Read once per file, and only when tags are in scope: this runs on
            // every keystroke in the rule list.
            let mut fields = Vec::new();
            if tags {
                if let Ok(meta) = noceur_tags::read(path) {
                    for (field, label) in FIELDS {
                        let Some(before) = value_of(&meta, field) else {
                            continue;
                        };
                        let after = cleanup::clean(&before, &rules);
                        if after != before {
                            fields.push(CleanField {
                                field: (*field).to_string(),
                                label: (*label).to_string(),
                                from: before,
                                to: after,
                            });
                        }
                    }
                }
            }

            out.push(CleanPreview {
                file_id: *file_id,
                unchanged: to == from && fields.is_empty(),
                from,
                to,
                fields,
                problem,
            });
        }

        Ok(out)
    })
    .await
    .map_err(|e| format!("cleanup preview task failed: {e}"))?
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
    if destination.exists() {
        return Err(format!("{safe} already exists"));
    }

    crate::renamer::rename_in_shell(path, &destination)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// One tag field's current value, or `None` when the file has nothing there.
fn value_of(meta: &noceur_tags::TrackMeta, field: &str) -> Option<String> {
    let value = match field {
        "title" => &meta.title,
        "artist" => &meta.artist,
        "album" => &meta.album,
        "albumartist" => &meta.album_artist,
        _ => return None,
    };
    value.clone().filter(|v| !v.trim().is_empty())
}
