//! Tauri commands. Every one that touches the filesystem runs off-thread.

use crate::dto::{
    DirListing, ListRequest, Order, ProbeProgress, RowWindow, RowsRequest, SearchHit,
    SearchRequest, Segment, Transport,
};
use crate::format;
use crate::state::AppState;
use noceur_fsmodel::path::simplify;
use noceur_fsmodel::{natural_cmp, EntryKind};
use noceur_index::{list, volume, Db, IndexError, ListOptions, Show, SortColumn};
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
    let background = state.background.clone();
    let path = PathBuf::from(&req.path);
    let show = req.show;

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;

        let started = Instant::now();
        let volume_id = volume::for_path(&db, &path).map_err(|e| e.to_string())?;
        let mut dir_id =
            noceur_index::scan_dir(&db, volume_id, &path).map_err(|e| e.to_string())?;
        // Copies a tag write left behind when the app stopped halfway through
        // one. Found in the index rather than by listing the folder again, so a
        // folder with none costs one query.
        let leftovers =
            noceur_index::query::names_containing(&db, dir_id, crate::background::WRITING)
                .map_err(|e| e.to_string())?;
        if !leftovers.is_empty() && crate::background::sweep(&background, &path, &leftovers) {
            dir_id = noceur_index::scan_dir(&db, volume_id, &path).map_err(|e| e.to_string())?;
        }
        let scan_ms = started.elapsed().as_millis() as u64;

        let total = noceur_index::count(&db, dir_id, options(&req.order, show, 0, FIRST_WINDOW)?)
            .map_err(|e| e.to_string())?;
        let audio = noceur_index::count(
            &db,
            dir_id,
            options(&req.order, Show::AudioOnly, 0, FIRST_WINDOW)?,
        )
        .map_err(|e| e.to_string())?;
        let first = window(&db, dir_id, &req.order, show, 0, FIRST_WINDOW)?;

        let shown = simplify(&path);
        Ok(DirListing {
            dir_id,
            path: shown.to_string_lossy().into_owned(),
            segments: segments_of(&shown),
            total,
            audio,
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
    let (dir_id, show) = (req.dir_id, req.show);
    let order = req.order.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        window(&db, dir_id, &order, show, offset, limit)
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
fn options(order: &Order, show: Show, offset: i64, limit: i64) -> Result<ListOptions, String> {
    let sort = match order.sort.as_deref() {
        None | Some("") => SortColumn::Name,
        Some(name) => {
            SortColumn::parse(name).ok_or_else(|| format!("unknown sort column: {name}"))?
        }
    };

    Ok(ListOptions {
        sort,
        desc: order.desc,
        show,
        offset,
        limit,
    })
}

/// Reads and formats one window of rows.
fn window(
    db: &Db,
    dir_id: i64,
    order: &Order,
    show: Show,
    offset: i64,
    limit: i64,
) -> Result<RowWindow, String> {
    let rows = list(db, dir_id, options(order, show, offset, limit)?)
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

    // The path and the tags in one trip: the bar shows artist, BPM and key
    // beside the transport, and re-reading them on every position poll would be
    // a query ten times a second for values that cannot change.
    let loaded = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let found = noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?;
        let tags = noceur_index::search::tags_of(&db, file_id).map_err(|e| e.to_string())?;
        Ok::<_, String>((found, tags))
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    let (found, tags) = loaded;
    let Some(found) = found.filter(|f| f.kind == 1) else {
        return Err("that row is not an audio file".into());
    };

    state
        .player
        .load(&found.path, true)
        .map_err(|e| e.to_string())?;

    if let Ok(mut np) = state.now_playing.lock() {
        np.file_id = file_id;
        np.artist = tags.artist.unwrap_or_default();
        np.title = tags.title.unwrap_or_default();
        np.bpm = format::bpm(tags.bpm, format::BpmSource::Tag);
        np.key = format::key(tags.key_norm, tags.key_tag.as_deref(), KeyNotation::Camelot);
        np.key_norm = tags.key_norm;
        np.sample_rate = tags.sample_rate.unwrap_or(0).max(0) as u32;
    }
    // What is being listened to is measured first.
    state.background.prioritise(file_id);

    // Waited for, because `load` only hands the path to the audio thread. Until
    // that thread has opened the file, the player still describes whatever was
    // playing before — so replying immediately answered "what is loaded now?"
    // with the wrong track, or with nothing at all on the first play of a
    // session. The front end then had no file id to fetch a waveform for and,
    // seeing `playing: false`, never started polling to find out otherwise.
    //
    // An MP3 opened fast enough to win that race and a large FLAC did not, which
    // is exactly why FLACs looked like they needed playing twice.
    let player = state.player.clone();
    let opening = found.path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        player.wait_until_loaded(&opening, LOAD_TIMEOUT);
    })
    .await
    .map_err(|e| format!("load task failed: {e}"))?;

    Ok(transport(&state))
}

