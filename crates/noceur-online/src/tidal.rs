//! Track metadata from Tidal's catalogue API.
//!
//! The official API (`openapi.tidal.com/v2`), with the client-credentials flow:
//! the user registers a free app on developer.tidal.com and pastes its client ID
//! and secret into Settings. No Tidal account or subscription is involved, and
//! nothing about the user is sent — only the search.
//!
//! What it gives a third-party app, checked on 2026-09-30: title and mix,
//! artists, album, release date, ISRC, length and a 640 px sleeve. BPM, key and
//! genres are in the published schema but are left out of the answers at this
//! access tier; they are still read if they ever appear. It has no label field.
//!
//! The answers are JSON:API documents: resources refer to each other by type
//! and id, and the ones asked for with `include` arrive in a flat `included`
//! list. So they are read as `serde_json::Value` and joined here, rather than
//! modelled as nested structs they are not.

use crate::tracks::TrackMatch;
use crate::{agent, OnlineError, RateLimit};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SERVICE: &str = "Tidal";
pub const NAME: &str = "Tidal";
const TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const API: &str = "https://openapi.tidal.com/v2";

/// Catalogue region. Availability differs by country; the US catalogue is the
/// largest, and only metadata is read, never streamed.
const COUNTRY: &str = "US";

/// No published figure; this keeps one search (three requests) well under a
/// second without hammering anything.
static RATE: RateLimit = RateLimit::new(200);

/// An access token, kept until shortly before it expires.
///
/// Tied to the credentials it was issued for, so changing them in Settings
/// cannot carry on using a token from the old ones.
struct Token {
    credentials: String,
    value: String,
    expires: Instant,
}

static TOKEN: Mutex<Option<Token>> = Mutex::new(None);

/// Searches tracks by artist and title.
///
/// Two requests after the token: the search, which answers with track ids, and
/// the tracks themselves with their albums, artists, genres and cover art.
pub fn search_tracks(
    artist: &str,
    title: &str,
    limit: usize,
    client_id: &str,
    secret: &str,
) -> Result<Vec<TrackMatch>, OnlineError> {
    let (client_id, secret) = (client_id.trim(), secret.trim());
    if client_id.is_empty() || secret.is_empty() {
        return Err(OnlineError::NeedsToken { service: SERVICE });
    }
    let query = format!("{} {}", artist.trim(), title.trim());
    let query = query.trim();
    if query.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }

    let token = token(client_id, secret)?;

    let found = get(
        &format!("{API}/searchResults"),
        &[
            ("countryCode", COUNTRY),
            ("filter[query]", query),
            ("include", "tracks"),
        ],
        &token,
    )?;
    let ids = linked_ids(&found);
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut params = vec![
        ("countryCode", COUNTRY),
        ("include", "albums,artists,genres,albums.coverArt"),
    ];
    params.extend(ids.iter().take(limit).map(|id| ("filter[id]", id.as_str())));
    let body = get(&format!("{API}/tracks"), &params, &token)?;
    parse_tracks(&body, &ids)
}

/// A token for these credentials, from the cache or freshly issued.
fn token(client_id: &str, secret: &str) -> Result<String, OnlineError> {
    let credentials = format!("{client_id}:{secret}");
    {
        let cached = TOKEN.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = cached.as_ref() {
            if t.credentials == credentials && t.expires > Instant::now() {
                return Ok(t.value.clone());
            }
        }
    }

    RATE.wait();
    let response = agent()
        .post(TOKEN_URL)
        .header(
            "Authorization",
            format!("Basic {}", base64(credentials.as_bytes())),
        )
        .send_form([("grant_type", "client_credentials")]);

    let body = match response {
        Ok(mut r) => r
            .body_mut()
            .read_to_string()
            .map_err(|e| OnlineError::Malformed {
                service: SERVICE,
                reason: e.to_string(),
            })?,
        // Tidal answers a wrong ID or secret with 400 or 401.
        Err(ureq::Error::StatusCode(400 | 401 | 403)) => {
            return Err(OnlineError::BadToken { service: SERVICE })
        }
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

    let parsed: Value = serde_json::from_str(&body).map_err(|e| OnlineError::Malformed {
        service: SERVICE,
        reason: e.to_string(),
    })?;
    let value = parsed["access_token"]
        .as_str()
        .ok_or_else(|| OnlineError::Malformed {
            service: SERVICE,
            reason: "no access token in the answer".into(),
        })?
        .to_string();
    // A minute early, so a token never expires between being read here and
    // being used.
    let lifetime = parsed["expires_in"]
        .as_u64()
        .unwrap_or(3600)
        .saturating_sub(60);

    *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = Some(Token {
        credentials,
        value: value.clone(),
        expires: Instant::now() + Duration::from_secs(lifetime),
    });
    Ok(value)
}

fn get(url: &str, query: &[(&str, &str)], token: &str) -> Result<String, OnlineError> {
    RATE.wait();
    let mut request = agent()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.api+json");
    for (k, v) in query {
        request = request.query(*k, *v);
    }

    match request.call() {
        Ok(mut r) => r
            .body_mut()
            .read_to_string()
            .map_err(|e| OnlineError::Malformed {
                service: SERVICE,
                reason: e.to_string(),
            }),
        Err(ureq::Error::StatusCode(401)) => {
            // Revoked or expired early; the next search asks for a new one.
            *TOKEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Err(OnlineError::BadToken { service: SERVICE })
        }
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

/// The track ids a search answered with, best first.
///
/// The search is one `searchResults` resource whose `tracks` relationship lists
/// them in rank order.
fn linked_ids(body: &str) -> Vec<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v["data"][0]["relationships"]["tracks"]["data"]
                .as_array()
                .cloned()
        })
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r["id"].as_str().map(str::to_string))
        .collect()
}

