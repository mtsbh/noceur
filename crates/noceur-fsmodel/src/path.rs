//! Path normalisation for long-path-safe filesystem access.

use std::path::{Path, PathBuf};

/// Windows extended-length path prefix.
const EXT_PREFIX: &str = r"\\?\";

/// Returns a path safe to pass to filesystem APIs regardless of length.
///
/// On Windows, absolute paths gain the `\\?\` prefix (`\\?\UNC\` for network
/// shares), which lifts the 260-character `MAX_PATH` limit. Relative paths and
/// already-prefixed paths are returned unchanged. On other platforms this is
/// the identity function.
#[cfg(windows)]
pub fn extended(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if s.starts_with(EXT_PREFIX) {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"{EXT_PREFIX}UNC\{rest}"));
    }
    if path.is_absolute() {
        return PathBuf::from(format!("{EXT_PREFIX}{s}"));
    }
    path.to_path_buf()
}

/// Identity on non-Windows platforms. See the Windows implementation for details.
#[cfg(not(windows))]
pub fn extended(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn prefixes_absolute_drive_path() {
        let p = extended(Path::new(r"C:\Music\track.wav"));
        assert_eq!(p, PathBuf::from(r"\\?\C:\Music\track.wav"));
    }

    #[cfg(windows)]
    #[test]
    fn is_idempotent() {
        let once = extended(Path::new(r"C:\Music"));
        let twice = extended(&once);
        assert_eq!(once, twice);
    }

    #[cfg(windows)]
    #[test]
    fn rewrites_unc_paths() {
        let p = extended(Path::new(r"\\nas\sets\friday"));
        assert_eq!(p, PathBuf::from(r"\\?\UNC\nas\sets\friday"));
    }

    #[cfg(windows)]
    #[test]
    fn leaves_relative_paths_alone() {
        let p = extended(Path::new(r"Music\track.wav"));
        assert_eq!(p, PathBuf::from(r"Music\track.wav"));
    }

    #[cfg(not(windows))]
    #[test]
    fn is_identity_on_unix() {
        let p = extended(Path::new("/music/track.wav"));
        assert_eq!(p, PathBuf::from("/music/track.wav"));
    }

    #[test]
    fn survives_a_very_long_path() {
        let long = "a".repeat(300);
        let input = if cfg!(windows) {
            PathBuf::from(format!(r"C:\{long}"))
        } else {
            PathBuf::from(format!("/{long}"))
        };
        // Must not panic and must preserve the tail.
        assert!(extended(&input).to_string_lossy().ends_with(&long));
    }
}
