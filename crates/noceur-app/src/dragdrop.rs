//! Native drag-and-drop, so files leave this window the way they leave Explorer.
//!
//! A web view cannot be an OLE drag source: its drag events stay inside the page
//! and produce nothing another process can accept. Dropping a track into
//! Rekordbox needs a real `IDataObject` carrying `CF_HDROP`, offered through
//! `DoDragDrop` from the window's own HWND — which is what this does.
//!
//! The shell builds both for us. `SHCreateDataObject` produces the data object
//! from item ID lists, and `SHDoDragDrop` supplies the default drop source and
//! the drag image Explorer shows, so neither `IDataObject` nor `IDropSource` has
//! to be implemented here.

#![cfg(windows)]

use std::mem::{size_of, ManuallyDrop};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Win32::Foundation::{GlobalFree, HWND, POINT};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::{
    IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{SHCreateDataObject, SHDoDragDrop, SHParseDisplayName, DROPFILES};

/// `CF_HDROP`, the format every application that accepts files reads.
///
/// Spelled out rather than imported: it is a stable, forty-year-old constant,
/// and the `windows` crate moves it between modules across releases.
const CF_HDROP: u16 = 15;

/// An item ID list that frees itself.
///
/// `SHParseDisplayName` allocates with the COM task allocator, and a drag that
/// returns early — a path that no longer exists, a shell that refuses — must not
/// leak one per attempted drag.
struct Pidl(*mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by SHParseDisplayName with the task allocator.
            unsafe { CoTaskMemFree(Some(self.0 as *const _)) };
        }
    }
}

/// Resolves a path to its item ID list.
fn pidl_for(path: &Path) -> Result<Pidl, String> {
    let wide = HSTRING::from(path.as_os_str());
    let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();

    // SAFETY: `wide` outlives the call; `pidl` receives a fresh allocation that
    // `Pidl` takes ownership of.
    unsafe { SHParseDisplayName(&wide, None, &mut pidl, 0, None) }
        .map_err(|e| format!("the shell does not recognise {}: {e}", path.display()))?;

    if pidl.is_null() {
        return Err(format!("no shell item for {}", path.display()));
    }
    Ok(Pidl(pidl))
}

/// Builds the classic file-drop payload: a `DROPFILES` header, then the paths.
///
/// The paths follow the header back to back, each terminated by a nul, and the
/// list itself terminated by one more — the same double-nul convention the shell
/// has used since Windows 3.1, and the reason a path list cannot be empty.
fn hdrop_medium(paths: &[PathBuf]) -> Result<STGMEDIUM, String> {
    let mut chars: Vec<u16> = Vec::new();
    for path in paths {
        chars.extend(path.as_os_str().encode_wide());
        chars.push(0);
    }
    chars.push(0);

    let header = size_of::<DROPFILES>();
    let bytes = header + chars.len() * 2;

    // SAFETY: a moveable block of a size we computed; ownership passes to the
    // data object below, which frees it with the medium.
    let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }
        .map_err(|e| format!("cannot allocate the drag payload: {e}"))?;

    // SAFETY: `handle` is a live moveable block; the writes below stay inside
    // the `bytes` just allocated, and the lock is released before returning.
    unsafe {
        let base = GlobalLock(handle) as *mut u8;
        if base.is_null() {
            let _ = GlobalFree(Some(handle));
            return Err("cannot lock the drag payload".into());
        }

        let files = base as *mut DROPFILES;
        (*files).pFiles = header as u32;
        (*files).pt = POINT { x: 0, y: 0 };
        (*files).fNC = false.into();
        // Paths are UTF-16. Without this the shell reads them as ANSI and every
        // non-ASCII name in the library arrives at the target as mojibake.
        (*files).fWide = true.into();

        std::ptr::copy_nonoverlapping(
            chars.as_ptr() as *const u8,
            base.add(header),
            chars.len() * 2,
        );
        let _ = GlobalUnlock(handle);
    }

    Ok(STGMEDIUM {
        tymed: TYMED_HGLOBAL.0 as u32,
        u: STGMEDIUM_0 { hGlobal: handle },
        pUnkForRelease: ManuallyDrop::new(None),
    })
}

/// The `FORMATETC` describing that payload.
fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

/// Puts `CF_HDROP` on a shell data object.
///
/// `SHCreateDataObject` with no parent folder hands back an object that offers
/// nothing at all — not `CF_HDROP`, not even `CFSTR_SHELLIDLIST` — so every drop
/// target refuses it and the drag shows the "no" cursor everywhere, including
/// over this application's own panes. It does accept `SetData`, so the format
/// the world actually reads is attached here.
fn attach_hdrop(data: &IDataObject, paths: &[PathBuf]) -> Result<(), String> {
    let medium = hdrop_medium(paths)?;
    // SAFETY: both describe the same HGLOBAL medium. `fRelease` hands the block
    // to the data object, which frees it when it is released.
    unsafe { data.SetData(&hdrop_format(), &medium, true) }
        .map_err(|e| format!("cannot attach the file list to the drag: {e}"))
}

