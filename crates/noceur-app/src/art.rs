//! Serving cover-art thumbnails to the web view.
//!
//! Thumbnails travel over a custom URI scheme rather than through a Tauri
//! command. A command would have to marshal every image through JSON as base64,
//! on the same thread that is trying to keep a scrolling list at refresh rate.
//! As a URL the web view fetches them itself, decodes them off the main thread,
//! and keeps its own memory cache of what it has already drawn — all of which is
//! work we would otherwise have to reimplement badly.

use crate::state::AppState;
use noceur_art::ThumbSize;
use noceur_index::thumb::{self, Cached};
use noceur_index::Db;
use tauri::http::{Request, Response, StatusCode};

/// Parses `/{file_id}?size={small|medium|large}` out of a request URI.
///
/// Returns `None` for anything malformed, which the caller turns into a 400. The
/// web view is the only thing that builds these URLs, so a malformed one is a bug
/// here rather than something to accommodate.
fn parse(uri: &str) -> Option<(i64, ThumbSize)> {
    let uri = uri.split("://").nth(1).unwrap_or(uri);
    let path_and_query = uri.split_once('/').map(|(_, rest)| rest).unwrap_or("");

    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_and_query, ""),
    };

    let file_id: i64 = path.trim_matches('/').parse().ok()?;
    let size = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("size="))
        .and_then(ThumbSize::parse)
        .unwrap_or(ThumbSize::Small);

    Some((file_id, size))
}

/// Produces the PNG for one file at one size, rendering and caching on a miss.
///
/// `Ok(None)` means "this file has no usable cover art" and is a normal answer,
/// not a failure: most libraries are full of untagged files.
fn png_for(db: &Db, file_id: i64, size: ThumbSize) -> Result<Option<Vec<u8>>, String> {
    let Some(found) = thumb::locate(db, file_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };

    match thumb::get(db, found.content_id, size.as_str()).map_err(|e| e.to_string())? {
        Cached::Png(png) => return Ok(Some(png)),
        Cached::Miss => return Ok(None),
        Cached::Unknown => {}
    }

    // The probe already recorded whether this file has a picture at all. Trusting
    // it here turns the common case — a folder of untagged WAVs — from one failed
    // decode per row into no work at all.
    if !found.has_artwork {
        thumb::put(db, found.content_id, size.as_str(), None).map_err(|e| e.to_string())?;
        return Ok(None);
    }

    // A file that will not decode is recorded as a miss rather than propagated:
    // one corrupt picture must cost its own row and nothing else.
    let rendered = noceur_art::thumbnail(&found.path, size)
        .ok()
        .flatten()
        .map(|t| t.png);

    thumb::put(db, found.content_id, size.as_str(), rendered.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(rendered)
}

/// Handles one `thumb://` request.
///
/// Runs on the caller's thread, which is not the UI thread — Tauri dispatches
/// asynchronous protocol handlers off the main loop, so a slow decode delays one
/// image rather than the window.
pub fn respond(state: &AppState, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some((file_id, size)) = parse(&request.uri().to_string()) else {
        return reply(StatusCode::BAD_REQUEST, Vec::new());
    };

    let Ok(db) = state.db.lock() else {
        return reply(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
    };

    match png_for(&db, file_id, size) {
        // 404 is the honest answer for "no cover art", and the <img> onerror
        // path in the frontend is exactly how a row falls back to its placeholder.
        Ok(None) => reply(StatusCode::NOT_FOUND, Vec::new()),
        Ok(Some(png)) => reply(StatusCode::OK, png),
        Err(_) => reply(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
    }
}

fn reply(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", "image/png")
        // Thumbnails are keyed by content and size and never change under a given
        // URL, so the web view may hold them for as long as it likes. This is what
        // keeps a scroll back over already-seen rows from touching Rust at all.
        .header("Cache-Control", "public, max-age=31536000, immutable")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_else(|_| Response::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_file_id_and_size() {
        assert_eq!(
            parse("thumb://localhost/42?size=large"),
            Some((42, ThumbSize::Large))
        );
    }

    /// Windows serves custom schemes over `http://thumb.localhost/...` instead,
    /// so both spellings have to land on the same file.
    #[test]
    fn parses_the_windows_http_form_identically() {
        assert_eq!(
            parse("http://thumb.localhost/42?size=large"),
            Some((42, ThumbSize::Large))
        );
    }

    #[test]
    fn defaults_to_small_when_no_size_is_given() {
        assert_eq!(parse("thumb://localhost/7"), Some((7, ThumbSize::Small)));
    }

    #[test]
    fn tolerates_other_query_parameters_around_the_size() {
        assert_eq!(
            parse("thumb://localhost/7?v=2&size=medium&x=1"),
            Some((7, ThumbSize::Medium))
        );
    }

    #[test]
    fn rejects_a_non_numeric_file_id() {
        assert_eq!(parse("thumb://localhost/not-a-number?size=small"), None);
        assert_eq!(parse("thumb://localhost/"), None);
    }
}
