//! Cover art from Apple's public search API.
//!
//! No key and no account: this is the same endpoint the iTunes affiliate tools
//! use. Coverage is excellent for anything that was released commercially, which
//! makes it a good complement to MusicBrainz — the archive's strength is
//! obscure and independent records, and Apple's is everything with a
//! distributor.

use crate::sources::Candidate;
use crate::{agent, OnlineError};
use serde::Deserialize;

const SERVICE: &str = "iTunes";
pub const NAME: &str = "iTunes";
const ENDPOINT: &str = "https://itunes.apple.com/search";

/// Results to ask for. The endpoint ranks by relevance and the useful matches
/// are at the top; more than this is a longer grid to read, not a better one.
const LIMIT: usize = 10;

/// Searches albums by artist and title.
pub fn search(artist: &str, album: &str) -> Result<Vec<Candidate>, OnlineError> {
    let term = format!("{artist} {album}");
    let term = term.trim();
    if term.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }

    let response = agent()
        .get(ENDPOINT)
        .query("term", term)
        .query("entity", "album")
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
///
/// Separate from the request so the shape can be tested against a recorded body
/// — a change in the service's format should be caught here rather than by
/// someone whose covers quietly stopped appearing.
pub fn parse(body: &str) -> Result<Vec<Candidate>, OnlineError> {
    let parsed: SearchResponse =
        serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
            service: SERVICE,
            reason: e.to_string(),
        })?;

    Ok(parsed
        .results
        .into_iter()
        .filter_map(|r| {
            let art = r.artwork_url100.or(r.artwork_url60)?;
            Some(Candidate {
                source: NAME,
                title: r.collection_name.unwrap_or_default(),
                artist: r.artist_name.unwrap_or_default(),
                detail: [
                    r.release_date
                        .as_deref()
                        .unwrap_or("")
                        .get(0..4)
                        .unwrap_or(""),
                    r.copyright_country.as_deref().unwrap_or(""),
                ]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" · "),
                thumbnail: resize(&art, 250),
                full: resize(&art, 600),
                size: Some(600),
            })
        })
        .collect())
}

/// Rewrites an artwork URL to a different rendering.
///
/// Apple encodes the size into the filename — `.../100x100bb.jpg` — and serves
/// whatever is asked for, so a 600px cover costs nothing extra to request. The
/// trailing `bb` is the crop style, and is left alone.
fn resize(url: &str, px: u32) -> String {
    let Some(slash) = url.rfind('/') else {
        return url.to_string();
    };
    let (base, file) = url.split_at(slash + 1);

    // The filename is `<w>x<h><style>.<ext>`; only the dimensions are replaced,
    // so an unfamiliar style or extension passes through untouched.
    let Some(x) = file.find('x') else {
        return url.to_string();
    };
    if !file[..x].chars().all(|c| c.is_ascii_digit()) || x == 0 {
        return url.to_string();
    }
    let rest = &file[x + 1..];
    let digits = rest
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .count();
    if digits == 0 {
        return url.to_string();
    }
    format!("{base}{px}x{px}{}", &rest[digits..])
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<RawAlbum>,
}

#[derive(Deserialize)]
struct RawAlbum {
    #[serde(rename = "collectionName")]
    collection_name: Option<String>,
    #[serde(rename = "artistName")]
    artist_name: Option<String>,
    #[serde(rename = "releaseDate")]
    release_date: Option<String>,
    #[serde(rename = "country")]
    copyright_country: Option<String>,
    #[serde(rename = "artworkUrl100")]
    artwork_url100: Option<String>,
    #[serde(rename = "artworkUrl60")]
    artwork_url60: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "resultCount": 2,
      "results": [
        {
          "collectionName": "Compro",
          "artistName": "Skee Mask",
          "releaseDate": "2018-05-15T07:00:00Z",
          "country": "USA",
          "artworkUrl60": "https://is1-ssl.mzstatic.com/image/thumb/Music/x/60x60bb.jpg",
          "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music/x/100x100bb.jpg"
        },
        { "collectionName": "No Art Here", "artistName": "Nobody" }
      ]
    }"#;

    #[test]
    fn parses_an_album_and_asks_for_a_larger_rendering() {
        let found = parse(SAMPLE).unwrap();
        assert_eq!(
            found.len(),
            1,
            "the album with no artwork is not a candidate"
        );
        let c = &found[0];
        assert_eq!(c.source, NAME);
        assert_eq!(c.title, "Compro");
        assert_eq!(c.artist, "Skee Mask");
        assert_eq!(c.detail, "2018 · USA");
        assert!(c.thumbnail.ends_with("/250x250bb.jpg"), "{}", c.thumbnail);
        assert!(c.full.ends_with("/600x600bb.jpg"), "{}", c.full);
    }

    /// The size sits in the filename, so rewriting it is how a bigger cover is
    /// requested. Getting this wrong silently embeds a 100px thumbnail.
    #[test]
    fn resizing_replaces_only_the_dimensions() {
        let url = "https://is1-ssl.mzstatic.com/image/thumb/a/b/100x100bb.jpg";
        assert_eq!(
            resize(url, 600),
            "https://is1-ssl.mzstatic.com/image/thumb/a/b/600x600bb.jpg"
        );
        // An unfamiliar crop style and extension survive.
        let odd = "https://is1-ssl.mzstatic.com/image/thumb/a/170x170bb-60.png";
        assert_eq!(
            resize(odd, 600),
            "https://is1-ssl.mzstatic.com/image/thumb/a/600x600bb-60.png"
        );
    }

    /// A URL that is not shaped the way Apple's are is left alone rather than
    /// mangled into something that will not load.
    #[test]
    fn an_unexpected_url_is_returned_unchanged() {
        for url in [
            "https://example.com/cover.jpg",
            "https://example.com/",
            "notaurl",
        ] {
            assert_eq!(resize(url, 600), url);
        }
    }

    #[test]
    fn an_empty_result_set_is_not_an_error() {
        assert!(parse(r#"{"resultCount":0,"results":[]}"#)
            .unwrap()
            .is_empty());
        assert!(parse(r#"{"resultCount":0}"#).unwrap().is_empty());
    }

    #[test]
    fn nonsense_is_reported() {
        assert!(matches!(
            parse("<html>").unwrap_err(),
            OnlineError::Malformed { .. }
        ));
    }
}
