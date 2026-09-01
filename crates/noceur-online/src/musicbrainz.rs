//! Finding a release in MusicBrainz.
//!
//! Only the release search is used, and only to get identifiers the Cover Art
//! Archive can be asked about. Nothing is written back, and no account or key is
//! involved — this is the public read-only web service.

use crate::{agent, OnlineError};
use serde::Deserialize;

const SERVICE: &str = "MusicBrainz";
const ENDPOINT: &str = "https://musicbrainz.org/ws/2/release";

/// Results one search asks for.
///
/// Enough to choose from without scrolling. The interesting candidates are
/// almost always in the first few — a release search ranks by how well the
/// artist and title matched, and a DJ looking for one track's cover is not going
/// to page through fifty pressings of the same record.
pub const LIMIT: usize = 12;

/// One release the search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// MusicBrainz identifier, and the key the Cover Art Archive is keyed by.
    pub id: String,
    pub title: String,
    pub artist: String,
    /// Release date as MusicBrainz gives it: a year, or a full date, or nothing.
    pub date: String,
    /// Two-letter country code, when the release names one.
    pub country: String,
    /// The label, when there is one — often the fastest way to tell two
    /// otherwise identical pressings apart.
    pub label: String,
    /// How well MusicBrainz thinks this matched, 0..=100.
    pub score: u32,
}

/// Searches releases by artist and title.
///
/// Both are matched as fields rather than concatenated into one query string, so
/// a track called "Remix" cannot match every release with "remix" in its name.
/// Either may be empty; both being empty is an error rather than a search that
/// would return an arbitrary slice of the database.
pub fn search_releases(artist: &str, title: &str) -> Result<Vec<Release>, OnlineError> {
    let artist = artist.trim();
    let title = title.trim();
    if artist.is_empty() && title.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }

    let mut terms = Vec::new();
    if !artist.is_empty() {
        terms.push(format!("artist:{}", lucene_escape(artist)));
    }
    if !title.is_empty() {
        // Matched against both the release title and the track title: a DJ
        // library names files after the track, and the track is very often not
        // the name of the record it came out on.
        terms.push(format!(
            "(release:{t} OR recording:{t})",
            t = lucene_escape(title)
        ));
    }
    let query = terms.join(" AND ");

    let response = agent()
        .get(ENDPOINT)
        .query("query", &query)
        .query("fmt", "json")
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

/// Turns a search response into releases.
///
/// Separate from the request so the shape of the answer can be tested against
/// recorded responses without touching the network — which also means a change
/// in the service's format is caught by a test rather than by a user.
pub fn parse(body: &str) -> Result<Vec<Release>, OnlineError> {
    let parsed: SearchResponse =
        serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
            service: SERVICE,
            reason: e.to_string(),
        })?;

    Ok(parsed
        .releases
        .into_iter()
        .map(|r| Release {
            id: r.id,
            title: r.title.unwrap_or_default(),
            artist: r
                .artist_credit
                .unwrap_or_default()
                .into_iter()
                .map(|c| c.name)
                .collect::<Vec<_>>()
                .join(", "),
            date: r.date.unwrap_or_default(),
            country: r.country.unwrap_or_default(),
            label: r
                .label_info
                .unwrap_or_default()
                .into_iter()
                .filter_map(|l| l.label.and_then(|l| l.name))
                .next()
                .unwrap_or_default(),
            score: r.score.unwrap_or(0),
        })
        .collect())
}

/// Escapes a user's text for a Lucene query.
///
/// The search field is a query language, not a string: an unescaped `:` or `-`
/// in a track title silently changes what is being asked for, and a stray quote
/// makes the whole query invalid. Wrapping in quotes and escaping the quote
/// character keeps a title like `Nothing's Left Besides Iron & Chef` a title.
fn lucene_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    releases: Vec<RawRelease>,
}

#[derive(Deserialize)]
struct RawRelease {
    id: String,
    title: Option<String>,
    date: Option<String>,
    country: Option<String>,
    score: Option<u32>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<ArtistCredit>>,
    #[serde(rename = "label-info")]
    label_info: Option<Vec<LabelInfo>>,
}

#[derive(Deserialize)]
struct ArtistCredit {
    name: String,
}

#[derive(Deserialize)]
struct LabelInfo {
    label: Option<Label>,
}

#[derive(Deserialize)]
struct Label {
    name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed-down copy of a real response, kept because the parser has to
    /// survive fields being absent — which they routinely are.
    const SAMPLE: &str = r#"{
      "count": 2,
      "releases": [
        {
          "id": "aaaaaaaa-1111-2222-3333-444444444444",
          "score": 100,
          "title": "Ostgut Ton Zehn",
          "date": "2015-05-11",
          "country": "DE",
          "artist-credit": [{ "name": "Various Artists" }],
          "label-info": [{ "label": { "name": "Ostgut Ton" } }]
        },
        {
          "id": "bbbbbbbb-1111-2222-3333-444444444444",
          "score": 71,
          "title": "Untitled"
        }
      ]
    }"#;

    #[test]
    fn parses_a_release_with_everything_present() {
        let releases = parse(SAMPLE).unwrap();
        assert_eq!(releases.len(), 2);
        let first = &releases[0];
        assert_eq!(first.id, "aaaaaaaa-1111-2222-3333-444444444444");
        assert_eq!(first.title, "Ostgut Ton Zehn");
        assert_eq!(first.artist, "Various Artists");
        assert_eq!(first.date, "2015-05-11");
        assert_eq!(first.country, "DE");
        assert_eq!(first.label, "Ostgut Ton");
        assert_eq!(first.score, 100);
    }

    /// Most releases in the database are missing most of these fields. A parser
    /// that needed them would return nothing for half the searches.
    #[test]
    fn a_release_missing_everything_optional_still_parses() {
        let releases = parse(SAMPLE).unwrap();
        let second = &releases[1];
        assert_eq!(second.title, "Untitled");
        assert_eq!(second.artist, "");
        assert_eq!(second.date, "");
        assert_eq!(second.label, "");
    }

    #[test]
    fn an_empty_result_set_is_not_an_error() {
        assert_eq!(parse(r#"{"count":0,"releases":[]}"#).unwrap(), vec![]);
        assert_eq!(parse(r#"{"count":0}"#).unwrap(), vec![]);
    }

    #[test]
    fn nonsense_is_reported_rather_than_swallowed() {
        assert!(matches!(
            parse("not json").unwrap_err(),
            OnlineError::Malformed { .. }
        ));
    }

    /// Several artists on one release read as one credit line, which is what the
    /// picker shows under the cover.
    #[test]
    fn multiple_artist_credits_are_joined() {
        let body = r#"{"releases":[{"id":"x","title":"Split",
            "artist-credit":[{"name":"Skee Mask"},{"name":"Zenker Brothers"}]}]}"#;
        assert_eq!(parse(body).unwrap()[0].artist, "Skee Mask, Zenker Brothers");
    }

    /// The search field is a query language. A title carrying its own quote or a
    /// colon would otherwise change what is being asked, or make it invalid.
    #[test]
    fn a_title_is_quoted_and_its_own_quotes_escaped() {
        assert_eq!(lucene_escape(r#"Nothing's Left"#), r#""Nothing's Left""#);
        assert_eq!(lucene_escape(r#"He said "no""#), r#""He said \"no\"""#);
        assert_eq!(lucene_escape("A:B - C"), r#""A:B - C""#);
    }

    #[test]
    fn searching_for_nothing_is_refused() {
        assert!(matches!(
            search_releases("  ", "").unwrap_err(),
            OnlineError::NothingToSearch
        ));
    }
}
