//! Ctrl+Z for renames, inside the app.
//!
//! Every rename goes through [`crate::renamer::rename_in_shell`], which notes
//! where the file went. A command that renames — the renamer, a button, a
//! cleanup, an edit in the list — seals what it did into one step when it
//! finishes, so one Ctrl+Z puts back one whole job rather than one file of it.
//!
//! Kept in memory only. An undo that survived a restart would be undoing onto
//! a disk that may have changed in between, and Explorer's own undo still
//! covers that case.

use std::path::PathBuf;
use std::sync::Mutex;

/// How many jobs can be undone.
const DEPTH: usize = 20;

/// One rename: where the file is now, and where it was.
type Move = (PathBuf, PathBuf);

/// Renames done by the command still running, not yet a step.
static PENDING: Mutex<Vec<Move>> = Mutex::new(Vec::new());
/// Finished jobs, newest last.
static STACK: Mutex<Vec<Vec<Move>>> = Mutex::new(Vec::new());

/// Notes one successful rename.
pub(crate) fn record(from: PathBuf, to: PathBuf) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push((to, from));
    }
}

/// Closes the running job into one undo step, if it renamed anything.
pub(crate) fn seal() {
    let Ok(mut pending) = PENDING.lock() else {
        return;
    };
    if pending.is_empty() {
        return;
    }
    let step = std::mem::take(&mut *pending);
    if let Ok(mut stack) = STACK.lock() {
        stack.push(step);
        if stack.len() > DEPTH {
            stack.remove(0);
        }
    }
}

/// What an undo put back.
#[derive(serde::Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Undone {
    /// Files renamed back. Zero when there was nothing to undo.
    pub restored: usize,
    /// Files that could not be: moved again since, or their old name taken.
    pub failed: usize,
}

/// Puts the last rename job back.
#[tauri::command]
pub async fn undo_rename() -> Result<Undone, String> {
    tauri::async_runtime::spawn_blocking(|| {
        // The job that just ran is still pending until something seals it.
        seal();
        let step = STACK.lock().map_err(|_| "undo lock poisoned")?.pop();
        let Some(step) = step else {
            return Ok(Undone::default());
        };

        let mut done = Undone::default();
        // Backwards, so a job that renamed a into b and then b into c unwinds
        // in the order it was wound.
        for (now, was) in step.into_iter().rev() {
            if !now.exists() || was.exists() {
                done.failed += 1;
                continue;
            }
            match rename_back(&now, &was) {
                Ok(()) => done.restored += 1,
                Err(_) => done.failed += 1,
            }
        }
        Ok(done)
    })
    .await
    .map_err(|e| format!("undo task failed: {e}"))?
}

/// Renames without recording: undoing must not itself become the next undo.
#[cfg(windows)]
fn rename_back(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    crate::fileops::rename(from, to)
}

#[cfg(not(windows))]
fn rename_back(from: &std::path::Path, to: &std::path::Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_is_one_step_and_nothing_seals_to_nothing() {
        seal();
        let before = STACK.lock().unwrap().len();
        record("a".into(), "b".into());
        record("c".into(), "d".into());
        seal();
        seal();
        let stack = STACK.lock().unwrap();
        assert_eq!(stack.len(), before + 1);
        assert_eq!(stack.last().unwrap().len(), 2);
        assert_eq!(stack.last().unwrap()[0], ("b".into(), "a".into()));
    }
}
