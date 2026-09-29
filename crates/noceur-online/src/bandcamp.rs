//! Tracks from Bandcamp.
//!
//! Where the records the other catalogues never got live: self-released EPs,
//! small labels that sell direct, anything that went up the week it was
//! finished. No key and no account.
//!
//! Two steps. The search is the site's own autocomplete endpoint, which answers
//! with a title, an artist, an album and a link. Everything else — release
//! date, ISRC, track number, the label — is on the track page, as the JSON-LD
//! block Bandcamp publishes for search engines. Reading that is reading a
//! format meant to be read, so it moves less than the page around it.
//!
//! Neither is a published API, so both fail soft the way Beatport does: a
//! response that no longer looks the same is no results, not an error.

use crate::tracks::TrackMatch;
use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

const SERVICE: &str = "Bandcamp";
pub const NAME: &str = "Bandcamp";
const ENDPOINT: &str = "https://bandcamp.com/api/bcsearch_public_api/1/autocomplete_elastic";

/// Search and track pages share one budget: they are the same site.
static RATE: RateLimit = RateLimit::new(1000);

/// Searches tracks by artist and title.
pub fn search_tracks(
    artist: &str,
    title: &str,
    limit: usize,
) -> Result<Vec<TrackMatch>, OnlineError> {
    let query = format!("{artist} {title}");
    let query = query.trim();
    if query.is_empty() {
        return Err(OnlineError::NothingToSearch);
    }

    // `t` limits the search to tracks; albums and artists would only have to
    // be thrown away.
    let request = serde_json::json!({
        "search_text": query,
        "search_filter": "t",
        "full_page": false,
        "fan_id": null,
    })
    .to_string();

    RATE.wait();
    let response = agent()
        .post(ENDPOINT)
        .header("Content-Type", "application/json")
        .send(&request);
    let body = read(response)?;

    let mut found = parse_search(&body);
    found.truncate(limit.max(1));
    Ok(found)
}

/// Turns an autocomplete response into matches.
pub fn parse_search(body: &str) -> Vec<TrackMatch> {
    let Ok(parsed) = serde_json::from_str::<SearchResponse>(body) else {
        return Vec::new();
    };

    parsed
        .auto
        .results
        .into_iter()
        .filter(|r| r.kind.as_deref() == Some("t"))
        .filter_map(|r| {
            let title = text(r.name)?;
            Some(TrackMatch {
                source: NAME,
                title,
                artists: text(r.band_name).into_iter().collect(),
                album: text(r.album_name),
                url: text(r.item_url_path).filter(|u| is_bandcamp(u)),
                // `a<id>_10` is the sleeve at 1200 px. The `img` the search
                // hands back is the 100 px thumbnail of the same picture.
                art: r
                    .art_id
                    .map(|id| format!("https://f4.bcbits.com/img/a{id:010}_10.jpg")),
                ..Default::default()
            })
        })
        .collect()
}

/// Fills in what the search leaves out, from the track's own page.
///
/// Separate for the same reason as Deezer's: it is one request per result, so
/// only the results the user might take are worth it.
pub fn enrich(track: &mut TrackMatch) -> Result<(), OnlineError> {
    // The URL came from a search, but only a Bandcamp page is ever fetched
    // from here, whatever is in the match.
    let Some(url) = track.url.clone().filter(|u| is_bandcamp(u)) else {
        return Ok(());
    };
    RATE.wait();
    let page = read(agent().get(&url).call())?;
    apply_page(track, &page);
    Ok(())
}

/// Copies what a track page knows into a match, without overwriting.
pub fn apply_page(track: &mut TrackMatch, page: &str) {
    let Some(ld) = json_ld(page) else {
        return;
    };

    if track.isrc.is_none() {
        track.isrc = text(ld.isrc_code);
    }
    if track.duration_ms.is_none() {
        track.duration_ms = ld.duration.as_deref().and_then(iso_duration_ms);
    }
    if track.release_date.is_none() {
        track.release_date = ld.date_published.as_deref().and_then(iso_date);
    }
    if track.track_no.is_none() {
        track.track_no = ld
            .additional_property
            .iter()
            .find(|p| p.name.as_deref() == Some("tracknum"))
            .and_then(|p| p.value.as_ref())
            .and_then(|v| v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n > 0);
    }

    // The page belongs to whoever sells the record. On a label's account that
    // is the label; on an artist's own page it is the artist again, which is
    // not a label and is not offered as one.
    if track.label.is_none() {
        let seller = ld.publisher.and_then(|p| text(p.name));
        let artist = track.artist();
        track.label = seller.filter(|s| !s.eq_ignore_ascii_case(artist.trim()));
    }
}