/// How long a load is waited for before the transport reports what it knows.
///
/// Long enough for a big lossless file on a cold network share, short enough
/// that a genuinely stuck read does not hold a command open indefinitely. A file
/// slower than this still loads — the transport just learns about it from the
/// next poll rather than from this reply.
const LOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
        "volume" => {
            // Recorded here as well as sent to the player. The audio thread has
            // no reason to report a level back, so this is the only copy the
            // transport can read — and without it the slider is handed its old
            // position on every reply and springs back under the pointer while
            // the sound it is setting changes anyway.
            let level = value.unwrap_or(100).min(100) as u32;
            if let Ok(mut np) = state.now_playing.lock() {
                np.volume = level;
            }
            state.player.set_volume(level as f32 / 100.0)
        }
        other => return Err(format!("unknown transport action: {other}")),
    };

    result.map_err(|e| e.to_string())?;
    // Reply with what the command did, not with the moment before it.
    state.player.settle(std::time::Duration::from_millis(250));
    Ok(transport(&state))
}

/// The newest frames sent to the output, for the meters in the transport.
///
/// Raw bytes rather than JSON: this is read on every display frame while
/// playing, and a few thousand numbers spelled out in decimal sixty times a
/// second is work for both ends that the meters do not need. The layout is the
/// sample rate as a little-endian `u32`, then `frames` stereo pairs of
/// little-endian `i16`, oldest first. Sixteen bits is ninety-odd decibels of
/// range, well past the bottom of any meter drawn here.
#[tauri::command]
pub fn scope_frames(state: State<'_, AppState>, frames: Option<u32>) -> tauri::ipc::Response {
    let (rate, data) = state.player.scope(frames.unwrap_or(2048) as usize);
    let mut out = Vec::with_capacity(4 + data.len() * 4);
    out.extend_from_slice(&rate.to_le_bytes());
    for [l, r] in data {
        for s in [l, r] {
            let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    tauri::ipc::Response::new(out)
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
    let np = state
        .now_playing
        .lock()
        .map(|n| n.clone())
        .unwrap_or_default();

    // Nothing loaded means nothing to say about a track, whatever was last read.
    let loaded = !path.is_empty();

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
        file_id: if loaded { np.file_id } else { 0 },
        sample_rate: if loaded { np.sample_rate } else { 0 },
        artist: if loaded { np.artist } else { String::new() },
        title: if loaded { np.title } else { String::new() },
        bpm: if loaded { np.bpm } else { String::new() },
        key: if loaded { np.key } else { String::new() },
        key_norm: if loaded { np.key_norm } else { None },
        volume: np.volume,
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
    use std::os::windows::process::CommandExt;

    // The command line is built by hand rather than through `arg`, because
    // Explorer wants the quotes around the *path* and `arg` puts them around the
    // whole switch: any track whose name has a space in it became
    // `"/select,E:\...\a b.flac"`, which Explorer cannot parse, so it silently
    // opened Documents instead of selecting anything.
    //
    // `\\?\` is stripped for the same reason — Explorer does not understand the
    // extended prefix and treats it as a path that does not exist.
    let shown = simplify(path);
    std::process::Command::new("explorer.exe")
        .raw_arg(format!("/select,\"{}\"", shown.display()))
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

/// Starts a native drag of the given rows.
///
/// This is what makes a track draggable into Rekordbox, Explorer, or anything
/// else that accepts files: the payload is a real `CF_HDROP`, offered from the
/// window's own HWND, so the receiving application sees a file drop and not a
/// web page's idea of one.
///
/// `DoDragDrop` runs a modal loop on the thread owning the window, so the drag
/// is scheduled onto the main thread and this command returns immediately. The
/// result of the drop is reported through the `drag-finished` event rather than
/// as a return value — the drag outlives the call by design.
#[tauri::command]
pub async fn start_drag(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let mut out = Vec::new();
        for id in file_ids {
            if let Some(found) = noceur_index::thumb::locate(&db, id).map_err(|e| e.to_string())? {
                out.push(found.path);
            }
        }
        Ok::<_, String>(out)
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    if paths.is_empty() {
        return Ok(());
    }

    drag_on_main_thread(app, paths)
}

#[cfg(windows)]
fn drag_on_main_thread(app: tauri::AppHandle, paths: Vec<PathBuf>) -> Result<(), String> {
    use tauri::{Emitter, Manager};

    let hwnd = {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| "no main window".to_string())?;
        window.hwnd().map_err(|e| e.to_string())?.0 as isize
    };

    let handle = app.clone();
    app.run_on_main_thread(move || {
        let app = handle;
        let outcome = match crate::dragdrop::drag_files(hwnd, &paths) {
            Ok(dropped) => dropped.as_str().to_string(),
            Err(e) => format!("error: {e}"),
        };
        // The listing may need re-reading: a move takes the files out of it.
        let _ = app.emit("drag-finished", outcome);
    })
    .map_err(|e| format!("cannot start the drag: {e}"))
}

#[cfg(not(windows))]
fn drag_on_main_thread(_app: tauri::AppHandle, _paths: Vec<PathBuf>) -> Result<(), String> {
    Err("native drag is only implemented on Windows".into())
}

/// Copies, moves or deletes rows through the shell.
///
/// Everything the shell does for Explorer it does here: the progress dialog, the
/// name-collision prompt, elevation where a destination needs it, deletion to the
/// recycle bin, and the system undo stack — so a mistaken move is Ctrl+Z away
/// rather than gone.
#[tauri::command]
pub async fn file_op(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    action: String,
    file_ids: Vec<i64>,
    destination: Option<String>,
) -> Result<(), String> {
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let mut out = Vec::new();
        for id in file_ids {
            if let Some(found) = noceur_index::thumb::locate(&db, id).map_err(|e| e.to_string())? {
                out.push(found.path);
            }
        }
        Ok::<_, String>(out)
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    shell_file_op(app, action, paths, destination.map(PathBuf::from)).await
}

/// The same operation on paths the index has never seen — files dropped in from
/// Explorer, which have no row here until they land.
#[tauri::command]
pub async fn file_op_paths(
    app: tauri::AppHandle,
    action: String,
    paths: Vec<String>,
    destination: Option<String>,
) -> Result<(), String> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    shell_file_op(app, action, paths, destination.map(PathBuf::from)).await
}

/// Decides copy-versus-move for a drop, the way Explorer decides it.
///
/// Within a volume a drop moves; across volumes it copies. Anything else would
/// surprise a user whose hands already know Explorer — including its keys:
/// Ctrl held at the drop copies, Shift moves.
#[tauri::command]
pub fn drop_effect(sources: Vec<String>, destination: String) -> String {
    if key_held(Key::Ctrl) {
        return "copy".into();
    }
    if key_held(Key::Shift) {
        return "move".into();
    }
    let dest = PathBuf::from(&destination);
    let same = sources
        .first()
        .map(|s| crate::fileops::same_volume(Path::new(s), &dest))
        .unwrap_or(false);
    if same {
        "move".into()
    } else {
        "copy".into()
    }
}

enum Key {
    Ctrl,
    Shift,
}

/// Whether a modifier is down right now. Asked of the system rather than the
/// web view, which sees no keys while the shell runs a drag.
#[cfg(windows)]
fn key_held(key: Key) -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_SHIFT};
    let vk = match key {
        Key::Ctrl => VK_CONTROL,
        Key::Shift => VK_SHIFT,
    };
    // SAFETY: a plain query of key state; any virtual-key code is valid.
    (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16) & 0x8000 != 0
}

