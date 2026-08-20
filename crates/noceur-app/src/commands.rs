//! Tauri commands. Every one that touches the filesystem runs off-thread.

use crate::dto::{DirListing, ListRequest, RowWindow, RowsRequest, Segment};
use crate::format;
use crate::state::AppState;
use noceur_fsmodel::path::simplify;
use noceur_fsmodel::{natural_cmp, EntryKind};
use noceur_index::{list, volume, Db, IndexError, ListOptions, SortColumn};
use noceur_tags::KeyNotation;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::State;

/// Largest window a single request may ask for.
///
/// The frontend asks for a few hundred rows at a time; anything far above that
/// is either a bug or an attempt to pull a whole 40k-file folder through the
/// bridge in one call, which is exactly what windowing exists to prevent.
const MAX_LIMIT: i64 = 1000;

/// Rows returned alongside the listing itself. Comfortably more than one
/// screenful at the 26px row height, so the first paint is never placeholders.
const FIRST_WINDOW: i64 = 200;

/// Opens a directory: scans it, probes it, and reports how many rows it holds.
///
/// Runs entirely on the blocking pool. A cold SMB mount has been measured at
/// twenty seconds for thirty-one entries; doing this on the main thread would
/// freeze the window for that whole time.
///
/// Returns the first window of rows but not the rest — see [`list_rows`].
#[tauri::command]
pub async fn open_dir(state: State<'_, AppState>, req: ListRequest) -> Result<DirListing, String> {
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

        let total = noceur_index::count(&db, dir_id, options(audio_only, 0, FIRST_WINDOW))
            .map_err(|e| e.to_string())?;
        let first = window(&db, dir_id, audio_only, 0, FIRST_WINDOW).map_err(|e| e.to_string())?;

        let shown = simplify(&path);
        Ok(DirListing {
            dir_id,
            path: shown.to_string_lossy().into_owned(),
            segments: segments_of(&shown),
            total,
            first,
            scan_ms,
            probe_ms,
        })
    })
    .await
    .map_err(|e| format!("listing task failed: {e}"))?
}

/// Reads one window of an already-opened directory.
///
/// Cheap by design — no scan, no probe, just an indexed `LIMIT`/`OFFSET` read —
/// because this is what fires while the user drags a scrollbar.
#[tauri::command]
pub async fn list_rows(state: State<'_, AppState>, req: RowsRequest) -> Result<RowWindow, String> {
    let db = state.db.clone();
    let offset = req.offset.max(0);
    let limit = req.limit.clamp(0, MAX_LIMIT);
    let (dir_id, audio_only) = (req.dir_id, req.audio_only);

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        window(&db, dir_id, audio_only, offset, limit).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("window task failed: {e}"))?
}

/// The one place list options are built, so `open_dir` and `list_rows` cannot
/// drift into cutting windows from differently ordered lists.
fn options(audio_only: bool, offset: i64, limit: i64) -> ListOptions {
    ListOptions {
        sort: SortColumn::Name,
        desc: false,
        audio_only,
        offset,
        limit,
    }
}

/// Reads and formats one window of rows.
fn window(
    db: &Db,
    dir_id: i64,
    audio_only: bool,
    offset: i64,
    limit: i64,
) -> Result<RowWindow, IndexError> {
    let rows = list(db, dir_id, options(audio_only, offset, limit))?;
    Ok(RowWindow {
        offset,
        rows: rows
            .iter()
            .map(|r| format::display(r, KeyNotation::Camelot))
            .collect(),
    })
}

/// Lists the subdirectories of a path, for one level of the folder tree.
///
/// Reads the filesystem directly rather than going through the index. The tree
/// expands nodes the user is merely looking at, and scanning each one into the
/// index would write a row for every folder they happened to unfold.
///
/// A directory that cannot be read yields an empty list rather than an error: a
/// permission-denied system folder should render as an empty branch, not stop
/// the tree.
#[tauri::command]
pub async fn subdirs(path: String) -> Result<Vec<Segment>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let listing = match noceur_fsmodel::read_dir(Path::new(&path)) {
            Ok(l) => l,
            Err(_) => return Vec::new(),
        };

        let mut dirs: Vec<Segment> = listing
            .entries
            .iter()
            .filter(|e| e.kind == EntryKind::Directory)
            .map(|e| Segment {
                label: e.name.clone(),
                path: Path::new(&path)
                    .join(&e.name)
                    .to_string_lossy()
                    .into_owned(),
            })
            .collect();

        dirs.sort_by(|a, b| natural_cmp(&a.label, &b.label));
        dirs
    })
    .await
    .map_err(|e| format!("subdirectory task failed: {e}"))
}

/// The drives the tree is rooted at.
///
/// Roots rather than a single home folder, because the library genuinely spans
/// volumes — local disks, the party SSD, and the `Z:` share — and the whole point
/// of the tree is moving between them.
#[tauri::command]
pub async fn roots() -> Result<Vec<Segment>, String> {
    tauri::async_runtime::spawn_blocking(volume_roots)
        .await
        .map_err(|e| format!("root task failed: {e}"))
}

#[cfg(windows)]
fn volume_roots() -> Vec<Segment> {
    // A drive letter that answers `read_dir` is mounted and readable. Probing
    // this way avoids a Win32 call and skips empty card readers for free.
    (b'A'..=b'Z')
        .map(|letter| format!("{}:\\", letter as char))
        .filter(|root| std::fs::read_dir(root).is_ok())
        .map(|root| Segment {
            label: root.trim_end_matches('\\').to_string(),
            path: root,
        })
        .collect()
}

#[cfg(not(windows))]
fn volume_roots() -> Vec<Segment> {
    vec![Segment {
        label: "/".into(),
        path: "/".into(),
    }]
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
