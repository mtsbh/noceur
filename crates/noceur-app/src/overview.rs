//! The library at a glance.

use crate::state::AppState;
use noceur_index::overview::Overview;
use tauri::State;

/// Statistics over the index, or only the folder `scope` and what is under it.
#[tauri::command]
pub async fn overview(
    state: State<'_, AppState>,
    scope: Option<String>,
) -> Result<Overview, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        noceur_index::overview::overview(&db, scope.as_deref()).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("overview task failed: {e}"))?
}
