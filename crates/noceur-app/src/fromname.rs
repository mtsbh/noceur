//! Tags from the file name: the other direction of the renamer.
//!
//! A pattern like `%artist% - %title%` is laid over each file's name and the
//! pieces go into the tags - the move Mp3tag calls "Filename - Tag", for the
//! downloads that arrive named properly and tagged not at all.
//!
//! Matching is left to right. A field runs up to the *first* occurrence of the
//! text that follows it, and the last field takes whatever is left - so with
//! `%artist% - %title%`, "A - B - Remix" is artist "A", title "B - Remix",
//! which is right far more often than the other split. A pattern with `\` in it
//! reaches up into the folders: `%artist% - %album%\%track% - %title%`.

use std::path::Path;

use serde::Serialize;
use tauri::State;

use crate::dto::{WriteFailure, WriteReport};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Field(String),
}

/// Splits a pattern into literal text and `%field%`s.
fn parse(pattern: &str) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut rest = pattern;
    while !rest.is_empty() {
        match rest.find('%') {
            None => {
                parts.push(Part::Text(rest.to_string()));
                break;
            }
            Some(at) => {
                if at > 0 {
                    parts.push(Part::Text(rest[..at].to_string()));
                }
                let after = &rest[at + 1..];
                let end = after
                    .find('%')
                    .ok_or_else(|| "a % has no closing %".to_string())?;
                let name = after[..end].trim().to_lowercase();
                if name.is_empty() {
                    return Err("%% names no field".into());
                }
                if matches!(parts.last(), Some(Part::Field(_))) {
                    return Err("two fields need something between them".into());
                }
                parts.push(Part::Field(name));
                rest = &after[end + 1..];
            }
        }
    }
    if !parts.iter().any(|p| matches!(p, Part::Field(_))) {
        return Err("the pattern has no %fields%".into());
    }
    Ok(parts)
}

/// Lays a parsed pattern over some text. `None` when it does not fit.
fn fit(parts: &[Part], text: &str) -> Option<Vec<(String, String)>> {
    let mut out = Vec::new();
    let mut at = 0;
    for (i, part) in parts.iter().enumerate() {
        match part {
            Part::Text(lit) => {
                if !text[at..].starts_with(lit.as_str()) {
                    return None;
                }
                at += lit.len();
            }
            Part::Field(name) => {
                let end = match parts.get(i + 1) {
                    Some(Part::Text(next)) => at + text[at..].find(next.as_str())?,
                    _ => text.len(),
                };
                let value = text[at..end].trim();
                if value.is_empty() {
                    return None;
                }
                if !matches!(name.as_str(), "dummy" | "ignore" | "skip") {
                    out.push((name.clone(), value.to_string()));
                }
                at = end;
            }
        }
    }
    (at == text.len()).then_some(out)
}

/// What the pattern is laid over: the name without its extension, plus one
/// parent folder for every `\` in the pattern.
fn subject(path: &Path, pattern: &str) -> String {
    let depth = pattern.matches(['\\', '/']).count();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut pieces = vec![stem];
    let mut dir = path.parent();
    for _ in 0..depth {
        match dir {
            Some(d) => {
                pieces.push(
                    d.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                );
                dir = d.parent();
            }
            None => break,
        }
    }
    pieces.reverse();
    pieces.join("\\")
}

/// The fields a pattern understands by name; anything else is a custom tag.
fn label(field: &str) -> String {
    match field {
        "artist" => "Artist",
        "title" => "Title",
        "album" => "Album",
        "albumartist" | "album artist" => "Album artist",
        "genre" => "Genre",
        "year" | "date" => "Year",
        "track" | "tracknumber" => "Track",
        "bpm" => "BPM",
        "key" | "initialkey" => "Key",
        "comment" => "Comment",
        "isrc" => "ISRC",
        other => return other.to_uppercase(),
    }
    .to_string()
}

fn edit_for(fields: &[(String, String)]) -> noceur_tags::Edit {
    use noceur_tags::Change::Set;
    let mut e = noceur_tags::Edit::default();
    for (field, value) in fields {
        let v = Set(value.clone());
        match field.as_str() {
            "artist" => e.artist = v,
            "title" => e.title = v,
            "album" => e.album = v,
            "albumartist" | "album artist" => e.album_artist = v,
            "genre" => e.genre = v,
            "year" | "date" => e.year = v,
            // "01" and "1/12" both mean track one.
            "track" | "tracknumber" => {
                let n = value
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('0');
                e.track_no = Set(if n.is_empty() { "0".into() } else { n.into() });
            }
            "bpm" => e.bpm = v,
            "key" | "initialkey" => e.key = v,
            "comment" => e.comment = v,
            "isrc" => e.isrc = v,
            other => e.custom.push((other.to_uppercase(), v)),
        }
    }
    e
}

/// One file, as the dialog shows it before anything is written.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FromNameRow {
    pub file_id: i64,
    /// What the pattern was laid over.
    pub name: String,
    /// `(label, value)` for each field that will be written.
    pub fields: Vec<(String, String)>,
    /// Why nothing will be written; empty when something will.
    pub problem: String,
}

fn locate(state: &AppState, ids: &[i64]) -> Result<Vec<(i64, std::path::PathBuf)>, String> {
    let db = state
        .db
        .lock()
        .map_err(|_| "index lock poisoned".to_string())?;
    Ok(ids
        .iter()
        .filter_map(|id| {
            noceur_index::thumb::locate(&db, *id)
                .ok()
                .flatten()
                .filter(|f| f.kind == 1)
                .map(|f| (*id, f.path))
        })
        .collect())
}

