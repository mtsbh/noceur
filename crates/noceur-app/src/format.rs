//! Every string the user reads is produced here.
//!
//! Formatting lives in Rust rather than the frontend so there is exactly one
//! implementation, it is unit-tested, and the UI cannot drift from it.

use noceur_index::ListRow;
use noceur_tags::{KeyNotation, MusicalKey};

/// Placeholder shown wherever a value is genuinely absent.
pub const ABSENT: &str = "—";

/// Where a BPM value came from. Analysed values are shown at full precision;
/// tag-derived values are marked so they are never mistaken for our own analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpmSource {
    /// Not produced anywhere yet — no milestone before this one runs analysis.
    /// Kept here (and covered by tests) so `display()` and the wire format are
    /// already correct the day analysis lands.
    #[allow(dead_code)]
    Analysed,
    Tag,
}

/// One row, entirely as strings, ready to render.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowDisplay {
    pub file_id: i64,
    /// 0 directory, 1 audio, 2 other — the frontend uses this only for icons.
    pub kind: i64,
    pub name: String,
    pub bpm: String,
    pub key: String,
    pub duration: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub size: String,
    /// Empty when the file is fine; otherwise the reason it is not.
    pub state: String,
    /// Whether this row has an image worth requesting — embedded cover art for
    /// audio, the shell's own icon for a directory. Files with neither are left
    /// alone, so a folder of untagged tracks issues no requests at all.
    pub has_art: bool,
}

/// `mm:ss`, or [`ABSENT`].
pub fn duration(ms: Option<i64>) -> String {
    match ms {
        Some(ms) if ms >= 0 => {
            let total = ms / 1000;
            format!("{}:{:02}", total / 60, total % 60)
        }
        _ => ABSENT.to_string(),
    }
}

/// Analysed BPM at one decimal; tag BPM as stored with a trailing marker.
pub fn bpm(value: Option<f64>, source: BpmSource) -> String {
    let Some(v) = value.filter(|v| v.is_finite() && *v > 0.0) else {
        return ABSENT.to_string();
    };
    match source {
        BpmSource::Analysed => format!("{v:.1}"),
        BpmSource::Tag => {
            // Keep the tag's own precision: 123 stays 123, 124.5 stays 124.5.
            let text = if (v.fract()).abs() < f64::EPSILON {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            };
            format!("{text}·")
        }
    }
}

/// The key in the requested notation, falling back to the raw tag text.
pub fn key(norm: Option<i64>, tag: Option<&str>, notation: KeyNotation) -> String {
    if let Some(k) = norm.and_then(|n| MusicalKey::from_canonical(n as i32)) {
        return k.format(notation);
    }
    match tag {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ => ABSENT.to_string(),
    }
}

/// Binary units, one decimal above a kilobyte.
pub fn size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes.max(0) as f64;
    if b < KB {
        return format!("{} B", bytes.max(0));
    }
    for (limit, unit) in [
        (KB * KB, "KB"),
        (KB * KB * KB, "MB"),
        (KB * KB * KB * KB, "GB"),
    ] {
        if b < limit {
            return format!("{:.1} {}", b / (limit / KB), unit);
        }
    }
    format!("{:.1} TB", b / (KB * KB * KB * KB))
}