#[cfg(not(windows))]
fn key_held(_key: Key) -> bool {
    false
}

#[cfg(windows)]
async fn shell_file_op(
    app: tauri::AppHandle,
    action: String,
    paths: Vec<PathBuf>,
    destination: Option<PathBuf>,
) -> Result<(), String> {
    use tauri::Manager;

    let op = crate::fileops::Operation::parse(&action)
        .ok_or_else(|| format!("unknown file operation: {action}"))?;

    let hwnd = {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| "no main window".to_string())?;
        window.hwnd().map_err(|e| e.to_string())?.0 as isize
    };

    // Off the UI thread: the shell shows its own modal dialogs and drives its own
    // message loop, and it must not be the loop this window is drawn from.
    tauri::async_runtime::spawn_blocking(move || {
        crate::fileops::run(hwnd, op, &paths, destination.as_deref())
    })
    .await
    .map_err(|e| format!("file operation task failed: {e}"))?
}

#[cfg(not(windows))]
async fn shell_file_op(
    _app: tauri::AppHandle,
    _action: String,
    _paths: Vec<PathBuf>,
    _destination: Option<PathBuf>,
) -> Result<(), String> {
    Err("shell file operations are only implemented on Windows".into())
}

/// Shows the shell's own context menu for the selected rows.
///
/// The same menu Explorer shows, built by the shell from the same items — which
/// means every installed shell extension appears in it, and every verb does what
/// it does in Explorer, because it is the same code running.
#[tauri::command]
pub async fn shell_menu(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let mut out = Vec::new();
        for id in file_ids {
            if let Some(found) = noceur_index::thumb::locate(&db, id).map_err(|e| e.to_string())? {
                out.push(found.path);
            }
        }
        Ok::<_, String>(out)
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    if paths.is_empty() {
        return Ok(());
    }

    show_shell_menu(app, paths)
}

