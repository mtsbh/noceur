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
//! stating plainly rather than leaving it to be discovered.

pub mod covers;
pub mod musicbrainz;

pub use covers::{fetch_front, CoverSize};
pub use musicbrainz::{search_releases, Release};

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
