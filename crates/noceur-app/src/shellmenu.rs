//! The real Explorer context menu.
//!
//! Not an imitation of it — the shell's own `IContextMenu` for the selected
//! items, which means every entry Explorer would show is here, including the
//! shell extensions the user has installed: their archiver, their tagger, their
//! antivirus, "Open with", "Send to", "Properties".
//!
//! Two details make the difference between this working and looking broken:
//!
//! 1. Extensions populate their submenus in response to `WM_INITMENUPOPUP` and
//!    friends, which they expect the *host* to forward to `IContextMenu2/3`. The
//!    owning window is subclassed for the life of the menu to do exactly that;
//!    without it, "Open with" and "Send to" open empty.
//! 2. `TrackPopupMenuEx` is modal and must run on the thread that owns the
//!    window, so the caller schedules this onto the main thread.

#![cfg(windows)]

use std::path::Path;
use windows::core::{Interface, HSTRING, PCSTR};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    DefSubclassProc, IContextMenu, IContextMenu2, IContextMenu3, ILFindLastID, IShellFolder,
    RemoveWindowSubclass, SHBindToParent, SHCreateDefaultContextMenu, SHParseDisplayName,
    SetWindowSubclass, CMF_NORMAL, CMINVOKECOMMANDINFO, DEFCONTEXTMENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenuEx,
    SW_SHOWNORMAL, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DRAWITEM, WM_INITMENUPOPUP,
    WM_MEASUREITEM, WM_MENUCHAR, WM_MENUSELECT,
};

/// Command identifier range the shell may use inside our popup.
///
/// Starting above zero keeps "the user picked nothing" distinguishable from a
/// real command, which is how `TPM_RETURNCMD` reports a dismissal.
const CMD_FIRST: u32 = 1;
const CMD_LAST: u32 = 0x7FFF;

/// Identifies our subclass on the window. It only has to be unique among the
/// subclasses this process installs on the same window.
const SUBCLASS_ID: usize = 0x4E4F_4345;

thread_local! {
    /// The menu being tracked, for the window procedure to forward messages to.
    ///
    /// A raw pointer in a thread-local rather than a shared handle: a menu is
    /// only ever tracked on the thread owning the window, and only one can be
    /// open at a time because `TrackPopupMenuEx` is modal.
    static ACTIVE: std::cell::Cell<*mut std::ffi::c_void> =
        const { std::cell::Cell::new(std::ptr::null_mut()) };
}

/// Forwards the messages shell extensions need in order to build their submenus.
///
/// # Safety
/// Installed and removed around a single `TrackPopupMenuEx` call, on the thread
/// owning `hwnd`.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if matches!(
        msg,
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR | WM_MENUSELECT
    ) {
        let active = ACTIVE.with(|a| a.get());
        if !active.is_null() {
            // SAFETY: a borrowed pointer to the menu currently being tracked; it
            // is cleared before that interface is released.
            if let Some(menu) = unsafe { IContextMenu::from_raw_borrowed(&active) } {
                // IContextMenu3 first: it is the only one that can answer
                // WM_MENUCHAR, and extensions implementing it expect to be asked.
                if let Ok(three) = menu.cast::<IContextMenu3>() {
                    let mut result = LRESULT(0);
                    // SAFETY: the message belongs to the menu being tracked.
                    if unsafe { three.HandleMenuMsg2(msg, wparam, lparam, Some(&mut result)) }
                        .is_ok()
                    {
                        return result;
                    }
                } else if let Ok(two) = menu.cast::<IContextMenu2>() {
                    // SAFETY: as above.
                    if unsafe { two.HandleMenuMsg(msg, wparam, lparam) }.is_ok() {
                        return LRESULT(0);
                    }
                }
            }
        }
    }

    // SAFETY: forwards to the next procedure in the subclass chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// An item ID list that frees itself.
struct Pidl(*mut ITEMIDLIST);

impl Drop for Pidl {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: allocated by SHParseDisplayName with the task allocator.
            unsafe { CoTaskMemFree(Some(self.0 as *const _)) };
        }
    }
}

fn pidl_for(path: &Path) -> Result<Pidl, String> {
    let wide = HSTRING::from(path.as_os_str());
    let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();

    // SAFETY: `wide` outlives the call; the allocation is taken over by `Pidl`.
    unsafe { SHParseDisplayName(&wide, None, &mut pidl, 0, None) }
        .map_err(|e| format!("the shell does not recognise {}: {e}", path.display()))?;

    if pidl.is_null() {
        return Err(format!("no shell item for {}", path.display()));
    }
    Ok(Pidl(pidl))
}

