//! One-click tag tools: the jobs Mp3tag actions are written for, built in.
//!
//! Each is a fixed operation with at most a number to pick, so nobody has to
//! write `$if($eql($regexp(…` to get one. They only work out the change; it is
//! listed old → new and then written by [`crate::cleanup::apply_cleanup`],
//! which already writes tags and names exactly as previewed. Moving into
//! folders is not here: that is a [`crate::actions`] move with a pattern.

use crate::cleanup::{file_name, CleanField, CleanPreview};
use crate::state::AppState;
use noceur_fsmodel::cleanup::{self, Rule};
use noceur_tags::TrackMeta;
use serde::Deserialize;
use std::collections::HashSet;
use tauri::State;

/// Which tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tool {
    /// The file name down to `Artist - Title`.
    CleanName,
    /// An empty album artist filled from the artist.
    AlbumArtist,
    /// `Artist - Title` into the title.
    ArtistIntoTitle,
    /// `Artist - Title` in the title, split back into both.
    ArtistOutOfTitle,
    /// `(Original Mix)` off title and album.
    RemoveOriginal,
    /// A trailing `(…)` or `[…]` off the title.
    TrailingBrackets,
    /// `2016-03-04` → `2016`.
    YearOnly,
    /// Underscores, site names and doubled spaces out of the main text fields.
    CleanTags,
    /// `ft`, `Ft.`, `featuring` → `feat.` in artist and title.
    TidyFeat,
    /// The first N characters off the file name.
    CutStart,
    /// The last N characters off the file name.
    CutEnd,
}

impl Tool {
    /// Whether the tool changes the name rather than the tags.
    fn renames(self) -> bool {
        matches!(self, Tool::CleanName | Tool::CutStart | Tool::CutEnd)
    }
}

/// What a tool does to one file: a new stem, or field changes.
fn plan(tool: Tool, count: usize, meta: &TrackMeta, stem: &str) -> (String, Vec<CleanField>) {
    let mut fields = Vec::new();
    let mut set = |field: &str, label: &str, from: &str, to: &str| {
        if from != to {
            fields.push(CleanField {
                field: field.into(),
                label: label.into(),
                from: from.into(),
                to: to.into(),
            });
        }
    };
    let text = |v: &Option<String>| v.as_deref().map(str::trim).unwrap_or("").to_string();
    let (artist, title, album) = (text(&meta.artist), text(&meta.title), text(&meta.album));

    let new_stem = match tool {
        Tool::CleanName => cleanup::clean_file_name(stem),
        Tool::CutStart => stem
            .chars()
            .skip(count)
            .collect::<String>()
            .trim()
            .to_string(),
        Tool::CutEnd => {
            let keep = stem.chars().count().saturating_sub(count);
            stem.chars()
                .take(keep)
                .collect::<String>()
                .trim()
                .to_string()
        }
        _ => stem.to_string(),
    };

    let main = [
        ("title", "Title", &title),
        ("artist", "Artist", &artist),
        ("album", "Album", &album),
        ("albumartist", "Album artist", &text(&meta.album_artist)),
    ];

    match tool {
        Tool::CleanTags => {
            for (field, label, value) in main {
                let rules = [Rule::Underscores, Rule::Junk, Rule::Spacing];
                set(field, label, value, &cleanup::clean(value, &rules));
            }
        }
        Tool::TidyFeat => {
            for (field, label, value) in main {
                set(field, label, value, &cleanup::clean(value, &[Rule::Feat]));
            }
        }
        Tool::AlbumArtist => {
            if text(&meta.album_artist).is_empty() && !artist.is_empty() {
                set("albumartist", "Album artist", "", &artist);
            }
        }
        Tool::ArtistIntoTitle => {
            let prefix = format!("{} - ", artist.to_lowercase());
            if !artist.is_empty() && !title.is_empty() && !title.to_lowercase().starts_with(&prefix)
            {
                set("title", "Title", &title, &format!("{artist} - {title}"));
            }
        }
        Tool::ArtistOutOfTitle => {
            if let Some((a, t)) = cleanup::split_artist_title(&title) {
                set("artist", "Artist", &artist, &a);
                set("title", "Title", &title, &t);
            }
        }
        Tool::RemoveOriginal => {
            set(
                "title",
                "Title",
                &title,
                &cleanup::clean(&title, &[Rule::Mix]),
            );
            set(
                "album",
                "Album",
                &album,
                &cleanup::clean(&album, &[Rule::Mix]),
            );
        }
        Tool::TrailingBrackets => {
            set(
                "title",
                "Title",
                &title,
                &cleanup::strip_trailing_brackets(&title),
            );
        }
        Tool::YearOnly => {
            let date = text(&meta.date);
            let year: String = date.chars().take(4).collect();
            if date.chars().count() > 4 && year.chars().all(|c| c.is_ascii_digit()) {
                set("date", "Year", &date, &year);
            }
        }
        _ => {}
    }

    (new_stem, fields)
}

