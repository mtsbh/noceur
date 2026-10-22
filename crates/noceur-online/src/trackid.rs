//! Tracklists of DJ sets, from TrackID.
//!
//! TrackID listens to sets published on SoundCloud, Mixcloud, YouTube and
//! HearThis and records which tracks it heard, and when. That is what this app
//! wants it for: a recorded set on disk, matched to its entry there, gets a
//! tracklist with a start and an end for every track it recognised.
//!
//! Needs the user's own API client, a client ID and a secret generated on their
//! TrackID account, sent together as one bearer token. Passed in rather than
//! stored here, as with Discogs.
//!
//! What leaves the machine is the words searched for and the slug of the set
//! chosen — and, only when the user uploads a set, the set itself. Uploading is
//! not part of the published API: it is the website's own upload, and takes the
//! token the site signs its users in with rather than the API client.

use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

const SERVICE: &str = "TrackID";
const BASE: &str = "https://trackid.net/api/integration";
const UPLOAD: &str = "https://uploads.trackid.net/api/private/audiostreams/upload";

/// The website's own limit.
pub const MAX_UPLOAD_BYTES: u64 = 2048 * 1024 * 1024;

/// No published limit. Two a second is far below anything a person clicking
/// through a search could need, and polite to a small service.
static RATE: RateLimit = RateLimit::new(500);

/// How far apart two hearings of one track can be and still be one play.
const MERGE_GAP_MS: u64 = 60_000;

/// Sets asked for per search.
pub const SEARCH_LIMIT: usize = 20;

/// One set, as a search lists it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSummary {
    pub slug: String,
    pub title: String,
    pub channel: String,
    /// Where the set is published.
    pub url: String,
    pub duration_ms: Option<u64>,
    /// When it was published, as `YYYY-MM-DD`.
    pub published: String,
    pub artwork: String,
    /// `ready`, `queued` or `processing`. Only a ready set has a tracklist.
    pub status: &'static str,
    /// Share of the set's length TrackID put a name to, 0..1, once it has
    /// listened.
    pub coverage: Option<f64>,
    /// Where it is published: `soundcloud`, `mixcloud`, `youtube`, `hearthis`,
    /// or `upload` for a file someone sent.
    pub source: &'static str,
}

/// One track heard in a set.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTrack {
    pub start_ms: u64,
    pub end_ms: u64,
    pub artist: String,
    pub title: String,
    pub label: String,
    /// How many sets TrackID has heard this track in, this one included.
    pub plays: u32,
}

/// A set and everything heard in it, in order.
#[derive(Debug, Clone, PartialEq)]
pub struct SetDetail {
    pub summary: SetSummary,
    pub tracks: Vec<SetTrack>,
}

/// Searches sets by free text: a title, a DJ, a venue.
///
/// Every set, whatever its state: asking for finished ones only also hides the
/// user's own private uploads, which are the sets most likely to be the file
/// on their disk. Whether a set is ready is in its summary.
pub fn search_sets(query: &str, key: &str) -> Result<Vec<SetSummary>, OnlineError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }
    let body = get(
        &format!("{BASE}/audiostreams"),
        &[
            ("keywords", query),
            ("pageSize", &SEARCH_LIMIT.to_string()),
            ("currentPage", "0"),
        ],
        key,
    )?;
    parse_search(&body)
}

/// Reads one set with its tracklist.
pub fn fetch_set(slug: &str, key: &str) -> Result<SetDetail, OnlineError> {
    // Slugs are lowercase words and dashes. Anything else would be a path
    // segment this app did not get from TrackID, so it is not sent.
    if slug.is_empty()
        || !slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
    {
        return Err(OnlineError::Malformed {
            service: SERVICE,
            reason: format!("{slug:?} is not a set's slug"),
        });
    }
    let body = get(&format!("{BASE}/audiostreams/{slug}"), &[], key)?;
    parse_set(&body)
}

