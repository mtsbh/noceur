//! Tracks from Beatport's own search page.
//!
//! The one source here that knows what a DJ tags: BPM, key, genre, label,
//! catalogue number and ISRC on every track, and a 1400 px sleeve. There is no
//! public API, but the search page is a Next.js app that ships its results as
//! JSON in a `__NEXT_DATA__` script, so reading that is reading the same data
//! the page renders from — no HTML scraping and no key.
//!
//! It is still not a published interface, so the parser fails soft: a page
//! that no longer looks the way it did is no results, not an error. Beatport
//! changing its front end should cost Beatport's candidates, and nobody else's.
//! The shape it expects is pinned by a saved page in `tests/fixtures`.

use crate::tracks::TrackMatch;
use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

const SERVICE: &str = "Beatport";
pub const NAME: &str = "Beatport";
const ENDPOINT: &str = "https://www.beatport.com/search/tracks";

/// A page is a third of a megabyte, fetched by a browser-shaped request. One a
/// second keeps a run through a folder from looking like a crawler.
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

    RATE.wait();
    let response = agent().get(ENDPOINT).query("q", query).call();

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

    let mut found = parse(&body);
    // The page always holds 150 results whatever is asked for; the ones past
    // the first few are Beatport's idea of related, not what was searched for.
    found.truncate(limit.max(1));
    Ok(found)
}

/// Turns a search page into matches.
///
/// Anything unexpected — no data script, a different layout, a track missing
/// its name — yields fewer results rather than an error, for the reason in the
/// module comment.
pub fn parse(page: &str) -> Vec<TrackMatch> {
    let Some(json) = next_data(page) else {
        return Vec::new();
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };

    // Found by its key rather than its position: the page carries several
    // queries (genres, menus) and their order is nobody's promise.
    let tracks = root
        .pointer("/props/pageProps/dehydratedState/queries")
        .and_then(|q| q.as_array())
        .into_iter()
        .flatten()
        .find(|q| q.pointer("/queryKey/0").and_then(|k| k.as_str()) == Some("search-tracks"))
        .and_then(|q| q.pointer("/state/data/data"))
        .and_then(|d| d.as_array());

    tracks
        .into_iter()
        .flatten()
        // One at a time, so a single odd track is dropped instead of the page.
        .filter_map(|t| RawTrack::deserialize(t).ok())
        .filter_map(RawTrack::into_match)
        .collect()
}

