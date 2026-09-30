//! Measuring BPM and key without being asked.
//!
//! Each track the player loads is measured by a low-priority thread while it
//! plays. Only what is listened to, not every folder that is opened: that is
//! the library the user is actually working through, and it keeps the disk
//! quiet while browsing. The measurement goes into
//! the index, where the list shows it beside the file's own tags; what happens
//! to the file itself is the user's setting:
//!
//! - [`Mode::Fill`] writes BPM and key only where the file has none. A tag the
//!   file already carries is never replaced by this module, whatever it says.
//! - [`Mode::Show`] writes nothing.
//! - [`Mode::Overwrite`] writes whatever was measured.
//!
//! Off until switched on. A file that is loaded in the player is written after
//! playback moves on, since Windows may refuse to rewrite a file that is open.

use crate::state::AppState;
use noceur_index::analysis::{self, Detected, Method};
use noceur_tags::{KeyNotation, MusicalKey};
use std::collections::{HashSet, VecDeque};
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

/// Bumped when the model or the way tempo is measured changes, so every stored
/// measurement is taken again.
pub const VERSION: i64 = 1;

/// Threads measuring at once. One is plenty for tracks as they are played:
/// a measurement takes a second or two and a track lasts minutes.
const WORKERS: usize = 1;

/// How often an idle worker looks again at writes held back for the player.
const RECHECK: Duration = Duration::from_secs(2);

/// What happens to a file once it is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Write only fields the file does not have.
    #[default]
    Fill,
    /// Write nothing; the list shows what was measured.
    Show,
    /// Write every measurement.
    Overwrite,
}

/// The background analysis settings, stored in the config.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    pub mode: Mode,
    /// The tempo range that settles half and double time.
    pub min: f64,
    pub max: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            enabled: false,
            mode: Mode::Fill,
            min: 70.0,
            max: 180.0,
        }
    }
}

impl Settings {
    pub fn method(&self) -> Method {
        Method {
            version: VERSION,
            range_min: self.min,
            range_max: self.max,
        }
    }
}

/// What the indicator shows.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Tracks waiting or being measured.
    pub pending: usize,
}

/// The queue and the threads that work through it.
pub struct Background {
    settings: Mutex<Settings>,
    queue: Mutex<Queue>,
    wake: Condvar,
}

#[derive(Default)]
struct Queue {
    order: VecDeque<i64>,
    queued: HashSet<i64>,
    /// Files whose write waits for the player to move on.
    held: Vec<i64>,
    /// Files being measured right now.
    active: usize,
}

impl Background {
    pub fn new(settings: Settings) -> Background {
        Background {
            settings: Mutex::new(settings),
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
        }
    }

