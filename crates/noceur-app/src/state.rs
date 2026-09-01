//! Application state.

use noceur_index::{Db, IndexError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Shared handle to the index.
///
/// `rusqlite::Connection` is `Send` but not `Sync`, so the database lives behind a
/// mutex and is cloned into blocking tasks rather than borrowed across await points.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
    /// The audio thread. Started once and shared: playback outlives any one
    /// listing, and the output device cannot be opened per command.
    pub player: Arc<noceur_audio::Player>,
    /// What the transport is showing beyond the audio itself.
    ///
    /// The player knows a path and a position; the bar wants the artist, the
    /// BPM, the key and the cover. Those come from the index at the moment a
    /// track is loaded and would otherwise be re-queried on every poll.
    pub now_playing: Arc<Mutex<NowPlaying>>,
}

/// Tags of the loaded track, read once when it is loaded.
#[derive(Debug, Clone, Default)]
pub struct NowPlaying {
    pub file_id: i64,
    pub artist: String,
    pub title: String,
    pub bpm: String,
    pub key: String,
    pub key_norm: Option<i64>,
    /// 0..100. Held here because the audio thread has no reason to report it
    /// back and the UI needs the slider to survive a reload.
    pub volume: u32,
}

impl AppState {
    /// Opens the index at the given path, creating it if needed.
    ///
    /// Starting the player cannot fail here — a machine with no output device
    /// still browses, and the failure surfaces on the first command instead.
    pub fn open(path: &std::path::Path) -> Result<Self, IndexError> {
        Ok(AppState {
            db: Arc::new(Mutex::new(Db::open(path)?)),
            player: Arc::new(noceur_audio::Player::start()),
            now_playing: Arc::new(Mutex::new(NowPlaying {
                volume: 100,
                ..Default::default()
            })),
        })
    }

    /// Where the index lives: alongside the executable, so a portable install
    /// stays portable. Falls back to the working directory.
    pub fn default_db_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("noceur-index.db")))
            .unwrap_or_else(|| PathBuf::from("noceur-index.db"))
    }
}
