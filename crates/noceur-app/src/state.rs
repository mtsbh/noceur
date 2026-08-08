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
}

impl AppState {
    /// Opens the index at the given path, creating it if needed.
    pub fn open(path: &std::path::Path) -> Result<Self, IndexError> {
        Ok(AppState {
            db: Arc::new(Mutex::new(Db::open(path)?)),
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