    pub fn settings(&self) -> Settings {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Applies new settings. Switching off empties the queue.
    pub fn set(&self, settings: Settings) {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = settings;
        if !settings.enabled {
            let mut q = self.lock();
            q.order.clear();
            q.queued.clear();
            q.held.clear();
        }
        self.wake.notify_all();
    }

    pub fn status(&self) -> Status {
        let q = self.lock();
        Status {
            pending: q.order.len() + q.active,
        }
    }

    /// Puts one file at the front, as the player does with what it loads.
    pub fn prioritise(&self, file_id: i64) {
        if !self.settings().enabled {
            return;
        }
        let mut q = self.lock();
        if q.queued.contains(&file_id) {
            q.order.retain(|id| *id != file_id);
        }
        q.queued.insert(file_id);
        q.order.push_front(file_id);
        drop(q);
        self.wake.notify_one();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What a setting would write to a file, given what it is tagged with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub bpm: Option<String>,
    pub key: Option<String>,
}

impl Plan {
    fn is_empty(&self) -> bool {
        self.bpm.is_none() && self.key.is_none()
    }
}

/// Decides what to write. `key_tag` is whether the file has any key text at
/// all, parseable or not: under Fill a key nobody can read is still a key the
/// user put there.
pub fn plan(
    mode: Mode,
    bpm_tag: Option<f64>,
    key_norm: Option<i64>,
    key_tag: bool,
    detected: &Detected,
) -> Plan {
    let bpm_text = detected
        .bpm
        .map(|v| crate::format::bpm(Some(v), crate::format::BpmSource::Tag));
    let key_text = detected
        .key_norm
        .and_then(|k| MusicalKey::from_canonical(k as i32))
        .map(|k| k.format(KeyNotation::Camelot));
    let has_bpm = bpm_tag.is_some_and(|v| v > 0.0);

    match mode {
        Mode::Show => Plan::default(),
        Mode::Fill => Plan {
            bpm: bpm_text.filter(|_| !has_bpm),
            key: key_text.filter(|_| !key_tag),
        },
        Mode::Overwrite => Plan {
            bpm: bpm_text.filter(|_| !has_bpm || crate::format::bpm_differs(bpm_tag, detected.bpm)),
            key: key_text
                .filter(|_| !key_tag || key_norm.is_none() || key_norm != detected.key_norm),
        },
    }
}

/// Starts the workers. Called once, from setup.
pub fn start(app: AppHandle, state: AppState) {
    for _ in 0..WORKERS {
        let (app, state) = (app.clone(), state.clone());
        std::thread::Builder::new()
            .name("noceur-analysis".into())
            .spawn(move || {
                lower_priority();
                worker(&app, &state);
            })
            .ok();
    }
}

/// Keeps measuring below everything the user is waiting for.
fn lower_priority() {
    #[cfg(windows)]
    // SAFETY: changes only the calling thread's own scheduling priority.
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

fn worker(app: &AppHandle, state: &AppState) {
    let bg = &state.background;
    loop {
        let file_id = {
            let mut q = bg.lock();
            loop {
                if bg.settings().enabled {
                    release_held(&mut q, state);
                    if let Some(id) = q.order.pop_front() {
                        q.queued.remove(&id);
                        q.active += 1;
                        break id;
                    }
                }
                q = bg
                    .wake
                    .wait_timeout(q, RECHECK)
                    .map(|(q, _)| q)
                    .unwrap_or_else(|e| e.into_inner().0);
            }
        };
        let _ = app.emit("analysis-status", bg.status());

        let changed = process(state, file_id).unwrap_or(false);

        bg.lock().active -= 1;
        if changed {
            let _ = app.emit("analysis-changed", file_id);
        }
        let _ = app.emit("analysis-status", bg.status());
    }
}

/// Puts back held writes whose file the player has let go of.
fn release_held(q: &mut Queue, state: &AppState) {
    if q.held.is_empty() {
        return;
    }
    let playing = playing(state);
    let (free, still): (Vec<i64>, Vec<i64>) = q.held.drain(..).partition(|id| Some(*id) != playing);
    q.held = still;
    for id in free {
        if q.queued.insert(id) {
            q.order.push_front(id);
        }
    }
}

fn playing(state: &AppState) -> Option<i64> {
    state
        .now_playing
        .lock()
        .ok()
        .map(|np| np.file_id)
        .filter(|id| *id != 0)
}

/// Measures one file if it needs it and applies the setting. `true` when
/// anything the list shows changed.
fn process(state: &AppState, file_id: i64) -> Result<bool, String> {
    let settings = state.background.settings();
    let method = settings.method();

    let (found, stored, tags) = {
        let db = state.db.lock().map_err(|_| "index lock poisoned")?;
        let found = noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?;
        let stored = match found.as_ref().and_then(|f| f.content_id) {
            Some(c) => analysis::get(&db, c).map_err(|e| e.to_string())?,
            None => None,
        };
        let tags = noceur_index::search::tags_of(&db, file_id).map_err(|e| e.to_string())?;
        (found, stored, tags)
    };
    let Some(found) = found.filter(|f| f.kind == 1) else {
        return Ok(false);
    };
    // Not linked by a probe yet, so there is nowhere to keep a measurement.
    let Some(content_id) = found.content_id else {
        return Ok(false);
    };

    let mut changed = false;
    let detected = match stored.filter(|s| s.is_current(&method)) {
        Some(s) => s.detected,
        None => {
            let detected = crate::analysis::measure_file(&found.path, &method);
            let db = state.db.lock().map_err(|_| "index lock poisoned")?;
            analysis::set(&db, content_id, &method, detected.as_ref())
                .map_err(|e| e.to_string())?;
            changed = true;
            detected
        }
    };
    let Some(detected) = detected else {
        return Ok(changed);
    };

    let key_text = tags
        .key_tag
        .as_deref()
        .is_some_and(|t| !t.trim().is_empty());
    if plan(settings.mode, tags.bpm, tags.key_norm, key_text, &detected).is_empty() {
        return Ok(changed);
    }
    // The index is a cache of the last probe. Before touching the file, ask the
    // file: under Fill, a tag written since by another program must survive.
    let Ok(meta) = noceur_tags::read(&found.path) else {
        return Ok(changed);
    };
    let fresh = plan(
        settings.mode,
        meta.bpm.map(f64::from),
        meta.key_norm.map(i64::from),
        meta.key_tag
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty()),
        &detected,
    );
    if fresh.is_empty() {
        return Ok(changed);
    }

    let edit = noceur_tags::Edit {
        bpm: noceur_tags::Change::from_input(fresh.bpm),
        key: noceur_tags::Change::from_input(fresh.key),
        ..Default::default()
    };
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;
    let in_player = playing(state) == Some(file_id);
    let written = crate::timestamp::preserving(&found.path, keep_dates, || {
        if in_player {
            write_beside(&found.path, &edit)
        } else {
            noceur_tags::edit::write(&found.path, &edit).map_err(|e| e.to_string())
        }
    });
    if let Err(e) = written {
        if !in_player {
            return Err(e);
        }
        // The swap was refused - a share that will not rename over an open
        // file, say. Written the ordinary way once the player lets go.
        let mut q = state.background.lock();
        if !q.held.contains(&file_id) {
            q.held.push(file_id);
        }
        return Ok(changed);
    }

    let db = state.db.lock().map_err(|_| "index lock poisoned")?;
    noceur_index::reprobe(&db, file_id).map_err(|e| e.to_string())?;
    if in_player {
        // The bar beside the transport shows the new values straight away.
        let tags = noceur_index::search::tags_of(&db, file_id).map_err(|e| e.to_string())?;
        if let Ok(mut np) = state.now_playing.lock() {
            if np.file_id == file_id {
                np.bpm = crate::format::bpm(tags.bpm, crate::format::BpmSource::Tag);
                np.key = crate::format::key(
                    tags.key_norm,
                    tags.key_tag.as_deref(),
                    KeyNotation::Camelot,
                );
                np.key_norm = tags.key_norm;
            }
        }
    }
    Ok(true)
}

/// Writes tags into a copy beside the file, then puts the copy in its place.
///
/// For the track in the player. Its decoder is reading the file through a
/// handle opened at load, from an offset worked out then; rewriting the file in
/// place can move the audio under it - an ID3 tag growing at the front of an
/// MP3 shifts every byte after it. Replaced instead, the player's handle keeps
/// the original bytes until the next load, and the path holds the new ones.
/// Windows allows the replacement because the player opened the file allowing
/// deletion, as every `std::fs::File` does.
///
/// Costs a copy of the file, on the background thread.
fn write_beside(path: &std::path::Path, edit: &noceur_tags::Edit) -> Result<(), String> {
    // The extension stays last: it is how the tag writer knows the format.
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or("no file name")?;
    let copy = match path.extension() {
        Some(ext) => {
            path.with_file_name(format!("{stem}.noceur-writing.{}", ext.to_string_lossy()))
        }
        None => path.with_file_name(format!("{stem}.noceur-writing")),
    };
    let created = std::fs::metadata(path).and_then(|m| m.created()).ok();

    let result = std::fs::copy(path, &copy)
        .map_err(|e| e.to_string())
        .and_then(|_| noceur_tags::edit::write(&copy, edit).map_err(|e| e.to_string()))
        .and_then(|_| std::fs::rename(&copy, path).map_err(|e| e.to_string()));
    if result.is_err() {
        let _ = std::fs::remove_file(&copy);
        return result;
    }

    // The copy was created just now; the track was not.
    #[cfg(windows)]
    if let Some(created) = created {
        use std::os::windows::fs::FileTimesExt;
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            let _ = file.set_times(std::fs::FileTimes::new().set_created(created));
        }
    }
    #[cfg(not(windows))]
    let _ = created;
    Ok(())
}

/// The background analysis settings.
#[tauri::command]
pub fn analysis_settings(state: State<'_, AppState>) -> Settings {
    state.background.settings()
}

/// Saves new background analysis settings and applies them at once.
#[tauri::command]
pub fn set_analysis_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let (min, max) = (
        settings.min.clamp(40.0, 250.0),
        settings.max.clamp(40.0, 250.0),
    );
    if max <= min * 1.5 {
        return Err("the range must span at least half an octave".into());
    }
    let settings = Settings {
        min,
        max,
        ..settings
    };
    let path = crate::config::Config::default_path();
    let mut config = crate::config::Config::load(&path);
    config.analysis = settings;
    config.save(&path)?;
    state.background.set(settings);
    Ok(settings)
}

