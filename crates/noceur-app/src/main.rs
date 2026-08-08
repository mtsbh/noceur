//! Noceur desktop shell.
//!
//! The only Tauri-aware crate in the workspace. Everything below it compiles and
//! tests headless.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod format;
mod state;

use state::AppState;

fn main() {
    let db_path = AppState::default_db_path();
    let app_state = match AppState::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to open index at {}: {e}", db_path.display());
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::parent_of
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Noceur");
}