/// Turns a `/tracks` document into matches, in the order the search ranked
/// them.
pub fn parse_tracks(body: &str, order: &[String]) -> Result<Vec<TrackMatch>, OnlineError> {
    let doc: Value = serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
        service: SERVICE,
        reason: e.to_string(),
    })?;

    let included: HashMap<(&str, &str), &Value> = doc["included"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some(((r["type"].as_str()?, r["id"].as_str()?), r)))
        .collect();
    let linked = |resource: &Value, relation: &str| -> Vec<&Value> {
        resource["relationships"][relation]["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| {
                included
                    .get(&(l["type"].as_str()?, l["id"].as_str()?))
                    .copied()
            })
            .collect()
    };

    let mut found: Vec<(usize, TrackMatch)> = doc["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|track| {
            let id = track["id"].as_str()?;
            let a = &track["attributes"];
            let name = a["title"].as_str()?.trim();
            let title = match a["version"].as_str().map(str::trim) {
                Some(v) if !v.is_empty() && !v.eq_ignore_ascii_case("original mix") => {
                    format!("{name} ({v})")
                }
                _ => name.to_string(),
            };
            let album = linked(track, "albums").into_iter().next();

            let m = TrackMatch {
                source: NAME,
                title,
                artists: linked(track, "artists")
                    .into_iter()
                    .filter_map(|r| text(&r["attributes"]["name"]))
                    .collect(),
                album: album.and_then(|r| text(&r["attributes"]["title"])),
                release_date: album.and_then(|r| text(&r["attributes"]["releaseDate"])),
                genres: linked(track, "genres")
                    .into_iter()
                    .filter_map(|r| text(&r["attributes"]["genreName"]))
                    .collect(),
                bpm: a["bpm"]
                    .as_f64()
                    .filter(|b| *b > 0.0)
                    .map(|b| b.round() as u32),
                key: key(
                    a["key"].as_str().unwrap_or(""),
                    a["keyScale"].as_str().unwrap_or(""),
                ),
                isrc: text(&a["isrc"]),
                duration_ms: a["duration"].as_str().and_then(iso_duration_ms),
                url: Some(format!("https://tidal.com/track/{id}")),
                art: album.and_then(|r| cover(&linked(r, "coverArt"))),
                ..Default::default()
            };
            let rank = order.iter().position(|o| o == id).unwrap_or(usize::MAX);
            Some((rank, m))
        })
        .collect();

    found.sort_by_key(|(rank, _)| *rank);
    Ok(found.into_iter().map(|(_, m)| m).collect())
}

/// A string attribute, trimmed; `None` when absent or blank.
fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The sleeve closest to 640 px, which is plenty to embed and small to fetch.
fn cover(artworks: &[&Value]) -> Option<String> {
    artworks
        .iter()
        .flat_map(|a| a["attributes"]["files"].as_array().into_iter().flatten())
        .filter_map(|f| {
            let href = f["href"].as_str()?;
            let width = f["meta"]["width"].as_i64().unwrap_or(0);
            Some(((width - 640).abs(), href.to_string()))
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, href)| href)
}

/// Tidal's key as the notation the rest of the app parses: `FSharp` + `MINOR`
/// is `F# Minor`. Modes other than major and minor are dropped, since no key
/// notation a DJ uses has a place for them.
fn key(note: &str, scale: &str) -> Option<String> {
    let note = match note {
        "" | "UNKNOWN" => return None,
        n => n.replace("Sharp", "#"),
    };
    let scale = match scale {
        "MAJOR" => "Major",
        "MINOR" => "Minor",
        _ => return None,
    };
    Some(format!("{note} {scale}"))
}

/// Milliseconds in an ISO 8601 duration such as `PT5M8S` or `PT1H2M3.5S`.
fn iso_duration_ms(text: &str) -> Option<u64> {
    let rest = text.strip_prefix("PT")?;
    let mut total = 0.0;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' | '.' => number.push(c),
            'H' | 'M' | 'S' => {
                let n: f64 = number.parse().ok()?;
                total += n * match c {
                    'H' => 3600.0,
                    'M' => 60.0,
                    _ => 1.0,
                };
                number.clear();
            }
            _ => return None,
        }
    }
    Some((total * 1000.0).round() as u64)
}

