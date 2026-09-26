//! User-defined actions: buttons that do one thing to a selection.
//!
//! Every one is a kind, a target and a [title-formatting
//! expression](noceur_fsmodel::titlefmt) evaluated per file. That is the whole
//! model, and it covers what a DJ actually wants a button for:
//!
//! - fix a tag — `Tag`, `title`, `$caps2(%title%)`
//! - rename — `Rename`, `%artist% - %title%`
//! - open in another program — `Open`, `D:\freac\freac.exe`, `"%_path%"`
//! - look it up — `Url`, `https://google.com/search?q=%artist%+%title%`
//! - file it away — `Move`, `E:\music\techno`
//!
//! These live in the app's own config beside the executable, never in the
//! repository: they are full of one person's drive letters and installed
//! programs, and none of that belongs to anybody else.
//!
//! `Open` runs a program the user named, with arguments the user wrote. That is
//! the point of it, and it is the same thing Mp3tag's tools and foobar2000's
//! context menu do. What it deliberately does *not* do is go through a shell —
//! the argument line is split here, so a track called `; del *.*` is an
//! argument and never a command.

use crate::state::AppState;
use noceur_fsmodel::titlefmt::{Fields, Script};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::State;

/// What an action does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Write the expression's result to a tag field.
    Tag,
    /// Rename the file to the expression's result.
    Rename,
    /// Run a program, with the expression as its argument line.
    Open,
    /// Open the expression's result as a link.
    Url,
    /// Copy the file into a folder.
    Copy,
    /// Move the file into a folder.
    Move,
}

impl Kind {
    /// Whether this kind changes the file, and so needs previewing and undo.
    pub fn writes(self) -> bool {
        matches!(self, Kind::Tag | Kind::Rename | Kind::Copy | Kind::Move)
    }
}

/// One button.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    /// Stable across renames, so a button keeps its place in the list.
    pub id: String,
    pub label: String,
    pub kind: Kind,
    /// Field name for `Tag`, program path for `Open`, folder for `Copy`/`Move`.
    /// Unused by `Rename` and `Url`.
    #[serde(default)]
    pub target: String,
    /// The expression, evaluated once per file.
    #[serde(default)]
    pub script: String,
    /// Which recipe in the frontend built `script`, or `custom`.
    ///
    /// Never read here — the backend runs the expression and nothing else. It
    /// is carried so the editor can show the recipe again rather than only its
    /// output, and defaulted so a config written before recipes existed still
    /// loads.
    #[serde(default)]
    pub recipe: String,
    /// The recipe's inputs, kept for the same reason.
    #[serde(default)]
    pub slots: Vec<String>,
    /// Name of the icon the button is drawn with, or empty for none. Only the
    /// frontend knows the set; a button with one also sits in the toolbar.
    #[serde(default)]
    pub icon: String,
}

impl Action {
    /// What is wrong with this action, or nothing.
    ///
    /// Checked when it is saved rather than when it is run, so a typo is
    /// reported while it is being written and not after it has renamed a folder.
    pub fn problem(&self) -> Option<String> {
        if self.label.trim().is_empty() {
            return Some("give it a name".into());
        }
        if let Err(e) = Script::parse(&self.script) {
            return Some(e.to_string());
        }

        match self.kind {
            Kind::Tag if self.target.trim().is_empty() => Some("name a tag field".into()),
            Kind::Open if self.target.trim().is_empty() => Some("name a program".into()),
            Kind::Copy | Kind::Move if self.target.trim().is_empty() => {
                Some("name a destination folder".into())
            }
            Kind::Rename | Kind::Url if self.script.trim().is_empty() => {
                Some("write an expression".into())
            }
            _ => None,
        }
    }
}

