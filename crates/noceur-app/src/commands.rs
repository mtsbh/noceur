//! Tauri commands. Every one that touches the filesystem runs off-thread.

use crate::dto::{DirListing, ListRequest, Segment};
use crate::format;
use crate::state::AppState;
use noceur_fsmodel::path::simplify;
use noceur_index::{list, volume, ListOptions, SortColumn};
use noceur_tags::KeyNotation;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::State;

/// Lists a directory, scanning and probing it first.
///
/// Runs entirely on the blocking pool. A cold SMB mount has been measured at
/// twenty seconds for thirty-one entries; doing this on the main thread would
/// freeze the window for that whole time.
#[tauri::command]
pub async fn list_dir(state: State<'_, AppState>, req: ListRequest) -> Result<DirListing, String> {
    let db = state.db.clone();
    let path = PathBuf::from(&req.path);
    let audio_only = req.audio_only;

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;

        let started = Instant::now();
        let volume_id = volume::for_path(&db, &path).map_err(|e| e.to_string())?;
        let dir_id = noceur_index::scan_dir(&db, volume_id, &path).map_err(|e| e.to_string())?;
        let scan_ms = started.elapsed().as_millis() as u64;

        let probe_started = Instant::now();
        noceur_index::probe_dir(&db, dir_id).map_err(|e| e.to_string())?;
        let probe_ms = probe_started.elapsed().as_millis() as u64;

        let opts = ListOptions {
            sort: SortColumn::Name,
            desc: false,
            audio_only,
            offset: 0,
            limit: -1,
        };
        let total = noceur_index::count(&db, dir_id, opts.clone()).map_err(|e| e.to_string())?;
        let rows = list(&db, dir_id, opts).map_err(|e| e.to_string())?;

        let display: Vec<_> = rows
            .iter()
            .map(|r| format::display(r, KeyNotation::Camelot))
            .collect();

        let shown = simplify(&path);
        Ok(DirListing {
            path: shown.to_string_lossy().into_owned(),
            segments: segments_of(&shown),
            rows: display,
            total,
            scan_ms,
            probe_ms,
        })
    })
    .await
    .map_err(|e| format!("listing task failed: {e}"))?
}

/// Resolves the parent of a path, or `None` at a root.
#[tauri::command]
pub fn parent_of(path: String) -> Option<String> {
    Path::new(&path)
        .parent()
        .map(|p| simplify(p).to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
}

/// Breaks a path into breadcrumb segments, root first.
///
/// On Windows a path yields a `Prefix` component (`C:`) followed by a `RootDir`
/// component (`\`). Emitting both would put an empty crumb after the drive letter,
/// and emitting only the prefix would make the first crumb navigate to `C:` — which
/// means "the current directory on C:", not the root. So the root separator is
/// folded into the prefix that precedes it.
fn segments_of(path: &Path) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut acc = PathBuf::new();

    for component in path.components() {
        acc.push(component.as_os_str());
        let raw = component.as_os_str().to_string_lossy();
        let label = raw.trim_end_matches(['\\', '/']).to_string();

        if label.is_empty() {
            if let Some(last) = out.last_mut() {
                last.path = acc.to_string_lossy().into_owned();
            }
            continue;
        }

        out.push(Segment {
            label,
            path: acc.to_string_lossy().into_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn segments_fold_the_root_into_the_drive_letter() {
        let s = segments_of(Path::new(r"C:\Music\House"));
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].label, "C:");
        assert_eq!(s[0].path, r"C:\", "first crumb must navigate to the root");
        assert_eq!(s[1].label, "Music");
        assert_eq!(s[1].path, r"C:\Music");
        assert_eq!(s[2].label, "House");
    }

    #[cfg(windows)]
    #[test]
    fn segments_never_contain_an_empty_label() {
        for p in [r"C:\", r"C:\Music", r"E:\music\artists\ricardo"] {
            assert!(
                segments_of(Path::new(p))
                    .iter()
                    .all(|s| !s.label.is_empty()),
                "{p} produced an empty crumb"
            );
        }
    }
}
