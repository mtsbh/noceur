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
    pub probe_ms: u64,
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
