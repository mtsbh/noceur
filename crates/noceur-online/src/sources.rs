//! What every cover source produces, and the rules for fetching what they point at.

use crate::{agent, OnlineError};

/// One sleeve a source found.
///
/// Carries URLs rather than bytes. A search across three services turns up
/// dozens of candidates and the user looks at a handful before choosing one, so
/// downloading them all up front would be most of a megabyte thrown away — the
/// picker points an `<img>` at each thumbnail and lets the browser fetch only
/// what it draws.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    /// Which service found it, so the picker can say where a cover came from.
    pub source: &'static str,
    pub title: String,
    pub artist: String,
    /// Year, country, label — whatever the source knows that helps tell two
    /// pressings apart. Pre-joined, because each service knows different things.
    pub detail: String,
    /// Small rendering, for the grid.
    pub thumbnail: String,
    /// What actually gets downloaded and embedded.
    pub full: String,
    /// Pixel width of `full` when the source states one.
    pub size: Option<u32>,
}

/// Hosts this app will download an image from.
///
/// The picker hands back a URL that came from a search, but the command that
/// receives it cannot know that — a compromised or simply buggy front end could
/// pass anything, and a backend that fetched arbitrary URLs on request would be
/// a way to probe whatever this machine can reach that the caller cannot.
/// Matching by suffix on the host covers the CDNs these services redirect to.
const ALLOWED_HOSTS: &[&str] = &[
    // Cover Art Archive, and the Internet Archive it redirects to.
    "coverartarchive.org",
    "archive.org",
    // Apple's artwork CDN.
    "mzstatic.com",
    // Deezer's image CDN.
    "dzcdn.net",
    // Discogs' image CDN.
    "discogs.com",
    // Beatport's image CDN, `geo-media.beatport.com`.
    "beatport.com",
    // Bandcamp's image CDN, `f4.bcbits.com`.
    "bcbits.com",
];

/// Whether an image URL may be fetched.
///
/// Requires HTTPS as well as a known host: the sources all serve it, and a
/// downgrade to plain HTTP would mean whatever is between here and them chooses
/// what gets written into the user's files.
pub fn is_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        // Credentials before an `@` would otherwise let any host masquerade as
        // an allowed one: `https://coverartarchive.org@evil.example/x`.
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();

    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")))
}

/// Downloads an image chosen from the picker.
///
/// Refuses anything that is not on a source's own CDN, and anything that does
/// not arrive looking like an image. Both checks matter: the bytes are about to
/// be written into the user's files, where a mislabelled blob would sit
/// unreadable inside every copy of the track from then on.
pub fn fetch_image(url: &str) -> Result<Vec<u8>, OnlineError> {
    if !is_allowed(url) {
        return Err(OnlineError::Refused {
            url: url.to_string(),
        });
    }

    let mut response = agent().get(url).call().map_err(|e| match e {
        ureq::Error::StatusCode(status) => OnlineError::Status {
            service: "the cover source",
            status,
        },
        other => OnlineError::Unreachable {
            service: "the cover source",
            reason: other.to_string(),
        },
    })?;

    let bytes = response
        .body_mut()
        .with_config()
        .limit(crate::covers::MAX_BYTES)
        .read_to_vec()
        .map_err(|e| OnlineError::Malformed {
            service: "the cover source",
            reason: e.to_string(),
        })?;

    if crate::covers::sniff_mime(&bytes).is_none() {
        return Err(OnlineError::NotAnImage {
            url: url.to_string(),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources_own_hosts_are_allowed() {
        assert!(is_allowed(
            "https://coverartarchive.org/release/x/front-500"
        ));
        assert!(is_allowed(
            "https://ia800207.us.archive.org/12/items/x/y.jpg"
        ));
        assert!(is_allowed(
            "https://is1-ssl.mzstatic.com/image/thumb/a/600x600bb.jpg"
        ));
        assert!(is_allowed(
            "https://e-cdns-images.dzcdn.net/images/cover/x/500x500.jpg"
        ));
        assert!(is_allowed("https://i.discogs.com/x/rx-600.jpeg"));
        assert!(is_allowed(
            "https://geo-media.beatport.com/image_size/1400x1400/x.jpg"
        ));
        assert!(is_allowed("https://f4.bcbits.com/img/a4230057167_10.jpg"));
    }

    #[test]
    fn anything_else_is_refused() {
        assert!(!is_allowed("https://example.com/cover.jpg"));
        assert!(!is_allowed(
            "https://evil.example/coverartarchive.org/x.jpg"
        ));
        assert!(!is_allowed("file:///C:/Windows/System32/config/SAM"));
        assert!(!is_allowed("http://localhost:8080/admin"));
        assert!(!is_allowed(""));
    }

    /// A host that merely ends with an allowed name is a different host.
    /// `notcoverartarchive.org` is not the Cover Art Archive.
    #[test]
    fn a_lookalike_host_is_not_the_real_one() {
        assert!(!is_allowed("https://notcoverartarchive.org/x.jpg"));
        assert!(!is_allowed("https://archive.org.evil.example/x.jpg"));
    }

    /// Credentials before an `@` are the classic way to make a URL read as one
    /// host while resolving to another.
    #[test]
    fn credentials_cannot_disguise_the_real_host() {
        assert!(!is_allowed(
            "https://coverartarchive.org@evil.example/x.jpg"
        ));
        assert!(!is_allowed("https://user:pass@evil.example/x.jpg"));
    }

    /// Plain HTTP would let whatever sits in between choose what gets written
    /// into the user's files.
    #[test]
    fn plain_http_is_refused_even_on_an_allowed_host() {
        assert!(!is_allowed(
            "http://coverartarchive.org/release/x/front-500"
        ));
    }

    #[test]
    fn a_port_does_not_confuse_the_host_check() {
        assert!(is_allowed("https://archive.org:443/x.jpg"));
        assert!(!is_allowed("https://evil.example:443/x.jpg"));
    }
}
