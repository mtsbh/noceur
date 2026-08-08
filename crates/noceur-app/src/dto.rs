//! Types crossing the IPC boundary.

use crate::format::RowDisplay;

/// Everything the UI needs to render one directory listing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// The path as a person should read it — never the `\\?\` form.
    pub path: String,
    /// Breadcrumb segments, root first.
    pub segments: Vec<Segment>,
    pub rows: Vec<RowDisplay>,
    /// Total rows in the view, ignoring any window.
    pub total: i64,
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