fn get(url: &str, query: &[(&str, &str)], key: &str) -> Result<String, OnlineError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(OnlineError::NeedsToken { service: SERVICE });
    }

    RATE.wait();
    let mut request = agent()
        .get(url)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Accept", "application/json");
    for (name, value) in query {
        request = request.query(*name, *value);
    }

    match request.call() {
        Ok(mut r) => r
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| OnlineError::Malformed {
                service: SERVICE,
                reason: e.to_string(),
            }),
        // TrackID answers 401 both for credentials it does not know and for an
        // account whose API access has lapsed. Either way the fix is in Settings.
        Err(ureq::Error::StatusCode(401)) => Err(OnlineError::BadToken { service: SERVICE }),
        Err(ureq::Error::StatusCode(status)) => Err(OnlineError::Status {
            service: SERVICE,
            status,
        }),
        Err(e) => Err(OnlineError::Unreachable {
            service: SERVICE,
            reason: e.to_string(),
        }),
    }
}

/// Turns a search response into sets.
pub fn parse_search(body: &str) -> Result<Vec<SetSummary>, OnlineError> {
    let parsed: Envelope<SearchResult> = from_json(body)?;
    Ok(parsed
        .result
        .audiostreams
        .into_iter()
        .filter(|s| !s.is_deleted)
        .map(Stream::summary)
        .collect())
}

/// Turns one set's response into its tracklist.
///
/// A set can have been listened to more than once: the first pass, then
/// reprocessing that finds what the first missed. The tracklist is all of them
/// together, in time order, with a track heard twice at the same moment kept
/// once.
pub fn parse_set(body: &str) -> Result<SetDetail, OnlineError> {
    let parsed: Envelope<Stream> = from_json(body)?;
    let stream = parsed.result;

    let mut tracks: Vec<(Option<i64>, SetTrack)> = stream
        .detection_processes
        .iter()
        .flat_map(|p| p.detection_process_music_tracks.iter())
        .filter_map(|t| {
            let start_ms = clock_ms(&t.start_time)?;
            let end_ms = clock_ms(&t.end_time).unwrap_or(start_ms).max(start_ms);
            Some((
                t.music_track_id,
                SetTrack {
                    start_ms,
                    end_ms,
                    artist: t.artist.clone().unwrap_or_default().trim().to_string(),
                    title: t.title.clone().unwrap_or_default().trim().to_string(),
                    label: t.label.clone().unwrap_or_default().trim().to_string(),
                    plays: t.reference_count.unwrap_or(0).max(0) as u32,
                },
            ))
        })
        .collect();
    tracks.sort_by_key(|(_, t)| t.start_ms);

    let mut kept: Vec<(Option<i64>, SetTrack)> = Vec::with_capacity(tracks.len());
    for (id, track) in tracks {
        // The same recording heard again before a minute has passed since it
        // was last heard is the same play: a second pass finding it again, or
        // one pass that lost it under a long blend and picked it back up.
        // Merged into one span rather than listed twice in a row.
        let same = kept.iter_mut().rev().find(|(kept_id, k)| {
            id.is_some() && *kept_id == id && track.start_ms <= k.end_ms + MERGE_GAP_MS
        });
        match same {
            Some((_, k)) => {
                k.end_ms = k.end_ms.max(track.end_ms);
                if k.label.is_empty() {
                    k.label = track.label;
                }
            }
            None => kept.push((id, track)),
        }
    }

    Ok(SetDetail {
        summary: stream.summary(),
        tracks: kept.into_iter().map(|(_, t)| t).collect(),
    })
}

