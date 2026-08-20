//! The unit of a directory listing.

use crate::kind::EntryKind;
use std::path::PathBuf;

/// One row of a directory listing, straight from the filesystem.
///
/// Deliberately carries no tag or analysis data — that is layered on by
/// `noceur-index`. This struct is what stage 1 of the scan produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    /// Full path to the entry: the caller's original directory path joined
    /// with this entry's file name.
    ///
    /// This is NOT the `\\?\`-extended form. Any filesystem I/O against this
    /// path (opening it to read tags, for example) must first pass it through
    /// [`crate::path::extended`] — the long-path fix is applied at the point
    /// of use, not stored here, so a downstream crate that opens this path
    /// directly loses long-path protection and can fail on deeply nested
    /// library trees.
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
    /// Whether the shell would keep this out of sight: hidden or system on
    /// Windows, a leading dot elsewhere.
    ///
    /// Carried here rather than decided by name downstream, because the names
    /// worth hiding — `$RECYCLE.BIN`, `System Volume Information` — are localised
    /// and per-drive, while the attribute is neither.
    pub hidden: bool,
}

impl FsEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}
