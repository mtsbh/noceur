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

use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{SHCreateDataObject, SHDoDragDrop, SHParseDisplayName};

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