/// Uploads a set for TrackID to listen to, privately.
///
/// MP3 only, which is what the site accepts. Streamed from disk, so a set of a
/// gigabyte costs no more memory than one of ten megabytes; `progress` hears
/// the bytes sent so far and the total. Returns once the upload is accepted —
/// TrackID then queues it, and the set turns up in a search, `queued` at first.
pub fn upload(
    path: &std::path::Path,
    title: &str,
    session: &str,
    progress: &(dyn Fn(u64, u64) + Sync),
) -> Result<(), OnlineError> {
    use std::io::Read;

    let session = session.trim();
    if session.is_empty() {
        return Err(OnlineError::NeedsToken { service: SERVICE });
    }
    let is_mp3 = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    if !is_mp3 {
        return Err(OnlineError::Malformed {
            service: SERVICE,
            reason: "only MP3 files can be uploaded; convert the set first".into(),
        });
    }
    let file = std::fs::File::open(path).map_err(|e| OnlineError::Unreachable {
        service: SERVICE,
        reason: format!("cannot read the set: {e}"),
    })?;
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    if size > MAX_UPLOAD_BYTES {
        return Err(OnlineError::TooLarge { size });
    }

    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().replace(['"', '\r', '\n'], "_"))
        .unwrap_or_else(|| "set.mp3".into());
    let boundary = format!("noceur{:016x}", size ^ 0x9e37_79b9_7f4a_7c15);
    let field = |key: &str, value: &str| {
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}\r\n")
    };
    let head = format!(
        "{}{}--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{name}\"\r\nContent-Type: audio/mpeg\r\n\r\n",
        field("title", title.trim()),
        field("isPrivate", "true"),
    );
    let tail = format!("\r\n--{boundary}--\r\n");
    let total = head.len() as u64 + size + tail.len() as u64;

    let mut body = Counted {
        inner: std::io::Cursor::new(head.into_bytes())
            .chain(file)
            .chain(std::io::Cursor::new(tail.into_bytes())),
        sent: 0,
        total,
        progress,
    };

    // Its own agent: the shared one gives up after fifteen seconds, and a set
    // on a home connection takes minutes.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60 * 60)))
        .user_agent(crate::USER_AGENT)
        .build()
        .into();
    let result = agent
        .post(UPLOAD)
        .header("Authorization", &format!("Bearer {session}"))
        .header("Accept", "application/json, text/plain, */*")
        .header("Origin", "https://trackid.net")
        .header("Referer", "https://trackid.net/")
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .header("Content-Length", &total.to_string())
        .send(ureq::SendBody::from_reader(&mut body));

    match result {
        Ok(_) => Ok(()),
        // The site's sign-in lasts a year; after that this is the answer.
        Err(ureq::Error::StatusCode(401)) => Err(OnlineError::BadToken { service: SERVICE }),
        Err(ureq::Error::StatusCode(status)) => Err(OnlineError::Status {
            service: SERVICE,
            status,
        }),
        Err(e) => Err(OnlineError::Unreachable {
            service: SERVICE,
            reason: e.to_string(),
        }),
    }
}

/// When a site sign-in token stops working, from its own `exp` claim, as Unix
/// seconds. `None` for anything that is not a token with one.
pub fn session_expires(token: &str) -> Option<i64> {
    let payload = token.trim().split('.').nth(1)?;
    let claims: serde_json::Value = serde_json::from_slice(&base64url(payload)?).ok()?;
    claims.get("exp")?.as_i64()
}

/// Decodes unpadded URL-safe base64, as a token's parts are written.
fn base64url(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.trim_end_matches('=').bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// A reader that says how far it has got.
struct Counted<'a, R> {
    inner: R,
    sent: u64,
    total: u64,
    progress: &'a (dyn Fn(u64, u64) + Sync),
}

impl<R: std::io::Read> std::io::Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        let before = self.sent / (1 << 20);
        self.sent += n as u64;
        // Once a megabyte, not once a read: a read is a few kilobytes.
        if self.sent / (1 << 20) != before || n == 0 {
            (self.progress)(self.sent, self.total);
        }
        Ok(n)
    }
}

fn from_json<'a, T: Deserialize<'a>>(body: &'a str) -> Result<T, OnlineError> {
    serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
        service: SERVICE,
        reason: e.to_string(),
    })
}

/// Milliseconds from a .NET `TimeSpan` as TrackID writes it.
///
/// `01:54:05`, `00:01:46.2340000` with seven digits of fraction, and
/// `1.02:03:04` for anything past a day.
pub fn clock_ms(text: &str) -> Option<u64> {
    let text = text.trim();
    let (days, rest) = match text.split_once('.') {
        // A dot before the first colon is the day separator; after it, the
        // fraction.
        Some((d, r)) if !d.contains(':') => (d.parse::<u64>().ok()?, r),
        _ => (0, text),
    };
    let (clock, fraction) = match rest.split_once('.') {
        Some((c, f)) => (c, f),
        None => (rest, ""),
    };

    let mut parts = clock.split(':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let s: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || m >= 60 || s >= 60 {
        return None;
    }

    let millis = if fraction.is_empty() {
        0
    } else {
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // The first three digits are milliseconds; the rest are finer than
        // anything a seek can land on.
        let three: String = fraction.chars().chain("000".chars()).take(3).collect();
        three.parse::<u64>().ok()?
    };

    Some((((days * 24 + h) * 60 + m) * 60 + s) * 1000 + millis)
}