/// The first `application/ld+json` block that describes a recording.
fn json_ld(page: &str) -> Option<RawRecording> {
    const OPEN: &str = r#"<script type="application/ld+json">"#;
    let mut rest = page;
    while let Some(at) = rest.find(OPEN) {
        let body = &rest[at + OPEN.len()..];
        let end = body.find("</script>")?;
        if let Ok(r) = serde_json::from_str::<RawRecording>(&body[..end]) {
            if r.kind.as_deref() == Some("MusicRecording") {
                return Some(r);
            }
        }
        rest = &body[end..];
    }
    None
}

/// `P00H03M44S` in milliseconds.
fn iso_duration_ms(s: &str) -> Option<u64> {
    let rest = s.strip_prefix('P')?.trim_start_matches('T');
    let mut total = 0u64;
    let mut number = String::new();
    for c in rest.chars() {
        match c {
            '0'..='9' => number.push(c),
            'H' | 'M' | 'S' => {
                let n: u64 = number.parse().ok()?;
                number.clear();
                total += n * match c {
                    'H' => 3_600_000,
                    'M' => 60_000,
                    _ => 1_000,
                };
            }
            'T' => {}
            _ => return None,
        }
    }
    (total > 0).then_some(total)
}

/// `15 May 2018 00:00:00 GMT` as `2018-05-15`, the form every other source uses.
fn iso_date(s: &str) -> Option<String> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let mut parts = s.split_whitespace();
    let day: u32 = parts.next()?.parse().ok()?;
    let month = parts.next()?.get(..3)?.to_ascii_lowercase();
    let month = MONTHS.iter().position(|m| *m == month)? + 1;
    let year: u32 = parts.next()?.parse().ok()?;
    ((1..=31).contains(&day) && year > 1000).then(|| format!("{year:04}-{month:02}-{day:02}"))
}

/// Whether a link is a page on Bandcamp, including an artist's own subdomain.
fn is_bandcamp(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    // The same order as `sources::is_allowed`: the authority, then whatever
    // follows credentials, then the host before a port. Cutting at `@` first
    // would read `https://bandcamp.com@evil.example/` as Bandcamp.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    host == "bandcamp.com" || host.ends_with(".bandcamp.com")
}

