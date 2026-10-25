//! Noceur desktop shell.
//!
//! The only Tauri-aware crate in the workspace. Everything below it compiles and
//! tests headless.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod analysis;
mod art;
mod autotag;
mod background;
mod cleanup;
mod clipboard;
mod commands;
mod config;
mod convert;
mod covers;
mod dragdrop;
mod dto;
mod fileops;
mod folders;
mod format;
mod fromname;
mod identity;
mod launch;
mod newfolder;
mod overview;
mod placement;
mod playlists;
mod quality;
mod rekordbox;
mod renamer;
mod sets;
mod shellmenu;
mod state;
mod tagedit;
mod telegram;
mod timestamp;
mod tools;
mod undo;

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
    // The installer's errand, before anything that would open a window or
    // the index.
    if let Some(code) = launch::context_menu_flag(std::env::args()) {
        std::process::exit(code);
    }

    let db_path = AppState::default_db_path();
    let app_state = match AppState::open(&db_path) {
        Ok(s) => s,
        Err(e) => fatal(&format!("Noceur could not open its index.\n\n{e}")),
    };

    let art_state = app_state.clone();
    playlists::init(&app_state);
    let launch = launch::Launch(std::sync::Mutex::new(launch::file_arg(std::env::args())));

    if let Err(e) = tauri::Builder::default()
        // First, so a second start hands its file to this window and exits
        // before it opens anything of its own.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            launch::forwarded(app, args)
        }))
        .manage(app_state)
        .manage(launch)
        // The window opens at whatever size it was closed at. Restored in
        // `setup` rather than through the configuration, because the saved
        // position has to be checked against the monitors that are actually
        // attached before it is used.
        .setup(|app| {
            use tauri::Manager;
            if let Some(view) = app.get_webview_window("main") {
                placement::restore(&view.as_ref().window());
            }
            let state = app.state::<AppState>().inner().clone();
            background::start(app.handle().clone(), state);
            Ok(())
        })
        // Saved on the way out rather than on every drag: a window being moved
        // across a screen would otherwise write the config file a hundred times
        // on the way.
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                placement::remember(window);
            }
        })
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
            commands::search_tag,
            commands::index_folders,
            commands::play,
            commands::transport_cmd,
            commands::transport_state,
            commands::scope_frames,
            convert::ffmpeg_info,
            convert::set_ffmpeg_path,
            convert::convert_plan,
            convert::convert_file,
            newfolder::create_folder,
            fromname::preview_from_name,
            fromname::apply_from_name,
            commands::reveal,
            commands::path_of,
            commands::start_drag,
            commands::start_drag_paths,
            commands::file_op,
            commands::file_op_paths,
            commands::drop_effect,
            commands::shell_menu,
            commands::open_url,
            commands::open_with,
            undo::undo_rename,
            clipboard::clipboard_put,
            clipboard::clipboard_files,
            clipboard::clipboard_release,
            commands::envelope,
            commands::spectrogram,
            tagedit::read_tags,
            tagedit::write_tags,
            tagedit::quick_fields,
            covers::find_covers,
            covers::apply_cover,
            covers::remove_cover,
            autotag::match_track,
            autotag::credentials,
            autotag::set_discogs_token,
            autotag::set_tidal,
            autotag::set_trackid,
            autotag::set_trackid_session,
            autotag::set_shazam,
            telegram::telegram_available,
            telegram::send_to_telegram,
            renamer::preview_rename,
            renamer::apply_rename,
            config::preserve_dates,
            config::set_preserve_dates,
            quality::analyse_quality,
            quality::write_replay_gain,
            analysis::analyse_tracks,
            background::analysis_settings,
            background::set_analysis_settings,
            background::analysis_status,
            identity::identify,
            identity::duplicates,
            overview::overview,
            actions::actions,
            actions::set_actions,
            actions::preview_action,
            actions::run_action,
            cleanup::apply_cleanup,
            tools::preview_tool,
            playlists::playlists,
            playlists::playlist_create,
            playlists::playlist_rename,
            playlists::playlist_delete,
            playlists::playlist_tracks,
            playlists::playlist_toggle,
            playlists::playlist_remove,
            playlists::playlist_reorder,
            playlists::playlists_holding,
            sets::set_search,
            sets::set_tracklist,
            sets::set_link,
            sets::set_preview,
            sets::set_unlink,
            sets::set_offset,
            sets::set_appearances,
            sets::set_upload,
            sets::wanted_toggle,
            sets::wanted_list,
            sets::wanted_remove,
            sets::wanted_marks,
            rekordbox::rekordbox_target,
            rekordbox::set_rekordbox_xml,
            rekordbox::send_to_rekordbox,
            launch::launch_file,
            launch::locate,
            launch::context_menu,
            launch::set_context_menu,
            commands::parent_of
        ])
        .run(tauri::generate_context!())
    {
        fatal(&format!("Noceur failed to start.\n\n{e}"));
    }
}
