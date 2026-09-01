//! Types crossing the IPC boundary.

use crate::format::RowDisplay;

/// Everything the UI needs to render one directory listing *except* its rows.
///
/// Rows arrive separately, a window at a time, so that opening a 40k-file folder
/// costs one `COUNT(*)` rather than forty thousand serialized rows. The first
/// window rides along with the listing: without it every navigation would paint
/// a screen of placeholders before the follow-up call landed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// Identifies the scanned directory for subsequent [`RowsRequest`]s.
    pub dir_id: i64,
    /// The path as a person should read it — never the `\\?\` form.
    pub path: String,
    /// Breadcrumb segments, root first.
    pub segments: Vec<Segment>,
    /// Total rows in the view, ignoring any window.
    pub total: i64,
    /// The window starting at row 0, so the first screen needs no round trip.
    pub first: RowWindow,
    /// How long enumeration took, so slow network paths are visible.
    pub scan_ms: u64,
}

/// One clickable breadcrumb segment.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    /// Text to show.
    pub label: String,
    /// Full path this segment navigates to.
    pub path: String,
}

/// Options the UI can set on a listing.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRequest {
    pub path: String,
    #[serde(default)]
    pub audio_only: bool,
    #[serde(flatten)]
    pub order: Order,
}

/// How a listing is ordered.
///
/// Shared by the request that opens a directory and the requests that read
/// windows of it, because a window cut in a different order than the listing it
/// belongs to shows duplicated and missing rows rather than an error.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    /// Wire name of the sort column. Unrecognised values are rejected rather than
    /// quietly falling back, so a frontend asking for a column that does not
    /// exist is a visible failure.
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub desc: bool,
}

/// A request for one window of an already-opened directory.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowsRequest {
    pub dir_id: i64,
    pub offset: i64,
    pub limit: i64,
    /// Must match the [`ListRequest`] that opened the directory, or the window
    /// will be cut from a differently-filtered list than the one being scrolled.
    #[serde(default)]
    pub audio_only: bool,
    #[serde(flatten)]
    pub order: Order,
}

/// A window of rows, carrying the offset it was cut from.
///
/// The offset is echoed back because windows are requested while scrolling and
/// may complete out of order; without it the frontend cannot tell which part of
/// the list a late reply belongs to.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowWindow {
    pub offset: i64,
    pub rows: Vec<RowDisplay>,
}

/// What one probe batch accomplished.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeProgress {
    /// Files this batch attempted to read.
    pub probed: i64,
    /// Nothing left to probe in this directory.
    pub done: bool,
    pub elapsed_ms: u64,
}

/// What the user is looking for, and where.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    pub query: String,
    /// Directory subtrees to search: the pinned folders plus the current tree.
    /// Empty searches everything indexed.
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// One search result, formatted for display.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub file_id: i64,
    /// 0 directory, 1 audio, 2 other.
    pub kind: i64,
    pub name: String,
    /// Folder holding it, as a person should read it.
    pub dir_path: String,
    pub bpm: String,
    pub key: String,
    pub duration: String,
    pub artist: String,
}

/// What the transport bar shows.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transport {
    /// Path of the loaded track, readable form, or empty when nothing is loaded.
    pub path: String,
    /// Just the file name, which is what the bar has room for.
    pub name: String,
    pub playing: bool,
    pub position_ms: u64,
    /// 0 when the decoder never reported one.
    pub duration_ms: u64,
    /// `mm:ss` of the position and the duration, formatted once here.
    pub position: String,
    pub duration: String,
    /// Why the last command did nothing, if it did nothing.
    pub error: String,
    /// The row being played, so the bar can show its art and tags.
    pub file_id: i64,
    pub artist: String,
    pub title: String,
    pub bpm: String,
    pub key: String,
    pub key_norm: Option<i64>,
    /// 0..100.
    pub volume: u32,
}

/// The tags of a selection, as the editor should show them.
///
/// Every field is `Option<String>`, and the two cases mean different things.
/// `Some("")` is "every file in the selection agrees that this is empty";
/// `None` is "they disagree". The editor shows the first as a blank box and the
/// second as a placeholder, because typing into a blank box that meant
/// "different values" and having it write that blank to twelve files would
/// destroy tags nobody asked to touch.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagForm {
    /// How many files the form covers.
    pub count: i64,
    /// The file name when there is one file, else how many there are.
    pub subject: String,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    pub year: Option<String>,
    pub track_no: Option<String>,
    pub bpm: Option<String>,
    pub key: Option<String>,
}

/// What the editor asks to change.
///
/// A field the user never touched is absent, which means leave it alone. A field
/// present but empty is a request to clear the tag. That distinction is the
/// whole reason this is not just a `TagForm` sent back.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TagEdit {
    /// Named fields this app does not model — `ENERGY`, `MOOD`, whatever a
    /// quick-tag group is pointed at. An empty value clears the field, matching
    /// what an emptied box means everywhere else.
    pub custom: std::collections::HashMap<String, String>,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    pub year: Option<String>,
    pub track_no: Option<String>,
    pub bpm: Option<String>,
    pub key: Option<String>,
}

/// What happened to each file a write touched.
///
/// Reported per file rather than as one pass/fail, because a selection of twenty
/// where one is read-only should write the other nineteen and say which one it
/// could not — not refuse the lot, and not claim success.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteReport {
    pub written: i64,
    /// One entry per file that failed, naming it and why.
    pub failures: Vec<WriteFailure>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteFailure {
    pub name: String,
    pub reason: String,
}