/// Standard base64, for the Basic authorization header.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape `/tracks?include=albums,artists,genres,albums.coverArt`
    /// answers with, trimmed.
    const TRACKS: &str = r#"{
      "data": [
        { "id": "2", "type": "tracks",
          "attributes": { "title": "Rev8617", "version": "Original Mix", "isrc": "DEU671800001",
            "duration": "PT6M2S", "bpm": 139.0, "key": "FSharp", "keyScale": "MINOR" },
          "relationships": {
            "albums": { "data": [{ "id": "20", "type": "albums" }] },
            "artists": { "data": [{ "id": "30", "type": "artists" }] },
            "genres": { "data": [{ "id": "40", "type": "genres" }] } } },
        { "id": "1", "type": "tracks",
          "attributes": { "title": "Compro", "version": "Call Super Remix",
            "duration": "PT5M8S", "key": "UNKNOWN", "keyScale": "UNKNOWN" },
          "relationships": { "artists": { "data": [{ "id": "30", "type": "artists" }] } } }
      ],
      "included": [
        { "id": "20", "type": "albums",
          "attributes": { "title": "Compro", "releaseDate": "2018-03-02" },
          "relationships": { "coverArt": { "data": [{ "id": "50", "type": "artworks" }] } } },
        { "id": "30", "type": "artists", "attributes": { "name": "Skee Mask" } },
        { "id": "40", "type": "genres", "attributes": { "genreName": "Electronic" } },
        { "id": "50", "type": "artworks", "attributes": { "files": [
          { "href": "https://resources.tidal.com/images/a/b/1280x1280.jpg", "meta": { "width": 1280, "height": 1280 } },
          { "href": "https://resources.tidal.com/images/a/b/640x640.jpg", "meta": { "width": 640, "height": 640 } },
          { "href": "https://resources.tidal.com/images/a/b/80x80.jpg", "meta": { "width": 80, "height": 80 } } ] } }
      ]
    }"#;

    #[test]
    fn tracks_are_joined_to_their_album_artists_genres_and_cover() {
        let found = parse_tracks(TRACKS, &["2".into(), "1".into()]).unwrap();
        let m = &found[0];
        assert_eq!(m.source, NAME);
        assert_eq!(m.title, "Rev8617", "an original mix is not worth a suffix");
        assert_eq!(m.artists, ["Skee Mask"]);
        assert_eq!(m.album.as_deref(), Some("Compro"));
        assert_eq!(m.release_date.as_deref(), Some("2018-03-02"));
        assert_eq!(m.genres, ["Electronic"]);
        assert_eq!(m.bpm, Some(139));
        assert_eq!(m.key.as_deref(), Some("F# Minor"));
        assert_eq!(m.isrc.as_deref(), Some("DEU671800001"));
        assert_eq!(m.duration_ms, Some(362_000));
        assert_eq!(
            m.art.as_deref(),
            Some("https://resources.tidal.com/images/a/b/640x640.jpg")
        );
        assert_eq!(m.url.as_deref(), Some("https://tidal.com/track/2"));
    }

    #[test]
    fn the_search_order_is_kept_and_missing_fields_stay_empty() {
        let found = parse_tracks(TRACKS, &["1".into(), "2".into()]).unwrap();
        assert_eq!(found[0].title, "Compro (Call Super Remix)");
        assert_eq!(found[0].key, None);
        assert_eq!(found[0].album, None);
        assert_eq!(found[0].art, None);
        assert_eq!(found[1].title, "Rev8617");
    }

    #[test]
    fn search_results_give_ids_best_first() {
        let body = r#"{"data":[{"id":"x","type":"searchResults",
            "relationships":{"tracks":{"data":[{"id":"9","type":"tracks"},{"id":"4","type":"tracks"}]}}}]}"#;
        assert_eq!(linked_ids(body), ["9", "4"]);
        assert!(linked_ids("{}").is_empty());
    }

    #[test]
    fn keys_and_durations_are_read() {
        assert_eq!(key("CSharp", "MAJOR").as_deref(), Some("C# Major"));
        assert_eq!(key("Eb", "MINOR").as_deref(), Some("Eb Minor"));
        assert_eq!(key("A", "DORIAN"), None);
        assert_eq!(key("UNKNOWN", "MINOR"), None);
        assert_eq!(iso_duration_ms("PT2M58S"), Some(178_000));
        assert_eq!(iso_duration_ms("PT1H2M3.5S"), Some(3_723_500));
        assert_eq!(iso_duration_ms("P1D"), None);
    }

    #[test]
    fn base64_matches_the_reference() {
        assert_eq!(base64(b"id:secret"), "aWQ6c2VjcmV0");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
    }

    #[test]
    fn missing_credentials_are_asked_for() {
        assert!(matches!(
            search_tracks("a", "b", 5, "", "").unwrap_err(),
            OnlineError::NeedsToken { .. }
        ));
    }
}
