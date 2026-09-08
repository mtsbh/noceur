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
    /// Canonical key value 0..23, or `None` when unknown.
    ///
    /// Carried beside the formatted text so the frontend can colour by position
    /// on the Camelot wheel without parsing a string whose notation the user can
    /// change.
    pub key_norm: Option<i64>,
    pub duration: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub size: String,
    /// `YYYY-MM-DD HH:MM` in UTC.
    pub modified: String,
    pub album_artist: String,
    pub genre: String,
    /// Release year from the tags, not the file's date on disk.
    pub year: String,
    pub track_no: String,
    pub comment: String,
    pub isrc: String,
    /// `44.1 kHz`, or empty.
    pub sample_rate: String,
    /// `16 bit`, or empty for formats that do not have one.
    pub bit_depth: String,
    pub codec: String,
    /// Empty when the file is fine; otherwise the reason it is not.
    pub state: String,
    /// Whether this row has an image worth requesting — embedded cover art for
    /// audio, the shell's own icon for a directory. Files with neither are left
    /// alone, so a folder of untagged tracks issues no requests at all.
    pub has_art: bool,
}

/// `44.1 kHz`, or nothing when the container did not say.
///
/// One decimal, and no trailing `.0`: 44.1 and 48 are the two numbers anyone
/// reads here, and `48.0 kHz` is noise in a narrow column.
pub fn rate(hz: Option<i64>) -> String {
    match hz {
        Some(hz) if hz > 0 => {
            let khz = hz as f64 / 1000.0;
            if (khz.fract()).abs() < 0.05 {
                format!("{khz:.0} kHz")
            } else {
                format!("{khz:.1} kHz")
            }
        }
        _ => String::new(),
    }
}

/// `16 bit`, or nothing — lossy formats have no bit depth to report.
pub fn depth(bits: Option<i64>) -> String {
    match bits {
        Some(b) if b > 0 => format!("{b} bit"),
        _ => String::new(),
    }
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

/// Civil date and time-of-day from a Unix timestamp, in UTC.
///
/// Written out rather than pulling in a date library: the whole requirement is
/// one fixed, sortable format with no locale and no parsing. The conversion is
/// Howard Hinnant's `civil_from_days`.
fn civil_utc(unix_seconds: i64) -> (i64, i64, i64, i64, i64) {
    let days = unix_seconds.div_euclid(86_400);
    let secs = unix_seconds.rem_euclid(86_400);

    // Shift the epoch to 0000-03-01 so leap days fall at the end of the cycle.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y, m, d, secs / 3_600, (secs % 3_600) / 60)
}

/// Last-modified time as `YYYY-MM-DD HH:MM`, or [`ABSENT`] where the filesystem
/// gave none.
///
/// Shown in **local** time. The stamp exists to be compared against what
/// Explorer says about the same file, and a column that is silently three hours
/// off is worse than no column: it looks authoritative and is wrong.
pub fn modified(unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return ABSENT.to_string();
    }

    let (y, m, d, h, mi) = local_civil(unix_seconds);
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}

/// Converts to the machine's local time, honouring daylight saving as it stood
/// on that date rather than as it stands today.
#[cfg(windows)]
fn local_civil(unix_seconds: i64) -> (i64, i64, i64, i64, i64) {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    let (y, m, d, h, mi) = civil_utc(unix_seconds);
    let utc = SYSTEMTIME {
        wYear: y as u16,
        wMonth: m as u16,
        wDayOfWeek: 0,
        wDay: d as u16,
        wHour: h as u16,
        wMinute: mi as u16,
        wSecond: (unix_seconds.rem_euclid(60)) as u16,
        wMilliseconds: 0,
    };

    let mut local = SYSTEMTIME {
        wYear: 0,
        wMonth: 0,
        wDayOfWeek: 0,
        wDay: 0,
        wHour: 0,
        wMinute: 0,
        wSecond: 0,
        wMilliseconds: 0,
    };

    // SAFETY: both structs are fully initialised and outlive the call; a null
    // time-zone pointer asks for the machine's current zone.
    let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };

    if ok == 0 {
        // A conversion that fails is still better answered with UTC than with
        // nothing; it is off by an offset rather than absent.
        return (y, m, d, h, mi);
    }

    (
        local.wYear as i64,
        local.wMonth as i64,
        local.wDay as i64,
        local.wHour as i64,
        local.wMinute as i64,
    )
}

