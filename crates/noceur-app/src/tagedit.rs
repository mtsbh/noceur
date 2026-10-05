//! Reading and writing tags for the metadata editor.
//!
//! Its own module rather than more of `commands.rs`, because this is the first
//! thing the app does that writes to the user's files outside the shell — the
//! copy, move and delete commands all go through `IFileOperation`, which gives
//! them Explorer's progress, its conflict prompts and its undo. Nothing here has
//! any of that, so it is worth keeping where it can be read in one sitting.

use crate::dto::{FileFacts, TagEdit, TagForm, WriteFailure, WriteReport};
use crate::state::AppState;
use std::path::PathBuf;
use tauri::State;

/// Reads the tags of a selection into one editable form.
///
/// Reads the files themselves rather than the index. The index is a cache of
/// what a probe last saw, and an editor showing a stale cache would quietly
/// write it back — turning a file someone retagged in another program into a
/// file with this app's old idea of its tags. A few dozen files is a few dozen
/// tag reads; the editor opens on a selection, not on a library.
#[tauri::command]
pub async fn read_tags(state: State<'_, AppState>, file_ids: Vec<i64>) -> Result<TagForm, String> {
    if file_ids.is_empty() {
        return Ok(TagForm::default());
    }
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            file_ids
                .iter()
                .filter_map(|id| noceur_index::thumb::locate(&db, *id).ok().flatten())
                .filter(|f| f.kind == 1)
                .collect::<Vec<_>>()
        };
        if found.is_empty() {
            return Err("none of those rows are audio files".to_string());
        }

        let mut merge = Merge::default();
        let mut facts = None;
        for row in &found {
            // A file that will not parse contributes nothing rather than failing
            // the form: one bad file in a selection of thirty should not stop the
            // other twenty-nine being retagged.
            if let Ok(meta) = noceur_tags::read(&row.path) {
                merge.add(&meta);
                if found.len() == 1 {
                    let size = std::fs::metadata(&row.path).map(|m| m.len()).unwrap_or(0);
                    facts = Some(file_facts(&meta, size));
                }
            }
        }

        let subject = if found.len() == 1 {
            found[0]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            format!("{} files", found.len())
        };

        Ok(TagForm {
            facts,
            ..merge.into_form(found.len() as i64, subject)
        })
    })
    .await
    .map_err(|e| format!("tag read task failed: {e}"))?
}

/// What one file is, in words.
///
/// Uncompressed PCM is worked out from its format rather than taken from the
/// container, the way rekordbox is told it: a WAV's bitrate is exactly rate
/// times depth times channels, and nothing else is worth showing for it.
fn file_facts(meta: &noceur_tags::TrackMeta, size: u64) -> FileFacts {
    use crate::format;
    let pcm = meta.lossless && !matches!(meta.codec.as_deref(), Some("FLAC" | "WavPack" | "APE"));
    let kbps = match (pcm, meta.sample_rate, meta.bit_depth) {
        (true, Some(sr), Some(bits)) => {
            Some(sr * u32::from(bits) * u32::from(meta.channels.unwrap_or(2)) / 1000)
        }
        _ => meta.bitrate_kbps,
    };
    FileFacts {
        format: meta.codec.clone().unwrap_or_default(),
        bitrate: kbps
            .filter(|k| *k > 0)
            .map(|k| format!("{k} kbps"))
            .unwrap_or_default(),
        length: format::duration(meta.duration_ms.map(|ms| ms as i64)),
        sample_rate: format::rate(meta.sample_rate.map(i64::from)),
        bit_depth: if meta.lossless {
            format::depth(meta.bit_depth.map(i64::from))
        } else {
            String::new()
        },
        channels: match meta.channels {
            Some(1) => "Mono".into(),
            Some(2) => "Stereo".into(),
            Some(n) if n > 0 => format!("{n} ch"),
            _ => String::new(),
        },
        size: format::size(size as i64),
    }
}

