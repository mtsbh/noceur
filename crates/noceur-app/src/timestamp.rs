//! Keeping a file's modified date across an edit.
//!
//! Writing a tag rewrites the file, and the filesystem stamps it with the time
//! it was rewritten. For most software that is exactly right. For a music
//! library it is destructive in a quiet way: "date modified" is what a DJ sorts
//! by to find what arrived last week, and retagging a folder of two hundred
//! tracks moves every one of them to the top of that sort at once. The
//! information is not recoverable afterwards.
//!
//! So the stamp is read before a write and put back after it, when the user has
//! asked for that. Both times are restored, not only the modified one: leaving
//! the access time moved would be half a job, and some tools sort by it.

use std::path::Path;
use std::time::SystemTime;

/// A file's times, held across a write.
#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    modified: SystemTime,
    accessed: Option<SystemTime>,
}

impl Stamp {
    /// Reads a file's times, if they can be read.
    ///
    /// `None` when they cannot, which is not worth failing an edit over — the
    /// tag write is the thing the user asked for, and a filesystem that will not
    /// report a timestamp will not accept one either.
    pub fn of(path: &Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Stamp {
            modified: meta.modified().ok()?,
            accessed: meta.accessed().ok(),
        })
    }

    /// Puts the times back.
    ///
    /// Failure is ignored on purpose. This runs after a successful write, and
    /// reporting "the tags were saved but the date moved" as a failure would
    /// tell the user their edit did not happen when it did.
    pub fn restore(&self, path: &Path) {
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) else {
            return;
        };
        let times = std::fs::FileTimes::new()
            .set_modified(self.modified)
            .set_accessed(self.accessed.unwrap_or(self.modified));
        let _ = file.set_times(times);
    }
}

/// Runs a write, optionally putting the file's date back afterwards.
///
/// Reads the stamp first whatever the answer, because by the time the closure
/// has run the original is gone.
pub fn preserving<T>(path: &Path, keep: bool, write: impl FnOnce() -> T) -> T {
    let before = if keep { Stamp::of(path) } else { None };
    let result = write();
    if let Some(stamp) = before {
        stamp.restore(path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    fn a_file() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.mp3");
        std::fs::write(&path, b"original").unwrap();

        // Backdated, so "unchanged" is distinguishable from "set to now".
        let old = SystemTime::now() - Duration::from_secs(60 * 60 * 24 * 30);
        let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.set_times(
            std::fs::FileTimes::new()
                .set_modified(old)
                .set_accessed(old),
        )
        .unwrap();

        (dir, path)
    }

    fn modified(path: &Path) -> SystemTime {
        std::fs::metadata(path).unwrap().modified().unwrap()
    }

    fn rewrite(path: &Path) {
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(b"rewritten with new tags").unwrap();
    }

    /// The point of the whole module: retagging a folder must not move every
    /// file in it to the top of a sort by date.
    #[test]
    fn a_preserved_write_leaves_the_date_alone() {
        let (_dir, path) = a_file();
        let before = modified(&path);

        preserving(&path, true, || rewrite(&path));

        assert_eq!(modified(&path), before);
        assert_eq!(std::fs::read(&path).unwrap(), b"rewritten with new tags");
    }

    #[test]
    fn without_the_option_the_date_moves_as_it_normally_would() {
        let (_dir, path) = a_file();
        let before = modified(&path);

        preserving(&path, false, || rewrite(&path));

        assert!(
            modified(&path) > before,
            "the filesystem should have stamped it"
        );
    }

    /// A file that has gone, or one whose times cannot be read, must not turn
    /// into a failed edit.
    #[test]
    fn a_missing_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.mp3");
        assert!(Stamp::of(&path).is_none());

        // And restoring against nothing is silent rather than a panic.
        let (_d2, real) = a_file();
        Stamp::of(&real).unwrap().restore(&path);
    }

    #[test]
    fn the_value_of_the_write_is_handed_back() {
        let (_dir, path) = a_file();
        let answer = preserving(&path, true, || 42);
        assert_eq!(answer, 42);
    }
}
