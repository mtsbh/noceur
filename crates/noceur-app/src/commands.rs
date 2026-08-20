//! Tauri commands. Every one that touches the filesystem runs off-thread.

use crate::dto::{
    DirListing, ListRequest, Order, ProbeProgress, RowWindow, RowsRequest, SearchHit,
    SearchRequest, Segment, Transport,
};
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

/// Opens a directory: scans it and reports how many rows it holds.
///
/// Deliberately does **not** read tags. Enumeration is milliseconds; reading tags
/// out of every file in the folder is seconds to minutes — thirty-five seconds
/// was measured on a 667-file folder — and doing it here meant the window sat on
/// "Reading…" for the whole of it while holding the index locked against
/// everything else. Explorer shows you the folder immediately because it does not
/// open the files; so does this now. Tags arrive afterwards, over [`probe`].
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

        let total = noceur_index::count(
            &db,
            dir_id,
            options(&req.order, audio_only, 0, FIRST_WINDOW)?,
        )
        .map_err(|e| e.to_string())?;
        let first = window(&db, dir_id, &req.order, audio_only, 0, FIRST_WINDOW)?;

        let shown = simplify(&path);
        Ok(DirListing {
            dir_id,
            path: shown.to_string_lossy().into_owned(),
            segments: segments_of(&shown),
            total,
            first,
            scan_ms,
        })
    })
    .await
    .map_err(|e| format!("listing task failed: {e}"))?
}

/// Reads tags for one batch of a directory's files.
///
/// The frontend drives this in a loop after the folder is already on screen, so
/// BPM, key, duration and cover art fill in behind a listing the user can already
/// read and scroll. Returning after a fixed number of files is what keeps the
/// index available: the lock is taken and released once per batch instead of once
/// per folder.
///
/// Returns how many files were attempted. Zero means the folder is fully probed
/// and the caller should stop.
#[tauri::command]
pub async fn probe(state: State<'_, AppState>, dir_id: i64) -> Result<ProbeProgress, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let started = Instant::now();
        let probed = noceur_index::probe_batch(&db, dir_id, noceur_index::PROBE_BATCH)
            .map_err(|e| e.to_string())?;
        Ok(ProbeProgress {
            probed: probed as i64,
            done: probed == 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    })
    .await
    .map_err(|e| format!("probe task failed: {e}"))?
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
    let order = req.order.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        window(&db, dir_id, &order, audio_only, offset, limit)
    })
    .await
    .map_err(|e| format!("window task failed: {e}"))?
}

/// The one place list options are built, so `open_dir` and `list_rows` cannot
/// drift into cutting windows from differently ordered lists.
///
/// An unrecognised column name is an error rather than a silent fall back to
/// name order: a frontend asking for a column that does not exist is a bug, and
/// a list that quietly ignores the header you clicked is worse than one that
/// says so.
fn options(
    order: &Order,
    audio_only: bool,
    offset: i64,
    limit: i64,
) -> Result<ListOptions, String> {
    let sort = match order.sort.as_deref() {
        None | Some("") => SortColumn::Name,
        Some(name) => {
            SortColumn::parse(name).ok_or_else(|| format!("unknown sort column: {name}"))?
        }
    };

    Ok(ListOptions {
        sort,
        desc: order.desc,
        audio_only,
        offset,
        limit,
    })
}

/// Reads and formats one window of rows.
fn window(
    db: &Db,
    dir_id: i64,
    order: &Order,
    audio_only: bool,
    offset: i64,
    limit: i64,
) -> Result<RowWindow, String> {
    let rows = list(db, dir_id, options(order, audio_only, offset, limit)?)
        .map_err(|e: IndexError| e.to_string())?;
    Ok(RowWindow {
        offset,
        rows: rows
            .iter()
            .map(|r| format::display(r, KeyNotation::Camelot))
            .collect(),
    })
}

/// Results the popover shows before the user has to scroll. Enough to find what
/// they meant; not so many that ranking stops mattering.
const SEARCH_LIMIT: i64 = 200;

/// Searches the indexed library.
///
/// Runs off-thread like every other index read, and takes the lock only for the
/// query itself — a search issued while a folder is still being probed waits for
/// one batch, not for the folder.
#[tauri::command]
pub async fn search(
    state: State<'_, AppState>,
    req: SearchRequest,
) -> Result<Vec<SearchHit>, String> {
    let db = state.db.clone();
    let limit = req.limit.unwrap_or(SEARCH_LIMIT).clamp(1, MAX_LIMIT);

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let hits =
            noceur_index::search(&db, &req.query, &req.scopes, limit).map_err(|e| e.to_string())?;

        Ok(hits
            .into_iter()
            .map(|h| {
                let is_audio = h.kind == 1;
                SearchHit {
                    file_id: h.file_id,
                    kind: h.kind,
                    name: h.name,
                    dir_path: simplify(Path::new(&h.dir_path))
                        .to_string_lossy()
                        .into_owned(),
                    bpm: if is_audio {
                        format::bpm(h.bpm, format::BpmSource::Tag)
                    } else {
                        String::new()
                    },
                    key: if is_audio {
                        format::key(h.key_norm, h.key_tag.as_deref(), KeyNotation::Camelot)
                    } else {
                        String::new()
                    },
                    duration: if is_audio {
                        format::duration(h.duration_ms)
                    } else {
                        String::new()
                    },
                    artist: h.artist.unwrap_or_default(),
                }
            })
            .collect())
    })
    .await
    .map_err(|e| format!("search task failed: {e}"))?
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
            .filter(|e| !is_hidden(&Path::new(&path).join(&e.name)))
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

