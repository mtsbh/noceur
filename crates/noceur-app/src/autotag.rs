//! Matching a track against online sources.
//!
//! Finds candidates and ranks them; it does not write anything. Applying a match
//! goes through the tag editor like any other edit, pre-filled — so the fields
//! about to be written are visible and adjustable before they are, and one
//! locked file in a selection is reported the same way it always was.

use crate::state::AppState;
use noceur_online::tracks::TrackMatch;
use tauri::State;

/// Searches every source for what this file might be.
///
/// Reads the file rather than the index, like the tag editor does. When the
/// tags have nothing to search with, it identifies the audio by fingerprint and
/// then falls back to the file name, because the files that most need this are
/// exactly the ones with no tags — promos, rips, things pulled off a stick.
///
/// The duration comes from the file's own header and is used only as a tiebreak.
#[tauri::command]
pub async fn match_track(state: State<'_, AppState>, file_id: i64) -> Result<Matched, String> {
    let db = state.db.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let found = {
            let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
            noceur_index::thumb::locate(&db, file_id).map_err(|e| e.to_string())?
        };
        let Some(found) = found.filter(|f| f.kind == 1) else {
            return Err("that row is not an audio file".into());
        };

        let meta = noceur_tags::read(&found.path).map_err(|e| e.to_string())?;
        let name = found
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let tagged_artist = meta.artist.clone().unwrap_or_default();
        let tagged_title = meta.title.clone().unwrap_or_default();
        let tagged = !tagged_artist.trim().is_empty() && !tagged_title.trim().is_empty();

        // Discogs and Tidal are only asked when the user has given them
        // credentials. Discogs has the label and catalogue number; Tidal, the
        // ISRC and release date from the distributors themselves.
        let config = crate::config::Config::load(&crate::config::Config::default_path());
        let accounts = noceur_online::Accounts {
            discogs_token: &config.discogs_token,
            tidal_id: &config.tidal_client_id,
            tidal_secret: &config.tidal_client_secret,
        };
        let search = |artist: &str, title: &str| {
            noceur_online::match_track(artist, title, meta.duration_ms, &accounts)
                .map(|m| trim(m, PER_SOURCE))
        };

        // Tags first. Without them, the sound: a fingerprint names the recording
        // outright, where a file name is only a guess. The name is the last
        // resort, and is only consulted for what the tags do not already say, so
        // a file with a good title and no artist still gets both.
        let mut heard = None;
        let (artist, title) = if tagged {
            (tagged_artist, tagged_title)
        } else if let Some(h) = identify(&found.path, meta.duration_ms) {
            let query = (h.artist(), h.title.clone());
            heard = Some(h);
            query
        } else {
            let guess = noceur_online::matching::from_file_name(&name);
            let pick = |tag: String, guess: String| {
                if tag.trim().is_empty() {
                    guess
                } else {
                    tag
                }
            };
            (
                pick(tagged_artist, guess.artist),
                pick(tagged_title, guess.title),
            )
        };

        let mut result = search(&artist, &title);

        // Tags that find nothing may be wrong, or say only "Track 01". The
        // fingerprint gets one try at a better query.
        if heard.is_none() && result.as_ref().map_or(true, Vec::is_empty) {
            if let Some(h) = identify(&found.path, meta.duration_ms) {
                if h.artist() != artist || h.title != title {
                    result = search(&h.artist(), &h.title);
                    heard = Some(h);
                }
            }
        }

        let mut matches = result.map_err(|e| e.to_string())?;

        // The fields the search endpoint leaves out are the ones worth having.
        // Fetched side by side: each is a page load, and one after another they
        // would hold the picker open for seconds.
        std::thread::scope(|s| {
            for m in matches.iter_mut() {
                if m.source == noceur_online::deezer::NAME {
                    s.spawn(|| noceur_online::deezer::enrich(m));
                } else if m.source == noceur_online::bandcamp::NAME {
                    s.spawn(|| noceur_online::bandcamp::enrich(m));
                }
            }
        });

        for m in &mut matches {
            m.key = m.key.take().map(|k| camelot(&k));
        }

        Ok(Matched {
            matches,
            heard: heard.map(|h| Heard {
                artist: h.artist(),
                title: h.title,
            }),
        })
    })
    .await
    .map_err(|e| format!("match task failed: {e}"))?
}

/// What the sources found, and what the fingerprint said the track is when
/// the search was made with that instead of the tags.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Matched {
    pub matches: Vec<TrackMatch>,
    pub heard: Option<Heard>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Heard {
    pub artist: String,
    pub title: String,
}

/// The recording a file's audio belongs to, by fingerprint.
///
/// `None` for anything short of a confident answer: no key built in, a file
/// that will not decode, AcoustID unreachable or never having heard it. Each of
/// those leaves the search where it would have been without this, so none is
/// worth failing the search over.
fn identify(
    path: &std::path::Path,
    duration_ms: Option<u64>,
) -> Option<noceur_online::acoustid::Identified> {
    use noceur_online::acoustid;
    if !acoustid::available() {
        return None;
    }
    let print = noceur_audio::fingerprint::fingerprint(path)
        .inspect_err(|e| eprintln!("fingerprint {}: {e}", path.display()))
        .ok()?;
    let secs = duration_ms
        .map(|ms| (ms / 1000) as u32)
        .or(print.duration_secs)?;
    acoustid::lookup(&print.encoded, secs)
        .inspect_err(|e| eprintln!("AcoustID: {e}"))
        .ok()?
        .into_iter()
        .find(|h| h.score >= acoustid::MIN_SCORE)
}

