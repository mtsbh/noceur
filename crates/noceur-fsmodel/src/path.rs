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
///
/// # Precondition: the input must already be normalised
///
/// Verbatim `\\?\` paths bypass the OS's usual path normalisation. `.` and
/// `..` segments, forward slashes, and trailing dots or spaces are NOT
/// resolved once the prefix is present, and will fail to open even where the
/// unprefixed path would have worked. Callers must pass an already-normalised,
/// fully-qualified path — do not feed this function a path fragment that still
/// contains `.`/`..` components, `/` separators, or a trailing dot/space.
#[cfg(windows)]
pub fn extended(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if s.starts_with(EXT_PREFIX) {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        // Assumes a drive- or UNC-share input (`\\server\share\...`). Windows
        // local-device paths such as `\\.\pipe\...` or `\\.\COM1` also start
        // with `\\` but are not UNC shares; rewriting them to `\\?\UNC\...`
        // would be wrong. We don't expect device paths here since this crate
        // only ever deals in filesystem directory/file paths, but a caller
        // that passes one would get a mis-rewritten result.
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

/// Windows UNC extended prefix.
const EXT_UNC_PREFIX: &str = r"\\?\UNC\";

/// Removes the `\\?\` extended-length prefix for display.
///
/// The inverse of [`extended`]. `std::fs::canonicalize` returns verbatim paths on
/// Windows, so this must be applied at every boundary where a path becomes text a
/// person reads — breadcrumbs, error messages, tooltips. Never apply it before a
/// filesystem call; that is what [`extended`] is for.
#[cfg(windows)]
pub fn simplify(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if let Some(rest) = s.strip_prefix(EXT_UNC_PREFIX) {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(EXT_PREFIX) {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// Identity on non-Windows platforms. See the Windows implementation for details.
#[cfg(not(windows))]
pub fn simplify(path: &Path) -> PathBuf {
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

    #[cfg(windows)]
    #[test]
    fn simplify_strips_the_drive_prefix() {
        assert_eq!(
            simplify(Path::new(r"\\?\C:\Music\track.wav")),
            PathBuf::from(r"C:\Music\track.wav")
        );
    }

    #[cfg(windows)]
    #[test]
    fn simplify_strips_the_unc_prefix() {
        assert_eq!(
            simplify(Path::new(r"\\?\UNC\nas\sets\friday")),
            PathBuf::from(r"\\nas\sets\friday")
        );
    }

    #[cfg(windows)]
    #[test]
    fn simplify_leaves_ordinary_paths_alone() {
        assert_eq!(simplify(Path::new(r"C:\Music")), PathBuf::from(r"C:\Music"));
    }

    #[test]
    fn simplify_round_trips_with_extended() {
        let original = if cfg!(windows) {
            PathBuf::from(r"C:\Music\House")
        } else {
            PathBuf::from("/music/house")
        };
        assert_eq!(simplify(&extended(&original)), original);
    }

    #[test]
    fn simplify_is_idempotent() {
        let p = if cfg!(windows) {
            PathBuf::from(r"C:\Music")
        } else {
            PathBuf::from("/music")
        };
        assert_eq!(simplify(&simplify(&p)), simplify(&p));
    }
}
