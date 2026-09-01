//! Metadata from Discogs.
//!
//! The one source here that needs a credential: Discogs requires a personal
//! access token, which the user generates on their own account and pastes into
//! settings. It is worth the friction — Discogs is the reference database for
//! records, and it carries the two fields a DJ library most often wants and
//! nobody else reliably has: the label and the catalogue number.
//!
//! The token is passed in rather than stored here. This crate makes requests; it
//! has no business owning a credential, and keeping it out means nothing in the
//! network layer can accidentally log or send it somewhere it does not belong.

use crate::tracks::TrackMatch;
use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

const SERVICE: &str = "Discogs";
pub const NAME: &str = "Discogs";
const SEARCH: &str = "https://api.discogs.com/database/search";

/// Their published limit for an authenticated client is sixty a minute.
static RATE: RateLimit = RateLimit::new(1050);

/// Searches releases containing a track.
///
/// Searches on the `track` field rather than the release title, because that is
/// the question being asked: which record is this track on? Discogs answers with
/// releases, so what comes back describes the record — the exact track title and
/// its position need [`enrich`].
pub fn search_tracks(
    artist: &str,
    title: &str,
    limit: usize,
    token: &str,
) -> Result<Vec<TrackMatch>, OnlineError> {
    let artist = artist.trim();
    let title = title.trim();
    if title.is_empty() && artist.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }
    if token.trim().is_empty() {
        return Err(OnlineError::NeedsToken { service: SERVICE });
    }

    RATE.wait();
    let mut request = agent()
        .get(SEARCH)
        .header("Authorization", &format!("Discogs token={}", token.trim()))
        .query("type", "release")
        .query("per_page", limit.max(1).to_string());
    if !artist.is_empty() {
        request = request.query("artist", artist);
    }
    if !title.is_empty() {
        request = request.query("track", title);
    }

    let body = match request.call() {
        Ok(mut r) if r.status().is_success() => {
            r.body_mut()
                .read_to_string()
                .map_err(|e| OnlineError::Malformed {
                    service: SERVICE,
                    reason: e.to_string(),
                })?
        }
        // Discogs answers 401 for a token it does not recognise. Saying so is
        // far more use than "the service returned an error".
        Ok(r) if r.status().as_u16() == 401 => {
            return Err(OnlineError::BadToken { service: SERVICE })
        }
        Ok(r) => {
            return Err(OnlineError::Status {
                service: SERVICE,
                status: r.status().as_u16(),
            })
        }
        Err(ureq::Error::StatusCode(401)) => {
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

    parse_search(&body, title)
}

/// Turns a search response into matches.
///
/// `searched_title` fills the track title, because a release search result names
/// the record rather than the track on it. Discogs matched the release *on* that
/// track, so it is an honest placeholder until [`enrich`] reads the real
/// tracklist — and it keeps the scoring meaningful in the meantime.
pub fn parse_search(body: &str, searched_title: &str) -> Result<Vec<TrackMatch>, OnlineError> {
    let parsed: SearchResponse =
        serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
            service: SERVICE,
            reason: e.to_string(),
        })?;

    Ok(parsed
        .results
        .into_iter()
        .map(|r| {
            let full = r.title.unwrap_or_default();
            // Search results name a release as `Artist - Title`. Splitting gives
            // back the two fields, and a release whose own name contains a dash
            // keeps it: only the first separator is used.
            let (artist, album) = match full.split_once(" - ") {
                Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
                None => (String::new(), full.clone()),
            };

            TrackMatch {
                source: NAME,
                title: searched_title.to_string(),
                artists: if artist.is_empty() {
                    Vec::new()
                } else {
                    // Discogs disambiguates duplicate artist names with a
                    // numeric suffix, as in `Rrose (2)`. It is a database
                    // artefact, not part of anybody's name.
                    vec![strip_disambiguator(&artist)]
                },
                album: Some(album).filter(|s| !s.is_empty()),
                label: r.label.unwrap_or_default().into_iter().next(),
                catalog_number: r.catno.filter(|c| !c.is_empty() && c != "none"),
                release_date: r.year.filter(|y| !y.is_empty()),
                // Discogs splits broad genres from specific styles, and the
                // styles are the useful half: "Techno" says more than
                // "Electronic". Styles first for that reason.
                genres: r
                    .style
                    .unwrap_or_default()
                    .into_iter()
                    .chain(r.genre.unwrap_or_default())
                    .collect(),
                art: r.cover_image.filter(|u| !u.is_empty()),
                url: r.uri.map(|u| {
                    if u.starts_with("http") {
                        u
                    } else {
                        format!("https://www.discogs.com{u}")
                    }
                }),
                ..Default::default()
            }
        })
        .collect())
}