/// Candidates kept per source.
///
/// More than the picker shows: a cover link can still fail to load, and the
/// picker drops those before showing its first three.
const PER_SOURCE: usize = 6;

/// Drops matches with no cover and keeps the best few of each source.
///
/// A release without a sleeve is hard to tell apart from its neighbours, and a
/// dozen near-identical rows from one source bury the others. Order is kept, so
/// the best of every source still comes first.
fn trim(matches: Vec<TrackMatch>, per_source: usize) -> Vec<TrackMatch> {
    let mut seen: std::collections::HashMap<&'static str, usize> = Default::default();
    matches
        .into_iter()
        .filter(|m| m.art.as_deref().is_some_and(|a| !a.is_empty()))
        .filter(|m| {
            let n = seen.entry(m.source).or_default();
            *n += 1;
            *n <= per_source
        })
        .collect()
}

/// A source's key in the notation the rest of the app writes.
///
/// Beatport says `Gb Major`; the key column, the tag editor and every tag this
/// app writes say `2B`. Applying a match as-is would leave the file in whatever
/// notation the source happened to use. Text that is not a key is passed
/// through, so the user still sees what the source claimed.
fn camelot(key: &str) -> String {
    noceur_tags::MusicalKey::parse(key)
        .map(|k| k.format(noceur_tags::KeyNotation::Camelot))
        .unwrap_or_else(|| key.to_string())
}

/// Whether the optional credentials are set, without revealing them.
///
/// The front end needs to know whether to offer Discogs and whether to prompt
/// for a token; it does not need the token, and never gets it back.
#[tauri::command]
pub fn credentials() -> Credentials {
    let config = crate::config::Config::load(&crate::config::Config::default_path());
    Credentials {
        discogs: !config.discogs_token.trim().is_empty(),
        tidal: !config.tidal_client_id.trim().is_empty()
            && !config.tidal_client_secret.trim().is_empty(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    /// True when a Discogs token is stored.
    pub discogs: bool,
    /// True when both halves of the Tidal credentials are stored.
    pub tidal: bool,
}

/// Stores or clears the Discogs token.
///
/// An empty string clears it, which is the only way to remove one — there is no
/// separate delete, because "empty the box and save" is what anyone would try.
#[tauri::command]
pub fn set_discogs_token(token: String) -> Result<Credentials, String> {
    let path = crate::config::Config::default_path();
    let mut config = crate::config::Config::load(&path);
    config.discogs_token = token.trim().to_string();
    config.save(&path)?;
    Ok(credentials())
}

/// Stores or clears the Tidal app credentials. Both empty clears them.
#[tauri::command]
pub fn set_tidal(client_id: String, secret: String) -> Result<Credentials, String> {
    let path = crate::config::Config::default_path();
    let mut config = crate::config::Config::load(&path);
    config.tidal_client_id = client_id.trim().to_string();
    config.tidal_client_secret = secret.trim().to_string();
    config.save(&path)?;
    Ok(credentials())
}

#[cfg(test)]
mod tests {
    use super::{camelot, trim, TrackMatch};

    fn found(source: &'static str, title: &str, art: bool) -> TrackMatch {
        TrackMatch {
            source,
            title: title.into(),
            art: art.then(|| format!("https://example.com/{title}.jpg")),
            ..Default::default()
        }
    }

    #[test]
    fn matches_without_a_cover_are_dropped_and_each_source_is_capped() {
        let all = vec![
            found("Deezer", "d1", true),
            found("Deezer", "d2", false),
            found("Beatport", "b1", true),
            found("Deezer", "d3", true),
            found("Deezer", "d4", true),
            found("Beatport", "b2", true),
        ];
        let kept: Vec<String> = trim(all, 2).into_iter().map(|m| m.title).collect();
        assert_eq!(kept, ["d1", "b1", "d3", "b2"]);
    }

    /// Every spelling Beatport uses, including both enharmonics.
    #[test]
    fn source_keys_are_written_as_camelot() {
        assert_eq!(camelot("E Major"), "12B");
        assert_eq!(camelot("A Minor"), "8A");
        assert_eq!(camelot("Gb Major"), "2B");
        assert_eq!(camelot("F# Minor"), "11A");
        assert_eq!(camelot("Gb Minor"), "11A");
        assert_eq!(camelot("A# Minor"), "3A");
        assert_eq!(camelot("Bb Minor"), "3A");
        assert_eq!(camelot("D# Major"), "5B");
        assert_eq!(camelot("8A"), "8A");
        // Tidal's spellings, after `tidal::key`.
        assert_eq!(camelot("C# Major"), "3B");
        assert_eq!(camelot("Eb Minor"), "2A");
        assert_eq!(camelot("Ab Major"), "4B");
        assert_eq!(camelot("not a key"), "not a key");
    }
}