/// Everything an expression can refer to, for one file.
///
/// The tag fields under the names foobar2000 uses, plus the file's own path in
/// the several forms an expression tends to want. Names are lower-case because
/// [`Script`] lower-cases what it looks up, so `%ALBUM ARTIST%` and
/// `%album artist%` are one field.
pub fn fields_of<'a>(meta: &noceur_tags::TrackMeta, path: &Path) -> Fields<'a> {
    let text = |v: &Option<String>| v.clone().unwrap_or_default();
    let mut f: HashMap<&str, String> = HashMap::new();

    f.insert("artist", text(&meta.artist));
    f.insert("title", text(&meta.title));
    f.insert("album", text(&meta.album));
    f.insert("album artist", text(&meta.album_artist));
    f.insert("albumartist", text(&meta.album_artist));
    f.insert("genre", text(&meta.genre));
    f.insert("comment", text(&meta.comment));
    f.insert("isrc", text(&meta.isrc));
    // The index keeps a year rather than a full date, so `%date%` and `%year%`
    // are the same thing here. `$left(%date%,4)` still gives the right answer,
    // which is the expression anyone writes against it.
    let year = meta.year.map(|v| v.to_string()).unwrap_or_default();
    f.insert("date", year.clone());
    f.insert("year", year);
    f.insert(
        "track",
        meta.track_no.map(|v| v.to_string()).unwrap_or_default(),
    );
    f.insert(
        "tracknumber",
        meta.track_no.map(|v| v.to_string()).unwrap_or_default(),
    );
    f.insert("key", text(&meta.key_tag));
    f.insert(
        "bpm",
        meta.bpm
            .map(|v| {
                if v.fract().abs() < f32::EPSILON {
                    format!("{}", v as i64)
                } else {
                    format!("{v}")
                }
            })
            .unwrap_or_default(),
    );

    // The path, in the forms foobar2000 offers. `_path` is the whole thing;
    // `filename` is the stem, which is what a rename expression wants.
    f.insert("_path", path.display().to_string());
    f.insert("path", path.display().to_string());
    f.insert(
        "filename",
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    f.insert(
        "filename_ext",
        path.file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    f.insert(
        "directoryname",
        path.parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    f.insert(
        "ext",
        path.extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    f
}

/// Turns a filed-away path pattern into folders and a file name.
///
/// The pattern is relative and may name folders that do not exist yet:
/// `%artist% - %album% '('%year%')'\%filename%` is a folder per release with the
/// track inside it. Both separators are accepted, because people write whichever
/// their other tools took.
///
/// Every segment is sanitised on its own, so a slash inside an album name
/// becomes a dash rather than an extra folder — the pattern decides the shape of
/// the tree, and a tag's contents must never be able to.
///
/// The extension is appended rather than taken from the pattern, exactly as
/// renaming does: a pattern that changed `.flac` to `.mp3` would produce a file
/// nothing can open.
pub fn filed_path(pattern: &str, ext: &str) -> Option<PathBuf> {
    let segments: Vec<String> = pattern
        .split(['\\', '/'])
        .map(|part| noceur_fsmodel::rename::sanitise(part.trim()))
        // `..` would climb out of the folder the user named, which is the one
        // thing a pattern must not be able to do.
        .filter(|part| !part.is_empty() && part != "." && part != "..")
        .collect();

    let (name, folders) = segments.split_last()?;
    let mut out = PathBuf::new();
    for folder in folders {
        out.push(folder);
    }
    out.push(if ext.is_empty() {
        name.clone()
    } else {
        format!("{name}.{ext}")
    });
    Some(out)
}

/// Splits an argument line into arguments, respecting double quotes.
///
/// Deliberately not a shell. A program is launched with these as its argument
/// vector, so nothing here can redirect, chain or expand anything — a track
/// named `a & del *.*` is one argument containing an ampersand and is never
/// anything else.
pub fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut any = false;

    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut current));
                    any = false;
                }
            }
            c => {
                current.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(current);
    }
    out
}

