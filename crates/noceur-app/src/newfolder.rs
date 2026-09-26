//! Making a folder, from the tree or a listing.
//!
//! Explorer's rules, because they are the ones people already know: a name is
//! checked for the characters Windows refuses before anything touches the disk,
//! and when no name is given the folder is "New folder", then "New folder (2)",
//! and so on, rather than failing because the first one exists.

use std::path::{Path, PathBuf};

/// Characters no Windows file name may contain.
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Names Windows reserves for devices, with or without an extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

const DEFAULT_NAME: &str = "New folder";

/// Why a name cannot be used, or `None` when it can.
pub fn check_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("A folder needs a name".into());
    }
    if name == "." || name == ".." {
        return Some("That name is taken by the system".into());
    }
    if let Some(c) = name
        .chars()
        .find(|c| FORBIDDEN.contains(c) || c.is_control())
    {
        return Some(format!("A name cannot contain {c}"));
    }
    // Windows strips these silently, so "Mix." would become "Mix" and the
    // folder the user asked for would not be the one that exists.
    if name.ends_with('.') || name.ends_with(' ') {
        return Some("A name cannot end with a dot or a space".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Some(format!("{name} is reserved by Windows"));
    }
    None
}

/// Creates a folder in `parent` and returns its path.
///
/// An empty name means "New folder", numbered past any that exist. A name
/// given explicitly that already exists is an error rather than silently
/// numbered: asking for "Techno" and getting "Techno (2)" would be a surprise.
pub fn create(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if !parent.is_dir() {
        return Err(format!("{} is not a folder", parent.display()));
    }

    let target = if name.is_empty() {
        (1..)
            .map(|n| {
                parent.join(if n == 1 {
                    DEFAULT_NAME.to_string()
                } else {
                    format!("{DEFAULT_NAME} ({n})")
                })
            })
            .find(|p| !p.exists())
            .expect("an unbounded range always finds a free name")
    } else {
        if let Some(why) = check_name(name) {
            return Err(why);
        }
        let p = parent.join(name);
        if p.exists() {
            return Err(format!("{name} already exists here"));
        }
        p
    };

    std::fs::create_dir(noceur_fsmodel::path::extended(&target))
        .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
    Ok(target)
}

/// Makes a folder and returns its full path.
#[tauri::command]
pub async fn create_folder(parent: String, name: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        create(Path::new(&parent), &name).map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| format!("folder task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_name_numbers_past_existing_folders() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = create(dir.path(), "").unwrap();
        let b = create(dir.path(), "  ").unwrap();
        assert_eq!(a.file_name().unwrap(), "New folder");
        assert_eq!(b.file_name().unwrap(), "New folder (2)");
        assert!(b.is_dir());
    }

    #[test]
    fn a_named_folder_is_made_and_a_duplicate_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let made = create(dir.path(), "Techno").unwrap();
        assert!(made.is_dir());
        assert!(create(dir.path(), "Techno").is_err());
    }

    #[test]
    fn names_windows_refuses_are_refused_here() {
        for bad in [
            "a/b", "a\\b", "what?", "x:y", "Mix.", "Mix ", "CON", "nul.txt", "..",
        ] {
            assert!(check_name(bad).is_some(), "{bad} must be refused");
        }
        for good in ["Techno", "2024-06 Promos", "Mix (Extended).v2", "Ünïcødé"] {
            assert!(check_name(good).is_none(), "{good} is fine");
        }
    }

    #[test]
    fn a_parent_that_is_not_a_folder_is_an_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("f.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(create(&file, "sub").is_err());
    }
}
