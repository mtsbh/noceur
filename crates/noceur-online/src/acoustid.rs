//! Identifying a track by how it sounds.
//!
//! For the files whose tags and names say nothing. The fingerprint is computed
//! locally (`noceur_audio::fingerprint`); this sends it to AcoustID, which
//! answers with the MusicBrainz recordings it belongs to. What comes back is an
//! artist and a title to search the other sources with, not a match to apply:
//! AcoustID knows what a recording is called, not its label, BPM or sleeve.
//!
//! What leaves the machine is the fingerprint and the track's length. Neither
//! can be turned back into audio.

use crate::{agent, OnlineError, RateLimit};
use serde::Deserialize;

const SERVICE: &str = "AcoustID";
const ENDPOINT: &str = "https://api.acoustid.org/v2/lookup";

/// This application's key, registered at acoustid.org.
///
/// Shipped in the app, as Picard and beets ship theirs: it identifies the
/// application, not the user, and the user never has to sign up for anything.
const KEY: &str = "";

/// Their published limit is three requests a second.
static RATE: RateLimit = RateLimit::new(340);

/// Results below this are other recordings that share a passage.
pub const MIN_SCORE: f32 = 0.5;

/// A recording the fingerprint belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct Identified {
    /// How closely the fingerprint matched, 0.0 to 1.0.
    pub score: f32,
    /// MusicBrainz recording identifier.
    pub recording_id: String,
    pub title: String,
    pub artists: Vec<String>,
}

impl Identified {
    /// The credited artists as one string, the way a tag stores them.
    pub fn artist(&self) -> String {
        self.artists.join(", ")
    }
}

/// Whether lookups can be made at all.
pub fn available() -> bool {
    !KEY.is_empty()
}

/// Looks a fingerprint up. Best first; empty when AcoustID has never heard it.
pub fn lookup(fingerprint: &str, duration_secs: u32) -> Result<Vec<Identified>, OnlineError> {
    if !available() {
        return Ok(Vec::new());
    }

    RATE.wait();

    // Posted rather than put in the address: two minutes of fingerprint is a
    // few kilobytes, which is past what some proxies accept in a URL.
    let duration = duration_secs.to_string();
    let response = agent().post(ENDPOINT).send_form([
        ("client", KEY),
        ("format", "json"),
        ("meta", "recordings compress"),
        ("duration", duration.as_str()),
        ("fingerprint", fingerprint),
    ]);

    let body = match response {
        Ok(mut r) => r
            .body_mut()
            .read_to_string()
            .map_err(|e| OnlineError::Malformed {
                service: SERVICE,
                reason: e.to_string(),
            })?,
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

    parse(&body)
}

/// Turns a lookup response into recordings.
///
/// One fingerprint can belong to several recordings — the original and a
/// remaster, or the same track credited two ways. They are all returned, best
/// first, and a recording AcoustID knows only by its identifier is skipped: it
/// has no title to search with.
pub fn parse(body: &str) -> Result<Vec<Identified>, OnlineError> {
    let parsed: Response = serde_json::from_str(body).map_err(|e| OnlineError::Malformed {
        service: SERVICE,
        reason: e.to_string(),
    })?;
    if parsed.status != "ok" {
        return Err(OnlineError::Malformed {
            service: SERVICE,
            reason: parsed
                .error
                .map(|e| e.message)
                .unwrap_or_else(|| parsed.status.clone()),
        });
    }

    let mut found: Vec<Identified> = parsed
        .results
        .into_iter()
        .flat_map(|r| {
            r.recordings.into_iter().filter_map(move |rec| {
                let title = rec.title.filter(|t| !t.trim().is_empty())?;
                Some(Identified {
                    score: r.score,
                    recording_id: rec.id,
                    title,
                    artists: rec.artists.into_iter().map(|a| a.name).collect(),
                })
            })
        })
        .collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(found)
}

#[derive(Deserialize)]
struct Response {
    status: String,
    #[serde(default)]
    results: Vec<RawResult>,
    error: Option<RawError>,
}

#[derive(Deserialize)]
struct RawError {
    message: String,
}

#[derive(Deserialize)]
struct RawResult {
    score: f32,
    #[serde(default)]
    recordings: Vec<RawRecording>,
}

#[derive(Deserialize)]
struct RawRecording {
    id: String,
    title: Option<String>,
    #[serde(default)]
    artists: Vec<RawArtist>,
}

#[derive(Deserialize)]
struct RawArtist {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a real answer, including a recording known only by its
    /// identifier, which AcoustID sends often.
    const SAMPLE: &str = r#"{
      "status": "ok",
      "results": [
        {
          "id": "9ff43b6a-4f16-427c-93c2-92307ca505e0",
          "score": 0.62,
          "recordings": [
            { "id": "rec-b", "title": "South London Boroughs (edit)",
              "artists": [{ "id": "a1", "name": "Burial" }] }
          ]
        },
        {
          "id": "0f7e7b63-1a5e-4b4b-9ad4-6b1f3b1f3b1f",
          "score": 0.97,
          "recordings": [
            { "id": "rec-a", "title": "South London Boroughs",
              "artists": [{ "id": "a1", "name": "Burial" }, { "id": "a2", "name": "Kode9" }] },
            { "id": "rec-bare" }
          ]
        }
      ]
    }"#;

    #[test]
    fn recordings_come_back_best_first() {
        let found = parse(SAMPLE).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].recording_id, "rec-a");
        assert_eq!(found[0].score, 0.97);
        assert_eq!(found[0].artist(), "Burial, Kode9");
        assert_eq!(found[1].title, "South London Boroughs (edit)");
    }

    #[test]
    fn nothing_heard_is_not_an_error() {
        assert_eq!(parse(r#"{"status":"ok","results":[]}"#).unwrap(), vec![]);
    }

    /// A bad key or a mangled fingerprint comes back as a status, with a
    /// message worth showing.
    #[test]
    fn an_error_answer_carries_its_message() {
        let body = r#"{"status":"error","error":{"code":4,"message":"invalid API key"}}"#;
        match parse(body).unwrap_err() {
            OnlineError::Malformed { reason, .. } => assert_eq!(reason, "invalid API key"),
            other => panic!("{other:?}"),
        }
    }
}