/// Removes Discogs' `(2)` style disambiguator from an artist name.
fn strip_disambiguator(name: &str) -> String {
    let trimmed = name.trim_end();
    let Some(open) = trimmed.rfind(" (") else {
        return trimmed.to_string();
    };
    let inside = &trimmed[open + 2..];
    if inside.ends_with(')')
        && inside[..inside.len() - 1]
            .chars()
            .all(|c| c.is_ascii_digit())
    {
        trimmed[..open].to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<RawResult>,
}

#[derive(Deserialize)]
struct RawResult {
    title: Option<String>,
    year: Option<String>,
    label: Option<Vec<String>>,
    catno: Option<String>,
    genre: Option<Vec<String>>,
    style: Option<Vec<String>>,
    cover_image: Option<String>,
    uri: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "results": [
        {
          "id": 12345,
          "title": "Skee Mask - Compro",
          "year": "2018",
          "country": "Germany",
          "label": ["Ilian Tape", "Ilian Tape"],
          "catno": "ITLP07",
          "genre": ["Electronic"],
          "style": ["Techno", "Breakbeat"],
          "cover_image": "https://i.discogs.com/x/rx-600.jpeg",
          "uri": "/release/12345-Skee-Mask-Compro"
        },
        { "id": 2, "title": "NoSeparator", "catno": "none" }
      ]
    }"#;

    #[test]
    fn parses_the_fields_that_are_the_reason_to_ask_discogs() {
        let found = parse_search(SAMPLE, "Rev8617").unwrap();
        let m = &found[0];
        assert_eq!(m.source, NAME);
        assert_eq!(m.title, "Rev8617", "the searched track names the match");
        assert_eq!(m.artists, vec!["Skee Mask".to_string()]);
        assert_eq!(m.album.as_deref(), Some("Compro"));
        assert_eq!(m.label.as_deref(), Some("Ilian Tape"));
        assert_eq!(m.catalog_number.as_deref(), Some("ITLP07"));
        assert_eq!(m.release_date.as_deref(), Some("2018"));
    }

    /// Styles are the useful half — "Techno" says more than "Electronic" — so
    /// they come first where a tag will only show the start of the list.
    #[test]
    fn styles_come_before_broad_genres() {
        let found = parse_search(SAMPLE, "x").unwrap();
        assert_eq!(found[0].genres, vec!["Techno", "Breakbeat", "Electronic"]);
    }

    /// `none` is Discogs' literal string for a release with no catalogue
    /// number, and writing it into a tag would be worse than leaving it empty.
    #[test]
    fn a_missing_catalogue_number_is_not_the_word_none() {
        let found = parse_search(SAMPLE, "x").unwrap();
        assert_eq!(found[1].catalog_number, None);
    }

    #[test]
    fn a_release_title_with_no_artist_separator_is_all_album() {
        let found = parse_search(SAMPLE, "x").unwrap();
        assert_eq!(found[1].album.as_deref(), Some("NoSeparator"));
        assert!(found[1].artists.is_empty());
    }

    /// Only the first separator splits, so a record actually called
    /// `Something - Something` keeps its name.
    #[test]
    fn only_the_first_separator_splits_artist_from_release() {
        let body = r#"{"results":[{"id":1,"title":"Artist - A - B"}]}"#;
        let found = parse_search(body, "x").unwrap();
        assert_eq!(found[0].artists, vec!["Artist".to_string()]);
        assert_eq!(found[0].album.as_deref(), Some("A - B"));
    }

    /// `Rrose (2)` is a database artefact for telling two artists of the same
    /// name apart. Nobody's tags should carry it.
    #[test]
    fn the_duplicate_name_disambiguator_is_removed() {
        assert_eq!(strip_disambiguator("Rrose (2)"), "Rrose");
        assert_eq!(strip_disambiguator("Aphex Twin"), "Aphex Twin");
        // A real parenthetical is not a disambiguator.
        assert_eq!(
            strip_disambiguator("Underground Resistance (UR)"),
            "Underground Resistance (UR)"
        );
    }

    #[test]
    fn searching_without_a_token_says_so() {
        assert!(matches!(
            search_tracks("a", "b", 5, "   ").unwrap_err(),
            OnlineError::NeedsToken { .. }
        ));
    }

    #[test]
    fn an_empty_result_set_is_not_an_error() {
        assert!(parse_search(r#"{"results":[]}"#, "x").unwrap().is_empty());
        assert!(parse_search("{}", "x").unwrap().is_empty());
    }
}