/// How many files are waiting, for the indicator's first paint.
#[tauri::command]
pub fn analysis_status(state: State<'_, AppState>) -> Status {
    state.background.status()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thirty seconds of a kick at 125 BPM over an A minor chord, as a WAV.
    fn write_track(path: &std::path::Path) {
        let rate = 44_100u32;
        let n = rate as usize * 30;
        let beat = 60.0 / 125.0;
        let data: Vec<u8> = (0..n)
            .flat_map(|i| {
                let t = i as f32 / rate as f32;
                let since = t % beat;
                let kick =
                    (2.0 * std::f32::consts::PI * 55.0 * since).sin() * (-since * 30.0).exp();
                let chord: f32 = [220.0, 261.63, 329.63]
                    .iter()
                    .map(|f| (2.0 * std::f32::consts::PI * f * t).sin())
                    .sum();
                let s = (kick * 0.6 + chord * 0.1).clamp(-1.0, 1.0);
                ((s * 32_000.0) as i16).to_le_bytes()
            })
            .collect();
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&rate.to_le_bytes());
        f.extend_from_slice(&(rate * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&(data.len() as u32).to_le_bytes());
        f.extend_from_slice(&data);
        std::fs::write(path, f).unwrap();
    }

    /// An index holding one folder with `track` in it, background analysis on.
    /// Returns the state, the track's file id and its folder's id.
    fn indexed(folder: &std::path::Path) -> (AppState, i64, i64) {
        let state = AppState::open(&folder.join("index.db")).unwrap();
        let (file_id, dir_id) = {
            let db = state.db.lock().unwrap();
            let vol =
                noceur_index::volume::upsert(&db, "u", noceur_index::VolumeKind::Fixed, "t", "/")
                    .unwrap();
            let dir_id = noceur_index::scan_dir(&db, vol, folder).unwrap();
            noceur_index::probe_dir(&db, dir_id).unwrap();
            let rows = noceur_index::list(&db, dir_id, Default::default()).unwrap();
            let track = rows.iter().find(|r| r.kind == 1).unwrap();
            (track.file_id, dir_id)
        };
        state.background.set(Settings {
            enabled: true,
            ..Default::default()
        });
        (state, file_id, dir_id)
    }

    /// The whole path a played track takes: measured, kept, written into the
    /// empty fields, and the measurement carried to the retagged file.
    #[test]
    fn a_played_track_gets_its_empty_fields_filled() {
        let dir = tempfile::TempDir::new().unwrap();
        let track = dir.path().join("untagged.wav");
        write_track(&track);
        let (state, file_id, _) = indexed(dir.path());

        assert!(process(&state, file_id).unwrap(), "something changed");

        let meta = noceur_tags::read(&track).unwrap();
        let bpm = meta.bpm.expect("BPM written");
        assert!((bpm - 125.0).abs() < 1.0, "wrote {bpm}");
        assert!(meta.key.is_some(), "key written");

        // Already measured and now tagged: a second pass does nothing.
        assert!(!process(&state, file_id).unwrap());
    }

    /// Under Fill, a tag the file already has is left as it is.
    #[test]
    fn a_tagged_track_keeps_its_tags() {
        let dir = tempfile::TempDir::new().unwrap();
        let track = dir.path().join("tagged.wav");
        write_track(&track);
        noceur_tags::edit::write(
            &track,
            &noceur_tags::Edit {
                bpm: noceur_tags::Change::Set("99".into()),
                key: noceur_tags::Change::Set("3B".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let (state, file_id, dir_id) = indexed(dir.path());

        assert!(process(&state, file_id).unwrap(), "measured and kept");
        let meta = noceur_tags::read(&track).unwrap();
        assert_eq!(meta.bpm, Some(99.0));
        assert_eq!(meta.key_tag.as_deref(), Some("3B"));

        // The list now knows the file disagrees with what was measured.
        let db = state.db.lock().unwrap();
        let rows = noceur_index::list(&db, dir_id, Default::default()).unwrap();
        let row = rows.iter().find(|r| r.file_id == file_id).unwrap();
        assert!(crate::format::display(row, noceur_tags::KeyNotation::Camelot).bpm_differs);
    }

    /// The track in the player is written while it plays, without disturbing
    /// the handle the player is reading through, and the bar shows the result.
    #[test]
    fn the_playing_track_is_written_while_it_plays() {
        use std::io::{Read, Seek, SeekFrom};

        let dir = tempfile::TempDir::new().unwrap();
        let track = dir.path().join("playing.wav");
        write_track(&track);
        let original = std::fs::read(&track).unwrap();
        let (state, file_id, _) = indexed(dir.path());
        state.now_playing.lock().unwrap().file_id = file_id;

        // Held open the way the player holds it.
        let mut player = std::fs::File::open(&track).unwrap();

        assert!(process(&state, file_id).unwrap());
        let meta = noceur_tags::read(&track).unwrap();
        assert!(meta.bpm.is_some(), "written while playing");
        assert!(state.background.lock().held.is_empty());
        assert!(!dir.path().join("playing.noceur-writing.wav").exists());

        let mut seen = Vec::new();
        player.seek(SeekFrom::Start(0)).unwrap();
        player.read_to_end(&mut seen).unwrap();
        assert_eq!(
            seen, original,
            "the player's handle still reads what it opened"
        );

        let np = state.now_playing.lock().unwrap();
        assert!(np.bpm.starts_with("12"), "bar shows {}", np.bpm);
    }

    /// A write the swap could not do waits for the player to move on.
    #[test]
    fn a_held_write_goes_back_in_the_queue_once_the_player_lets_go() {
        let dir = tempfile::TempDir::new().unwrap();
        let state = AppState::open(&dir.path().join("index.db")).unwrap();
        state.now_playing.lock().unwrap().file_id = 7;
        let bg = &state.background;
        bg.set(Settings {
            enabled: true,
            ..Default::default()
        });
        bg.lock().held.push(7);

        release_held(&mut bg.lock(), &state);
        assert_eq!(bg.lock().held, [7], "still playing");

        state.now_playing.lock().unwrap().file_id = 8;
        release_held(&mut bg.lock(), &state);
        assert!(bg.lock().held.is_empty());
        assert_eq!(bg.lock().order.front(), Some(&7));
    }

    fn detected(bpm: f64, key: i64) -> Detected {
        Detected {
            bpm: Some(bpm),
            key_norm: Some(key),
            key_confidence: Some(0.5),
        }
    }

    /// The default must never replace what a file already says.
    #[test]
    fn fill_writes_only_what_is_missing() {
        let d = detected(125.0, 14); // 8A
        assert_eq!(
            plan(Mode::Fill, None, None, false, &d),
            Plan {
                bpm: Some("125".into()),
                key: Some("8A".into())
            }
        );
        assert_eq!(
            plan(Mode::Fill, Some(124.0), Some(3), true, &d),
            Plan::default(),
            "both tagged, however wrong"
        );
        assert_eq!(
            plan(Mode::Fill, Some(124.0), None, false, &d),
            Plan {
                bpm: None,
                key: Some("8A".into())
            }
        );
    }

    /// A key someone typed that does not parse is still theirs.
    #[test]
    fn fill_leaves_an_unreadable_key_alone() {
        let d = detected(125.0, 14);
        assert_eq!(plan(Mode::Fill, Some(125.0), None, true, &d).key, None);
    }

    #[test]
    fn show_writes_nothing() {
        assert_eq!(
            plan(Mode::Show, None, None, false, &detected(125.0, 14)),
            Plan::default()
        );
    }

    #[test]
    fn overwrite_writes_what_differs_and_only_that() {
        let d = detected(124.5, 14);
        assert_eq!(
            plan(Mode::Overwrite, Some(124.5), Some(14), true, &d),
            Plan::default(),
            "already the same"
        );
        assert_eq!(
            plan(Mode::Overwrite, Some(124.0), Some(3), true, &d),
            Plan {
                bpm: Some("124.5".into()),
                key: Some("8A".into())
            }
        );
    }

    #[test]
    fn nothing_measured_means_nothing_written() {
        let d = Detected::default();
        assert_eq!(
            plan(Mode::Overwrite, None, None, false, &d),
            Plan::default()
        );
    }

    #[test]
    fn switching_off_empties_the_queue() {
        let bg = Background::new(Settings {
            enabled: true,
            ..Default::default()
        });
        bg.prioritise(1);
        bg.prioritise(9);
        assert_eq!(bg.status().pending, 2);
        assert_eq!(bg.lock().order.front(), Some(&9), "the latest track first");
        bg.set(Settings::default());
        assert_eq!(bg.status().pending, 0);
        bg.prioritise(4);
        assert_eq!(bg.status().pending, 0, "nothing queues while off");
    }

    #[test]
    fn the_default_is_off_and_fills_empty_fields() {
        let s = Settings::default();
        assert!(!s.enabled);
        assert_eq!(s.mode, Mode::Fill);
    }
}
