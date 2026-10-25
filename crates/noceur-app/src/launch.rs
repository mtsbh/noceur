//! "Show in Noceur" on an audio file's right-click menu in Explorer.
//!
//! The verb is written under the current user's classes on every start rather
//! than by the installer, so a build run straight out of `target\release` gets
//! it too, and the command always names the executable that is actually here.
//! It is written once for each audio extension Noceur lists.
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

/// Writes the Explorer verb. Failing is not worth stopping the app over.
#[cfg(windows)]
pub fn register() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };
    use windows_sys::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let exe = simplify(&exe).to_string_lossy().into_owned();

    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // Sets one value on a key, creating the key on the way.
    let set = |key: &str, name: Option<&str>, value: &str| {
        let key = wide(key);
        let name = name.map(wide);
        let data = wide(value);
        // SAFETY: the strings are NUL-terminated, the name is null for the
        // key's default value, and the size is the data's length in bytes
        // including its terminator.
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            );
        }
    };

    // The command as it is now, to tell whether there is anything to write.
    let current = |key: &str| -> Option<String> {
        let key = wide(key);
        let mut buf = vec![0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: the key is NUL-terminated, the null name reads the default
        // value, and `size` is the buffer's length in bytes.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    };

    // Per extension rather than once under the `audio` perceived type: the
    // menus Explorer and Directory Opus build for a file do not always take in
    // the perceived type's verbs, and an extension's own entry they always do.
    // `mp4` is left out, since most of those are video.
    const CLASSES: &str = r"Software\Classes\SystemFileAssociations";
    let command = format!("\"{exe}\" \"%1\"");
    let mut changed = false;
    for ext in noceur_fsmodel::AUDIO_EXTENSIONS
        .iter()
        .filter(|e| **e != "mp4")
    {
        let verb = format!(r"{CLASSES}\.{ext}\shell\noceur");
        let command_key = format!(r"{verb}\command");
        if current(&command_key).as_deref() == Some(command.as_str()) {
            continue;
        }
        set(&verb, None, "Show in Noceur");
        set(&verb, Some("Icon"), &format!("\"{exe}\",0"));
        set(&command_key, None, &command);
        changed = true;
    }
    // The perceived-type entry earlier builds wrote, which would now show the
    // verb twice on files that did get it.
    let old = wide(&format!(r"{CLASSES}\audio\shell\noceur"));
    // SAFETY: the subkey is NUL-terminated; deleting one that is not there
    // fails harmlessly.
    if unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, old.as_ptr()) } == 0 {
        changed = true;
    }
    if !changed {
        return;
    }
    // Explorer caches what each file type's menu holds, and a new verb stays
    // out of it until it is told the associations changed.
    // SAFETY: the event takes no items, so both are null.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}

#[cfg(not(windows))]
pub fn register() {}
