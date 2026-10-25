//! "Show in Noceur" on an audio file's right-click menu in Explorer.
//!
//! Off until it is ticked, on the installer's last page or in Settings. Then
//! it is written under the current user's classes once for each audio
//! extension Noceur lists, naming the executable that was running when it was
//! ticked.
//!
//! Explorer starts a new process per click. The single-instance plugin hands
//! that process's arguments to the window already open and ends it, so a
//! second click reaches the running app as a `show-file` event; the very first
//! one is kept here until the front end asks for it.

use crate::state::AppState;
use noceur_fsmodel::path::simplify;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// The file the app was started on, until the front end has taken it.
#[derive(Default)]
pub struct Launch(pub Mutex<Option<String>>);

/// The first argument that names a file, ignoring flags.
pub fn file_arg<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && Path::new(a).is_file())
}

/// A second start, forwarded by the single-instance plugin.
pub fn forwarded(app: &AppHandle, args: Vec<String>) {
    if let Some(view) = app.get_webview_window("main") {
        let _ = view.unminimize();
        let _ = view.set_focus();
    }
    if let Some(path) = file_arg(args) {
        let _ = app.emit("show-file", path);
    }
}

/// The file the app was started on, once.
#[tauri::command]
pub fn launch_file(launch: State<'_, Launch>) -> Option<String> {
    launch.0.lock().ok()?.take()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Located {
    pub folder: String,
    pub file_id: Option<i64>,
}

/// The folder holding `path`, and the file's id in it so it can be selected.
///
/// The folder is scanned first: Explorer can point at a file in a folder the
/// index has never seen.
#[tauri::command]
pub async fn locate(state: State<'_, AppState>, path: String) -> Result<Located, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        let dir = path.parent().ok_or("not a file in a folder")?;
        let name = path.file_name().ok_or("not a file")?.to_string_lossy();
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        let volume_id = noceur_index::for_path(&db, dir).map_err(|e| e.to_string())?;
        let dir_id = noceur_index::scan_dir(&db, volume_id, dir).map_err(|e| e.to_string())?;
        let file_id =
            noceur_index::query::file_named(&db, dir_id, &name).map_err(|e| e.to_string())?;
        Ok(Located {
            folder: simplify(dir).to_string_lossy().into_owned(),
            file_id,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Whether "Show in Noceur" is on the menu, and the switch for it.
///
/// Settings and the installer both come here: the installer runs the
/// executable with `--context-menu on` or `off` rather than keeping its own
/// copy of which extensions are audio.
#[tauri::command]
pub fn context_menu() -> bool {
    menu::installed()
}

#[tauri::command]
pub fn set_context_menu(on: bool) -> Result<(), String> {
    menu::set(on)
}

/// `--context-menu on|off`: does that and nothing else. The answer, when it
/// is not a bare flag, is the process's exit code.
pub fn context_menu_flag<I: IntoIterator<Item = String>>(args: I) -> Option<i32> {
    let mut args = args.into_iter().skip(1);
    while let Some(a) = args.next() {
        if a == "--context-menu" {
            let on = match args.next().as_deref() {
                Some("on") => true,
                Some("off") => false,
                _ => return Some(2),
            };
            return Some(if menu::set(on).is_ok() { 0 } else { 1 });
        }
    }
    None
}

#[cfg(windows)]
mod menu {
    use noceur_fsmodel::path::simplify;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    const CLASSES: &str = r"Software\Classes\SystemFileAssociations";

    /// Per extension rather than once under the `audio` perceived type: the
    /// menus Explorer and Directory Opus build for a file do not always take
    /// in the perceived type's verbs, and an extension's own entry they always
    /// do. `mp4` is left out, since most of those are video.
    fn extensions() -> impl Iterator<Item = &'static str> {
        noceur_fsmodel::AUDIO_EXTENSIONS
            .iter()
            .copied()
            .filter(|e| *e != "mp4")
    }

    fn verb(ext: &str) -> String {
        format!(r"{CLASSES}\.{ext}\shell\noceur")
    }

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn installed() -> bool {
        let key = wide(&format!(r"{}\command", verb("mp3")));
        let mut size = 0u32;
        // SAFETY: the key is NUL-terminated; a null buffer asks only whether
        // the value is there.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        status == 0
    }

    pub fn set(on: bool) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = simplify(&exe).to_string_lossy().into_owned();

        // Sets one value on a key, creating the key on the way.
        let put = |key: &str, name: Option<&str>, value: &str| -> Result<(), String> {
            let key = wide(key);
            let name = name.map(wide);
            let data = wide(value);
            // SAFETY: the strings are NUL-terminated, the name is null for the
            // key's default value, and the size is the data's length in bytes
            // including its terminator.
            let status = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if status == 0 {
                Ok(())
            } else {
                Err(format!("could not write the menu entry (error {status})"))
            }
        };
        let remove = |key: &str| {
            let key = wide(key);
            // SAFETY: the subkey is NUL-terminated; deleting one that is not
            // there fails harmlessly.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr()) };
        };

        // The perceived-type entry the first builds wrote, which would show
        // the verb twice on the files that did get it.
        remove(&format!(r"{CLASSES}\audio\shell\noceur"));
        for ext in extensions() {
            let verb = verb(ext);
            if on {
                put(&verb, None, "Show in Noceur")?;
                put(&verb, Some("Icon"), &format!("\"{exe}\",0"))?;
                put(
                    &format!(r"{verb}\command"),
                    None,
                    &format!("\"{exe}\" \"%1\""),
                )?;
            } else {
                remove(&verb);
            }
        }
        // Explorer caches what each file type's menu holds, and a change
        // stays out of it until it is told the associations changed.
        // SAFETY: the event takes no items, so both are null.
        unsafe {
            SHChangeNotify(
                SHCNE_ASSOCCHANGED as i32,
                SHCNF_IDLIST,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        Ok(())
    }
}

#[cfg(not(windows))]
mod menu {
    pub fn installed() -> bool {
        false
    }

    pub fn set(_on: bool) -> Result<(), String> {
        Err("only on Windows".into())
    }
}
