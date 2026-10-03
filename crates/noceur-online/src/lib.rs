//! Everything in this app that talks to the internet.
//!
//! Deliberately one crate, so the whole network surface can be read in one
//! sitting. A file browser has no business making requests the user did not ask
//! for, and keeping this apart from the crates that read disks makes it obvious
//! when that line is crossed: nothing here runs unless a command in the app
//! calls it, and every command that does is one the user triggered from a menu.
//!
//! What leaves the machine is an artist and a title, typed as a search query.
//! That is inherent to looking a release up in a database, but it is worth
//! stating plainly rather than leaving it to be discovered. The one exception is
//! identifying a track by sound (`acoustid`, and `shazam` when the user has
//! turned it on), which sends a fingerprint of the audio and its length instead.

pub mod acoustid;
pub mod bandcamp;
pub mod beatport;
pub mod covers;
pub mod deezer;
pub mod discogs;
pub mod itunes;
pub mod matching;
pub mod musicbrainz;
pub mod shazam;
pub mod sources;
pub mod tidal;
pub mod tracks;

pub use covers::{fetch_front, CoverSize};
pub use musicbrainz::{search_releases, Release};
pub use sources::{fetch_image, Candidate};

#[derive(Debug, thiserror::Error)]
pub enum OnlineError {
    #[error("nothing to search for: the track has no artist or title")]
    NothingToSearch,
    #[error("{service} could not be reached: {reason}")]
    Unreachable {
        service: &'static str,
        reason: String,
    },
    #[error("{service} answered with {status}")]
    Status { service: &'static str, status: u16 },
    #[error("{service} sent something unexpected: {reason}")]
    Malformed {
        service: &'static str,
        reason: String,
    },
    #[error("that image is {size} bytes, which is larger than this app will embed")]
    TooLarge { size: u64 },
    #[error("this app will not download from {url}")]
    Refused { url: String },
    #[error("what came back from {url} is not an image")]
    NotAnImage { url: String },
    #[error("{service} needs a personal access token; add one in Settings")]
    NeedsToken { service: &'static str },
    #[error("{service} did not accept that token")]
    BadToken { service: &'static str },
}

/// How this app identifies itself.
///
/// MusicBrainz requires an application name, a version and a contact, and
/// rate-limits or blocks clients that do not provide one. It is also simply
/// good manners: an operator seeing this in their logs can tell what is asking.
pub const USER_AGENT: &str = concat!(
    "Noceur/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/mtsbh/noceur )"
);

/// How long any single request may take.
///
/// Short enough that a menu item never appears to hang, long enough for a slow
/// mobile connection to answer.
pub const TIMEOUT_SECS: u64 = 15;

/// Builds the shared HTTP agent.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(TIMEOUT_SECS)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// Paces requests to one service, so this app stays inside its published limit.
///
/// Per service rather than global: MusicBrainz asks for one request a second and
/// Discogs for sixty a minute, and making them share a budget would halve both
/// for no reason. A client that ignored either would be indistinguishable from
/// abuse and would earn the block it deserved.
pub struct RateLimit {
    gap: std::time::Duration,
    last: std::sync::Mutex<Option<std::time::Instant>>,
}

impl RateLimit {
    pub const fn new(gap_ms: u64) -> RateLimit {
        RateLimit {
            gap: std::time::Duration::from_millis(gap_ms),
            last: std::sync::Mutex::new(None),
        }
    }

    /// Blocks until enough time has passed since the previous request.
    ///
    /// Called on the blocking pool, where sleeping is what that pool is for.
    pub fn wait(&self) {
        let pause = {
            let mut last = match self.last.lock() {
                Ok(l) => l,
                // A poisoned lock means another thread panicked mid-request.
                // Pacing is a courtesy, not a correctness property, so recover
                // rather than propagate.
                Err(e) => e.into_inner(),
            };
            let now = std::time::Instant::now();
            let pause = last
                .map(|t| self.gap.saturating_sub(now.duration_since(t)))
                .unwrap_or_default();
            *last = Some(now + pause);
            pause
        };
        if !pause.is_zero() {
            std::thread::sleep(pause);
        }
    }
}

/// Searches every source for cover art.
///
/// Each source is asked independently and one that fails is simply absent from
/// the results: a service being down, rate-limiting, or having changed its
/// format should cost its own candidates and nobody else's. Returning an error
/// only when every source failed keeps a temporary outage at one provider from
/// looking like the feature is broken.
///
/// Results are interleaved rather than concatenated, so the grid opens showing
/// each source's best guess rather than ten from whichever answered first.
pub fn search_all(artist: &str, album: &str) -> Result<Vec<Candidate>, OnlineError> {
    let mut lists = Vec::new();
    let mut last_error = None;

    for result in [
        musicbrainz::search_candidates(artist, album),
        itunes::search(artist, album),
        deezer::search(artist, album),
    ] {
        match result {
            Ok(found) => lists.push(found.into_iter()),
            Err(e) => last_error = Some(e),
        }
    }

    if lists.is_empty() {
        return Err(last_error.unwrap_or(OnlineError::NothingToSearch));
    }

    let mut merged = Vec::new();
    loop {
        let mut any = false;
        for list in &mut lists {
            if let Some(candidate) = list.next() {
                merged.push(candidate);
                any = true;
            }
        }
        if !any {
            break;
        }
    }
    Ok(merged)
}

/// Results asked of each source when matching a track.
pub const MATCH_LIMIT: usize = 8;

/// Matches above this are worth showing at all.
///
/// Below it the results are other records by the same artist, or tracks that
/// share a common word. Showing them would bury the real answer in a list
/// nobody reads to the bottom of.
pub const MIN_SCORE: f32 = 0.45;

/// The user's own credentials for the sources that need them. Empty strings
/// for the ones they have not set, which leaves those sources out.
#[derive(Debug, Clone, Copy, Default)]
pub struct Accounts<'a> {
    pub discogs_token: &'a str,
    pub tidal_id: &'a str,
    pub tidal_secret: &'a str,
}

impl Accounts<'_> {
    pub fn has_tidal(&self) -> bool {
        !self.tidal_id.trim().is_empty() && !self.tidal_secret.trim().is_empty()
    }
}

/// Searches every source for a track and ranks what comes back.
///
/// Ranked, not chosen. The sources disagree constantly — a different release
/// date, a genre one calls Techno and another calls Electronic, a title with
/// and without its remix credit — and which of those is right for a particular
/// file is a judgement the person who owns it makes. This returns the ordered
/// evidence.
///
/// `duration_ms` is what the file itself decodes to, used only as a tiebreak.
/// A source failing costs its own results and nothing else.
pub fn match_track(
    artist: &str,
    title: &str,
    duration_ms: Option<u64>,
    accounts: &Accounts,
) -> Result<Vec<tracks::TrackMatch>, OnlineError> {
    let clean = matching::clean_title(title);
    let mut found = Vec::new();
    let mut last_error = None;
    let mut any_source_answered = false;

    let mut results = vec![
        beatport::search_tracks(artist, &clean, MATCH_LIMIT),
        bandcamp::search_tracks(artist, &clean, MATCH_LIMIT),
        deezer::search_tracks(artist, &clean, MATCH_LIMIT),
        itunes::search_tracks(artist, &clean, MATCH_LIMIT),
    ];
    // Only asked when there is a token to ask with. Without one it would fail
    // every time and its error would masquerade as the last word on the search.
    if !accounts.discogs_token.trim().is_empty() {
        results.push(discogs::search_tracks(
            artist,
            &clean,
            MATCH_LIMIT,
            accounts.discogs_token,
        ));
    }
    if accounts.has_tidal() {
        results.push(tidal::search_tracks(
            artist,
            &clean,
            MATCH_LIMIT,
            accounts.tidal_id,
            accounts.tidal_secret,
        ));
    }

    for result in results {
        match result {
            Ok(list) => {
                any_source_answered = true;
                found.extend(list);
            }
            Err(e) => last_error = Some(e),
        }
    }

    if !any_source_answered {
        return Err(last_error.unwrap_or(OnlineError::NothingToSearch));
    }

    for m in &mut found {
        m.score = matching::score(
            artist,
            title,
            &m.artist(),
            &m.title,
            duration_ms,
            m.duration_ms,
        );
    }

    found.retain(|m| m.score >= MIN_SCORE);
    // Best first; among equal scores, the result that carries more fields.
    found.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| b.is_useful().cmp(&a.is_useful()))
    });
    Ok(found)
}
