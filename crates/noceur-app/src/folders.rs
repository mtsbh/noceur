//! Filing into folders that already exist under a slightly different spelling.
//!
//! A library filed by hand is full of `anamorph & co` beside a tag reading
//! `Anamorph & Co.`, and `Dj Koze` beside `DJ Koze`. Making a second folder for
//! each of those is worse than not filing at all, so a pattern's folders are
//! first looked for among the ones that are there, compared on letters and
//! digits only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What a folder name is compared on: lower-case letters and digits, with `&`
/// read as `and`. `Anamorph & Co.`, `anamorph and co` and `ANAMORPH&CO` are one.
pub fn key(name: &str) -> String {
    name.replace('&', "and")
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric())
        .collect()
}

/// The first artist of a credit: `Alineat, Anrra` and `Alineat feat. Anrra`
/// both start with `Alineat`. The whole credit is tried before this, so a
/// folder for a duo named with an ampersand still wins.
pub fn lead(credit: &str) -> &str {
    let lower = credit.to_lowercase();
    let cut = [
        ",", ";", " & ", " and ", " feat", " ft.", " ft ", " x ", " vs", " with ",
    ]
    .iter()
    .filter_map(|sep| lower.find(sep))
    .min()
    .unwrap_or(credit.len());
    // Lower-casing can change byte lengths; only cut where the original agrees.
    if credit.is_char_boundary(cut) {
        credit[..cut].trim()
    } else {
        credit.trim()
    }
}

/// The first entry of a `,` or `;` list, or the whole text.
fn first_of_list(text: &str) -> &str {
    text.split([',', ';']).next().unwrap_or(text).trim()
}

/// Where a pattern's path lands once its folders are matched to existing ones.
#[derive(Debug, Clone, PartialEq)]
pub struct Landing {
    pub path: PathBuf,
    /// The first folder that is not there yet and would be made, if any.
    pub new_folder: Option<PathBuf>,
}

/// Matches folders, listing each parent once however many files go into it.
#[derive(Default)]
pub struct Finder {
    listed: HashMap<PathBuf, Vec<String>>,
}

impl Finder {
    /// Resolves `relative` (folders then a file name) under `root`.
    pub fn land(&mut self, root: &Path, relative: &Path) -> Landing {
        let parts: Vec<String> = relative
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let Some((name, folders)) = parts.split_last() else {
            return Landing {
                path: root.to_path_buf(),
                new_folder: None,
            };
        };

        let mut at = root.to_path_buf();
        let mut new_folder = None;
        for folder in folders {
            if new_folder.is_none() {
                if let Some(found) = self.find(&at, folder) {
                    at.push(found);
                    continue;
                }
                // A list of artists gets a folder for the first of them; a
                // name with an ampersand is usually one act and is kept whole.
                at.push(first_of_list(folder));
                new_folder = Some(at.clone());
            } else {
                at.push(folder);
            }
        }
        at.push(name);
        Landing {
            path: at,
            new_folder,
        }
    }

    /// The existing folder in `dir` that `wanted` means, if there is one.
    fn find(&mut self, dir: &Path, wanted: &str) -> Option<String> {
        if dir.join(wanted).is_dir() {
            return Some(wanted.to_string());
        }
        let names = self.listed.entry(dir.to_path_buf()).or_insert_with(|| {
            std::fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        });
        [key(wanted), key(lead(wanted))]
            .into_iter()
            .filter(|k| !k.is_empty())
            .find_map(|k| names.iter().find(|n| key(n) == k).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn spelling_capitals_and_symbols_do_not_tell_folders_apart() {
        assert_eq!(key("Anamorph & Co."), key("anamorph and co"));
        assert_eq!(key("DJ Koze"), key("dj-koze"));
        assert_ne!(key("Alineat"), key("Alinea"));
    }

    #[test]
    fn the_lead_artist_is_the_first_of_the_credit() {
        assert_eq!(lead("Alineat, Anrra"), "Alineat");
        assert_eq!(lead("Alineat feat. Anrra"), "Alineat");
        assert_eq!(lead("Anestie Gomez x Arapu"), "Anestie Gomez");
        assert_eq!(lead("Anrra"), "Anrra");
    }

    #[test]
    fn an_existing_folder_is_found_however_it_is_spelled() {
        let d = TempDir::new().unwrap();
        std::fs::create_dir(d.path().join("anamorph & co")).unwrap();
        std::fs::create_dir(d.path().join("alineat")).unwrap();
        let mut finder = Finder::default();

        let landed = finder.land(d.path(), Path::new("Anamorph And Co.\\a.wav"));
        assert_eq!(landed.path, d.path().join("anamorph & co").join("a.wav"));
        assert_eq!(landed.new_folder, None);

        // Not the whole credit, but its first artist has a folder.
        let landed = finder.land(d.path(), Path::new("Alineat, Anrra\\b.wav"));
        assert_eq!(landed.path, d.path().join("alineat").join("b.wav"));
    }

    #[test]
    fn a_missing_folder_is_reported_rather_than_guessed() {
        let d = TempDir::new().unwrap();
        let mut finder = Finder::default();
        let landed = finder.land(d.path(), Path::new("Anrra\\c.wav"));
        assert_eq!(landed.path, d.path().join("Anrra").join("c.wav"));
        assert_eq!(landed.new_folder, Some(d.path().join("Anrra")));

        let landed = finder.land(d.path(), Path::new("Alec Falconer; Harry Wills\\d.wav"));
        assert_eq!(landed.new_folder, Some(d.path().join("Alec Falconer")));
    }
}