/// The actions the user has defined.
#[tauri::command]
pub fn actions() -> Vec<Action> {
    crate::config::Config::load(&crate::config::Config::default_path()).actions
}

/// Replaces the whole list.
///
/// All of them at once rather than one at a time: the front end edits a list and
/// this is what it has. Every action is checked first, so a list with a broken
/// expression in it is refused as a whole and the stored one is left alone.
#[tauri::command]
pub fn set_actions(actions: Vec<Action>) -> Result<Vec<Action>, String> {
    for action in &actions {
        if let Some(problem) = action.problem() {
            return Err(format!("{}: {problem}", action.label));
        }
    }

    let path = crate::config::Config::default_path();
    let mut config = crate::config::Config::load(&path);
    config.actions = actions;
    config.save(&path)?;
    Ok(config.actions)
}

/// What one action would do to one file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub file_id: i64,
    pub name: String,
    /// The expression's result: the new tag value, name, URL or argument line.
    pub result: String,
    /// Why it will be skipped, or empty.
    pub problem: String,
}

/// Works out what an action would do. Changes nothing.
#[tauri::command]
pub async fn preview_action(
    state: State<'_, AppState>,
    action: Action,
    file_ids: Vec<i64>,
) -> Result<Vec<Preview>, String> {
    if let Some(problem) = action.problem() {
        return Err(problem);
    }
    let script = Script::parse(&action.script).map_err(|e| e.to_string())?;
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let targets = locate_all(&db, &file_ids)?;

        Ok(targets
            .into_iter()
            .map(|(file_id, path)| {
                let meta = noceur_tags::read(&path).unwrap_or_default();
                let result = script.run(&fields_of(&meta, &path));

                // A copy or move with a pattern is previewed as the path it
                // would land on, which is the only part worth checking.
                let result = if matches!(action.kind, Kind::Copy | Kind::Move)
                    && !action.script.trim().is_empty()
                {
                    let ext = path
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    filed_path(&result, &ext)
                        .map(|rel| Path::new(&action.target).join(rel).display().to_string())
                        .unwrap_or_default()
                } else {
                    result
                };

                let problem = match action.kind {
                    Kind::Rename if noceur_fsmodel::rename::sanitise(&result).is_empty() => {
                        "the expression leaves nothing to name it".to_string()
                    }
                    Kind::Copy | Kind::Move
                        if !action.script.trim().is_empty() && result.is_empty() =>
                    {
                        "the pattern leaves nothing to name it".to_string()
                    }
                    Kind::Copy | Kind::Move if !Path::new(&action.target).is_dir() => {
                        format!("{} is not a folder", action.target)
                    }
                    Kind::Open if !Path::new(&action.target).is_file() => {
                        format!("{} is not there", action.target)
                    }
                    _ => String::new(),
                };

                Preview {
                    file_id,
                    name: file_name(&path),
                    result,
                    problem,
                }
            })
            .collect())
    })
    .await
    .map_err(|e| format!("preview task failed: {e}"))?
}

