//! The unit of a directory listing.

use crate::kind::EntryKind;
use std::path::PathBuf;

/// One row of a directory listing, straight from the filesystem.
///
/// Deliberately carries no tag or analysis data — that is layered on by
/// `noceur-index`. This struct is what stage 1 of the scan produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    /// Full path to the entry.
    pub path: PathBuf,
    /// File name including extension.
    pub name: String,
    /// Lowercase extension without the dot; empty for directories and
    /// extensionless files.
    pub ext: String,
    /// Size in bytes. Always 0 for directories.
    pub size: u64,
    /// Modification time as seconds since the Unix epoch. 0 if unavailable.
    pub mtime: i64,
    pub kind: EntryKind,
}

impl FsEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}
