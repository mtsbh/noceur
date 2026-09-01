//! Fetching cover art from the Cover Art Archive.
//!
//! The archive is keyed by MusicBrainz release id, which is the only reason
//! [`crate::musicbrainz`] is consulted at all — the search exists to turn an
//! artist and a title into identifiers this can be asked about.

use crate::{agent, OnlineError};

const SERVICE: &str = "the Cover Art Archive";

/// Largest image this will embed in a file.
///
/// Cover art rides inside every copy of the track from then on, so a 20MB scan
/// of a gatefold sleeve is not a favour to anyone: it makes the file slower to
/// copy to a USB stick and slower for every program that reads its tags. The
/// archive's own 1200px renderings sit comfortably under this.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Which rendering to ask the archive for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSize {
    /// 250px, for the grid of candidates.
    Thumbnail,
    /// 500px. What actually gets embedded — large enough to look right on a CDJ
    /// screen or a laptop, small enough not to bloat the file.
    Embed,
    /// 1200px, for someone who wants the best available.
    Large,
}

impl CoverSize {
    fn suffix(self) -> &'static str {
        match self {
            CoverSize::Thumbnail => "front-250",
            CoverSize::Embed => "front-500",
            CoverSize::Large => "front-1200",
        }
    }
}

/// The URL of a release's front cover at a given size.
///
/// Public and stable, which is why the picker can point an `<img>` straight at
/// it instead of routing every candidate thumbnail through this process.
pub fn front_url(release_id: &str, size: CoverSize) -> String {
    format!(
        "https://coverartarchive.org/release/{release_id}/{}",
        size.suffix()
    )
}

/// Downloads a release's front cover.
///
/// Returns `Ok(None)` when the archive has no front image for the release —
/// which is common, and not a failure worth showing anyone. Anything else that
/// goes wrong is reported.
pub fn fetch_front(release_id: &str, size: CoverSize) -> Result<Option<Vec<u8>>, OnlineError> {
    let url = front_url(release_id, size);

    let mut response = match agent().get(&url).call() {
        Ok(r) => r,
        // The archive answers 404 for a release it holds no art for, and ureq
        // treats that as an error rather than a response.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
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

    if response.status().as_u16() == 404 {
        return Ok(None);
    }

    // Checked before reading, so a mislabelled or hostile response cannot be
    // pulled into memory in full before anyone notices how big it is.
    if let Some(len) = content_length(&response) {
        if len > MAX_BYTES {
            return Err(OnlineError::TooLarge { size: len });
        }
    }

    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| OnlineError::Malformed {
            service: SERVICE,
            reason: e.to_string(),
        })?;

    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn content_length(response: &ureq::http::Response<ureq::Body>) -> Option<u64> {
    response
        .headers()
        .get("content-length")?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// Guesses an image's MIME type from its first bytes.
///
/// The type has to be recorded in the tag, and the archive's `Content-Type` is
/// lost by the time the bytes reach whoever embeds them. Sniffing the magic
/// number is more reliable than trusting a header anyway: what matters is what
/// the file actually is.
pub fn sniff_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_name_the_size_that_was_asked_for() {
        let id = "aaaaaaaa-1111-2222-3333-444444444444";
        assert!(front_url(id, CoverSize::Thumbnail).ends_with("/front-250"));
        assert!(front_url(id, CoverSize::Embed).ends_with("/front-500"));
        assert!(front_url(id, CoverSize::Large).ends_with("/front-1200"));
        assert!(front_url(id, CoverSize::Embed).contains(id));
    }

    /// The type is written into the tag, so getting it wrong leaves a picture
    /// that some programs will refuse to display.
    #[test]
    fn recognises_the_formats_the_archive_serves() {
        assert_eq!(sniff_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_mime(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
    }

    #[test]
    fn refuses_to_guess_at_something_that_is_not_an_image() {
        assert_eq!(sniff_mime(b"<html>"), None);
        assert_eq!(sniff_mime(b""), None);
        assert_eq!(sniff_mime(&[0xFF]), None);
    }
}
