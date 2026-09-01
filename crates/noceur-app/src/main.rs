//! Noceur desktop shell.
//!
//! The only Tauri-aware crate in the workspace. Everything below it compiles and
//! tests headless.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod commands;
mod dragdrop;
mod dto;
mod fileops;
mod format;
mod shellmenu;
mod state;
mod tagedit;

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

    let art_state = app_state.clone();

    if let Err(e) = tauri::Builder::default()
        .manage(app_state)
        // Cover art is served as images the web view fetches by URL rather than
        // as command results, so scrolling does not push megabytes of base64
        // through the IPC bridge. `register_asynchronous_*` keeps the decode off
        // the main loop: a slow thumbnail delays its own row, not the window.
        .register_asynchronous_uri_scheme_protocol("thumb", move |_ctx, request, responder| {
            let state = art_state.clone();
            // The blocking pool, not a fresh thread per request. A screen of rows
            // asks for its images all at once and again on every scroll, and
            // spawning an OS thread for each was paying thread-creation cost per
            // thumbnail and letting unbounded numbers of them contend for the
            // index at the same time.
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(art::respond(&state, &request));
            });
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_dir,
            commands::subdirs,
            commands::roots,
            commands::list_rows,
            commands::probe,
            commands::search,
            commands::play,
            commands::transport_cmd,
            commands::transport_state,
            commands::reveal,
            commands::path_of,
            commands::start_drag,
            commands::file_op,
            commands::file_op_paths,
            commands::drop_effect,
            commands::shell_menu,
            commands::open_url,
            commands::envelope,
            tagedit::read_tags,
            tagedit::write_tags,
            commands::parent_of
        ])
        .run(tauri::generate_context!())
    {
        fatal(&format!("Noceur failed to start.\n\n{e}"));
    }
}