/// Elsewhere, UTC. Local-time rules without a date library are not worth the
/// risk of being quietly wrong, and Windows is the tested target.
#[cfg(not(windows))]
fn local_civil(unix_seconds: i64) -> (i64, i64, i64, i64, i64) {
    civil_utc(unix_seconds)
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
        key_norm: if is_audio { row.key_norm } else { None },
        duration: if is_audio {
            duration(row.duration_ms)
        } else {
            blank.clone()
        },
        artist: row.artist.clone().unwrap_or_default(),
        title: row.title.clone().unwrap_or_default(),
        album: row.album.clone().unwrap_or_default(),
        album_artist: row.album_artist.clone().unwrap_or_default(),
        genre: row.genre.clone().unwrap_or_default(),
        year: row
            .year
            .filter(|y| *y > 0)
            .map(|y| y.to_string())
            .unwrap_or_default(),
        track_no: row
            .track_no
            .filter(|n| *n > 0)
            .map(|n| n.to_string())
            .unwrap_or_default(),
        comment: row.comment.clone().unwrap_or_default(),
        isrc: row.isrc.clone().unwrap_or_default(),
        sample_rate: if is_audio {
            rate(row.sample_rate)
        } else {
            blank.clone()
        },
        bit_depth: if is_audio {
            depth(row.bit_depth)
        } else {
            blank.clone()
        },
        codec: if is_audio {
            row.codec.clone().unwrap_or_default()
        } else {
            blank.clone()
        },
        size: if row.kind == 0 { blank } else { size(row.size) },
        modified: modified(row.mtime),
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
            album_artist: None,
            genre: Some("Techno".into()),
            year: Some(2016),
            track_no: Some(3),
            comment: None,
            isrc: None,
            sample_rate: Some(44_100),
            bit_depth: Some(16),
            codec: Some("pcm_s16le".into()),
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

    /// The civil conversion is asserted in UTC, where the answer is fixed.
    /// `modified` renders local time, which depends on the machine.
    #[test]
    fn civil_utc_converts_known_instants() {
        assert_eq!(civil_utc(1_609_459_200), (2021, 1, 1, 0, 0));
        assert_eq!(civil_utc(1_700_000_000), (2023, 11, 14, 22, 13));
    }

    /// Leap years are where a hand-written civil date conversion goes wrong.
    #[test]
    fn civil_utc_handles_leap_days_and_century_rules() {
        // 2020-02-29 is a leap day; 2000 is a leap year, 1900 was not.
        assert_eq!(civil_utc(1_582_934_400), (2020, 2, 29, 0, 0));
        assert_eq!(civil_utc(951_782_400), (2000, 2, 29, 0, 0));
        assert_eq!(civil_utc(-2_208_988_800), (1900, 1, 1, 0, 0));
    }

    #[test]
    fn modified_renders_a_sortable_stamp() {
        let stamp = modified(1_700_000_000);
        assert_eq!(stamp.len(), 16, "expected YYYY-MM-DD HH:MM, got {stamp}");
        assert_eq!(&stamp[4..5], "-");
        assert_eq!(&stamp[10..11], " ");
        assert_eq!(&stamp[13..14], ":");
    }

    /// Whatever the machine's zone, local time is within a day of UTC — enough
    /// to catch a conversion that lands in the wrong year or fails silently.
    #[test]
    fn modified_stays_near_the_utc_instant() {
        let (y, m, _, _, _) = civil_utc(1_700_000_000);
        let stamp = modified(1_700_000_000);
        let year: i64 = stamp[..4].parse().unwrap();
        let month: i64 = stamp[5..7].parse().unwrap();
        assert_eq!(year, y);
        assert!(
            (month - m).abs() <= 1,
            "local month {month} is nowhere near UTC month {m}"
        );
    }

    #[test]
    fn an_absent_timestamp_is_an_em_dash() {
        assert_eq!(modified(0), "—");
        assert_eq!(modified(-1), "—");
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
