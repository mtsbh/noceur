//! Reading a directory into `FsEntry` rows.

use crate::entry::FsEntry;
use crate::kind::{extension_of, EntryKind};
use crate::path::extended;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, thiserror::Error)]
pub enum EnumerateError {
    #[error("cannot read directory {path}: {source}")]
    ReadDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// One entry that could not be read during enumeration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedEntry {
    /// File name if it was known before the failure, otherwise empty.
    pub name: String,
    /// Human-readable reason, for display and logs.
    pub reason: String,
}

/// The result of reading one directory.
///
/// Entries that could not be stat'd are recorded in `skipped` rather than
/// failing the listing — a single locked or vanished file must never cost the
/// user the folder. Callers that do not care can ignore `skipped`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirListing {
    pub entries: Vec<FsEntry>,
    pub skipped: Vec<SkippedEntry>,
}

/// Reads one directory into `FsEntry` rows.
///
/// Individual entries that cannot be stat'd are skipped rather than failing the
/// whole listing — a single locked or vanished file must never cost you the
/// folder. Only failure to open the directory itself is an error. Skipped
/// entries are recorded in the returned `DirListing::skipped` so the caller
/// can surface an incomplete-listing warning (e.g. on an SMB share).
pub fn read_dir(dir: &Path) -> Result<DirListing, EnumerateError> {
    let iter = std::fs::read_dir(extended(dir)).map_err(|source| EnumerateError::ReadDir {
        path: dir.display().to_string(),
        source,
    })?;

    let mut out = DirListing::default();
    for item in iter {
        let entry = match item {
            Ok(entry) => entry,
            Err(err) => {
                out.skipped.push(SkippedEntry {
                    name: String::new(),
                    reason: err.to_string(),
                });
                continue;
            }
        };

        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(err) => {
                out.skipped.push(SkippedEntry {
                    name,
                    reason: err.to_string(),
                });
                continue;
            }
        };
        let is_dir = meta.is_dir();

        let ext = if is_dir {
            String::new()
        } else {
            extension_of(&name)
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        out.entries.push(FsEntry {
            kind: EntryKind::classify(Path::new(&name), is_dir),
            path: dir.join(&name),
            name,
            ext,
            size: if is_dir { 0 } else { meta.len() },
            mtime,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Unsorted")).unwrap();
        fs::write(d.path().join("track.wav"), b"0123456789").unwrap();
        fs::write(d.path().join("cover.jpg"), b"xx").unwrap();
        d
    }

    #[test]
    fn reads_every_entry() {
        let d = fixture();
        let listing = read_dir(d.path()).unwrap();
        assert_eq!(listing.entries.len(), 3);
    }

    #[test]
    fn populates_name_ext_and_kind() {
        let d = fixture();
        let listing = read_dir(d.path()).unwrap();
        let entries = listing.entries;

        let track = entries.iter().find(|e| e.name == "track.wav").unwrap();
        assert_eq!(track.ext, "wav");
        assert_eq!(track.kind, EntryKind::Audio);
        assert_eq!(track.size, 10);

        let dir = entries.iter().find(|e| e.name == "Unsorted").unwrap();
        assert_eq!(dir.ext, "");
        assert_eq!(dir.kind, EntryKind::Directory);
        assert_eq!(dir.size, 0);

        let cover = entries.iter().find(|e| e.name == "cover.jpg").unwrap();
        assert_eq!(cover.kind, EntryKind::Other);
    }

    #[test]
    fn lowercases_the_extension() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("LOUD.WAV"), b"x").unwrap();
        let listing = read_dir(d.path()).unwrap();
        assert_eq!(listing.entries[0].ext, "wav");
        assert_eq!(
            listing.entries[0].name, "LOUD.WAV",
            "name keeps original case"
        );
    }

    #[test]
    fn records_a_plausible_mtime() {
        let d = fixture();
        let listing = read_dir(d.path()).unwrap();
        // Any file written during this test is after 2020-01-01.
        assert!(listing.entries.iter().all(|e| e.mtime > 1_577_836_800));
    }

    #[test]
    fn errors_on_a_missing_directory() {
        let err = read_dir(Path::new("./definitely-not-here-9f2a")).unwrap_err();
        assert!(matches!(err, EnumerateError::ReadDir { .. }));
    }

    #[test]
    fn a_normal_directory_produces_no_skipped_entries() {
        let d = fixture();
        let listing = read_dir(d.path()).unwrap();
        assert!(listing.skipped.is_empty());
    }

    #[test]
    fn dir_listing_default_is_empty_in_both_fields() {
        let listing = DirListing::default();
        assert!(listing.entries.is_empty());
        assert!(listing.skipped.is_empty());
    }
}
