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

/// Reports a failure that happens before the window exists, then exits.
///
/// Release builds are linked against the Windows subsystem, so there is no
/// console for `eprintln!` to reach and a bare `exit(1)` here is
/// indistinguishable from the app doing nothing at all. Leave the reason on
/// disk for a later look, and put it on screen while the user is still
/// watching.
fn fatal(message: &str) -> ! {
    eprintln!("{message}");

    let log = AppState::default_db_path().with_file_name("noceur-startup-error.log");
    let _ = std::fs::write(&log, message);

    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

        fn wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        let (text, caption) = (wide(message), wide("Noceur"));
        // SAFETY: both strings are NUL-terminated and outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                caption.as_ptr(),
                MB_OK | MB_ICONERROR,
            );
        }
    }

    std::process::exit(1)
}

fn main() {
    let db_path = AppState::default_db_path();
    let app_state = match AppState::open(&db_path) {
        Ok(s) => s,
        Err(e) => fatal(&format!("Noceur could not open its index.\n\n{e}")),
    };

    if let Err(e) = tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::parent_of
        ])
        .run(tauri::generate_context!())
    {
        fatal(&format!("Noceur failed to start.\n\n{e}"));
    }
}