#[cfg(windows)]
fn show_shell_menu(app: tauri::AppHandle, paths: Vec<PathBuf>) -> Result<(), String> {
    use tauri::{Emitter, Manager};

    let hwnd = {
        let window = app
            .get_webview_window("main")
            .ok_or_else(|| "no main window".to_string())?;
        window.hwnd().map_err(|e| e.to_string())?.0 as isize
    };

    // Tracking a popup is modal and belongs to the thread owning the window.
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = crate::shellmenu::show(hwnd, &paths);
        // Whatever the user picked may have renamed, deleted or moved something,
        // and the shell does not tell us which — so the listing is re-read.
        let _ = handle.emit("shell-menu-closed", ());
    })
    .map_err(|e| format!("cannot show the menu: {e}"))
}

#[cfg(not(windows))]
fn show_shell_menu(_app: tauri::AppHandle, _paths: Vec<PathBuf>) -> Result<(), String> {
    Err("the shell context menu is only implemented on Windows".into())
}

/// Opens files in another program, all of them in one launch.
///
/// One process with every path as an argument, rather than one per file: an
/// editor like Mp3tag handed forty files should open one window, not forty.
#[tauri::command]
pub async fn open_with(
    state: State<'_, AppState>,
    program: String,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let program = program.trim().trim_matches('"').to_string();
    if program.is_empty() {
        return Err("no program set".into());
    }
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        Ok::<_, String>(
            file_ids
                .iter()
                .filter_map(|id| noceur_index::thumb::locate(&db, *id).ok().flatten())
                .map(|f| {
                    let p = f.path.display().to_string();
                    // Programs rarely understand the extended-length prefix.
                    p.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(p)
                })
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    if paths.is_empty() {
        return Err("nothing to open".into());
    }
    std::process::Command::new(&program)
        .args(&paths)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {program}: {e}"))
}

/// Opens a URL in the default browser.
///
/// Used by the search actions in the row menu — looking a track up on Discogs is
/// something a DJ does constantly, and it is one of the few things a music
/// browser can do that a file manager cannot.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    // Only http(s). A context menu that will launch anything the frontend hands
    // it is a way to run arbitrary programs by way of a crafted listing.
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("only http and https links can be opened".into());
    }
    open_in_browser(&url)
}

#[cfg(windows)]
fn open_in_browser(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let verb = wide("open");
    let target = wide(url);

    // SAFETY: both strings are NUL-terminated and outlive the call.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW reports success as a value above 32 — a handle-shaped return
    // from an API old enough to predate error codes.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err("no application is registered to open links".into())
    }
}

#[cfg(not(windows))]
fn open_in_browser(_url: &str) -> Result<(), String> {
    Err("opening links is only implemented on Windows".into())
}

