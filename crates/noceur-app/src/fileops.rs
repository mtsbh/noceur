//! Copying, moving and deleting through the shell.
//!
//! `IFileOperation` is what Explorer itself uses, and using it means the whole
//! apparatus comes free and behaves the way people already expect: the progress
//! dialog, the "a file with this name already exists" prompt, elevation when a
//! destination needs it, deletion to the recycle bin, and — the reason it is
//! worth insisting on — the system undo stack, so a mistaken move is Ctrl+Z in
//! Explorer rather than gone.
//!
//! Reimplementing any of that with `std::fs` would be worse in every one of
//! those respects, and silently so.

#![cfg(windows)]

use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FileOperation, IFileOperation, IShellItem, SHCreateItemFromParsingName, FOF_ALLOWUNDO,
    FOF_NOCONFIRMMKDIR,
};

/// What to do with the files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Copy,
    Move,
    /// To the recycle bin, not oblivion — `FOF_ALLOWUNDO` is what makes the
    /// difference, and it is why this never calls `remove_file`.
    Delete,
}

impl Operation {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "copy" => Some(Operation::Copy),
            "move" => Some(Operation::Move),
            "delete" => Some(Operation::Delete),
            _ => None,
        }
    }
}

/// Initialises COM for the calling thread and uninitialises it on the way out.
///
/// The shell's file operation needs an apartment, and a thread from the blocking
/// pool has none. Balancing it here keeps the pool's threads as they were found.
struct Apartment(bool);

impl Apartment {
    fn enter() -> Apartment {
        // SAFETY: paired with CoUninitialize in Drop. A thread already in an
        // apartment returns RPC_E_CHANGED_MODE or S_FALSE; either way it is not
        // ours to uninitialise, which is what the flag records.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Apartment(hr.is_ok())
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the CoInitializeEx that succeeded above.
            unsafe { CoUninitialize() };
        }
    }
}

fn shell_item(path: &Path) -> Result<IShellItem, String> {
    let wide = HSTRING::from(path.as_os_str());
    // SAFETY: `wide` outlives the call and the returned interface is owned.
    unsafe { SHCreateItemFromParsingName(&wide, None) }
        .map_err(|e| format!("the shell does not recognise {}: {e}", path.display()))
}

/// Runs one shell file operation over `paths`.
///
/// `hwnd` owns any dialog the shell decides to show, so a progress or conflict
/// window is modal to this app rather than orphaned behind it.
///
/// Blocks until the operation finishes, which is why the caller runs it off the
/// UI thread. A user who cancels the shell's dialog is not an error: nothing
/// happened, which is what they asked for.
pub fn run(
    hwnd: isize,
    op: Operation,
    paths: &[std::path::PathBuf],
    destination: Option<&Path>,
) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }

    let _apartment = Apartment::enter();

    // SAFETY: every call below is on interfaces created here and released by
    // their Drop impls; the operation is not shared across threads.
    unsafe {
        let operation: IFileOperation = CoCreateInstance(&FileOperation, None, CLSCTX_ALL)
            .map_err(|e| format!("cannot start a file operation: {e}"))?;

        operation
            .SetOwnerWindow(HWND(hwnd as *mut _))
            .map_err(|e| format!("cannot parent the file dialog: {e}"))?;

        // ALLOWUNDO is the whole point: it puts the operation on the system undo
        // stack and sends deletions to the recycle bin.
        operation
            .SetOperationFlags(FOF_ALLOWUNDO | FOF_NOCONFIRMMKDIR)
            .map_err(|e| format!("cannot configure the file operation: {e}"))?;

        let dest = match destination {
            Some(d) => Some(shell_item(d)?),
            None => None,
        };

        for path in paths {
            let item = shell_item(path)?;
            let queued = match (op, &dest) {
                (Operation::Copy, Some(d)) => operation.CopyItem(&item, d, None, None),
                (Operation::Move, Some(d)) => operation.MoveItem(&item, d, None, None),
                (Operation::Delete, _) => operation.DeleteItem(&item, None),
                // Copy and move without somewhere to put them is a caller bug.
                (_, None) => {
                    return Err("copy and move need a destination folder".into());
                }
            };
            queued.map_err(|e| format!("cannot queue {}: {e}", path.display()))?;
        }

        operation
            .PerformOperations()
            .map_err(|e| format!("the file operation failed: {e}"))?;
    }

    Ok(())
}

/// Whether two paths sit on the same volume.
///
/// Explorer moves within a volume and copies across one, and a drop that does
/// something else than Explorer would is a drop that surprises the user. Compared
/// by path prefix rather than by volume id: this decides a default the shell will
/// confirm anyway, not something irreversible.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    fn root(p: &Path) -> Option<String> {
        p.components().next().map(|c| {
            c.as_os_str()
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase()
        })
    }
    match (root(a), root(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn operations_round_trip_through_their_wire_names() {
        for (name, op) in [
            ("copy", Operation::Copy),
            ("move", Operation::Move),
            ("delete", Operation::Delete),
        ] {
            assert_eq!(Operation::parse(name), Some(op));
        }
        assert_eq!(Operation::parse("obliterate"), None);
    }

    #[test]
    fn paths_on_one_drive_are_the_same_volume() {
        assert!(same_volume(
            &PathBuf::from(r"E:\music\a.wav"),
            &PathBuf::from(r"E:\other")
        ));
    }

    /// Case differs constantly between what the shell reports and what is typed.
    #[test]
    fn volume_comparison_ignores_case() {
        assert!(same_volume(
            &PathBuf::from(r"e:\music"),
            &PathBuf::from(r"E:\music")
        ));
    }

    #[test]
    fn different_drives_are_different_volumes() {
        assert!(!same_volume(
            &PathBuf::from(r"E:\music"),
            &PathBuf::from(r"Z:\share")
        ));
    }
}
