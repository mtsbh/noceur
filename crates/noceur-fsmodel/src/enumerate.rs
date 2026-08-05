//! Reading a directory into `FsEntry` rows.

use crate::entry::FsEntry;
use crate::kind::EntryKind;
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

/// Reads one directory into `FsEntry` rows.
///
/// Individual entries that cannot be stat'd are skipped rather than failing the
/// whole listing — a single locked or vanished file must never cost you the
/// folder. Only failure to open the directory itself is an error.
pub fn read_dir(dir: &Path) -> Result<Vec<FsEntry>, EnumerateError> {
    let iter = std::fs::read_dir(extended(dir)).map_err(|source| EnumerateError::ReadDir {
        path: dir.display().to_string(),
        source,
    })?;

    let mut out = Vec::new();
    for entry in iter.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = meta.is_dir();

        let ext = if is_dir {
            String::new()
        } else {
            Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default()
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        out.push(FsEntry {
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
        let entries = read_dir(d.path()).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn populates_name_ext_and_kind() {
        let d = fixture();
        let entries = read_dir(d.path()).unwrap();

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
        let entries = read_dir(d.path()).unwrap();
        assert_eq!(entries[0].ext, "wav");
        assert_eq!(entries[0].name, "LOUD.WAV", "name keeps original case");
    }

    #[test]
    fn records_a_plausible_mtime() {
        let d = fixture();
        let entries = read_dir(d.path()).unwrap();
        // Any file written during this test is after 2020-01-01.
        assert!(entries.iter().all(|e| e.mtime > 1_577_836_800));
    }

    #[test]
    fn errors_on_a_missing_directory() {
        let err = read_dir(Path::new("./definitely-not-here-9f2a")).unwrap_err();
        assert!(matches!(err, EnumerateError::ReadDir { .. }));
    }
}
