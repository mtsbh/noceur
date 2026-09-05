//! Serving cover-art thumbnails to the web view.
//!
//! Thumbnails travel over a custom URI scheme rather than through a Tauri
//! command. A command would have to marshal every image through JSON as base64,
//! on the same thread that is trying to keep a scrolling list at refresh rate.
//! As a URL the web view fetches them itself, decodes them off the main thread,
//! and keeps its own memory cache of what it has already drawn — all of which is
//! work we would otherwise have to reimplement badly.

use crate::state::AppState;
use noceur_art::shell::{self, IconKey};
use noceur_art::ThumbSize;
use noceur_index::thumb::{self, Cached};
use noceur_index::Db;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tauri::http::{Request, Response, StatusCode};

/// What a `thumb://` URL is asking for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    /// A row in the listing, identified by its `file` id.
    Row(i64),
    /// A directory named directly by path. The tree reads the filesystem rather
    /// than the index, so its nodes have no row to point at.
    Folder(String),
}

/// What size a `thumb://` URL is asking for.
///
/// `Full` is not another thumbnail size — it is the picture the file actually
/// carries, at whatever size it was embedded, served straight out of the tag.
/// The editor shows artwork to be judged rather than recognised, and 88 pixels
/// cannot answer "is this the right cover, and is it worth keeping".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    Thumb(ThumbSize),
    Full,
}

/// Decodes `%XX` escapes. Windows paths arrive with `:` and `\` encoded.
fn percent_decode(s: &str) -> String {
    let raw = s.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses `/{file_id}?size={small|medium|large}[&path={percent-encoded}]`.
///
/// A `path` parameter wins over the id, and the id is then a placeholder the
/// frontend sends because the URL shape requires one. Returns `None` for anything
/// malformed, which the caller turns into a 400: the web view is the only thing
/// that builds these URLs, so a malformed one is a bug here rather than something
/// to accommodate.
fn parse(uri: &str) -> Option<(Target, Want)> {
    let uri = uri.split("://").nth(1).unwrap_or(uri);
    let path_and_query = uri.split_once('/').map(|(_, rest)| rest).unwrap_or("");

    let (path, query) = match path_and_query.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path_and_query, ""),
    };

    let param = |key: &str| {
        query
            .split('&')
            .find_map(|kv| kv.strip_prefix(key))
            .map(|v| v.to_string())
    };

    let raw = param("size=").unwrap_or_default();
    let size = if raw == "full" {
        Want::Full
    } else {
        Want::Thumb(ThumbSize::parse(&raw).unwrap_or(ThumbSize::Small))
    };

    if let Some(folder) = param("path=").filter(|p| !p.is_empty()) {
        return Some((Target::Folder(percent_decode(&folder)), size));
    }

    let file_id: i64 = path.trim_matches('/').parse().ok()?;
    Some((Target::Row(file_id), size))
}

/// Rendered shell icons, kept in memory for the life of the process.
///
/// Not in the database: an icon belongs to the running Windows theme, not to the
/// library, and would go stale the moment the user changed it. The key collapses
/// an entire drive of ordinary folders onto one entry per size, because they all
/// resolve to the same icon file and index — while a folder that sets its own
/// icon through `desktop.ini` still gets its own.
static ICONS: OnceLock<Mutex<HashMap<IconKey, Option<Vec<u8>>>>> = OnceLock::new();

fn icon_cache() -> &'static Mutex<HashMap<IconKey, Option<Vec<u8>>>> {
    ICONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The shell's own icon for a directory, rendered once per icon and size.
fn folder_png(path: &Path, size: ThumbSize) -> Option<Vec<u8>> {
    let key = shell::locate(path, size)?;

    if let Ok(cache) = icon_cache().lock() {
        if let Some(hit) = cache.get(&key) {
            return hit.clone();
        }
    }

    let png = shell::render(&key).ok().map(|t| t.png);
    if let Ok(mut cache) = icon_cache().lock() {
        cache.insert(key, png.clone());
    }
    png
}

/// Produces the PNG for one row at one size, rendering and caching on a miss.
///
/// `Ok(None)` means "nothing to draw here" and is a normal answer, not a failure:
/// most libraries are full of untagged files.
fn png_for(db: &Db, file_id: i64, size: ThumbSize) -> Result<Option<Vec<u8>>, String> {
    let Some(found) = thumb::locate(db, file_id).map_err(|e| e.to_string())? else {
        return Ok(None);
    };

    // These rows are real directories, so they get the icon the shell gives them
    // rather than a glyph of our own invention — custom folder icons included.
    if found.kind == 0 {
        return Ok(folder_png(&found.path, size));
    }

    // A file no probe has linked to content yet has nothing to look up and
    // nowhere to cache the answer.
    let Some(content_id) = found.content_id else {
        return Ok(None);
    };

    match thumb::get(db, content_id, size.as_str()).map_err(|e| e.to_string())? {
        Cached::Png(png) => return Ok(Some(png)),
        Cached::Miss => return Ok(None),
        Cached::Unknown => {}
    }

    // The probe already recorded whether this file has a picture at all. Trusting
    // it here turns the common case — a folder of untagged WAVs — from one failed
    // decode per row into no work at all.
    if !found.has_artwork {
        thumb::put(db, content_id, size.as_str(), None).map_err(|e| e.to_string())?;
        return Ok(None);
    }

    // A file that will not decode is recorded as a miss rather than propagated:
    // one corrupt picture must cost its own row and nothing else.
    let rendered = noceur_art::thumbnail(&found.path, size)
        .ok()
        .flatten()
        .map(|t| t.png);

    thumb::put(db, content_id, size.as_str(), rendered.as_deref()).map_err(|e| e.to_string())?;
    Ok(rendered)
}