/// Writes tags to a selection and re-reads what changed.
///
/// Per-file failures are collected rather than thrown. A selection of twenty
/// where one file is open in another program should write the nineteen and name
/// the one it could not; refusing the lot, or claiming success, are both worse
/// than saying what happened.
#[tauri::command]
pub async fn write_tags(
    state: State<'_, AppState>,
    file_ids: Vec<i64>,
    edit: TagEdit,
) -> Result<WriteReport, String> {
    let db = state.db.clone();
    // Read once per command rather than per file: it cannot change mid-write,
    // and reading it two hundred times would be two hundred file reads to
    // answer the same question.
    let keep_dates =
        crate::config::Config::load(&crate::config::Config::default_path()).preserve_dates;
    // The track in the player is written beside itself and swapped in, so the
    // decoder reading it is not disturbed - the sidebar edits what is playing.
    let in_player = crate::background::playing(&state);
    let background = state.background.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let change = noceur_tags::Edit {
            artist: noceur_tags::Change::from_input(edit.artist),
            title: noceur_tags::Change::from_input(edit.title),
            album: noceur_tags::Change::from_input(edit.album),
            album_artist: noceur_tags::Change::from_input(edit.album_artist),
            genre: noceur_tags::Change::from_input(edit.genre),
            comment: noceur_tags::Change::from_input(edit.comment),
            isrc: noceur_tags::Change::from_input(edit.isrc),
            label: noceur_tags::Change::from_input(edit.label),
            year: noceur_tags::Change::from_input(edit.year),
            track_no: noceur_tags::Change::from_input(edit.track_no),
            bpm: noceur_tags::Change::from_input(edit.bpm),
            key: noceur_tags::Change::from_input(edit.key),
            rating: noceur_tags::Change::from_input(edit.rating),
            // Quick tag sends only the fields it is changing, so every entry
            // here is a deliberate set or clear — there is no "leave alone" to
            // express, because leaving alone is not sending it.
            custom: edit
                .custom
                .into_iter()
                .filter(|(name, _)| name != HASHTAG_FIELD)
                .map(|(name, value)| (name, noceur_tags::Change::from_input(Some(value))))
                .collect(),
            date_to_year: false,
        };
        let hashtags = edit.hashtags.filter(|h| !h.set.is_empty() || !h.clear.is_empty());
        if change.is_empty() && hashtags.is_none() {
            return Ok(WriteReport::default());
        }

        // Paths first, holding the lock only for that. Writing tags rewrites
        // whole files, which for a large selection is far too long to keep the
        // index — and every listing and thumbnail behind it — waiting.
        let targets: Vec<(i64, PathBuf)> = {
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
                .collect()
        };

        let mut report = WriteReport::default();
        let mut written = Vec::new();

        for (file_id, path) in targets {
            // Tags go into each file's own comment, so the change is per file.
            let mut change = change.clone();
            if let Some(h) = &hashtags {
                let comment = noceur_tags::read(&path)
                    .ok()
                    .and_then(|m| m.comment)
                    .unwrap_or_default();
                let next = retag(&comment, &h.set, &h.clear);
                if next != comment {
                    change.comment = noceur_tags::Change::from_input(Some(next));
                }
            }
            if change.is_empty() {
                continue;
            }
            match crate::timestamp::preserving(&path, keep_dates, || {
                if in_player == Some(file_id) {
                    crate::background::write_beside(&background, &path, &change)
                } else {
                    noceur_tags::edit::write(&path, &change).map_err(|e| e.to_string())
                }
            }) {
                Ok(()) => {
                    report.written += 1;
                    written.push(file_id);
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

        // Re-read what changed, so the listing and the search index agree with
        // the files without waiting for the folder to be reopened.
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

/// Collects one field's value across a selection.
///
/// `None` while nothing has been seen; `Some(Some(v))` while every file so far
/// agrees; `Some(None)` once two have disagreed and the form must say so.
#[derive(Default)]
struct Field(Option<Option<String>>);

impl Field {
    fn add(&mut self, value: String) {
        match &self.0 {
            None => self.0 = Some(Some(value)),
            Some(Some(seen)) if *seen == value => {}
            Some(Some(_)) => self.0 = Some(None),
            Some(None) => {}
        }
    }

    /// An empty selection reports agreement on emptiness rather than conflict —
    /// there is nothing to conflict about, and a placeholder saying otherwise
    /// would be a lie about files that were never read.
    fn value(self) -> Option<String> {
        self.0.unwrap_or(Some(String::new()))
    }
}

#[derive(Default)]
struct Merge {
    artist: Field,
    title: Field,
    album: Field,
    album_artist: Field,
    genre: Field,
    comment: Field,
    isrc: Field,
    label: Field,
    year: Field,
    track_no: Field,
    bpm: Field,
    key: Field,
}

impl Merge {
    fn add(&mut self, meta: &noceur_tags::TrackMeta) {
        let text = |v: &Option<String>| v.clone().unwrap_or_default();
        self.artist.add(text(&meta.artist));
        self.title.add(text(&meta.title));
        self.album.add(text(&meta.album));
        self.album_artist.add(text(&meta.album_artist));
        self.genre.add(text(&meta.genre));
        self.comment.add(text(&meta.comment));
        self.isrc.add(text(&meta.isrc));
        self.label.add(text(&meta.label));
        self.year
            .add(meta.year.map(|v| v.to_string()).unwrap_or_default());
        self.track_no
            .add(meta.track_no.map(|v| v.to_string()).unwrap_or_default());
        // Shown as it is stored, not as the list column formats it: the list
        // marks a tagged BPM with a trailing dot, and this is the box its value
        // gets written back out of.
        self.bpm.add(meta.bpm.map(format_bpm).unwrap_or_default());
        self.key.add(text(&meta.key_tag));
    }

    fn into_form(self, count: i64, subject: String) -> TagForm {
        TagForm {
            count,
            subject,
            artist: self.artist.value(),
            title: self.title.value(),
            album: self.album.value(),
            album_artist: self.album_artist.value(),
            genre: self.genre.value(),
            comment: self.comment.value(),
            isrc: self.isrc.value(),
            label: self.label.value(),
            year: self.year.value(),
            track_no: self.track_no.value(),
            bpm: self.bpm.value(),
            key: self.key.value(),
            facts: None,
        }
    }
}

/// A whole BPM stays whole, so a box that read `128` does not come back `128.0`
/// and look like an edit nobody made.
fn format_bpm(v: f32) -> String {
    if v.fract().abs() < f32::EPSILON {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    /// Mixed In Key's key and energy stay in front; tags go at the end.
    #[test]
    fn tags_go_after_what_the_comment_already_says() {
        assert_eq!(retag("7A - 5", &v(&["peak", "dark"]), &[]), "7A - 5 #peak #dark");
        assert_eq!(retag("", &v(&["warmup"]), &[]), "#warmup");
    }

    /// Choosing another situation takes the old one out and leaves the rest.
    #[test]
    fn a_cleared_tag_leaves_and_nothing_else_moves() {
        assert_eq!(
            retag("7A - 5 #warmup #dark", &v(&["peak"]), &v(&["warmup", "build"])),
            "7A - 5 #dark #peak"
        );
        assert_eq!(retag("#peak", &[], &v(&["peak"])), "");
    }

    #[test]
    fn a_tag_already_there_is_not_added_twice_whatever_its_case() {
        assert_eq!(retag("7A - 5 #Peak", &v(&["peak"]), &[]), "7A - 5 #Peak");
        assert_eq!(retag("#PEAK", &[], &v(&["peak"])), "");
    }

    /// One word, so a DJ app's "contains #peak-time" finds it.
    #[test]
    fn spaces_in_a_value_become_dashes() {
        assert_eq!(retag("", &v(&["peak time"]), &[]), "#peak-time");
        assert_eq!(hashtags_of("7A - 5 #peak-time #dark"), v(&["peak-time", "dark"]));
    }

    fn meta(artist: &str, bpm: Option<f32>) -> noceur_tags::TrackMeta {
        noceur_tags::TrackMeta {
            artist: Some(artist.to_string()),
            bpm,
            ..Default::default()
        }
    }

    #[test]
    fn one_file_reports_its_own_values() {
        let mut m = Merge::default();
        m.add(&meta("Skee Mask", Some(140.0)));
        let form = m.into_form(1, "t.wav".into());
        assert_eq!(form.artist.as_deref(), Some("Skee Mask"));
        assert_eq!(form.bpm.as_deref(), Some("140"));
    }

    #[test]
    fn agreeing_files_report_the_shared_value() {
        let mut m = Merge::default();
        m.add(&meta("Skee Mask", None));
        m.add(&meta("Skee Mask", None));
        assert_eq!(
            m.into_form(2, "2 files".into()).artist.as_deref(),
            Some("Skee Mask")
        );
    }

    /// The distinction the whole form exists for: a field the files disagree on
    /// must come back as `None`, so the editor can show a placeholder instead of
    /// a blank box that would write emptiness to all of them.
    #[test]
    fn disagreeing_files_report_no_value_at_all() {
        let mut m = Merge::default();
        m.add(&meta("Skee Mask", None));
        m.add(&meta("Zenker Brothers", None));
        assert_eq!(m.into_form(2, "2 files".into()).artist, None);
    }

    /// Agreement on emptiness is not disagreement. Two untagged files should
    /// offer an empty box to type into, not a "various" placeholder.
    #[test]
    fn files_that_are_all_empty_agree_on_being_empty() {
        let mut m = Merge::default();
        m.add(&noceur_tags::TrackMeta::default());
        m.add(&noceur_tags::TrackMeta::default());
        assert_eq!(m.into_form(2, "2 files".into()).artist.as_deref(), Some(""));
    }

    /// One tagged and one untagged file disagree, which is the case that would
    /// otherwise quietly wipe the tagged one.
    #[test]
    fn a_tagged_and_an_untagged_file_disagree() {
        let mut m = Merge::default();
        m.add(&meta("Skee Mask", None));
        m.add(&noceur_tags::TrackMeta::default());
        assert_eq!(m.into_form(2, "2 files".into()).artist, None);
    }

    /// A WAV's bitrate is its format multiplied out; an MP3's is what it says.
    #[test]
    fn facts_name_the_format_in_words() {
        let wav = noceur_tags::TrackMeta {
            codec: Some("WAV".into()),
            lossless: true,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
            channels: Some(2),
            duration_ms: Some(372_000),
            ..Default::default()
        };
        let f = file_facts(&wav, 65_640_000);
        assert_eq!(f.bitrate, "1411 kbps");
        assert_eq!(f.bit_depth, "16-bit");
        assert_eq!(f.channels, "Stereo");
        assert_eq!(f.length, "6:12");
        assert_eq!(f.sample_rate, "44.1 kHz");

        let mp3 = noceur_tags::TrackMeta {
            codec: Some("MP3".into()),
            bitrate_kbps: Some(320),
            bit_depth: Some(16),
            channels: Some(1),
            ..Default::default()
        };
        let f = file_facts(&mp3, 0);
        assert_eq!(f.bitrate, "320 kbps");
        assert_eq!(f.bit_depth, "", "a lossy file has no bit depth");
        assert_eq!(f.channels, "Mono");
    }

    #[test]
    fn a_whole_bpm_keeps_no_decimal_point() {
        assert_eq!(format_bpm(128.0), "128");
        assert_eq!(format_bpm(127.5), "127.5");
    }
}

/// The key a rating travels under in the quick-tag map.
///
/// Not a tag name: ratings live in a frame of their own in every format, and
/// this is only how the panel and the backend refer to one. The colon keeps it
/// from colliding with any field somebody names.
pub const RATING_FIELD: &str = "noceur:rating";

/// The quick-tag name for values kept as `#tags` in the comment. Not a tag
/// name; the colon keeps it from colliding with one, as with [`RATING_FIELD`].
pub const HASHTAG_FIELD: &str = "noceur:hashtags";

/// How a value is written as a tag: `#` and the value, spaces as `-`, so
/// "peak time" is one word to a DJ app's "contains" filter.
fn hashtag(value: &str) -> String {
    format!("#{}", value.trim().split_whitespace().collect::<Vec<_>>().join("-"))
}

/// The `#tags` in a comment, without their `#`, in order.
pub fn hashtags_of(comment: &str) -> Vec<String> {
    comment
        .split_whitespace()
        .filter_map(|w| w.strip_prefix('#'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The comment with `clear` removed and `set` added at its end.
///
/// Everything else stays where it was: Mixed In Key's `7A - 5` in front, any
/// words typed by hand. Tags already there keep their place; matching ignores
/// case, so `#Peak` is not added again beside `#peak`.
pub fn retag(comment: &str, set: &[String], clear: &[String]) -> String {
    let gone: Vec<String> = clear.iter().map(|v| hashtag(v).to_lowercase()).collect();
    let mut words: Vec<String> = comment
        .split_whitespace()
        .filter(|w| !gone.contains(&w.to_lowercase()))
        .map(str::to_string)
        .collect();
    for v in set {
        let tag = hashtag(v);
        if tag.len() > 1 && !words.iter().any(|w| w.eq_ignore_ascii_case(&tag)) {
            words.push(tag);
        }
    }
    words.join(" ")
}

/// Reads the named fields a quick-tag panel shows for one track.
///
/// Reads the file rather than the index for the same reason the editor does:
/// this is about to be written back, and showing a stale cache would mean
/// writing one.
#[tauri::command]
pub async fn quick_fields(
    state: State<'_, AppState>,
    file_id: i64,
    names: Vec<String>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?
        };
        let Some(found) = found.filter(|f| f.kind == 1) else {
            return Ok(Default::default());
        };

        // A file that will not parse has no fields to show; the panel simply
        // starts empty rather than refusing to open.
        let fields = noceur_tags::read_fields(&found.path, &names).unwrap_or_default();
        let mut out: std::collections::HashMap<String, String> =
            fields.values.into_iter().collect();
        // The rating rides in the same map under a name no tag uses, so the
        // panel reads one reply rather than two.
        if let Some(stars) = fields.rating {
            out.insert(RATING_FIELD.to_string(), stars.to_string());
        }
        if names.iter().any(|n| n == HASHTAG_FIELD) {
            let comment = noceur_tags::read(&found.path)
                .ok()
                .and_then(|m| m.comment)
                .unwrap_or_default();
            out.insert(HASHTAG_FIELD.to_string(), hashtags_of(&comment).join(", "));
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("quick tag read failed: {e}"))?
}