/// The JSON inside `<script id="__NEXT_DATA__" …>…</script>`.
fn next_data(page: &str) -> Option<&str> {
    let tag = page.find(r#"id="__NEXT_DATA__""#)?;
    let start = tag + page[tag..].find('>')? + 1;
    let len = page[start..].find("</script>")?;
    Some(&page[start..start + len])
}

#[derive(Deserialize)]
struct RawTrack {
    track_id: Option<u64>,
    track_name: Option<String>,
    mix_name: Option<String>,
    #[serde(default)]
    artists: Vec<RawArtist>,
    bpm: Option<f64>,
    key_name: Option<String>,
    #[serde(default)]
    genre: Vec<RawGenre>,
    label: Option<RawLabel>,
    catalog_number: Option<String>,
    isrc: Option<String>,
    /// Milliseconds.
    length: Option<u64>,
    release_date: Option<String>,
    track_number: Option<u32>,
    release: Option<RawRelease>,
}

#[derive(Deserialize)]
struct RawArtist {
    artist_name: Option<String>,
    artist_type_name: Option<String>,
}

#[derive(Deserialize)]
struct RawGenre {
    genre_name: Option<String>,
}

#[derive(Deserialize)]
struct RawLabel {
    label_name: Option<String>,
}

#[derive(Deserialize)]
struct RawRelease {
    release_name: Option<String>,
    release_image_uri: Option<String>,
}

impl RawTrack {
    fn into_match(self) -> Option<TrackMatch> {
        let name = self.track_name.filter(|n| !n.trim().is_empty())?;

        // Beatport keeps the mix apart from the name; a tag keeps it in the
        // title. "Original Mix" says nothing and is left off, like everywhere
        // else in this app.
        let title = match self.mix_name.as_deref().map(str::trim) {
            Some(mix) if !mix.is_empty() && !mix.eq_ignore_ascii_case("original mix") => {
                format!("{name} ({mix})")
            }
            _ => name,
        };

        // Remixers are listed alongside the artists. They belong in the title's
        // mix credit, which already names them, not in the artist tag.
        let named = |a: &RawArtist| a.artist_name.clone().filter(|n| !n.trim().is_empty());
        let mut artists: Vec<String> = self
            .artists
            .iter()
            .filter(|a| a.artist_type_name.as_deref() != Some("Remixer"))
            .filter_map(named)
            .collect();
        if artists.is_empty() {
            artists = self.artists.iter().filter_map(named).collect();
        }

        let text = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

        Some(TrackMatch {
            source: NAME,
            title,
            artists,
            album: self
                .release
                .as_ref()
                .and_then(|r| text(r.release_name.clone())),
            label: self.label.and_then(|l| text(l.label_name)),
            catalog_number: text(self.catalog_number),
            release_date: text(self.release_date)
                .map(|d| d.split('T').next().unwrap_or_default().to_string()),
            genres: self
                .genre
                .into_iter()
                .filter_map(|g| text(g.genre_name))
                .collect(),
            bpm: self.bpm.filter(|b| *b > 0.0).map(|b| b.round() as u32),
            key: text(self.key_name),
            isrc: text(self.isrc),
            duration_ms: self.length.filter(|l| *l > 0),
            track_no: self.track_number,
            // The slug is decoration; Beatport resolves a track by its id.
            url: self
                .track_id
                .map(|id| format!("https://www.beatport.com/track/-/{id}")),
            art: self.release.and_then(|r| text(r.release_image_uri)),
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real search page for "Bakteria Les Points", cut down to three tracks:
    /// an original mix, a remix, and one Beatport has no BPM for.
    const PAGE: &str = include_str!("../tests/fixtures/beatport_search.html");

    #[test]
    fn reads_every_field_a_dj_tags() {
        let found = parse(PAGE);
        assert_eq!(found.len(), 3);

        let t = &found[0];
        assert_eq!(t.source, NAME);
        assert_eq!(t.title, "Bakteria", "Original Mix is noise and is left off");
        assert_eq!(t.artists, ["Les Points"]);
        assert_eq!(t.album.as_deref(), Some("Anthoms LP"));
        assert_eq!(t.label.as_deref(), Some("Les Points"));
        assert_eq!(t.catalog_number.as_deref(), Some("LESPOINTS007"));
        assert_eq!(t.release_date.as_deref(), Some("2016-08-13"));
        assert_eq!(t.genres, ["Techno (Peak Time / Driving)"]);
        assert_eq!(t.bpm, Some(135));
        assert_eq!(t.key.as_deref(), Some("E Major"));
        assert_eq!(t.isrc.as_deref(), Some("DEPL91689078"));
        assert_eq!(t.duration_ms, Some(312_929));
        assert_eq!(t.track_no, Some(8));
        assert_eq!(
            t.url.as_deref(),
            Some("https://www.beatport.com/track/-/8205782")
        );
        let art = t.art.as_deref().unwrap();
        assert!(art.contains("1400x1400"), "{art}");
        assert!(
            crate::sources::is_allowed(art),
            "the sleeve must be fetchable"
        );
    }

    /// The remixer is credited in the title, not added to the artist tag.
    #[test]
    fn a_remix_keeps_its_mix_name_and_drops_the_remixer_from_the_artists() {
        let found = parse(PAGE);
        let remix = &found[1];
        assert_eq!(remix.title, "skee (Jennifer Walton Remix)");
        assert_eq!(remix.artists, ["KAVARI"]);
    }

    #[test]
    fn a_track_with_no_bpm_is_still_offered() {
        let found = parse(PAGE);
        let t = &found[2];
        assert_eq!(t.bpm, None);
        assert_eq!(t.key.as_deref(), Some("C Major"));
    }

    /// The page changing shape must cost Beatport's results and nothing more.
    #[test]
    fn a_page_that_no_longer_looks_the_same_is_no_results() {
        assert!(parse("").is_empty());
        assert!(parse("<html><body>Just a moment...</body></html>").is_empty());
        assert!(
            parse(r#"<script id="__NEXT_DATA__" type="application/json">{not json</script>"#)
                .is_empty()
        );
        assert!(parse(
            r#"<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>"#
        )
        .is_empty());
    }

    #[test]
    fn a_track_without_a_name_is_skipped_not_fatal() {
        let page = r#"<script id="__NEXT_DATA__" type="application/json">
            {"props":{"pageProps":{"dehydratedState":{"queries":[{"queryKey":["search-tracks"],
            "state":{"data":{"data":[{"track_id":1},{"track_id":2,"track_name":"Kept","bpm":"fast"},
            {"track_id":3,"track_name":"Also kept","artists":[]}]}}}]}}}}</script>"#;
        let found = parse(page);
        let titles: Vec<&str> = found.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            ["Also kept"],
            "a malformed field drops only its own track"
        );
    }
}
