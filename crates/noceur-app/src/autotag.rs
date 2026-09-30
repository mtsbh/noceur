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
/// Reads the file rather than the index, like the tag editor does. Falls back to
/// the file name when the tags have nothing to search with, because the files
/// that most need this are exactly the ones with no tags — promos, rips, things
/// pulled off a stick — and `Artist - Title.wav` is what a DJ library calls
/// them.
///
/// The duration comes from the file's own header and is used only as a tiebreak.
#[tauri::command]
pub async fn match_track(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Vec<TrackMatch>, String> {
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

        // The name is only consulted for what the tags do not already say, so a
        // file with a good title and no artist still gets both.
        let guess = noceur_online::matching::from_file_name(&name);
        let artist = if tagged_artist.trim().is_empty() {
            guess.artist
        } else {
            tagged_artist
        };
        let title = if tagged_title.trim().is_empty() {
            guess.title
        } else {
            tagged_title
        };

        // Discogs is only asked when the user has given it a token. Its label
        // and catalogue number are the fields nothing else here reliably has.
        let token =
            crate::config::Config::load(&crate::config::Config::default_path()).discogs_token;

        let matches = noceur_online::match_track(&artist, &title, meta.duration_ms, &token)
            .map_err(|e| e.to_string())?;
        let mut matches = trim(matches, PER_SOURCE);

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

        Ok(matches)
    })
    .await
    .map_err(|e| format!("match task failed: {e}"))?
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
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    /// True when a Discogs token is stored.
    pub discogs: bool,
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
        assert_eq!(camelot("not a key"), "not a key");
    }
}
