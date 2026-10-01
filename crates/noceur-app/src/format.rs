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
    /// Stars, 0 for unrated. A number rather than text: the column draws stars
    /// and sets them by click.
    pub rating: u8,
    /// Empty when the file is fine; otherwise the reason it is not.
    pub state: String,
    /// Tempo measured from the audio, as it would be written; empty when it has
    /// not been measured.
    pub detected_bpm: String,
    /// Key measured from the audio, in the row's notation; empty when not measured.
    pub detected_key: String,
    pub detected_key_norm: Option<i64>,
    /// The file is tagged with a BPM and it is not the one measured.
    pub bpm_differs: bool,
    /// The file is tagged with a key and it is not the one measured.
    pub key_differs: bool,
    /// Integrated loudness, `-9.4`; empty until Check quality has measured it.
    pub lufs: String,
    /// True peak in dBTP, `+0.6`, or `≥ +0.5` when it is only a lower bound.
    pub true_peak: String,
    /// What Check quality found broken, `0:30 missing · 1 dropout`; empty when
    /// sound or not checked.
    pub damage: String,
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

/// Analysed BPM at one decimal; tag BPM as stored.
pub fn bpm(value: Option<f64>, source: BpmSource) -> String {
    let Some(v) = value.filter(|v| v.is_finite() && *v > 0.0) else {
        return ABSENT.to_string();
    };
    match source {
        BpmSource::Analysed => format!("{v:.1}"),
        BpmSource::Tag => {
            // Keep the tag's own precision: 123 stays 123, 124.5 stays 124.5.
            // Tags are read as f32, so 122.2 arrives as 122.19999694824219;
            // two decimals is more than any tag carries.
            let v = (v * 100.0).round() / 100.0;
            if (v.fract()).abs() < f64::EPSILON {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            }
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

/// Days since 1970-01-01 for a civil date. The inverse of [`civil_from_days`].
///
/// Used to ask two questions a `YYYY-MM-DD` string cannot answer: which weekday
/// a date fell on, and how many days ago it was.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The weekday a day number falls on.
///
/// Day zero is 1970-01-01, which was a Thursday — index 4 counting from Sunday,
/// which is where the offset comes from.
fn weekday(days: i64) -> &'static str {
    WEEKDAYS[(days + 4).rem_euclid(7) as usize]
}

/// A 24-hour clock as the twelve-hour one with its half of the day.
fn twelve(h: i64) -> (i64, &'static str) {
    match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    }
}

/// Last-modified time, in **local** time, or [`ABSENT`] where the filesystem
/// gave none.
///
/// Recent files are named by their weekday — `Tuesday 3:54 AM` — because within
/// the last week that is how anybody actually thinks about when a file arrived,
/// and `2026-09-08 03:54` makes you count. Older ones get the date,
/// `10-Aug-26 1:14 AM`, where the weekday would mean nothing.
///
/// The window is the last seven days including today, so the seven weekday
/// names are all distinct and none of them is ambiguous — an eighth day would
/// put two Tuesdays in the column with no way to tell them apart.
///
/// Sorting is unaffected: the column orders on `f.mtime` in SQL, so this string
/// is only ever read.
pub fn modified(unix_seconds: i64) -> String {
    modified_at(unix_seconds, now_seconds())
}

/// The clock, as seconds since the epoch. Zero if it is before 1970.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// [`modified`], against a given moment. Split out so it can be tested.
pub fn modified_at(unix_seconds: i64, now: i64) -> String {
    if unix_seconds <= 0 {
        return ABSENT.to_string();
    }

    let (y, m, d, h, mi) = local_civil(unix_seconds);
    let (hour, half) = twelve(h);

    let (ny, nm, nd, _, _) = local_civil(now);
    let ago = days_from_civil(ny, nm, nd) - days_from_civil(y, m, d);

    if (0..7).contains(&ago) {
        let name = weekday(days_from_civil(y, m, d));
        return format!("{name} {hour}:{mi:02} {half}");
    }

    let month = MONTHS[(m - 1).clamp(0, 11) as usize];
    format!(
        "{d:02}-{month}-{:02} {hour}:{mi:02} {half}",
        y.rem_euclid(100)
    )
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
        rating: if is_audio {
            row.rating.unwrap_or(0).clamp(0, 5) as u8
        } else {
            0
        },
        size: if row.kind == 0 { blank } else { size(row.size) },
        modified: modified(row.mtime),
        state: if row.state == "ok" {
            String::new()
        } else {
            row.state.clone()
        },
        detected_bpm: match row.detected_bpm {
            Some(v) if is_audio => bpm(Some(v), BpmSource::Tag),
            _ => String::new(),
        },
        detected_key: match row.detected_key {
            Some(k) if is_audio => key(Some(k), None, notation),
            _ => String::new(),
        },
        detected_key_norm: row.detected_key.filter(|_| is_audio),
        bpm_differs: is_audio && bpm_differs(row.bpm, row.detected_bpm),
        key_differs: is_audio
            && key_differs(row.key_norm, row.key_tag.as_deref(), row.detected_key),
        lufs: match row.lufs {
            Some(v) if is_audio => format!("{v:.1}"),
            _ => String::new(),
        },
        true_peak: match row.true_peak {
            Some(v) if is_audio => peak(v, row.peak_floor),
            _ => String::new(),
        },
        damage: row.damage.clone().filter(|_| is_audio).unwrap_or_default(),
        // A directory always has one: the shell will give us its icon.
        has_art: row.kind == 0 || (is_audio && row.has_artwork),
    }
}

