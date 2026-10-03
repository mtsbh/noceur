//! Identifying a track by sound through Shazam.
//!
//! For what AcoustID does not know, which is most new and underground
//! electronic music: AcoustID's database is what its users have submitted,
//! Shazam's is what the labels and distributors deliver. The signature is made
//! locally (`noceur_audio::shazam`) and sent the way the Android app sends it,
//! as SongRec does.
//!
//! This is Shazam's private app API, not a published one. It can change without
//! notice and it rate-limits by address, so it is off until the user turns it
//! on, and asked one file at a time.
//!
//! What leaves the machine is the signature (spectral peaks from twelve seconds
//! of the track, which cannot be turned back into audio) and its length.

use crate::tracks::TrackMatch;
use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

pub const NAME: &str = "Shazam";
const SERVICE: &str = NAME;

/// Several seconds between lookups. Shazam publishes no limit; this is about
/// the pace of someone tagging songs on a phone, well under what gets an
/// address blocked.
static RATE: RateLimit = RateLimit::new(4_000);

/// The app's own user agent, which is what the endpoint expects to see.
const USER_AGENT: &str = "Dalvik/2.1.0 (Linux; U; Android 5.0.2; VS980 4G Build/LRX22G)";

/// Looks a signature up. `None` when Shazam does not recognise it.
pub fn identify(uri: &str, sample_ms: u32) -> Result<Option<TrackMatch>, OnlineError> {
    RATE.wait();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    let body = serde_json::json!({
        "geolocation": { "altitude": 300, "latitude": 45, "longitude": 2 },
        "signature": { "samplems": sample_ms, "timestamp": now as u32, "uri": uri },
        "timestamp": now as u32,
        "timezone": "Europe/Paris",
    });
    let url = format!(
        "https://amp.shazam.com/discovery/v5/en/US/android/-/tag/{}/{}\
         ?sync=true&webv3=true&sampling=true&connected=&shazamapiversion=v3&sharehub=true&video=v3",
        uuid().to_uppercase(),
        uuid(),
    );

    let response = agent()
        .post(&url)
        .header("User-Agent", USER_AGENT)
        .header("Content-Language", "en_US")
        .header("Content-Type", "application/json")
        .send(body.to_string());

    let text = match response {
        Ok(mut r) => r
            .body_mut()
            .read_to_string()
            .map_err(|e| OnlineError::Malformed {
                service: SERVICE,
                reason: e.to_string(),
            })?,
        Err(ureq::Error::StatusCode(status)) => {
            return Err(OnlineError::Status {
                service: SERVICE,
                status,
            })
        }
        Err(e) => {
            return Err(OnlineError::Unreachable {
                service: SERVICE,
                reason: e.to_string(),
            })
        }
    };
    parse(&text)
}

/// How far the audio may run faster or slower, or sit higher or lower, than
/// what Shazam matched it to, as a fraction.
///
/// The same recording comes back within about 0.0001 of both. A different track
/// built on the same sample, or the same loop re-pitched in an edit, comes back
/// at 0.007 to 0.04 (measured on the library this was built against), and
/// naming the file after it would be wrong with full confidence. A record
/// played from vinyl drifts by a few thousandths at most.
pub const MAX_SKEW: f32 = 0.005;

/// Turns an answer into a match. `None` when nothing was recognised, or when
/// what was recognised is not this recording (see [`MAX_SKEW`]).
pub fn parse(body: &str) -> Result<Option<TrackMatch>, OnlineError> {
    let parsed: Response = serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
        service: SERVICE,
        reason: e.to_string(),
    })?;
    // The track is the first match's.
    let Some(best) = parsed.matches.first() else {
        return Ok(None);
    };
    if best.timeskew.abs() > MAX_SKEW || best.frequencyskew.abs() > MAX_SKEW {
        return Ok(None);
    }
    let Some(track) = parsed.track else {
        return Ok(None);
    };
    let title = track.title.trim().to_string();
    let artist = track.subtitle.trim().to_string();
    if title.is_empty() || artist.is_empty() {
        return Ok(None);
    }

    // Album, label and year are in the song section, as captioned rows.
    let field = |name: &str| {
        track
            .sections
            .iter()
            .filter(|s| s.kind == "SONG")
            .flat_map(|s| &s.metadata)
            .find(|m| m.title == name)
            .map(|m| m.text.trim().to_string())
            .filter(|t| !t.is_empty())
    };
    let images = track.images.unwrap_or_default();

    Ok(Some(TrackMatch {
        source: NAME,
        title,
        // One string, as Shazam writes it: "A & B" and "A feat. B" are credits,
        // and splitting them would guess at what the label meant.
        artists: vec![artist],
        album: field("Album"),
        label: field("Label"),
        release_date: field("Released"),
        genres: track
            .genres
            .and_then(|g| g.primary)
            .filter(|g| !g.trim().is_empty())
            .into_iter()
            .collect(),
        isrc: track.isrc.filter(|i| !i.trim().is_empty()),
        url: track.url.filter(|u| !u.is_empty()),
        art: images
            .coverarthq
            .or(images.coverart)
            .filter(|a| !a.is_empty())
            .map(|a| larger_art(&a)),
        // It heard the track itself: as sure a match as any search gets.
        score: 1.0,
        ..Default::default()
    }))
}

