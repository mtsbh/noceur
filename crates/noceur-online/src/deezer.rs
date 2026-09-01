//! Cover art from Deezer's public API.
//!
//! No key and no account. Worth having alongside the other two because its
//! catalogue is built from the same distributors that supply beatport-adjacent
//! dance music, so it often has a sleeve for a twelve-inch that Apple never
//! carried and nobody got round to adding to MusicBrainz.

use crate::sources::Candidate;
use crate::{agent, OnlineError};
use serde::Deserialize;

const SERVICE: &str = "Deezer";
pub const NAME: &str = "Deezer";
const ENDPOINT: &str = "https://api.deezer.com/search/album";

const LIMIT: usize = 10;

/// Searches albums by artist and title.
pub fn search(artist: &str, album: &str) -> Result<Vec<Candidate>, OnlineError> {
    let query = format!("{artist} {album}");
    let query = query.trim();
    if query.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }

    let response = agent()
        .get(ENDPOINT)
        .query("q", query)
        .query("limit", LIMIT.to_string())
        .call();

    let body = match response {
        Ok(mut r) if r.status().is_success() => {
            r.body_mut()
                .read_to_string()
                .map_err(|e| OnlineError::Malformed {
                    service: SERVICE,
                    reason: e.to_string(),
                })?
        }
        Ok(r) => {
            return Err(OnlineError::Status {
                service: SERVICE,
                status: r.status().as_u16(),
            })
        }
        Err(e) => {
            return Err(OnlineError::Unreachable {
                service: SERVICE,
                reason: e.to_string(),
            })
        }
    };

    parse(&body)
}

/// Turns a response into candidates.
pub fn parse(body: &str) -> Result<Vec<Candidate>, OnlineError> {
    let parsed: SearchResponse =
        serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
            service: SERVICE,
            reason: e.to_string(),
        })?;

    Ok(parsed
        .data
        .into_iter()
        .filter_map(|a| {
            // `cover_big` is 500px, which is the size worth embedding. Falling
            // back to the plain `cover` rather than skipping the album: a
            // smaller sleeve is still a sleeve.
            let full = a.cover_big.clone().or_else(|| a.cover.clone())?;
            let thumbnail = a.cover_medium.clone().or_else(|| a.cover.clone())?;
            Some(Candidate {
                source: NAME,
                title: a.title.unwrap_or_default(),
                artist: a.artist.and_then(|x| x.name).unwrap_or_default(),
                detail: a
                    .nb_tracks
                    .map(|n| format!("{n} tracks"))
                    .unwrap_or_default(),
                thumbnail,
                full,
                size: a.cover_big.map(|_| 500),
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<RawAlbum>,
}

#[derive(Deserialize)]
struct RawAlbum {
    title: Option<String>,
    cover: Option<String>,
    cover_medium: Option<String>,
    cover_big: Option<String>,
    nb_tracks: Option<u32>,
    artist: Option<RawArtist>,
}

#[derive(Deserialize)]
struct RawArtist {
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "data": [
        {
          "id": 1,
          "title": "Compro",
          "cover": "https://e-cdns-images.dzcdn.net/images/cover/x/120x120.jpg",
          "cover_medium": "https://e-cdns-images.dzcdn.net/images/cover/x/250x250.jpg",
          "cover_big": "https://e-cdns-images.dzcdn.net/images/cover/x/500x500.jpg",
          "nb_tracks": 12,
          "artist": { "name": "Skee Mask" }
        },
        { "id": 2, "title": "No Cover" }
      ]
    }"#;

    #[test]
    fn parses_an_album_and_prefers_the_larger_cover() {
        let found = parse(SAMPLE).unwrap();
        assert_eq!(found.len(), 1, "an album with no cover is not a candidate");
        let c = &found[0];
        assert_eq!(c.source, NAME);
        assert_eq!(c.title, "Compro");
        assert_eq!(c.artist, "Skee Mask");
        assert_eq!(c.detail, "12 tracks");
        assert!(c.full.ends_with("500x500.jpg"));
        assert!(c.thumbnail.ends_with("250x250.jpg"));
        assert_eq!(c.size, Some(500));
    }

    /// A smaller sleeve is still a sleeve — dropping the album because only the
    /// plain `cover` field was present would lose covers for no reason.
    #[test]
    fn an_album_with_only_the_small_cover_is_still_offered() {
        let body = r#"{"data":[{"id":3,"title":"Small",
            "cover":"https://e-cdns-images.dzcdn.net/images/cover/y/120x120.jpg"}]}"#;
        let found = parse(body).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, None, "the size is only claimed when known");
    }

    #[test]
    fn an_empty_result_set_is_not_an_error() {
        assert!(parse(r#"{"data":[]}"#).unwrap().is_empty());
        assert!(parse("{}").unwrap().is_empty());
    }

    #[test]
    fn nonsense_is_reported() {
        assert!(matches!(
            parse("<html>").unwrap_err(),
            OnlineError::Malformed { .. }
        ));
    }
}
