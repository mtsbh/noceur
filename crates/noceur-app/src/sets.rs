//! Sets: recorded DJ sets linked to their tracklists on TrackID, and the
//! commands behind the Set panel and the sidebar's "In your sets".
//!
//! Storage is `noceur_index::sets`; the requests are `noceur_online::trackid`.
//! A tracklist is fetched once, when the user links it, and read from the
//! index after that.

use crate::config::Config;
use crate::state::AppState;
use noceur_index::sets::{self, LinkedSet, SetLink, SetTrack};
use noceur_index::Db;
use noceur_online::trackid::{self, SetSummary};
use std::path::PathBuf;
use tauri::{Emitter, State};

async fn with_db<T: Send + 'static>(
    state: &State<'_, AppState>,
    job: impl FnOnce(&Db) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let db = state.db.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;
        job(&db)
    })
    .await
    .map_err(|e| format!("set task failed: {e}"))?
}

/// The path of an indexed audio file.
fn audio_path(db: &Db, file_id: i64) -> Result<PathBuf, String> {
    noceur_index::thumb::locate(db, file_id)
        .map_err(|e| e.to_string())?
        .filter(|f| f.kind == 1)
        .map(|f| f.path)
        .ok_or_else(|| "that row is not an audio file".to_string())
}

fn key() -> Result<String, String> {
    let key = Config::load(&Config::default_path()).trackid_key();
    if key.is_empty() {
        Err("TrackID needs your API client; add it in Settings".into())
    } else {
        Ok(key)
    }
}

/// Searches TrackID's finished sets.
#[tauri::command]
pub async fn set_search(query: String) -> Result<Vec<SetSummary>, String> {
    let key = key()?;
    tauri::async_runtime::spawn_blocking(move || {
        trackid::search_sets(&query, &key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("search failed: {e}"))?
}

/// The tracklist a file is linked to, or `None`.
#[tauri::command]
pub async fn set_tracklist(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Option<LinkedSet>, String> {
    with_db(&state, move |db| {
        let path = audio_path(db, file_id)?;
        sets::get(db, &path).map_err(|e| e.to_string())
    })
    .await
}

/// Fetches a set's tracklist from TrackID and links the file to it.
///
/// The request is made without the index held: it can take seconds, and every
/// listing waits on that lock.
#[tauri::command]
pub async fn set_link(
    state: State<'_, AppState>,
    file_id: i64,
    slug: String,
) -> Result<LinkedSet, String> {
    let key = key()?;
    let path = with_db(&state, move |db| audio_path(db, file_id)).await?;

    let (link, tracks) = fetch(slug, key).await?;

    with_db(&state, move |db| {
        sets::link(db, &path, &link, &tracks).map_err(|e| e.to_string())?;
        sets::get(db, &path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "the link was not stored".to_string())
    })
    .await
}

/// A set's tracklist from TrackID, without linking it to anything: what a
/// search result is looked at with before it is chosen.
#[tauri::command]
pub async fn set_preview(slug: String) -> Result<LinkedSet, String> {
    let (link, tracks) = fetch(slug, key()?).await?;
    Ok(LinkedSet { link, tracks })
}

/// Reads a set from TrackID as the index stores it.
async fn fetch(slug: String, key: String) -> Result<(SetLink, Vec<SetTrack>), String> {
    let detail = tauri::async_runtime::spawn_blocking(move || {
        trackid::fetch_set(&slug, &key).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("fetching the set failed: {e}"))??;

    let link = SetLink {
        source: "trackid".into(),
        slug: detail.summary.slug.clone(),
        title: detail.summary.title.clone(),
        channel: detail.summary.channel.clone(),
        url: detail.summary.url.clone(),
        duration_ms: detail.summary.duration_ms.map(|d| d as i64),
        offset_ms: 0,
        artwork: detail.summary.artwork.clone(),
        published: detail.summary.published.clone(),
    };
    let tracks = detail
        .tracks
        .iter()
        .map(|t| SetTrack {
            start_ms: t.start_ms as i64,
            end_ms: t.end_ms as i64,
            artist: t.artist.clone(),
            title: t.title.clone(),
            label: t.label.clone(),
            plays: i64::from(t.plays),
        })
        .collect();
    Ok((link, tracks))
}

/// Forgets a file's tracklist.
#[tauri::command]
pub async fn set_unlink(state: State<'_, AppState>, file_id: i64) -> Result<(), String> {
    with_db(&state, move |db| {
        let path = audio_path(db, file_id)?;
        sets::unlink(db, &path).map_err(|e| e.to_string())
    })
    .await
}

/// Moves a file's tracklist by `offset_ms` to line it up with this copy.
#[tauri::command]
pub async fn set_offset(
    state: State<'_, AppState>,
    file_id: i64,
    offset_ms: i64,
) -> Result<Option<LinkedSet>, String> {
    with_db(&state, move |db| {
        let path = audio_path(db, file_id)?;
        sets::set_offset(db, &path, offset_ms).map_err(|e| e.to_string())?;
        sets::get(db, &path).map_err(|e| e.to_string())
    })
    .await
}

/// A place a track was played, for the sidebar.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub set_title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    /// The set's file, when the index has it; `None` when its drive is not
    /// attached or it was moved outside the app.
    pub file_id: Option<i64>,
}

/// The linked sets a file's track was played in.
///
/// Matched on the tags, or on `Artist - Title` in the name when the tags say
/// nothing. A set does not list itself.
#[tauri::command]
pub async fn set_appearances(
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<Vec<Appearance>, String> {
    let path = with_db(&state, move |db| audio_path(db, file_id)).await?;

    // Read outside the lock: a tag read is a disk read.
    let (artist, title) = {
        let path = path.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let meta = noceur_tags::read(&path).ok();
            let artist = meta.as_ref().and_then(|m| m.artist.clone()).unwrap_or_default();
            let title = meta.as_ref().and_then(|m| m.title.clone()).unwrap_or_default();
            if !title.trim().is_empty() {
                return (artist, title);
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            match stem.split_once(" - ") {
                Some((a, t)) => (a.to_string(), t.to_string()),
                None => (artist, stem),
            }
        })
        .await
        .map_err(|e| format!("reading tags failed: {e}"))?
    };

    with_db(&state, move |db| {
        let found = sets::appearances(db, &artist, &title, Some(&path)).map_err(|e| e.to_string())?;
        Ok(found
            .into_iter()
            .map(|a| Appearance {
                set_title: a.set_title,
                start_ms: a.start_ms,
                end_ms: a.end_ms,
                file_id: a.row.map(|r| r.file_id),
            })
            .collect())
    })
    .await
}

/// How far an upload has got, for the panel's progress.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadProgress {
    file_id: i64,
    sent: u64,
    total: u64,
}

/// Uploads a set to TrackID, privately, under its file name.
///
/// Returns once TrackID has the file. Listening to it takes TrackID minutes to
/// hours after that; the set then turns up in a search, and the panel links it.
#[tauri::command]
pub async fn set_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_id: i64,
) -> Result<String, String> {
    let session = Config::load(&Config::default_path()).trackid_session;
    if session.trim().is_empty() {
        return Err("Uploading needs your trackid.net sign-in; add it in Settings".into());
    }
    let path = with_db(&state, move |db| audio_path(db, file_id)).await?;
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let sent_title = title.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let report = move |sent: u64, total: u64| {
            let _ = app.emit("set-upload-progress", UploadProgress { file_id, sent, total });
        };
        trackid::upload(&path, &sent_title, &session, &report).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("upload failed: {e}"))??;
    Ok(title)
}