/// Apple's artwork URLs end in the size wanted; Shazam asks for 400 pixels,
/// and the same image is there at 1000.
fn larger_art(url: &str) -> String {
    match url.rsplit_once('/') {
        Some((base, name)) if name.starts_with("400x400") => {
            format!("{base}/1000x1000{}", &name["400x400".len()..])
        }
        _ => url.to_string(),
    }
}

/// A random version-4 UUID, which the endpoint wants twice in its path.
///
/// Random enough without a crate: the standard library's hasher is seeded per
/// process from the operating system, and mixing in the time and a counter
/// gives each call its own.
fn uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let state = std::collections::hash_map::RandomState::new();
    let mut bytes = [0u8; 16];
    for (i, half) in bytes.chunks_mut(8).enumerate() {
        let mut h = state.build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
        h.write_usize(i);
        half.copy_from_slice(&h.finish().to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    matches: Vec<Match>,
    track: Option<Track>,
}

#[derive(Deserialize)]
struct Match {
    #[serde(default)]
    timeskew: f32,
    #[serde(default)]
    frequencyskew: f32,
}

#[derive(Deserialize)]
struct Track {
    #[serde(default)]
    title: String,
    #[serde(default)]
    subtitle: String,
    images: Option<Images>,
    genres: Option<Genres>,
    isrc: Option<String>,
    url: Option<String>,
    #[serde(default)]
    sections: Vec<Section>,
}

#[derive(Deserialize, Default)]
struct Images {
    coverart: Option<String>,
    coverarthq: Option<String>,
}

#[derive(Deserialize)]
struct Genres {
    primary: Option<String>,
}

#[derive(Deserialize)]
struct Section {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    metadata: Vec<Metadatum>,
}

#[derive(Deserialize)]
struct Metadatum {
    #[serde(default)]
    title: String,
    #[serde(default)]
    text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real answer, trimmed to what is read.
    const SAMPLE: &str = r#"{
      "matches": [{ "id": "11528720", "offset": 61.3, "timeskew": 0.00008, "frequencyskew": -0.00008 }],
      "tagid": "2D1B0A6C-0000-0000-0000-000000000000",
      "track": {
        "key": "585430470",
        "title": "Rumble",
        "subtitle": "Skrillex, Fred again.. & Flowdan",
        "images": {
          "coverart": "https://is1-ssl.mzstatic.com/image/thumb/Music126/v4/aa/bb/cc/x.jpg/400x400cc.jpg",
          "coverarthq": "https://is1-ssl.mzstatic.com/image/thumb/Music126/v4/aa/bb/cc/x.jpg/400x400cc.jpg"
        },
        "genres": { "primary": "Dance" },
        "isrc": "GBAHS2101056",
        "url": "https://www.shazam.com/track/585430470/rumble",
        "sections": [
          { "type": "SONG", "metadata": [
              { "title": "Album", "text": "Rumble - Single" },
              { "title": "Label", "text": "Atlantic Records UK" },
              { "title": "Released", "text": "2022" } ] },
          { "type": "LYRICS", "text": [] }
        ]
      }
    }"#;

    #[test]
    fn a_recognised_track_carries_what_the_picker_shows() {
        let m = parse(SAMPLE).unwrap().expect("a match");
        assert_eq!(m.source, "Shazam");
        assert_eq!(m.title, "Rumble");
        assert_eq!(m.artist(), "Skrillex, Fred again.. & Flowdan");
        assert_eq!(m.album.as_deref(), Some("Rumble - Single"));
        assert_eq!(m.label.as_deref(), Some("Atlantic Records UK"));
        assert_eq!(m.release_date.as_deref(), Some("2022"));
        assert_eq!(m.genres, ["Dance"]);
        assert_eq!(m.isrc.as_deref(), Some("GBAHS2101056"));
        assert_eq!(
            m.art.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/Music126/v4/aa/bb/cc/x.jpg/1000x1000cc.jpg")
        );
        assert!(crate::sources::is_allowed(m.art.as_deref().unwrap()));
        assert!(m.is_useful());
    }

    /// Not recognised: an empty list of matches, often with a hint to retry.
    #[test]
    fn nothing_heard_is_not_an_error() {
        let body = r#"{"matches":[],"tagid":"X","retryms":12000,"timestamp":1}"#;
        assert_eq!(parse(body).unwrap(), None);
    }

    /// Another track on the same sample, a few percent faster: Shazam names it,
    /// but it is not this recording.
    #[test]
    fn a_match_at_another_speed_or_pitch_is_not_this_recording() {
        let skewed = SAMPLE.replace(
            r#""timeskew": 0.00008, "frequencyskew": -0.00008"#,
            r#""timeskew": -0.0226, "frequencyskew": -0.0156"#,
        );
        assert_ne!(skewed, SAMPLE);
        assert_eq!(parse(&skewed).unwrap(), None);

        let pitched = SAMPLE.replace(r#""frequencyskew": -0.00008"#, r#""frequencyskew": 0.008"#);
        assert_eq!(parse(&pitched).unwrap(), None);
    }

    #[test]
    fn something_that_is_not_an_answer_is_reported() {
        assert!(matches!(
            parse("<html>blocked</html>"),
            Err(OnlineError::Malformed { .. })
        ));
    }

    #[test]
    fn uuids_are_version_4_and_differ() {
        let (a, b) = (uuid(), uuid());
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert!("89ab".contains(&a[19..20]), "{a}");
    }
}