/// Shows the shell's context menu for `paths` at the cursor, and runs whatever
/// the user chooses.
///
/// Every path must live in the same folder: the shell builds one menu for one
/// parent, which is also the only case a single folder's listing can produce.
pub fn show(hwnd: isize, paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let hwnd = HWND(hwnd as *mut _);
    let pidls: Vec<Pidl> = paths
        .iter()
        .map(|p| pidl_for(p))
        .collect::<Result<_, _>>()?;

    // The shell wants the parent folder plus item IDs relative to it, which is
    // what SHBindToParent yields from an absolute one.
    let mut parent: Option<IShellFolder> = None;
    let mut children: Vec<*mut ITEMIDLIST> = Vec::with_capacity(pidls.len());

    for pidl in &pidls {
        if parent.is_none() {
            // SAFETY: `pidl` is a live absolute item list.
            parent = Some(
                unsafe { SHBindToParent(pidl.0, None) }
                    .map_err(|e| format!("cannot reach the parent folder: {e}"))?,
            );
        }
        // SAFETY: borrows the last ID from `pidl`, which outlives the menu; it
        // is not a separate allocation and must not be freed.
        children.push(unsafe { ILFindLastID(pidl.0) });
    }

    let parent = parent.ok_or_else(|| "cannot reach the parent folder".to_string())?;

    let request = DEFCONTEXTMENU {
        hwnd,
        pcmcb: std::mem::ManuallyDrop::new(None),
        pidlFolder: std::ptr::null_mut(),
        psf: std::mem::ManuallyDrop::new(Some(parent)),
        cidl: children.len() as u32,
        apidl: children.as_mut_ptr(),
        punkAssociationInfo: std::mem::ManuallyDrop::new(None),
        cKeys: 0,
        aKeys: std::ptr::null(),
    };

    // SAFETY: `request` and everything it points at outlive the call.
    let menu: IContextMenu = unsafe { SHCreateDefaultContextMenu(&request) }
        .map_err(|e| format!("cannot build the shell menu: {e}"))?;

    // SAFETY: destroyed below on every path out of this function.
    let popup = unsafe { CreatePopupMenu() }.map_err(|e| format!("cannot create a menu: {e}"))?;

    let outcome = track(hwnd, &menu, popup);

    // SAFETY: `popup` was created above and is not used afterwards.
    unsafe {
        let _ = DestroyMenu(popup);
    }

    outcome
}

/// Fills the popup, tracks it, and invokes whatever was chosen.
fn track(
    hwnd: HWND,
    menu: &IContextMenu,
    popup: windows::Win32::UI::WindowsAndMessaging::HMENU,
) -> Result<(), String> {
    // SAFETY: `popup` is a live, empty menu owned by the caller.
    unsafe { menu.QueryContextMenu(popup, 0, CMD_FIRST, CMD_LAST, CMF_NORMAL) }
        .ok()
        .map_err(|e| format!("the shell declined to fill the menu: {e}"))?;

    let mut at = POINT::default();
    // SAFETY: `at` is a valid destination.
    unsafe { GetCursorPos(&mut at) }.map_err(|e| format!("cannot find the cursor: {e}"))?;

    ACTIVE.with(|a| a.set(menu.as_raw()));
    // SAFETY: removed below before the menu interface is released.
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
    }

    // Without foreground, a popup tracked from a window that is not active
    // dismisses itself the moment it opens.
    // SAFETY: `hwnd` is this app's own window.
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }

    // SAFETY: modal until the user chooses or dismisses.
    let chosen = unsafe {
        TrackPopupMenuEx(
            popup,
            (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_RIGHTBUTTON).0,
            at.x,
            at.y,
            hwnd,
            None,
        )
    };

    // SAFETY: matches the SetWindowSubclass above.
    unsafe {
        let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
    }
    ACTIVE.with(|a| a.set(std::ptr::null_mut()));

    if chosen.0 == 0 {
        // Dismissed without choosing. Not an error.
        return Ok(());
    }

    // Verbs are identified by offset from the base we passed to QueryContextMenu,
    // packed into the pointer field — an ancient convention the shell still uses.
    let verb = (chosen.0 as u32) - CMD_FIRST;
    let invoke = CMINVOKECOMMANDINFO {
        cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
        fMask: 0,
        hwnd,
        lpVerb: PCSTR(verb as usize as *const u8),
        lpParameters: PCSTR::null(),
        lpDirectory: PCSTR::null(),
        nShow: SW_SHOWNORMAL.0,
        dwHotKey: 0,
        hIcon: HANDLE(std::ptr::null_mut()),
    };

    // SAFETY: `invoke` names a command the shell itself just put in the menu.
    unsafe { menu.InvokeCommand(&invoke) }.map_err(|e| format!("the command failed: {e}"))
}
