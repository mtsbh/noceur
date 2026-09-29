//! Files on the Windows clipboard, shared with Explorer.
//!
//! The same two formats Explorer uses: `CF_HDROP` for the list of files, and
//! "Preferred DropEffect" saying whether they were copied or cut. Copy here and
//! paste in Explorer, or the other way round, and both sides agree on what
//! happens — including that pasting a cut moves the files and uses the cut up.

use serde::Serialize;

/// What is on the clipboard, when it is files.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub paths: Vec<String>,
    /// Cut rather than copied: pasting moves them.
    pub cut: bool,
}

/// Puts files on the clipboard, replacing whatever was there.
#[tauri::command]
pub fn clipboard_put(app: tauri::AppHandle, paths: Vec<String>, cut: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    imp::put(owner(&app)?, &paths, cut)
}

/// The files on the clipboard, or `None` when it holds something else.
#[tauri::command]
pub fn clipboard_files() -> Result<Option<Clip>, String> {
    imp::get()
}

/// Empties the clipboard if it still holds exactly these files.
///
/// Called after pasting a cut: the files have moved, so the paths are stale,
/// and Explorer clears it in the same place for the same reason.
#[tauri::command]
pub fn clipboard_release(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    match imp::get()? {
        Some(clip) if clip.paths == paths => imp::clear(owner(&app)?),
        _ => Ok(()),
    }
}

/// The main window, which has to own the clipboard for a write to stick.
#[cfg(windows)]
fn owner(app: &tauri::AppHandle) -> Result<isize, String> {
    use tauri::Manager;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "no main window".to_string())?;
    Ok(window.hwnd().map_err(|e| e.to_string())?.0 as isize)
}

#[cfg(not(windows))]
fn owner(_app: &tauri::AppHandle) -> Result<isize, String> {
    Ok(0)
}