/// A true peak with its sign, `+0.6` / `-1.2`, and `≥` when it is a lower
/// bound.
pub fn peak(db: f64, floor: bool) -> String {
    let v = (db * 10.0).round() / 10.0;
    let text = if v == 0.0 {
        "0.0".to_string()
    } else {
        format!("{v:+.1}")
    };
    if floor {
        format!("≥ {text}")
    } else {
        text
    }
}

/// Whether a tagged BPM disagrees with a measured one. Closer than 0.05 is the
/// same number written with different precision.
pub fn bpm_differs(tagged: Option<f64>, detected: Option<f64>) -> bool {
    match (tagged.filter(|v| *v > 0.0), detected) {
        (Some(t), Some(d)) => (t - d).abs() >= 0.05,
        _ => false,
    }
}

/// Whether a tagged key disagrees with a measured one. A tag that does not
/// parse as a key disagrees with any measurement.
pub fn key_differs(norm: Option<i64>, tag: Option<&str>, detected: Option<i64>) -> bool {
    let Some(d) = detected else { return false };
    match norm {
        Some(n) => n != d,
        None => tag.is_some_and(|t| !t.trim().is_empty()),
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
            rating: Some(4),
            detected_bpm: None,
            detected_key: None,
            lufs: None,
            true_peak: None,
            peak_floor: false,
            damage: None,
        }
    }

    #[test]
    fn loudness_shows_once_measured() {
        let d = display(&row(), KeyNotation::Camelot);
        assert_eq!((d.lufs.as_str(), d.true_peak.as_str()), ("", ""));
        let measured = ListRow {
            lufs: Some(-9.44),
            true_peak: Some(0.52),
            peak_floor: true,
            ..row()
        };
        let d = display(&measured, KeyNotation::Camelot);
        assert_eq!(d.lufs, "-9.4");
        assert_eq!(d.true_peak, "≥ +0.5");
        assert_eq!(peak(-1.23, false), "-1.2");
        assert_eq!(peak(-0.02, false), "0.0");
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

    /// The day number and the civil date are inverses of each other. Everything
    /// the friendly stamp says about "which weekday" and "how long ago" rests
    /// on this, and a leap-year slip here would shift a whole column by a day.
    #[test]
    fn day_numbers_round_trip_through_civil_dates() {
        for days in [-25_000, -1, 0, 1, 18_500, 20_000, 25_000] {
            let (y, m, d, _, _) = civil_utc(days * 86_400);
            assert_eq!(days_from_civil(y, m, d), days, "{y}-{m}-{d}");
        }
    }

    /// 1970-01-01 was a Thursday. If that anchor is wrong every weekday is,
    /// and dates before the epoch are where a plain `%` would go negative.
    #[test]
    fn the_epoch_was_a_thursday() {
        assert_eq!(weekday(0), "Thursday");
        assert_eq!(weekday(1), "Friday");
        assert_eq!(weekday(7), "Thursday");
        assert_eq!(weekday(-1), "Wednesday");
        assert_eq!(weekday(-7), "Thursday");
    }

    /// Anything within the last seven days is named by its weekday, and
    /// anything older gets a date — the whole point of the column.
    #[test]
    fn recent_files_are_named_by_their_weekday() {
        // A fixed local noon, so no zone can push these over a day boundary.
        let now = 1_757_505_600; // 2025-09-10, around midday UTC
        let day = 86_400;

        assert!(
            WEEKDAYS.contains(&modified_at(now, now).split(' ').next().unwrap()),
            "today should be a weekday name, got {}",
            modified_at(now, now)
        );
        let week_ago = modified_at(now - 6 * day, now);
        assert!(
            WEEKDAYS.contains(&week_ago.split(' ').next().unwrap()),
            "six days back is still a weekday, got {week_ago}"
        );
        let older = modified_at(now - 40 * day, now);
        assert!(
            !WEEKDAYS.contains(&older.split(' ').next().unwrap()),
            "forty days back should be a date, got {older}"
        );
        assert!(older.contains('-'), "expected a dated stamp, got {older}");
    }

    /// Seven weekday names and no eighth, or two Tuesdays would sit in the
    /// column with nothing to tell them apart.
    #[test]
    fn the_weekday_window_stops_before_it_repeats() {
        let now = 1_757_505_600;
        let day = 86_400;
        let mut seen = std::collections::HashSet::new();
        for back in 0..7 {
            let stamp = modified_at(now - back * day, now);
            let name = stamp.split(' ').next().unwrap().to_string();
            assert!(WEEKDAYS.contains(&name.as_str()), "{stamp}");
            assert!(seen.insert(name), "a weekday repeated within the window");
        }
        let eighth = modified_at(now - 7 * day, now);
        assert!(
            !WEEKDAYS.contains(&eighth.split(' ').next().unwrap()),
            "the eighth day back must not be a weekday, got {eighth}"
        );
    }

    /// Noon and midnight are where a twelve-hour clock goes wrong.
    #[test]
    fn the_twelve_hour_clock_handles_both_ends() {
        assert_eq!(twelve(0), (12, "AM"));
        assert_eq!(twelve(11), (11, "AM"));
        assert_eq!(twelve(12), (12, "PM"));
        assert_eq!(twelve(13), (1, "PM"));
        assert_eq!(twelve(23), (11, "PM"));
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
    fn bpm_from_tags_keeps_its_own_precision() {
        assert_eq!(bpm(Some(123.0), BpmSource::Tag), "123");
        assert_eq!(bpm(Some(124.5), BpmSource::Tag), "124.5");
        // An f32 tag widened to f64.
        assert_eq!(bpm(Some(f64::from(122.2f32)), BpmSource::Tag), "122.2");
        assert_eq!(bpm(Some(f64::from(127.99f32)), BpmSource::Tag), "127.99");
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

    /// The same tempo written with more or fewer decimals is not a disagreement.
    #[test]
    fn a_bpm_differs_only_by_more_than_rounding() {
        assert!(!bpm_differs(Some(124.0), Some(124.0)));
        assert!(!bpm_differs(Some(124.02), Some(124.0)));
        assert!(bpm_differs(Some(123.0), Some(124.0)));
        assert!(
            !bpm_differs(None, Some(124.0)),
            "no tag is missing, not different"
        );
        assert!(!bpm_differs(Some(124.0), None));
    }

    #[test]
    fn a_key_differs_when_the_wheel_position_does() {
        assert!(!key_differs(Some(14), Some("8A"), Some(14)));
        assert!(key_differs(Some(14), Some("8A"), Some(15)));
        assert!(
            !key_differs(None, None, Some(14)),
            "no tag is missing, not different"
        );
        assert!(key_differs(None, Some("banana"), Some(14)));
        assert!(!key_differs(Some(14), Some("8A"), None));
    }

    #[test]
    fn a_measurement_is_shown_beside_the_tag() {
        let d = display(
            &ListRow {
                detected_bpm: Some(124.5),
                detected_key: Some(15),
                ..row()
            },
            KeyNotation::Camelot,
        );
        assert_eq!(d.detected_bpm, "124.5");
        assert_eq!(d.detected_key, "8B");
        assert!(d.bpm_differs, "tagged 123");
        assert!(d.key_differs, "tagged 8A");

        let dir = display(
            &ListRow {
                kind: 0,
                detected_bpm: Some(124.5),
                ..row()
            },
            KeyNotation::Camelot,
        );
        assert_eq!(dir.detected_bpm, "", "a folder has no tempo");
        assert!(!dir.bpm_differs);
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
        assert_eq!(d.bpm, "123");
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