/// What a pattern would write to each file, without writing anything.
#[tauri::command]
pub fn preview_from_name(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    pattern: String,
) -> Result<Vec<FromNameRow>, String> {
    let parts = parse(&pattern)?;
    Ok(locate(&state, &file_ids)?
        .into_iter()
        .map(|(id, path)| {
            let name = subject(&path, &pattern);
            match fit(&parts, &name) {
                Some(fields) => FromNameRow {
                    file_id: id,
                    fields: fields.iter().map(|(f, v)| (label(f), v.clone())).collect(),
                    name,
                    problem: String::new(),
                },
                None => FromNameRow {
                    file_id: id,
                    name,
                    fields: Vec::new(),
                    problem: "does not fit the pattern".into(),
                },
            }
        })
        .collect())
}

/// Writes what the preview showed. Files the pattern does not fit are skipped.
#[tauri::command]
pub async fn apply_from_name(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    pattern: String,
) -> Result<WriteReport, String> {
    let parts = parse(&pattern)?;
    let targets = locate(&state, &file_ids)?;
    let db = state.db.clone();
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;

    tauri::async_runtime::spawn_blocking(move || {
        let mut report = WriteReport::default();
        let mut written = Vec::new();
        for (id, path) in targets {
            let Some(fields) = fit(&parts, &subject(&path, &pattern)) else {
                continue;
            };
            let edit = edit_for(&fields);
            match crate::timestamp::preserving(&path, keep_dates, || {
                noceur_tags::edit::write(&path, &edit)
            }) {
                Ok(()) => {
                    report.written += 1;
                    written.push(id);
                }
                Err(e) => report.failures.push(WriteFailure {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    reason: e.to_string(),
                }),
            }
        }
        if !written.is_empty() {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            for id in written {
                noceur_index::reprobe(&db, id).map_err(|e| e.to_string())?;
            }
        }
        Ok(report)
    })
    .await
    .map_err(|e| format!("tag write task failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pattern: &str, text: &str) -> Option<Vec<(String, String)>> {
        fit(&parse(pattern).unwrap(), text)
    }

    fn pairs(v: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
        Some(
            v.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        )
    }

    #[test]
    fn artist_and_title_split_at_the_first_separator() {
        assert_eq!(
            run(
                "%artist% - %title%",
                "Murphie Cooper, Vadim Svoboda - Never Leave (Extended Mix)"
            ),
            pairs(&[
                ("artist", "Murphie Cooper, Vadim Svoboda"),
                ("title", "Never Leave (Extended Mix)")
            ])
        );
        // The last field keeps any later separators.
        assert_eq!(
            run("%artist% - %title%", "A - B - Remix"),
            pairs(&[("artist", "A"), ("title", "B - Remix")])
        );
    }

    #[test]
    fn a_name_that_does_not_fit_is_left_alone() {
        assert_eq!(run("%artist% - %title%", "just a title"), None);
        assert_eq!(run("%track%. %title%", "Intro"), None);
        // A field may not come out empty.
        assert_eq!(run("%artist% - %title%", " - Title"), None);
    }

    #[test]
    fn leading_track_numbers_and_dummies() {
        assert_eq!(
            run("%track% - %artist% - %title%", "01 - A - B"),
            pairs(&[("track", "01"), ("artist", "A"), ("title", "B")])
        );
        assert_eq!(
            run("%dummy% - %artist% - %title%", "[FREE] - A - B"),
            pairs(&[("artist", "A"), ("title", "B")])
        );
    }

    #[test]
    fn bad_patterns_are_explained() {
        assert!(parse("%artist%%title%").is_err());
        assert!(parse("%artist - %title%").is_err());
        assert!(parse("no fields").is_err());
        assert!(parse("%artist").is_err());
    }

    #[test]
    fn folders_come_in_with_a_backslash() {
        let path = Path::new("/m/Artist - Album/03 - Track Name.flac");
        let pattern = "%artist% - %album%\\%track% - %title%";
        let text = subject(path, pattern);
        assert_eq!(text, "Artist - Album\\03 - Track Name");
        assert_eq!(
            run(pattern, &text),
            pairs(&[
                ("artist", "Artist"),
                ("album", "Album"),
                ("track", "03"),
                ("title", "Track Name")
            ])
        );
    }

    #[test]
    fn track_numbers_lose_padding_and_totals() {
        let e = edit_for(&[("track".into(), "03/12".into())]);
        assert_eq!(e.track_no, noceur_tags::Change::Set("3".into()));
        let e = edit_for(&[("energy".into(), "7".into())]);
        assert_eq!(
            e.custom,
            vec![("ENERGY".into(), noceur_tags::Change::Set("7".into()))]
        );
    }

    /// End to end on a real file: the pattern's pieces land in the tags.
    #[test]
    fn written_tags_read_back() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("Artist A - Title B.wav");
        // A minimal silent WAV.
        let mut f = Vec::new();
        let data_len = 400u32;
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&44_100u32.to_le_bytes());
        f.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        f.extend_from_slice(&4u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend(std::iter::repeat_n(0u8, data_len as usize));
        std::fs::write(&path, f).unwrap();

        let fields = run("%artist% - %title%", &subject(&path, "%artist% - %title%")).unwrap();
        noceur_tags::edit::write(&path, &edit_for(&fields)).unwrap();
        let meta = noceur_tags::read(&path).unwrap();
        assert_eq!(meta.artist.as_deref(), Some("Artist A"));
        assert_eq!(meta.title.as_deref(), Some("Title B"));
    }
}