fn text(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// One request's body, with the service's errors mapped.
fn read(
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<String, OnlineError> {
    match response {
        Ok(mut r) if r.status().is_success() => {
            r.body_mut()
                .read_to_string()
                .map_err(|e| OnlineError::Malformed {
                    service: SERVICE,
                    reason: e.to_string(),
                })
        }
        Ok(r) => Err(OnlineError::Status {
            service: SERVICE,
            status: r.status().as_u16(),
        }),
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

#[derive(Deserialize)]
struct SearchResponse {
    auto: RawAuto,
}

#[derive(Deserialize)]
struct RawAuto {
    #[serde(default)]
    results: Vec<RawResult>,
}

#[derive(Deserialize)]
struct RawResult {
    #[serde(rename = "type")]
    kind: Option<String>,
    name: Option<String>,
    band_name: Option<String>,
    album_name: Option<String>,
    item_url_path: Option<String>,
    art_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawRecording {
    #[serde(rename = "@type")]
    kind: Option<String>,
    #[serde(rename = "isrcCode")]
    isrc_code: Option<String>,
    duration: Option<String>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
    publisher: Option<RawPublisher>,
    #[serde(rename = "additionalProperty", default)]
    additional_property: Vec<RawProperty>,
}

#[derive(Deserialize)]
struct RawPublisher {
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawProperty {
    name: Option<String>,
    value: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real autocomplete answer for "Skee Mask Rev8617".
    const SEARCH: &str = include_str!("../tests/fixtures/bandcamp_search.json");
    /// The real track page, cut down to its JSON-LD block.
    const PAGE: &str = include_str!("../tests/fixtures/bandcamp_track.html");

    #[test]
    fn a_search_result_becomes_a_match() {
        let found = parse_search(SEARCH);
        assert_eq!(found.len(), 1);
        let t = &found[0];
        assert_eq!(t.source, NAME);
        assert_eq!(t.title, "Rev8617");
        assert_eq!(t.artists, ["Skee Mask"]);
        assert_eq!(t.album.as_deref(), Some("ITLP04 - Compro"));
        assert_eq!(
            t.url.as_deref(),
            Some("https://iliantape.bandcamp.com/track/rev8617")
        );
        assert_eq!(
            t.art.as_deref(),
            Some("https://f4.bcbits.com/img/a4230057167_10.jpg")
        );
        assert!(crate::sources::is_allowed(t.art.as_deref().unwrap()));
    }

    /// Short ids are zero-padded in Bandcamp's image URLs.
    #[test]
    fn a_short_art_id_is_padded_the_way_bandcamp_writes_it() {
        let body = r#"{"auto":{"results":[{"type":"t","name":"X","art_id":24824945}]}}"#;
        assert_eq!(
            parse_search(body)[0].art.as_deref(),
            Some("https://f4.bcbits.com/img/a0024824945_10.jpg")
        );
    }

    #[test]
    fn only_tracks_are_kept() {
        let body = r#"{"auto":{"results":[
            {"type":"a","name":"An Album"},
            {"type":"b","name":"An Artist"},
            {"type":"t","name":"A Track","band_name":"Someone"}]}}"#;
        let titles: Vec<String> = parse_search(body).into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["A Track"]);
    }

    /// A link that is not Bandcamp's is never kept, so `enrich` can never be
    /// pointed somewhere else by what a search returned.
    #[test]
    fn a_link_off_bandcamp_is_dropped() {
        let body = r#"{"auto":{"results":[{"type":"t","name":"X",
            "item_url_path":"https://evil.example/track/x"}]}}"#;
        assert_eq!(parse_search(body)[0].url, None);
        assert!(is_bandcamp("https://lespoints.bandcamp.com/track/bakteria"));
        assert!(!is_bandcamp("https://bandcamp.com.evil.example/x"));
        assert!(!is_bandcamp("https://bandcamp.com@evil.example/x"));
        assert!(!is_bandcamp("http://lespoints.bandcamp.com/track/x"));
    }

    #[test]
    fn the_track_page_supplies_date_isrc_number_duration_and_label() {
        let mut t = parse_search(SEARCH).remove(0);
        apply_page(&mut t, PAGE);
        assert_eq!(t.release_date.as_deref(), Some("2018-05-15"));
        assert_eq!(t.isrc.as_deref(), Some("DEOB61600327"));
        assert_eq!(t.track_no, Some(3));
        assert_eq!(t.duration_ms, Some(224_000));
        assert_eq!(t.label.as_deref(), Some("ILIAN TAPE"));
    }

    /// On an artist's own page the seller is the artist, not a label.
    #[test]
    fn an_artist_selling_their_own_record_is_not_a_label() {
        let mut t = TrackMatch {
            artists: vec!["Ilian Tape".into()],
            ..Default::default()
        };
        apply_page(&mut t, PAGE);
        assert_eq!(t.label, None);
    }

    #[test]
    fn what_a_match_already_has_is_not_overwritten() {
        let mut t = TrackMatch {
            isrc: Some("KEPT".into()),
            ..parse_search(SEARCH).remove(0)
        };
        apply_page(&mut t, PAGE);
        assert_eq!(t.isrc.as_deref(), Some("KEPT"));
    }

    #[test]
    fn responses_that_changed_shape_are_no_results() {
        assert!(parse_search("").is_empty());
        assert!(parse_search("<html>").is_empty());
        assert!(parse_search(r#"{"auto":{}}"#).is_empty());

        let mut t = TrackMatch::default();
        apply_page(&mut t, "<html><body>nothing here</body></html>");
        apply_page(
            &mut t,
            r#"<script type="application/ld+json">{broken</script>"#,
        );
        assert_eq!(t, TrackMatch::default());
    }

    #[test]
    fn durations_and_dates_are_read_the_way_bandcamp_writes_them() {
        assert_eq!(iso_duration_ms("P00H03M44S"), Some(224_000));
        assert_eq!(iso_duration_ms("PT1H2M3S"), Some(3_723_000));
        assert_eq!(iso_duration_ms("P00H00M00S"), None);
        assert_eq!(iso_duration_ms("3:44"), None);

        assert_eq!(
            iso_date("15 May 2018 00:00:00 GMT").as_deref(),
            Some("2018-05-15")
        );
        assert_eq!(
            iso_date("01 Dec 2023 00:00:00 GMT").as_deref(),
            Some("2023-12-01")
        );
        assert_eq!(iso_date("sometime"), None);
    }
}