/// Renders one indexed row into display strings.
///
/// Directories deliberately show nothing in the audio columns even if the index
/// happens to hold values for them — a folder has no BPM.
pub fn display(row: &ListRow, notation: KeyNotation) -> RowDisplay {
    let is_audio = row.kind == 1;
    let blank = String::new();

    RowDisplay {
        file_id: row.file_id,
        kind: row.kind,
        name: row.name.clone(),
        bpm: if is_audio {
            bpm(row.bpm, BpmSource::Tag)
        } else {
            blank.clone()
        },
        key: if is_audio {
            key(row.key_norm, row.key_tag.as_deref(), notation)
        } else {
            blank.clone()
        },
        duration: if is_audio {
            duration(row.duration_ms)
        } else {
            blank.clone()
        },
        artist: row.artist.clone().unwrap_or_default(),
        title: row.title.clone().unwrap_or_default(),
        album: row.album.clone().unwrap_or_default(),
        size: if row.kind == 0 { blank } else { size(row.size) },
        state: if row.state == "ok" {
            String::new()
        } else {
            row.state.clone()
        },
        // A directory always has one: the shell will give us its icon.
        has_art: row.kind == 0 || (is_audio && row.has_artwork),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ListRow {
        ListRow {
            file_id: 1,
            name: "track.wav".into(),
            ext: "wav".into(),
            size: 1024,
            mtime: 0,
            kind: 1,
            state: "ok".into(),
            artist: Some("G76".into()),
            title: Some("it takes time".into()),
            album: None,
            bpm: Some(123.0),
            key_tag: Some("8A".into()),
            key_norm: Some(14),
            duration_ms: Some(671_000),
            has_artwork: true,
        }
    }

    /// A directory has no cover art but always has a shell icon, so it still
    /// advertises an image. A non-audio file has neither.
    #[test]
    fn directories_and_tagged_audio_advertise_an_image() {
        assert!(display(&row(), KeyNotation::Camelot).has_art);

        let dir = ListRow { kind: 0, ..row() };
        assert!(display(&dir, KeyNotation::Camelot).has_art);

        let other = ListRow { kind: 2, ..row() };
        assert!(!display(&other, KeyNotation::Camelot).has_art);
    }

    /// Even without artwork, a folder still has an icon to draw.
    #[test]
    fn a_directory_advertises_an_image_regardless_of_artwork() {
        let dir = ListRow {
            kind: 0,
            has_artwork: false,
            ..row()
        };
        assert!(display(&dir, KeyNotation::Camelot).has_art);
    }

    #[test]
    fn an_audio_row_without_artwork_does_not_advertise_it() {
        let bare = ListRow {
            has_artwork: false,
            ..row()
        };
        assert!(!display(&bare, KeyNotation::Camelot).has_art);
    }

    #[test]
    fn duration_is_minutes_and_padded_seconds() {
        assert_eq!(duration(Some(671_000)), "11:11");
        assert_eq!(duration(Some(6_000)), "0:06");
        assert_eq!(duration(Some(3_600_000)), "60:00");
    }

    #[test]
    fn absent_duration_is_an_em_dash() {
        assert_eq!(duration(None), "—");
    }

    #[test]
    fn bpm_from_tags_is_marked_with_a_trailing_dot() {
        // Milestone 1 has no analysis, so every BPM is tag-derived.
        assert_eq!(bpm(Some(123.0), BpmSource::Tag), "123·");
        assert_eq!(bpm(Some(124.5), BpmSource::Tag), "124.5·");
    }

    #[test]
    fn analysed_bpm_is_shown_at_one_decimal_with_no_marker() {
        assert_eq!(bpm(Some(123.0), BpmSource::Analysed), "123.0");
    }

    #[test]
    fn absent_bpm_is_an_em_dash() {
        assert_eq!(bpm(None, BpmSource::Tag), "—");
        assert_eq!(bpm(None, BpmSource::Analysed), "—");
    }

    #[test]
    fn key_renders_in_the_requested_notation() {
        assert_eq!(key(Some(14), None, KeyNotation::Camelot), "8A");
        assert_eq!(key(Some(14), None, KeyNotation::OpenKey), "1m");
        assert_eq!(key(Some(14), None, KeyNotation::Classical), "Am");
    }

    #[test]
    fn an_unparseable_key_falls_back_to_its_original_text() {
        assert_eq!(key(None, Some("banana"), KeyNotation::Camelot), "banana");
    }

    #[test]
    fn absent_key_is_an_em_dash() {
        assert_eq!(key(None, None, KeyNotation::Camelot), "—");
    }

    #[test]
    fn size_uses_binary_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1024), "1.0 KB");
        assert_eq!(size(1_048_576), "1.0 MB");
        assert_eq!(size(132_120_576), "126.0 MB");
    }

    #[test]
    fn directories_show_no_audio_columns() {
        let mut r = row();
        r.kind = 0;
        r.bpm = Some(999.0);
        r.duration_ms = Some(1000);
        let d = display(&r, KeyNotation::Camelot);
        assert_eq!(d.bpm, "");
        assert_eq!(d.key, "");
        assert_eq!(d.duration, "");
        assert_eq!(d.size, "");
    }

    #[test]
    fn audio_rows_show_every_column() {
        let d = display(&row(), KeyNotation::Camelot);
        assert_eq!(d.name, "track.wav");
        assert_eq!(d.bpm, "123·");
        assert_eq!(d.key, "8A");
        assert_eq!(d.duration, "11:11");
        assert_eq!(d.artist, "G76");
    }

    #[test]
    fn a_bad_state_is_surfaced_rather_than_hidden() {
        let mut r = row();
        r.state = "undecodable".into();
        assert_eq!(display(&r, KeyNotation::Camelot).state, "undecodable");

        let mut ok = row();
        ok.state = "ok".into();
        assert_eq!(display(&ok, KeyNotation::Camelot).state, "");
    }
}
