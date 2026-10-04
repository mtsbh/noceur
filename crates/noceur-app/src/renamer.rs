//! Renaming files from a template.
//!
//! Preview first, always. Renaming a folder is the one bulk operation with no
//! real undo — a name that came out wrong has to be identified and repaired by
//! hand, and by then every file has already changed. So this is two commands:
//! one that computes every new name and changes nothing, and one that applies
//! exactly what was previewed.

use crate::state::AppState;
use noceur_fsmodel::rename;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tauri::State;

/// One file's proposed rename.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePreview {
    pub file_id: i64,
    pub from: String,
    pub to: String,
    /// Why this one will be skipped, or empty when it will go ahead.
    pub problem: String,
    /// Whether the name is already what the template asks for.
    pub unchanged: bool,
}

/// Works out what every selected file would be called.
///
/// Reads the files rather than the index, like every other write path here: a
/// preview built from a stale cache would be a preview of the wrong rename.
#[tauri::command]
pub async fn preview_rename(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    template: String,
) -> Result<Vec<RenamePreview>, String> {
    let db = state.db.clone();

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

        // Names already claimed, so two tracks that would become the same file
        // are reported rather than one silently overwriting the other. Seeded
        // with the names of files that are not being renamed at all.
        let mut taken: HashSet<String> = HashSet::new();
        let mut out = Vec::with_capacity(found.len());

        for (file_id, path) in &found {
            let from = file_name(path);
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();

            let meta = noceur_tags::read(path).unwrap_or_default();
            let to = rename::build(&template, &fields_of(&meta, path, &ext), &ext);

            let mut problem = String::new();
            if to.is_empty() {
                problem = "the template leaves nothing to name it".into();
            } else if to != from {
                let key = to.to_lowercase();
                if taken.contains(&key) {
                    problem = "another selected track would take this name".into();
                } else if path.with_file_name(&to).exists() {
                    problem = "a file of that name is already here".into();
                }
            }

            if problem.is_empty() && to != from {
                taken.insert(to.to_lowercase());
            }

            out.push(RenamePreview {
                file_id: *file_id,
                unchanged: to == from,
                from,
                to,
                problem,
            });
        }

        Ok(out)
    })
    .await
    .map_err(|e| format!("preview task failed: {e}"))?
}

/// Renames the files, using the names the preview showed.
///
/// The new names come from the caller rather than being recomputed, so what is
/// applied is exactly what was on screen — recomputing could quietly differ if a
/// file changed in between, which is precisely when a rename should not surprise
/// anyone.
#[tauri::command]
pub async fn apply_rename(
    state: State<'_, AppState>,
    renames: Vec<(i64, String)>,
) -> Result<crate::dto::WriteReport, String> {
    // Whatever the last job renamed becomes its own undo step; this one starts.
    crate::undo::seal();
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let targets: Vec<(i64, PathBuf, String)> = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            renames
                .into_iter()
                .filter_map(|(id, to)| {
                    noceur_index::thumb::locate(&db, id)
                        .ok()
                        .flatten()
                        .filter(|f| f.kind == 1)
                        .map(|f| (id, f.path, to))
                })
                .collect()
        };

        let mut report = crate::dto::WriteReport::default();
        let mut renamed = Vec::new();

        for (file_id, path, to) in targets {
            // Checked again here rather than trusted: the preview was computed
            // before the user read it, and a name arriving from the front end
            // has been nowhere safe in between.
            let safe = rename::sanitise(&to);
            if safe.is_empty() || safe != to || to.contains(['/', '\\']) {
                report.failures.push(crate::dto::WriteFailure {
                    name: file_name(&path),
                    reason: "that is not a name this will write".into(),
                });
                continue;
            }

            let destination = path.with_file_name(&safe);
            if destination == path {
                continue;
            }
            if destination.exists() {
                report.failures.push(crate::dto::WriteFailure {
                    name: file_name(&path),
                    reason: format!("{safe} already exists"),
                });
                continue;
            }

            match rename_in_shell(&path, &destination) {
                Ok(()) => {
                    report.written += 1;
                    renamed.push(file_id);
                }
                Err(e) => report.failures.push(crate::dto::WriteFailure {
                    name: file_name(&path),
                    reason: e,
                }),
            }
        }

        // The index stores the name, so a renamed file has to be re-read or the
        // listing keeps offering the old one.
        if !renamed.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for id in renamed {
                noceur_index::reprobe(&db, id).map_err(|e| e.to_string())?;
            }
        }

        Ok(report)
    })
    .await
    .map_err(|e| format!("rename task failed: {e}"))?
}

/// Renames one file through the shell.
///
/// The same `IFileOperation` the copy, move and delete commands use, for the
/// same reason: it puts the change on Windows' own undo stack, so a rename that
/// turns out wrong is Ctrl+Z in Explorer rather than a repair job.
///
/// Also noted for the app's own Ctrl+Z — see [`crate::undo`].
pub(crate) fn rename_in_shell(from: &Path, to: &Path) -> Result<(), String> {
    #[cfg(windows)]
    let done = crate::fileops::rename(from, to);
    #[cfg(not(windows))]
    let done = std::fs::rename(from, to).map_err(|e| e.to_string());
    if done.is_ok() {
        crate::undo::record(from.to_path_buf(), to.to_path_buf());
        crate::playlists::follow(from.to_path_buf(), to.to_path_buf());
    }
    done
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What a template can refer to, for one track.
fn fields_of<'a>(
    meta: &noceur_tags::TrackMeta,
    path: &Path,
    ext: &str,
) -> HashMap<&'a str, String> {
    let text = |v: &Option<String>| v.clone().unwrap_or_default();
    let mut f = HashMap::new();

    f.insert("artist", text(&meta.artist));
    f.insert("title", text(&meta.title));
    f.insert("album", text(&meta.album));
    f.insert("albumartist", text(&meta.album_artist));
    f.insert("genre", text(&meta.genre));
    f.insert("label", text(&meta.comment));
    f.insert("year", meta.year.map(|v| v.to_string()).unwrap_or_default());
    f.insert(
        "track",
        meta.track_no.map(|v| v.to_string()).unwrap_or_default(),
    );
    // Padded, because a folder sorted by name puts 10 before 2 otherwise.
    f.insert(
        "track2",
        meta.track_no.map(|v| format!("{v:02}")).unwrap_or_default(),
    );
    f.insert(
        "bpm",
        meta.bpm
            .map(|v| {
                if v.fract().abs() < f32::EPSILON {
                    format!("{}", v as i64)
                } else {
                    format!("{v}")
                }
            })
            .unwrap_or_default(),
    );
    f.insert("key", text(&meta.key_tag));
    f.insert("ext", ext.to_string());
    f.insert(
        "filename",
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    f
}