/// The spectrogram of one file: time across, frequency up, loudness as a byte.
///
/// Cached like the envelope and for stronger reasons — measuring one is a full
/// decode plus several thousand transforms. Unlike the envelope it is never
/// computed during a scan, only for a track somebody has asked to look at.
///
/// Returns an empty vector rather than an error when there is nothing to draw,
/// which is what a directory, an unreadable file and a file with no content row
/// all amount to from the front end's side.
#[tauri::command]
pub async fn spectrogram(state: State<'_, AppState>, file_id: i64) -> Result<Vec<u8>, String> {
    let db = state.db.clone();

    // Three short holds of the lock rather than one long one, exactly as the
    // envelope does: measuring decodes the whole track, and holding the index
    // across that would freeze every listing and thumbnail for the duration.
    tauri::async_runtime::spawn_blocking(move || {
        let columns = noceur_audio::spectrogram::COLUMNS;
        let bands = noceur_audio::spectrogram::BANDS;

        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?
        };
        let Some(found) = found.filter(|f| f.kind == 1) else {
            return Ok(Vec::new());
        };

        // Without a linked content there is nowhere to cache the result, so it
        // is measured and handed back uncached rather than refused.
        let Some(content_id) = found.content_id else {
            return Ok(
                noceur_audio::spectrogram::spectrogram(&found.path, columns, bands)
                    .unwrap_or_default(),
            );
        };

        {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            match noceur_index::spectrogram::get(&db, content_id, columns, bands)
                .map_err(|e| e.to_string())?
            {
                noceur_index::spectrogram::Cached::Grid(data) => return Ok(data),
                noceur_index::spectrogram::Cached::Miss => return Ok(Vec::new()),
                noceur_index::spectrogram::Cached::Unknown => {}
            }
        }

        let measured = noceur_audio::spectrogram::spectrogram(&found.path, columns, bands).ok();

        {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::spectrogram::put(&db, content_id, columns, bands, measured.as_deref())
                .map_err(|e| e.to_string())?;
        }

        Ok(measured.unwrap_or_default())
    })
    .await
    .map_err(|e| format!("spectrogram task failed: {e}"))?
}

/// The amplitude envelope of one file, for the waveform.
///
/// Returned as raw bytes for the UI to draw rather than as a rendered image.
/// Cover art goes over `thumb://` because it is tens of kilobytes apiece and a
/// scrolling list wants hundreds of them; an envelope is two kilobytes and there
/// is exactly one on screen. Drawing it in the client also means the played
/// portion can be recoloured as the playhead moves, and the bar can be redrawn at
/// a new width on resize, without asking the backend for anything.
///
/// Empty means "nothing to draw" — not audio, or audio that would not decode.
/// The transport falls back to a plain progress bar, which is what it had before.
#[tauri::command]
pub async fn envelope(state: State<'_, AppState>, file_id: i64) -> Result<Vec<u8>, String> {
    let db = state.db.clone();

    // Deliberately three short holds of the lock rather than one long one:
    // extracting an envelope decodes the entire track, which for a long lossless
    // file is seconds. Holding the index across that would freeze every listing,
    // every thumbnail and every search for the duration.
    tauri::async_runtime::spawn_blocking(move || {
        let buckets = noceur_audio::ENVELOPE_BUCKETS;
        let bands = noceur_audio::ENVELOPE_BANDS;

        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?
        };
        let Some(found) = found.filter(|f| f.kind == 1) else {
            return Ok(Vec::new());
        };

        // Without a linked content there is nowhere to cache the result, so the
        // envelope is computed and handed back uncached rather than refused: a
        // freshly scanned folder should still draw a waveform.
        let Some(content_id) = found.content_id else {
            return Ok(noceur_audio::envelope(&found.path, buckets).unwrap_or_default());
        };

        {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            match noceur_index::envelope::get(&db, content_id, buckets, bands)
                .map_err(|e| e.to_string())?
            {
                noceur_index::envelope::Cached::Envelope(data) => return Ok(data),
                noceur_index::envelope::Cached::Miss => return Ok(Vec::new()),
                noceur_index::envelope::Cached::Unknown => {}
            }
        }

        let extracted = noceur_audio::envelope(&found.path, buckets).ok();

        {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::envelope::put(&db, content_id, buckets, extracted.as_deref())
                .map_err(|e| e.to_string())?;
        }

        Ok(extracted.unwrap_or_default())
    })
    .await
    .map_err(|e| format!("envelope task failed: {e}"))?
}

#[cfg(test)]
mod transport_tests {
    use super::*;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::TempDir::new().unwrap();
        let state = AppState::open(&dir.path().join("index.db")).unwrap();
        (dir, state)
    }

    /// The transport bar's volume slider is a controlled input: it is redrawn
    /// from whatever the last reply said, ten times a second while playing. A
    /// report of zero on a fresh player would peg the slider to silence and hold
    /// it there against every attempt to drag it.
    #[test]
    fn a_fresh_transport_reports_full_volume() {
        let (_dir, state) = state();
        assert_eq!(transport(&state).volume, 100);
    }

    /// The level lives here rather than in the audio thread, which has no reason
    /// to report one back. If a change is not recorded, the slider is handed its
    /// old position on the very next reply and springs back under the pointer.
    #[test]
    fn a_changed_volume_survives_into_the_next_report() {
        let (_dir, state) = state();
        state.now_playing.lock().unwrap().volume = 40;
        assert_eq!(transport(&state).volume, 40);
    }
}