/// Works out what a tool would do to the selection. Changes nothing.
#[tauri::command]
pub async fn preview_tool(
    state: State<'_, AppState>,
    tool: Tool,
    count: Option<usize>,
    file_ids: Vec<i64>,
) -> Result<Vec<CleanPreview>, String> {
    let db = state.db.clone();
    let count = count.unwrap_or(0);

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            file_ids
                .iter()
                .filter_map(|id| {
                    noceur_index::thumb::locate(&db, *id)
                        .ok()
                        .flatten()
                        .filter(|f| f.kind == 1)
                        .map(|f| (*id, f.path))
                })
                .collect::<Vec<_>>()
        };

        // Names already spoken for, so two tracks that would end up with the
        // same name are reported rather than one landing on the other.
        let mut taken: HashSet<String> = HashSet::new();
        let mut out = Vec::with_capacity(found.len());

        for (file_id, path) in &found {
            let from = file_name(path);
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default();

            // A name tool reads no tags: it runs on every keystroke of the count.
            let meta = if tool.renames() {
                TrackMeta::default()
            } else {
                noceur_tags::read(path).unwrap_or_default()
            };
            let (stem_to, fields) = plan(tool, count, &meta, &stem);

            let mut to = from.clone();
            let mut problem = String::new();
            if tool.renames() {
                let clean = noceur_fsmodel::rename::sanitise(&stem_to);
                if clean.is_empty() {
                    problem = "that leaves nothing to name it".into();
                } else {
                    to = if ext.is_empty() {
                        clean
                    } else {
                        format!("{clean}.{ext}")
                    };
                }
            }
            if problem.is_empty() && to != from {
                let key = to.to_lowercase();
                if taken.contains(&key) {
                    problem = "another selected track would take this name".into();
                } else if !from.eq_ignore_ascii_case(&to) && path.with_file_name(&to).exists() {
                    problem = "a file of that name is already here".into();
                } else {
                    taken.insert(key);
                }
            }

            out.push(CleanPreview {
                file_id: *file_id,
                unchanged: to == from && fields.is_empty(),
                from,
                to,
                fields,
                problem,
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("tool preview task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(artist: &str, title: &str) -> TrackMeta {
        let some = |s: &str| (!s.is_empty()).then(|| s.to_string());
        TrackMeta {
            artist: some(artist),
            title: some(title),
            ..Default::default()
        }
    }

    fn changes(tool: Tool, m: &TrackMeta) -> Vec<(String, String, String)> {
        plan(tool, 0, m, "x")
            .1
            .into_iter()
            .map(|f| (f.field, f.from, f.to))
            .collect()
    }

    #[test]
    fn album_artist_is_filled_only_when_empty() {
        let m = meta("Skee Mask", "Rev8617");
        assert_eq!(
            changes(Tool::AlbumArtist, &m),
            vec![("albumartist".into(), "".into(), "Skee Mask".into())]
        );
        let mut m = m;
        m.album_artist = Some("Ilian Tape".into());
        assert!(changes(Tool::AlbumArtist, &m).is_empty());
    }

    #[test]
    fn artist_goes_into_the_title_once() {
        let m = meta("Skee Mask", "Rev8617");
        assert_eq!(
            changes(Tool::ArtistIntoTitle, &m),
            vec![(
                "title".into(),
                "Rev8617".into(),
                "Skee Mask - Rev8617".into()
            )]
        );
        let m = meta("Skee Mask", "Skee Mask - Rev8617");
        assert!(changes(Tool::ArtistIntoTitle, &m).is_empty());
    }

    #[test]
    fn artist_comes_back_out_of_the_title() {
        let m = meta("", "Skee Mask - Rev8617");
        assert_eq!(
            changes(Tool::ArtistOutOfTitle, &m),
            vec![
                ("artist".into(), "".into(), "Skee Mask".into()),
                (
                    "title".into(),
                    "Skee Mask - Rev8617".into(),
                    "Rev8617".into()
                ),
            ]
        );
        // Already right: only the title changes.
        let m = meta("Skee Mask", "Skee Mask - Rev8617");
        assert_eq!(changes(Tool::ArtistOutOfTitle, &m).len(), 1);
    }

    #[test]
    fn original_mix_leaves_title_and_album() {
        let mut m = meta("A", "Rev8617 (Original Mix)");
        m.album = Some("Compro (Original Mix)".into());
        let c = changes(Tool::RemoveOriginal, &m);
        assert_eq!(c[0].2, "Rev8617");
        assert_eq!(c[1].2, "Compro");
    }

    #[test]
    fn a_date_becomes_a_year_and_a_year_is_left_alone() {
        let mut m = meta("A", "B");
        m.date = Some("2016-03-04".into());
        assert_eq!(
            changes(Tool::YearOnly, &m),
            vec![("date".into(), "2016-03-04".into(), "2016".into())]
        );
        m.date = Some("2016".into());
        assert!(changes(Tool::YearOnly, &m).is_empty());
    }

    #[test]
    fn tags_are_cleaned_and_featuring_settles() {
        let mut m = meta("Skee_Mask ft Other", "Rev8617  [www.site.com]");
        m.album = Some("Compro".into());
        assert_eq!(
            changes(Tool::CleanTags, &m),
            vec![
                (
                    "title".into(),
                    "Rev8617  [www.site.com]".into(),
                    "Rev8617".into()
                ),
                (
                    "artist".into(),
                    "Skee_Mask ft Other".into(),
                    "Skee Mask ft Other".into()
                ),
            ]
        );
        assert_eq!(
            changes(Tool::TidyFeat, &m)
                .into_iter()
                .find(|c| c.0 == "artist"),
            Some((
                "artist".into(),
                "Skee_Mask ft Other".into(),
                "Skee_Mask feat. Other".into()
            ))
        );
    }

    #[test]
    fn the_name_can_be_cut_from_either_end() {
        let m = TrackMeta::default();
        assert_eq!(plan(Tool::CutStart, 3, &m, "01 Rev8617").0, "Rev8617");
        assert_eq!(plan(Tool::CutEnd, 4, &m, "Rev8617 xyz").0, "Rev8617");
        assert_eq!(plan(Tool::CutEnd, 99, &m, "Rev8617").0, "");
    }
}