/// Handles one `thumb://` request.
///
/// Runs on the caller's thread, which is not the UI thread — Tauri dispatches
/// asynchronous protocol handlers off the main loop, so a slow decode delays one
/// image rather than the window.
pub fn respond(state: &AppState, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some((target, want)) = parse(&request.uri().to_string()) else {
        return reply(StatusCode::BAD_REQUEST, Vec::new());
    };

    // A folder named by path needs no index at all, so it never waits on the
    // database lock — which matters because the tree asks for one of these per
    // visible node. There is no full-size version of a shell icon, so the
    // largest one stands in.
    let size = match want {
        Want::Thumb(size) => size,
        Want::Full => ThumbSize::Large,
    };
    let file_id = match target {
        Target::Folder(path) => {
            return match folder_png(Path::new(&path), size) {
                Some(png) => reply(StatusCode::OK, png),
                None => reply(StatusCode::NOT_FOUND, Vec::new()),
            }
        }
        Target::Row(id) => id,
    };

    let Ok(db) = state.db.lock() else {
        return reply(StatusCode::INTERNAL_SERVER_ERROR, Vec::new());
    };

    if want == Want::Full {
        return match full_art(&db, file_id) {
            Some((mime, bytes)) => uncached(StatusCode::OK, &mime, bytes),
            None => uncached(StatusCode::NOT_FOUND, "image/png", Vec::new()),
        };
    }

    match png_for(&db, file_id, size) {
        // 404 is the honest answer for "no cover art", and the <img> onerror
        // path in the frontend is exactly how a row falls back to its placeholder.
        Ok(None) => reply(StatusCode::NOT_FOUND, Vec::new()),
        Ok(Some(png)) => reply(StatusCode::OK, png),
        Err(_) => reply(StatusCode::INTERNAL_SERVER_ERROR, Vec::new()),
    }
}

/// The picture a file actually carries, with the type the tag declares.
///
/// Read from the file every time rather than cached. There is one of these on
/// screen at a time, in a dialog somebody opened deliberately, and the whole
/// point of showing it is that it is the current truth about the file — a cached
/// copy would go on showing artwork that had just been removed.
fn full_art(db: &Db, file_id: i64) -> Option<(String, Vec<u8>)> {
    let found = thumb::locate(db, file_id).ok().flatten()?;
    if found.kind != 1 {
        return None;
    }
    let art = noceur_tags::artwork(&found.path).ok().flatten()?;
    let mime = art.mime.unwrap_or_else(|| "image/jpeg".to_string());
    Some((mime, art.data))
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

/// Replies with something that must not be remembered.
///
/// The opposite of the thumbnail rule: a full-size cover is served under a URL
/// that says nothing about which picture it holds, and the picture behind it
/// changes the moment somebody replaces or removes it.
fn uncached(status: StatusCode, mime: &str, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Cache-Control", "no-store")
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
            Some((Target::Row(42), Want::Thumb(ThumbSize::Large)))
        );
    }

    /// Windows serves custom schemes over `http://thumb.localhost/...` instead,
    /// so both spellings have to land on the same file.
    #[test]
    fn parses_the_windows_http_form_identically() {
        assert_eq!(
            parse("http://thumb.localhost/42?size=large"),
            Some((Target::Row(42), Want::Thumb(ThumbSize::Large)))
        );
    }

    #[test]
    fn defaults_to_small_when_no_size_is_given() {
        assert_eq!(
            parse("thumb://localhost/7"),
            Some((Target::Row(7), Want::Thumb(ThumbSize::Small)))
        );
    }

    #[test]
    fn tolerates_other_query_parameters_around_the_size() {
        assert_eq!(
            parse("thumb://localhost/7?v=2&size=medium&x=1"),
            Some((Target::Row(7), Want::Thumb(ThumbSize::Medium)))
        );
    }

    /// The editor asks for the picture itself rather than a rendering of it.
    #[test]
    fn full_is_a_request_for_the_embedded_picture() {
        assert_eq!(
            parse("thumb://localhost/42?size=full"),
            Some((Target::Row(42), Want::Full))
        );
    }

    #[test]
    fn rejects_a_non_numeric_file_id() {
        assert_eq!(parse("thumb://localhost/not-a-number?size=small"), None);
        assert_eq!(parse("thumb://localhost/"), None);
    }

    /// The tree names folders by path because it never touches the index.
    #[test]
    fn a_path_parameter_names_a_folder_directly() {
        assert_eq!(
            parse("thumb://localhost/0?size=small&path=E%3A%5Cmusic%5Cdownloads"),
            Some((
                Target::Folder("E:\\music\\downloads".into()),
                Want::Thumb(ThumbSize::Small)
            ))
        );
    }

    /// A path wins over the id, so the placeholder the frontend sends is ignored.
    #[test]
    fn a_path_parameter_beats_the_file_id() {
        assert_eq!(
            parse("thumb://localhost/99?size=small&path=C%3A%5C"),
            Some((Target::Folder("C:\\".into()), Want::Thumb(ThumbSize::Small)))
        );
    }

    #[test]
    fn percent_decoding_leaves_ordinary_characters_alone() {
        assert_eq!(percent_decode("E%3A%5Cmusic"), "E:\\music");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a%20b"), "a b");
        // A stray escape at the end must not panic on a short slice.
        assert_eq!(percent_decode("trailing%"), "trailing%");
    }

    #[test]
    fn non_ascii_paths_survive_the_round_trip() {
        // "Rêves" as UTF-8 percent escapes.
        assert_eq!(percent_decode("R%C3%AAves"), "Rêves");
    }
}