#[derive(Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Deserialize)]
struct SearchResult {
    #[serde(default)]
    audiostreams: Vec<Stream>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stream {
    slug: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    created_on: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    #[serde(default)]
    is_deleted: bool,
    #[serde(default)]
    status: Option<i64>,
    #[serde(default)]
    time_hit_rate: Option<f64>,
    #[serde(default)]
    audio_stream_type: Option<i64>,
    #[serde(default)]
    detection_processes: Vec<Process>,
}

impl Stream {
    fn summary(self) -> SetSummary {
        SetSummary {
            slug: self.slug,
            title: self.title.unwrap_or_default().trim().to_string(),
            channel: self.channel.unwrap_or_default().trim().to_string(),
            url: self.url.unwrap_or_default(),
            duration_ms: self.duration.as_deref().and_then(clock_ms),
            published: self
                .created_on
                .unwrap_or_default()
                .chars()
                .take(10)
                .collect(),
            artwork: self.artwork_url.unwrap_or_default(),
            status: match self.status {
                Some(1) => "queued",
                Some(2) => "processing",
                _ => "ready",
            },
            coverage: self.time_hit_rate.map(|r| r.clamp(0.0, 1.0)),
            source: match self.audio_stream_type {
                Some(1) => "soundcloud",
                Some(2) => "mixcloud",
                Some(4) => "youtube",
                Some(5) => "hearthis",
                _ => "upload",
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Process {
    #[serde(default)]
    detection_process_music_tracks: Vec<Heard>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Heard {
    #[serde(default)]
    music_track_id: Option<i64>,
    #[serde(default)]
    start_time: String,
    #[serde(default)]
    end_time: String,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    reference_count: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_ways_of_writing_a_time_are_read() {
        assert_eq!(clock_ms("01:54:05"), Some(6_845_000));
        assert_eq!(clock_ms("00:01:46.2340000"), Some(106_234));
        assert_eq!(clock_ms("02:00:00.0530000"), Some(7_200_053));
        assert_eq!(clock_ms("00:00:01.5"), Some(1_500));
        assert_eq!(
            clock_ms("1.02:03:04"),
            Some(((26 * 60 + 3) * 60 + 4) * 1000)
        );
    }

    #[test]
    fn a_time_that_is_not_one_is_refused() {
        assert_eq!(clock_ms(""), None);
        assert_eq!(clock_ms("12:00"), None);
        assert_eq!(clock_ms("00:61:00"), None);
        assert_eq!(clock_ms("00:01:46.x"), None);
    }

    const SET: &str = r#"{"result":{
        "slug":"katzenohr-mit-dir-festival-2023-muhle",
        "url":"https://soundcloud.com/katzenohr/mit-dir-festival-2023",
        "title":"Katzenohr @ Mit Dir Festival 2023 | Mühle",
        "channel":"Katzenohr",
        "duration":"02:00:00.0530000",
        "createdOn":"2023-08-09T00:00:00Z",
        "artworkUrl":"https://i1.sndcdn.com/artworks-x-original.jpg",
        "trackCount":0,
        "audioStreamType":1,
        "timeHitRate":0.41,
        "detectionProcesses":[
          {"id":1,"isReprocessing":false,"detectionProcessMusicTracks":[
            {"musicTrackId":561358,"startTime":"00:13:50.2340000","endTime":"00:18:29.2340000",
             "artist":"Trak B, Gandolfi B.","title":"DIesel Out The Flower","label":"Trak B, Gandolfi B."},
            {"musicTrackId":1169143,"startTime":"00:01:46.2340000","endTime":"00:07:05.2340000",
             "artist":"Stevie Nicks","title":"Edge of Seventeen (2019 Remaster)","label":"Modern Records","referenceCount":13}
          ]},
          {"id":2,"isReprocessing":true,"detectionProcessMusicTracks":[
            {"musicTrackId":145805,"startTime":"01:54:05","endTime":"01:55:13",
             "artist":"Vintage Culture, Bhaskar, Meca","title":"Tina","label":"Higher Ground"},
            {"musicTrackId":561358,"startTime":"00:13:52","endTime":"00:18:29",
             "artist":"Trak B, Gandolfi B.","title":"DIesel Out The Flower","label":null}
          ]},
          {"id":3,"isReprocessing":true,"detectionProcessMusicTracks":[]}
        ]}}"#;

    /// Every pass contributes, in time order, and a later pass hearing the same
    /// track where an earlier one did adds nothing.
    #[test]
    fn a_set_reads_every_pass_into_one_tracklist() {
        let set = parse_set(SET).unwrap();
        assert_eq!(set.summary.duration_ms, Some(7_200_053));
        assert_eq!(set.summary.published, "2023-08-09");
        assert_eq!(set.summary.channel, "Katzenohr");
        assert_eq!(set.summary.source, "soundcloud");
        assert_eq!(set.summary.coverage, Some(0.41));
        assert_eq!(set.tracks[0].plays, 13);

        let starts: Vec<u64> = set.tracks.iter().map(|t| t.start_ms).collect();
        assert_eq!(starts, vec![106_234, 830_234, 6_845_000]);
        assert_eq!(set.tracks[0].artist, "Stevie Nicks");
        assert_eq!(set.tracks[0].end_ms, 425_234);
        assert_eq!(set.tracks[2].title, "Tina");
    }

    /// A track heard in two pieces either side of a short gap is one play.
    #[test]
    fn a_track_heard_in_pieces_is_one_span() {
        let body = r#"{"result":{"slug":"s","detectionProcesses":[{"detectionProcessMusicTracks":[
            {"musicTrackId":7,"startTime":"00:12:19","endTime":"00:15:00","artist":"A","title":"Lucy"},
            {"musicTrackId":7,"startTime":"00:15:20","endTime":"00:18:43","artist":"A","title":"Lucy"},
            {"musicTrackId":8,"startTime":"00:18:45","endTime":"00:23:12","artist":"B","title":"Me"}
        ]}]}}"#;
        let tracks = parse_set(body).unwrap().tracks;
        assert_eq!(tracks.len(), 2);
        assert_eq!((tracks[0].start_ms, tracks[0].end_ms), (739_000, 1_123_000));
    }

    /// The same track played twice in one set is two entries.
    #[test]
    fn a_track_played_again_later_is_kept() {
        let body = r#"{"result":{"slug":"s","detectionProcesses":[{"detectionProcessMusicTracks":[
            {"musicTrackId":7,"startTime":"00:10:00","endTime":"00:12:00","artist":"A","title":"T"},
            {"musicTrackId":7,"startTime":"01:10:00","endTime":"01:12:00","artist":"A","title":"T"}
        ]}]}}"#;
        assert_eq!(parse_set(body).unwrap().tracks.len(), 2);
    }

    #[test]
    fn a_search_lists_its_sets() {
        let body = r#"{"result":{"audiostreams":[
            {"slug":"a-set","title":" A Set ","channel":"Radio","duration":"01:26:25.2210000","createdOn":"2025-08-28T10:00:00Z","status":3},
            {"slug":"waiting","title":"Waiting","status":1},
            {"slug":"gone","title":"Gone","isDeleted":true}
        ],"rowCount":2}}"#;
        let found = parse_search(body).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].title, "A Set");
        assert_eq!(found[0].status, "ready");
        assert_eq!(found[1].status, "queued");
        assert_eq!(found[0].duration_ms, Some(5_185_221));
    }

    #[test]
    fn a_slug_that_is_not_one_is_never_sent() {
        assert!(matches!(
            fetch_set("../account", "k"),
            Err(OnlineError::Malformed { .. })
        ));
    }

    #[test]
    fn a_sign_in_token_says_when_it_runs_out() {
        // {"exp":1797979162}, unsigned: only the claim is read.
        let token = "eyJhbGciOiJIUzI1NiJ9.eyJleHAiOjE3OTc5NzkxNjJ9.x";
        assert_eq!(session_expires(token), Some(1_797_979_162));
        assert_eq!(session_expires("not a token"), None);
    }

    #[test]
    fn only_an_mp3_is_uploaded() {
        let err = upload(std::path::Path::new("D:/Sets/a.flac"), "a", "t", &|_, _| {});
        assert!(matches!(err, Err(OnlineError::Malformed { .. })));
    }

    #[test]
    fn no_credentials_means_no_request() {
        assert!(matches!(
            search_sets("katzenohr", " "),
            Err(OnlineError::NeedsToken { .. })
        ));
    }
}