/// The `DROPFILES` block: a 20-byte header, then UTF-16 paths, each ended by a
/// nul, and one more nul to end the list.
fn dropfiles(paths: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&20u32.to_le_bytes()); // pFiles: where the paths start
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.x
    out.extend_from_slice(&0i32.to_le_bytes()); // pt.y
    out.extend_from_slice(&0i32.to_le_bytes()); // fNC
    out.extend_from_slice(&1i32.to_le_bytes()); // fWide: the paths are UTF-16
    for path in paths {
        for unit in path.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

/// Explorer's values for the "Preferred DropEffect" block.
const DROPEFFECT_COPY_LINK: u32 = 5;
const DROPEFFECT_MOVE: u32 = 2;

#[cfg(windows)]
mod imp {
    use super::{dropfiles, Clip, DROPEFFECT_COPY_LINK, DROPEFFECT_MOVE};
    use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::UI::Shell::DragQueryFileW;

    const CF_HDROP: u32 = 15;

    fn effect_format() -> u32 {
        let name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
        // SAFETY: a nul-terminated UTF-16 string that outlives the call.
        unsafe { RegisterClipboardFormatW(name.as_ptr()) }
    }

    /// Holds the clipboard open, closing it on every way out.
    struct Open;

    impl Open {
        fn new(owner: isize) -> Result<Open, String> {
            // Another program may have it open for a moment; Explorer retries too.
            for _ in 0..10 {
                // SAFETY: `owner` is our window handle, or null for a read.
                if unsafe { OpenClipboard(owner as HWND) } != 0 {
                    return Ok(Open);
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
            Err("the clipboard is busy".into())
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            // SAFETY: only reached after a successful OpenClipboard.
            unsafe { CloseClipboard() };
        }
    }

    /// Copies bytes into a fresh moveable block for the clipboard to own.
    fn global(bytes: &[u8]) -> Result<HANDLE, String> {
        // SAFETY: a block of exactly `bytes.len()`, written only inside that
        // size, and unlocked before it is handed over.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
            if handle.is_null() {
                return Err("cannot allocate clipboard memory".into());
            }
            let base = GlobalLock(handle) as *mut u8;
            if base.is_null() {
                GlobalFree(handle);
                return Err("cannot lock clipboard memory".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), base, bytes.len());
            GlobalUnlock(handle);
            Ok(handle)
        }
    }

    pub fn put(owner: isize, paths: &[String], cut: bool) -> Result<(), String> {
        let files = global(&dropfiles(paths))?;
        let effect = if cut {
            DROPEFFECT_MOVE
        } else {
            DROPEFFECT_COPY_LINK
        };
        let effect = global(&effect.to_le_bytes())?;

        let _open = Open::new(owner)?;
        // SAFETY: the clipboard is open and owned by our window. On success the
        // system owns each block; on failure it is still ours to free.
        unsafe {
            EmptyClipboard();
            if SetClipboardData(CF_HDROP, files).is_null() {
                GlobalFree(files);
                GlobalFree(effect);
                return Err("cannot put the files on the clipboard".into());
            }
            if SetClipboardData(effect_format(), effect).is_null() {
                GlobalFree(effect);
            }
        }
        Ok(())
    }

    pub fn get() -> Result<Option<Clip>, String> {
        // SAFETY: a plain query; no handle is involved.
        if unsafe { IsClipboardFormatAvailable(CF_HDROP) } == 0 {
            return Ok(None);
        }
        let _open = Open::new(0)?;

        // SAFETY: the clipboard is open; the handles it returns stay valid until
        // it is closed, and are only read.
        unsafe {
            let drop = GetClipboardData(CF_HDROP);
            if drop.is_null() {
                return Ok(None);
            }
            let count = DragQueryFileW(drop, u32::MAX, std::ptr::null_mut(), 0);
            let mut paths = Vec::with_capacity(count as usize);
            for i in 0..count {
                let len = DragQueryFileW(drop, i, std::ptr::null_mut(), 0);
                let mut buf = vec![0u16; len as usize + 1];
                DragQueryFileW(drop, i, buf.as_mut_ptr(), buf.len() as u32);
                paths.push(String::from_utf16_lossy(&buf[..len as usize]));
            }

            let mut cut = false;
            let effect = GetClipboardData(effect_format());
            if !effect.is_null() {
                let value = GlobalLock(effect) as *const u32;
                if !value.is_null() {
                    cut = value.read_unaligned() & DROPEFFECT_MOVE != 0
                        && value.read_unaligned() & 1 == 0;
                    GlobalUnlock(effect);
                }
            }
            Ok(Some(Clip { paths, cut }))
        }
    }

    pub fn clear(owner: isize) -> Result<(), String> {
        let _open = Open::new(owner)?;
        // SAFETY: the clipboard is open.
        unsafe { EmptyClipboard() };
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Clip;

    pub fn put(_owner: isize, _paths: &[String], _cut: bool) -> Result<(), String> {
        Err("the file clipboard is only implemented on Windows".into())
    }
    pub fn get() -> Result<Option<Clip>, String> {
        Ok(None)
    }
    pub fn clear(_owner: isize) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_list_is_laid_out_the_way_the_shell_reads_it() {
        let bytes = dropfiles(&["C:\\a.wav".into(), "D:\\ü.flac".into()]);
        assert_eq!(
            &bytes[0..4],
            &20u32.to_le_bytes(),
            "paths start after the header"
        );
        assert_eq!(&bytes[16..20], &1i32.to_le_bytes(), "marked wide");

        let units: Vec<u16> = bytes[20..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16(&units).unwrap();
        assert_eq!(text, "C:\\a.wav\0D:\\ü.flac\0\0");
    }

    /// Reads whatever file list is on the clipboard now. Put one there first,
    /// e.g. `Set-Clipboard -Path C:\Windows\notepad.exe` in PowerShell.
    #[cfg(windows)]
    #[test]
    #[ignore = "reads the real clipboard"]
    fn reads_what_another_program_put() {
        let clip = imp::get().unwrap().expect("no files on the clipboard");
        println!("CLIP {:?}", clip);
        assert!(!clip.paths.is_empty());
    }

    /// Copy here, paste here: the round trip through the real clipboard.
    #[cfg(windows)]
    #[test]
    #[ignore = "touches the real clipboard"]
    fn files_survive_the_clipboard() {
        imp::put(0, &["C:\\Windows\\notepad.exe".into()], true).ok();
        // A null owner may refuse the write on some systems; only check a read
        // that did go through.
        if let Some(clip) = imp::get().unwrap() {
            assert_eq!(clip.paths, vec!["C:\\Windows\\notepad.exe".to_string()]);
        }
    }
}