/// Runs an action over a selection.
#[tauri::command]
pub async fn run_action(
    state: State<'_, AppState>,
    window: tauri::Window,
    action: Action,
    file_ids: Vec<i64>,
) -> Result<crate::dto::WriteReport, String> {
    if let Some(problem) = action.problem() {
        return Err(problem);
    }
    let script = Script::parse(&action.script).map_err(|e| e.to_string())?;
    let db = state.db.clone();
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;

    // Needed by the shell's own copy and move, so its progress and conflict
    // dialogs belong to this window rather than floating loose.
    let hwnd = window
        .hwnd()
        .map(|h| h.0 as isize)
        .map_err(|e| format!("cannot find the window: {e}"))?;

    tauri::async_runtime::spawn_blocking(move || {
        let targets = locate_all(&db, &file_ids)?;
        let mut report = crate::dto::WriteReport::default();
        let mut touched = Vec::new();

        // Copy and move go to the shell in one operation rather than one file at
        // a time, so they get a single progress dialog, a single undo entry, and
        // Explorer's own conflict handling.
        if matches!(action.kind, Kind::Copy | Kind::Move) {
            let op = if action.kind == Kind::Copy {
                crate::fileops::Operation::Copy
            } else {
                crate::fileops::Operation::Move
            };
            let root = Path::new(&action.target);

            if action.script.trim().is_empty() {
                // No pattern: everything into one folder under the names it
                // already has, which is what dropping a selection does.
                let paths: Vec<PathBuf> = targets.iter().map(|(_, p)| p.clone()).collect();
                crate::fileops::run(hwnd, op, &paths, Some(root))?;
                report.written = paths.len() as i64;
            } else {
                // A pattern files them apart: a folder per release, a name per
                // track. The folders are made here because the shell will not
                // make them, and this is what knows which ones were implied.
                let mut items = Vec::new();
                for (_, path) in &targets {
                    let meta = noceur_tags::read(path).unwrap_or_default();
                    let ext = path
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();

                    let Some(relative) = filed_path(&script.run(&fields_of(&meta, path)), &ext)
                    else {
                        report.failures.push(crate::dto::WriteFailure {
                            name: file_name(path),
                            reason: "the pattern leaves nothing to name it".into(),
                        });
                        continue;
                    };

                    let full = root.join(&relative);
                    let (Some(folder), Some(name)) = (full.parent(), full.file_name()) else {
                        continue;
                    };
                    if let Err(e) = std::fs::create_dir_all(folder) {
                        report.failures.push(crate::dto::WriteFailure {
                            name: file_name(path),
                            reason: format!("cannot make {}: {e}", folder.display()),
                        });
                        continue;
                    }
                    items.push((
                        path.clone(),
                        folder.to_path_buf(),
                        name.to_string_lossy().into_owned(),
                    ));
                }

                crate::fileops::run_each(hwnd, op, &items)?;
                report.written = items.len() as i64;
            }

            if action.kind == Kind::Move {
                touched.extend(targets.iter().map(|(id, _)| *id));
            }
            reprobe(&db, &touched)?;
            return Ok(report);
        }

        for (file_id, path) in targets {
            let meta = noceur_tags::read(&path).unwrap_or_default();
            let result = script.run(&fields_of(&meta, &path));

            let outcome = match action.kind {
                Kind::Tag => crate::timestamp::preserving(&path, keep_dates, || {
                    let mut edit = noceur_tags::Edit::default();
                    set_field(&mut edit, action.target.trim(), &result);
                    noceur_tags::edit::write(&path, &edit).map_err(|e| e.to_string())
                }),
                Kind::Rename => rename_to(&path, &result),
                Kind::Open => launch(&action.target, &result),
                Kind::Url => crate::commands::open_url(result.clone()),
                Kind::Copy | Kind::Move => unreachable!("handled above"),
            };

            match outcome {
                Ok(()) => {
                    report.written += 1;
                    if action.kind.writes() {
                        touched.push(file_id);
                    }
                }
                Err(reason) => report.failures.push(crate::dto::WriteFailure {
                    name: file_name(&path),
                    reason,
                }),
            }
        }

        reprobe(&db, &touched)?;
        Ok(report)
    })
    .await
    .map_err(|e| format!("action task failed: {e}"))?
}

/// Points one named field of an edit at a value.
///
/// An unknown name becomes a custom field rather than an error: that is how
/// `ENERGY` and `MOOD` are written everywhere else here, and refusing them would
/// make half the useful actions impossible to write.
fn set_field(edit: &mut noceur_tags::Edit, field: &str, value: &str) {
    let change = noceur_tags::Change::from_input(Some(value.to_string()));
    match field.to_lowercase().as_str() {
        "artist" => edit.artist = change,
        "title" => edit.title = change,
        "album" => edit.album = change,
        "album artist" | "albumartist" => edit.album_artist = change,
        "genre" => edit.genre = change,
        "comment" => edit.comment = change,
        "isrc" => edit.isrc = change,
        "date" | "year" => edit.year = change,
        "track" | "tracknumber" => edit.track_no = change,
        "bpm" => edit.bpm = change,
        "key" => edit.key = change,
        "rating" => edit.rating = change,
        other => edit.custom.push((other.to_uppercase(), change)),
    }
}

