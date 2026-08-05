//! Provisional content identity from file size plus head and tail bytes.

use std::path::Path;

/// Bytes read from each end of the file.
pub const WINDOW: usize = 64 * 1024;

use noceur_fsmodel::path::extended;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use xxhash_rust::xxh3::Xxh3;

/// Computes the quick key for a file: `"{size}-{hash}"`.
///
/// Reads at most `2 * WINDOW` bytes regardless of file size, so this is safe to
/// run across a whole library in the background. Two different files sharing a
/// quick key is possible but vanishingly unlikely; the full audio hash added in
/// a later milestone is what makes identity authoritative.
pub fn compute(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(extended(path))?;
    let size = f.metadata()?.len();

    let mut hasher = Xxh3::new();
    hasher.update(&size.to_le_bytes());

    let window = WINDOW.min(size as usize);
    let mut buf = vec![0u8; window];

    if window > 0 {
        f.read_exact(&mut buf)?;
        hasher.update(&buf);

        if size > window as u64 {
            f.seek(SeekFrom::End(-(window as i64)))?;
            f.read_exact(&mut buf)?;
            hasher.update(&buf);
        }
    }

    Ok(format!("{size}-{:016x}", hasher.digest()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn identical_files_share_a_key() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"the same bytes exactly");
        let b = write(&d, "b.wav", b"the same bytes exactly");
        assert_eq!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn different_content_gives_different_keys() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"first content here");
        let b = write(&d, "b.wav", b"other content here");
        assert_ne!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn same_length_different_bytes_still_differ() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"AAAAAAAAAA");
        let b = write(&d, "b.wav", b"BBBBBBBBBB");
        assert_ne!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn is_deterministic_across_calls() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"stable");
        assert_eq!(compute(&a).unwrap(), compute(&a).unwrap());
    }

    #[test]
    fn handles_files_larger_than_two_windows() {
        let d = TempDir::new().unwrap();
        let mut big = vec![7u8; WINDOW * 3];
        big[WINDOW * 3 - 1] = 42; // differs only in the tail
        let a = write(&d, "a.wav", &vec![7u8; WINDOW * 3]);
        let b = write(&d, "b.wav", &big);
        assert_ne!(
            compute(&a).unwrap(),
            compute(&b).unwrap(),
            "tail must be covered"
        );
    }

    #[test]
    fn handles_empty_files() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"");
        assert!(compute(&a).is_ok());
    }

    #[test]
    fn key_encodes_the_size() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"1234567890");
        assert!(compute(&a).unwrap().starts_with("10-"));
    }

    #[test]
    fn errors_on_a_missing_file() {
        assert!(compute(Path::new("./nope-4c1a.wav")).is_err());
    }
}
