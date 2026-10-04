//! Sending files to Telegram Desktop.
//!
//! Done the way Explorer's own "Send to → Telegram" does it: Telegram is started
//! with `--` and the files, and the running copy comes to the front with its
//! chat picker. The user picks the chat and sends, from their own account: no
//! bot to set up, and Telegram's usual 2 GB limit rather than a bot's 50 MB.
//! Nothing is sent until they choose where.

use crate::state::AppState;
use std::path::{Path, PathBuf};
use tauri::State;

/// The uninstall entry Telegram's installer writes, which says where it put it.
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{53F49750-6209-4FBF-9CA8-7A333C87D1ED}_is1";

/// Windows refuses a command line past 32,767 characters. Kept well short of
/// it, with room for the program's own path.
const MAX_ARGS_LEN: usize = 30_000;

/// Where Telegram Desktop is, or `None` when it is not installed.
///
/// The installer's default folder first, then wherever the installer says it
/// put it, then the Microsoft Store edition's app alias, which takes the same
/// arguments.
pub fn find() -> Option<PathBuf> {
    candidates().into_iter().find(|p| p.is_file())
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(roaming) = std::env::var_os("APPDATA") {
        out.push(
            Path::new(&roaming)
                .join("Telegram Desktop")
                .join("Telegram.exe"),
        );
    }
    #[cfg(windows)]
    if let Some(dir) = installed_at() {
        out.push(dir.join("Telegram.exe"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        out.push(
            Path::new(&local)
                .join("Microsoft")
                .join("WindowsApps")
                .join("Telegram.exe"),
        );
    }
    out
}

/// The installer's `InstallLocation`, for an install outside the default.
#[cfg(windows)]
fn installed_at() -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};

    let wide = |s: &str| -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let key = wide(UNINSTALL_KEY);
    let name = wide("InstallLocation");
    let mut buf = vec![0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: both names are NUL-terminated, and `size` is the buffer's length
    // in bytes; the call writes at most that many.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
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
    let dir = PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]));
    (!dir.as_os_str().is_empty()).then_some(dir)
}

/// Whether there is a Telegram to send to. The button is hidden when not.
#[tauri::command]
pub fn telegram_available() -> bool {
    find().is_some()
}

/// Hands the selected files to Telegram, which opens its chat picker.
///
/// Folders are skipped: Telegram sends files, and would only say so after the
/// user had picked a chat.
#[tauri::command]
pub async fn send_to_telegram(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
) -> Result<(), String> {
    let telegram = find().ok_or("Telegram Desktop is not installed")?;
    let db = state.db.clone();
    let paths = tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        Ok::<_, String>(
            file_ids
                .iter()
                .filter_map(|id| noceur_index::thumb::locate(&db, *id).ok().flatten())
                .map(|f| f.path)
                .filter(|p| p.is_file())
                .map(|p| plain(&p))
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|e| format!("lookup task failed: {e}"))??;

    let args = arguments(&paths)?;
    std::process::Command::new(&telegram)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start Telegram: {e}"))
}

/// A path without the extended-length prefix, which Telegram would read as
/// part of the name.
fn plain(path: &Path) -> String {
    let p = path.display().to_string();
    p.strip_prefix(r"\?\").map(str::to_string).unwrap_or(p)
}

/// `--` and the files, as Explorer's Send to shortcut passes them. Everything
/// after `--` is taken as a file, so a name starting with a dash cannot be
/// mistaken for one of Telegram's switches.
fn arguments(paths: &[String]) -> Result<Vec<String>, String> {
    if paths.is_empty() {
        return Err("nothing to send: select files, not folders".into());
    }
    // Each argument may gain quotes and a space.
    let len: usize = paths.iter().map(|p| p.len() + 3).sum();
    if len > MAX_ARGS_LEN {
        return Err(format!(
            "{} files are too many to hand over at once; send fewer",
            paths.len()
        ));
    }
    Ok(std::iter::once("--".to_string())
        .chain(paths.iter().cloned())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_follow_the_separator() {
        let args = arguments(&[r"C:\a b\-x.flac".into(), r"D:\y.mp3".into()]).unwrap();
        assert_eq!(args, ["--", r"C:\a b\-x.flac", r"D:\y.mp3"]);
    }

    #[test]
    fn nothing_to_send_is_an_error() {
        assert!(arguments(&[]).is_err());
    }

    #[test]
    fn a_selection_too_long_for_one_command_line_is_refused() {
        let many: Vec<String> = (0..400)
            .map(|i| format!(r"X:\{}\{i}.aiff", "d".repeat(80)))
            .collect();
        assert!(arguments(&many).unwrap_err().contains("400 files"));
    }

    #[test]
    fn the_extended_length_prefix_is_dropped() {
        assert_eq!(plain(Path::new(r"\?\X:\ssd\a.aiff")), r"X:\ssd\a.aiff");
        assert_eq!(plain(Path::new(r"X:\ssd\a.aiff")), r"X:\ssd\a.aiff");
    }

    /// The default install comes first, as it is where nearly everyone has it.
    #[test]
    fn the_default_install_is_looked_for_first() {
        let first = candidates().into_iter().next().unwrap();
        assert!(
            first.ends_with(r"Telegram Desktop\Telegram.exe"),
            "{}",
            first.display()
        );
    }
}