/// Renames one file, checking the name again first.
fn rename_to(path: &Path, to: &str) -> Result<(), String> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();

    // The extension is carried over rather than taken from the expression. A
    // rename that changed `.flac` to `.mp3` would produce a file nothing can
    // open, and no expression should be able to do that by accident.
    let stem = noceur_fsmodel::rename::sanitise(to);
    if stem.is_empty() {
        return Err("that leaves nothing to name it".into());
    }
    let name = if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    };

    let destination = path.with_file_name(&name);
    if destination == path {
        return Ok(());
    }
    if destination.exists() {
        return Err(format!("{name} already exists"));
    }
    crate::renamer::rename_in_shell(path, &destination)
}

/// Starts a program with an argument line.
#[cfg(windows)]
fn launch(program: &str, args: &str) -> Result<(), String> {
    std::process::Command::new(program)
        .args(split_args(args))
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start {program}: {e}"))
}

#[cfg(not(windows))]
fn launch(_program: &str, _args: &str) -> Result<(), String> {
    Err("opening a file in another program is only implemented on Windows".into())
}

/// Resolves rows to the audio files they name.
fn locate_all(
    db: &std::sync::Arc<std::sync::Mutex<noceur_index::Db>>,
    file_ids: &[i64],
) -> Result<Vec<(i64, PathBuf)>, String> {
    let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
    Ok(file_ids
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

/// Re-reads whatever changed, so the listing agrees with the files.
fn reprobe(
    db: &std::sync::Arc<std::sync::Mutex<noceur_index::Db>>,
    ids: &[i64],
) -> Result<(), String> {
    if ids.is_empty() {
        return Ok(());
    }
    let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
    for id in ids {
        noceur_index::reprobe(&db, *id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(kind: Kind, target: &str, script: &str) -> Action {
        Action {
            id: "a".into(),
            label: "Test".into(),
            kind,
            target: target.into(),
            script: script.into(),
            recipe: String::new(),
            slots: Vec::new(),
            icon: String::new(),
        }
    }

    // ── Arguments ─────────────────────────────────────────────────────────

    /// The reason this is not a shell: a track named after a command must be an
    /// argument and nothing else.
    #[test]
    fn arguments_are_split_but_never_interpreted() {
        assert_eq!(
            split_args(r#"-sendpath "E:\music\a & b.mp3""#),
            vec!["-sendpath", r"E:\music\a & b.mp3"]
        );
        assert_eq!(
            split_args(r#""C:\x\y; del *.*.mp3""#),
            vec![r"C:\x\y; del *.*.mp3"]
        );
    }

    #[test]
    fn an_unquoted_line_splits_on_spaces() {
        assert_eq!(split_args("-a -b c"), vec!["-a", "-b", "c"]);
    }

    /// An empty pair of quotes is an argument: a program told to open "" should
    /// be told that rather than handed nothing at all.
    #[test]
    fn empty_quotes_are_still_an_argument() {
        assert_eq!(split_args(r#"-x "" -y"#), vec!["-x", "", "-y"]);
    }

    #[test]
    fn nothing_produces_no_arguments() {
        assert!(split_args("   ").is_empty());
    }

    // ── Filing a selection apart ──────────────────────────────────────────

    /// The pattern this was asked for: a folder per release, the track inside.
    #[test]
    fn a_pattern_becomes_folders_and_a_name() {
        assert_eq!(
            filed_path(r"Skee Mask - Compro (2016)\Rev8617", "flac"),
            Some(PathBuf::from(r"Skee Mask - Compro (2016)\Rev8617.flac"))
        );
    }

    #[test]
    fn either_separator_is_accepted() {
        assert_eq!(
            filed_path("Ilian Tape/2016/Rev8617", "wav"),
            Some(PathBuf::from(r"Ilian Tape\2016\Rev8617.wav"))
        );
    }

    #[test]
    fn a_pattern_with_no_folders_is_just_a_name() {
        assert_eq!(
            filed_path("Rev8617", "aiff"),
            Some(PathBuf::from("Rev8617.aiff"))
        );
    }

    /// The pattern decides the shape of the tree. A slash inside an album name
    /// must become part of a name, never another folder.
    #[test]
    fn a_separator_from_a_tag_cannot_add_a_folder() {
        // As it would arrive with the album already substituted in.
        let built = filed_path(r"AC-DC - Back In Black\Hells Bells", "mp3");
        assert_eq!(
            built,
            Some(PathBuf::from(r"AC-DC - Back In Black\Hells Bells.mp3"))
        );

        // And the characters Windows refuses are replaced per segment.
        assert_eq!(
            filed_path(r"Untitled: Part 2|x\What?", "mp3"),
            Some(PathBuf::from(r"Untitled - Part 2-x\What.mp3"))
        );
    }

    /// The one thing a pattern must not be able to do is climb out of the
    /// folder the user named.
    #[test]
    fn a_pattern_cannot_climb_out_of_its_folder() {
        assert_eq!(
            filed_path(r"..\..\windows\system32\evil", "exe"),
            Some(PathBuf::from(r"windows\system32\evil.exe"))
        );
    }

    #[test]
    fn empty_segments_are_dropped_rather_than_making_empty_folders() {
        assert_eq!(
            filed_path(r"Artist\\  \Title", "flac"),
            Some(PathBuf::from(r"Artist\Title.flac"))
        );
    }

    #[test]
    fn a_pattern_that_empties_produces_nothing() {
        assert_eq!(filed_path("", "flac"), None);
        assert_eq!(filed_path(r"  \  ", "flac"), None);
    }

    /// A pattern that changed the extension would produce a file nothing can
    /// open, so it is appended rather than read from the pattern.
    #[test]
    fn the_extension_is_appended_not_taken_from_the_pattern() {
        assert_eq!(
            filed_path("Rev8617.mp3", "flac"),
            Some(PathBuf::from("Rev8617.mp3.flac"))
        );
        assert_eq!(filed_path("Rev8617", ""), Some(PathBuf::from("Rev8617")));
    }

    // ── What is refused, and when ─────────────────────────────────────────

    /// Caught while the action is being written, not after it has renamed a
    /// folder.
    #[test]
    fn a_broken_expression_is_refused_when_it_is_saved() {
        let bad = action(Kind::Tag, "title", "$nope(%title%)");
        assert!(bad.problem().unwrap().contains("nope"));
    }

    #[test]
    fn each_kind_insists_on_what_it_needs() {
        assert!(action(Kind::Tag, "", "%title%").problem().is_some());
        assert!(action(Kind::Open, "", "%_path%").problem().is_some());
        assert!(action(Kind::Move, "", "").problem().is_some());
        assert!(action(Kind::Rename, "", "").problem().is_some());
        assert!(action(Kind::Url, "", "").problem().is_some());
    }

    #[test]
    fn a_complete_action_has_no_problem() {
        assert_eq!(
            action(Kind::Tag, "title", "$caps2(%title%)").problem(),
            None
        );
        assert_eq!(
            action(Kind::Rename, "", "%artist% - %title%").problem(),
            None
        );
        assert_eq!(
            action(Kind::Url, "", "https://x.test/?q=%artist%").problem(),
            None
        );
    }

    #[test]
    fn an_action_without_a_name_is_refused() {
        let mut a = action(Kind::Rename, "", "%title%");
        a.label = "  ".into();
        assert!(a.problem().is_some());
    }

    // ── Fields ────────────────────────────────────────────────────────────

    fn meta() -> noceur_tags::TrackMeta {
        noceur_tags::TrackMeta {
            artist: Some("Skee Mask".into()),
            title: Some("Rev8617".into()),
            album_artist: Some("Ilian Tape".into()),
            year: Some(2016),
            ..Default::default()
        }
    }

    #[test]
    fn an_expression_sees_the_tags_and_the_path() {
        let path = Path::new(r"E:\music\downloads\track.flac");
        let f = fields_of(&meta(), path);

        let run = |src: &str| Script::parse(src).unwrap().run(&f);
        assert_eq!(run("%artist% - %title%"), "Skee Mask - Rev8617");
        assert_eq!(run("%filename%"), "track");
        assert_eq!(run("%filename_ext%"), "track.flac");
        assert_eq!(run("%_path%"), r"E:\music\downloads\track.flac");
        assert_eq!(run("%directoryname%"), "downloads");
        assert_eq!(run("%ext%"), "flac");
    }

    /// foobar2000 users write `%ALBUM ARTIST%`, and the lookup is lower-cased,
    /// so both spellings have to be present.
    #[test]
    fn album_artist_answers_to_both_of_its_names() {
        let path = Path::new("a.flac");
        let f = fields_of(&meta(), path);
        let run = |src: &str| Script::parse(src).unwrap().run(&f);

        assert_eq!(run("%ALBUM ARTIST%"), "Ilian Tape");
        assert_eq!(run("%albumartist%"), "Ilian Tape");
    }

    /// The expressions this whole feature was asked for, run end to end.
    #[test]
    fn the_requested_expressions_produce_what_they_should() {
        let path = Path::new(r"E:\music\tg\01 - Skee Mask - Rev8617 (Original Mix).flac");
        let mut m = meta();
        m.title = Some("Rev8617 (Original Mix)".into());
        m.year = Some(2016);
        let f = fields_of(&m, path);
        let run = |src: &str| Script::parse(src).unwrap().run(&f);

        assert_eq!(run("$caps2(%artist%)"), "Skee Mask");
        // Written against a full date; the index has a year, and the answer is
        // the same either way, which is why the idiom is worth supporting as-is.
        assert_eq!(run("$left(%date%,4)"), "2016");
        assert_eq!(
            run("%artist% - %title%"),
            "Skee Mask - Rev8617 (Original Mix)"
        );
        assert_eq!(
            run("$ifgreater($strchr(%title%,'('),0,$trim($left(%title%,$sub($strchr(%title%,'('),1))),%title%)"),
            "Rev8617"
        );
    }

    // ── Writing to fields ─────────────────────────────────────────────────

    #[test]
    fn a_known_field_goes_to_its_own_place() {
        let mut edit = noceur_tags::Edit::default();
        set_field(&mut edit, "Title", "x");
        assert_eq!(edit.title, noceur_tags::Change::Set("x".into()));
        assert!(edit.custom.is_empty());
    }

    /// Half the useful actions write a field this crate does not model, so an
    /// unknown name has to become a custom field rather than an error.
    #[test]
    fn an_unknown_field_becomes_a_custom_one() {
        let mut edit = noceur_tags::Edit::default();
        set_field(&mut edit, "energy", "4");
        assert_eq!(
            edit.custom,
            vec![("ENERGY".to_string(), noceur_tags::Change::Set("4".into()))]
        );
    }

    #[test]
    fn only_the_kinds_that_change_a_file_are_marked_as_writing() {
        assert!(Kind::Tag.writes());
        assert!(Kind::Rename.writes());
        assert!(Kind::Move.writes());
        assert!(!Kind::Open.writes());
        assert!(!Kind::Url.writes());
    }
}