/// What the drop target did with the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    Copied,
    Moved,
    Linked,
    /// The drag was cancelled, or dropped somewhere that wanted nothing.
    Nothing,
}

/// Runs a drag of `paths` from `hwnd` until the user drops or cancels.
///
/// **Blocks** for the whole drag: `DoDragDrop` runs its own modal message loop,
/// exactly as it does inside Explorer. It must therefore be called on the thread
/// that owns the window, and that thread is busy until the drag ends — which is
/// the correct behaviour, not a stall.
///
/// Move is offered alongside copy, so dragging within a drive behaves the way
/// Explorer does rather than always duplicating.
pub fn drag_files(hwnd: isize, paths: &[std::path::PathBuf]) -> Result<Dropped, String> {
    if paths.is_empty() {
        return Ok(Dropped::Nothing);
    }

    let pidls: Vec<Pidl> = paths
        .iter()
        .map(|p| pidl_for(p))
        .collect::<Result<_, _>>()?;
    let raw: Vec<*const ITEMIDLIST> = pidls.iter().map(|p| p.0 as *const _).collect();

    // SAFETY: every pointer in `raw` is a live PIDL owned by `pidls`, which
    // outlives this call. A null folder PIDL means the item lists are absolute.
    let data: IDataObject = unsafe { SHCreateDataObject(None, Some(&raw), None) }
        .map_err(|e| format!("cannot build the drag payload: {e}"))?;

    attach_hdrop(&data, paths)?;

    let allowed = DROPEFFECT_COPY | DROPEFFECT_MOVE | DROPEFFECT_LINK;

    // SAFETY: `hwnd` belongs to this thread's window, and a null drop source
    // asks the shell for its default one.
    let effect = unsafe { SHDoDragDrop(Some(HWND(hwnd as *mut _)), &data, None, allowed) }
        .map_err(|e| format!("the drag failed: {e}"))?;

    Ok(match effect {
        e if e.0 & DROPEFFECT_MOVE.0 != 0 => Dropped::Moved,
        e if e.0 & DROPEFFECT_COPY.0 != 0 => Dropped::Copied,
        e if e.0 & DROPEFFECT_LINK.0 != 0 => Dropped::Linked,
        _ => Dropped::Nothing,
    })
}

impl Dropped {
    /// The wire form the frontend reads.
    pub fn as_str(self) -> &'static str {
        match self {
            Dropped::Copied => "copied",
            Dropped::Moved => "moved",
            Dropped::Linked => "linked",
            Dropped::Nothing => "none",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    /// The payload carries the format drop targets actually read.
    ///
    /// This is the whole bug it was written for: the shell's own data object
    /// offered nothing, so nothing anywhere would accept a dropped track — not
    /// Explorer, not Rekordbox, not the pane next to it. Asserted by reading the
    /// paths back out exactly as a drop target does.
    #[test]
    #[ignore = "needs a real path"]
    fn the_drag_payload_carries_the_files() {
        // SAFETY: a test thread with no apartment of its own yet.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }

        let path = PathBuf::from(
            std::env::var("NOCEUR_DRAG_PATH").expect("set NOCEUR_DRAG_PATH to a real file"),
        );
        let pidl = pidl_for(&path).expect("pidl");
        let raw = [pidl.0 as *const ITEMIDLIST];

        // SAFETY: `raw` holds a live PIDL owned by `pidl`.
        let data: IDataObject =
            unsafe { SHCreateDataObject(None, Some(&raw), None) }.expect("data object");
        attach_hdrop(&data, std::slice::from_ref(&path)).expect("attach");

        // SAFETY: asking for the format just attached.
        let got = unsafe { data.GetData(&hdrop_format()) }.expect("CF_HDROP is missing");

        // SAFETY: the medium is an HGLOBAL holding a DROPFILES, by construction.
        let names = unsafe {
            let base = GlobalLock(got.u.hGlobal) as *const u8;
            assert!(!base.is_null(), "the medium did not lock");
            let files = base as *const DROPFILES;
            assert!((*files).fWide.as_bool(), "paths must be UTF-16");

            let mut chars = base.add((*files).pFiles as usize) as *const u16;
            let mut out: Vec<String> = Vec::new();
            loop {
                let mut one = Vec::new();
                while *chars != 0 {
                    one.push(*chars);
                    chars = chars.add(1);
                }
                chars = chars.add(1);
                if one.is_empty() {
                    break;
                }
                out.push(String::from_utf16_lossy(&one));
            }
            let _ = GlobalUnlock(got.u.hGlobal);
            out
        };

        assert_eq!(names, vec![path.display().to_string()]);
    }
}