/// Whether the shell would keep this out of sight.
///
/// Tested by attribute rather than by name, so `$RECYCLE.BIN` and
/// `System Volume Information` are excluded for the reason Explorer excludes
/// them — they are hidden system directories — and so is anything else that is,
/// on every drive, in every language, without a list to maintain.
#[cfg(windows)]
fn is_hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;

    std::fs::metadata(path)
        .map(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0)
        .unwrap_or(false)
}

/// Dotfiles are the closest equivalent where there are no file attributes.
#[cfg(not(windows))]
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
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

/// Loads a file into the player and starts it.
///
/// Takes a `file_id` rather than a path: the list already has ids, and resolving
/// through the index keeps the frontend from constructing paths of its own.
#[tauri::command]
pub async fn play(state: State<'_, AppState>, file_id: i64) -> Result<Transport, String> {
    let db = state.db.clone();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    let Some(found) = found.filter(|f| f.kind == 1) else {
        return Err("that row is not an audio file".into());
    };

    state
        .player
        .load(&found.path, true)
        .map_err(|e| e.to_string())?;
    Ok(transport(&state))
}

/// Play/pause, stop, and seek. One command rather than four, because they are
/// the same interaction from the UI's point of view: change what the transport
/// is doing and tell me what it is doing now.
#[tauri::command]
pub async fn transport_cmd(
    state: State<'_, AppState>,
    action: String,
    value: Option<u64>,
) -> Result<Transport, String> {
    let result = match action.as_str() {
        "toggle" => state.player.toggle(),
        "pause" => state.player.pause(),
        "resume" => state.player.resume(),
        "stop" => state.player.stop(),
        "seek" => state
            .player
            .seek(std::time::Duration::from_millis(value.unwrap_or(0))),
        "volume" => state.player.set_volume(value.unwrap_or(100) as f32 / 100.0),
        other => return Err(format!("unknown transport action: {other}")),
    };

    result.map_err(|e| e.to_string())?;
    Ok(transport(&state))
}

/// Polled by the transport bar while something is playing.
#[tauri::command]
pub fn transport_state(state: State<'_, AppState>) -> Transport {
    transport(&state)
}

fn transport(state: &AppState) -> Transport {
    let p = state.player.state();
    let path = p.path.unwrap_or_default();
    let name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let duration_ms = p.duration_ms.unwrap_or(0);

    Transport {
        path,
        name,
        playing: p.playing,
        position_ms: p.position_ms,
        duration_ms,
        position: format::duration(Some(p.position_ms as i64)),
        duration: if duration_ms > 0 {
            format::duration(Some(duration_ms as i64))
        } else {
            format::ABSENT.to_string()
        },
        error: p.error.unwrap_or_default(),
    }
}

/// Shows a row in Explorer, selected.
///
/// The shell is asked to select the file rather than just open its folder, so a
/// track in a folder of four hundred is actually findable when it gets there.
#[tauri::command]
pub async fn reveal(state: State<'_, AppState>, file_id: i64) -> Result<(), String> {
    let db = state.db.clone();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    let Some(found) = found else {
        return Err("no such row".into());
    };
    reveal_in_shell(&found.path)
}

#[cfg(windows)]
fn reveal_in_shell(path: &Path) -> Result<(), String> {
    // `/select,` takes the path as one argument and Explorer parses it itself,
    // so it is passed unquoted here; quoting it would make the quotes part of
    // the path Explorer looks for.
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(|_| ())
        // Explorer returns a non-zero exit code even when it succeeds, so only
        // the failure to launch it at all is worth reporting.
        .map_err(|e| format!("cannot open Explorer: {e}"))
}

#[cfg(not(windows))]
fn reveal_in_shell(_path: &Path) -> Result<(), String> {
    Err("revealing a file is only implemented on Windows".into())
}

/// The full path of a row, for copying.
#[tauri::command]
pub async fn path_of(state: State<'_, AppState>, file_id: i64) -> Result<String, String> {
    let db = state.db.clone();
    let found = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    found
        .map(|f| f.path.display().to_string())
        .ok_or_else(|| "no such row".into())
}
