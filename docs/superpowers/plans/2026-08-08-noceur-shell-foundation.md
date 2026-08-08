# Noceur Shell Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the headless engine into an application — a Tauri window that browses a real music library, showing name, BPM, key, duration and artist per row, with a working breadcrumb and back/forward history.

**Architecture:** A new `noceur-app` crate is the only Tauri-aware code in the workspace; it owns commands, app state and all display formatting. Enumeration and probing run on a blocking thread pool so a cold SMB mount can never freeze the window. The frontend is Solid + Vite and is deliberately dumb — it renders pre-formatted strings the Rust side hands it.

**Tech Stack:** Tauri v2 (`tauri-cli 2.11.4`), Solid + Vite + TypeScript, `serde`, plus the existing `noceur-fsmodel` / `noceur-tags` / `noceur-index` crates. Node 24.5, pnpm 11.18, WebView2 151 confirmed present.

---

## Prerequisites

Confirmed on this machine: Rust 1.97.1, Node v24.5.0, pnpm 11.18.0, `tauri-cli 2.11.4`, WebView2 Runtime 151. MSVC build tools present (already required by `rusqlite`'s bundled feature).

## Scope

### In

- `path::simplify()` — the inverse of `extended()`, so `\\?\` never reaches the user (carryforward from milestone 1)
- Provisional volume resolution, so real drives get real `volume` rows
- The `noceur-app` Tauri crate: state, async commands, display formatting
- Off-thread enumeration and probing (carryforward from milestone 1)
- A Solid frontend that opens, lists a folder, navigates into directories, and goes back and forward
- The Sodium theme applied

### Out — each has its own milestone

Virtualized rendering, resizable/reorderable/persisted columns, the folder tree, pinned folders, split panes, tabs, search, file operations, waveform, playback, the action toolbar, tag writing. This milestone renders a plain scrolling list; it will visibly struggle above a few thousand rows and that is expected — virtualization is next.

Also out: real filesystem volume identity (`GetVolumeInformationW` and friends). Task 2 uses a documented provisional scheme that milestone "identity and freshness" replaces.

## File Structure

```
crates/noceur-fsmodel/src/path.rs        + simplify()
crates/noceur-index/src/volume.rs        + for_path()
crates/noceur-index/Cargo.toml           + serde
crates/noceur-index/src/query.rs         + Serialize on ListRow
crates/noceur-app/
  Cargo.toml
  build.rs                               tauri-build
  tauri.conf.json                        window + dev server config
  src/main.rs                            entry point, builder wiring
  src/state.rs                           AppState: the index behind a mutex
  src/format.rs                          all display formatting, unit-tested
  src/dto.rs                             wire types the frontend receives
  src/commands.rs                        Tauri commands, all off-thread
ui/
  package.json
  vite.config.ts
  tsconfig.json
  index.html
  src/index.tsx                          mount point
  src/App.tsx                            breadcrumb + list + navigation state
  src/api.ts                             typed wrappers over invoke()
  src/theme.css                          Sodium tokens
```

`format.rs` exists so display rules are testable with `cargo test` and live in exactly one place. The frontend never computes a duration string, a BPM precision, or a key notation.

---

## Task 1: `path::simplify()`

The inverse of `extended()`. `std::fs::canonicalize` returns Windows verbatim paths, so without this the UI would show `\\?\E:\music` in its breadcrumb and `cannot read directory \\?\...` in its errors.

**Files:**
- Modify: `crates/noceur-fsmodel/src/path.rs`

- [ ] **Step 1: Write the failing tests**

Add this test module content inside the existing `#[cfg(test)] mod tests` block in `crates/noceur-fsmodel/src/path.rs`, after the existing tests:

```rust
    #[cfg(windows)]
    #[test]
    fn simplify_strips_the_drive_prefix() {
        assert_eq!(
            simplify(Path::new(r"\\?\C:\Music\track.wav")),
            PathBuf::from(r"C:\Music\track.wav")
        );
    }

    #[cfg(windows)]
    #[test]
    fn simplify_strips_the_unc_prefix() {
        assert_eq!(
            simplify(Path::new(r"\\?\UNC\nas\sets\friday")),
            PathBuf::from(r"\\nas\sets\friday")
        );
    }

    #[cfg(windows)]
    #[test]
    fn simplify_leaves_ordinary_paths_alone() {
        assert_eq!(
            simplify(Path::new(r"C:\Music")),
            PathBuf::from(r"C:\Music")
        );
    }

    #[test]
    fn simplify_round_trips_with_extended() {
        let original = if cfg!(windows) {
            PathBuf::from(r"C:\Music\House")
        } else {
            PathBuf::from("/music/house")
        };
        assert_eq!(simplify(&extended(&original)), original);
    }

    #[test]
    fn simplify_is_idempotent() {
        let p = if cfg!(windows) {
            PathBuf::from(r"C:\Music")
        } else {
            PathBuf::from("/music")
        };
        assert_eq!(simplify(&simplify(&p)), simplify(&p));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-fsmodel simplify`
Expected: FAIL with `cannot find function 'simplify' in this scope`.

- [ ] **Step 3: Write the implementation**

In `crates/noceur-fsmodel/src/path.rs`, add below the existing `extended` functions and above the `#[cfg(test)]` module:

```rust
/// Windows UNC extended prefix.
const EXT_UNC_PREFIX: &str = r"\\?\UNC\";

/// Removes the `\\?\` extended-length prefix for display.
///
/// The inverse of [`extended`]. `std::fs::canonicalize` returns verbatim paths on
/// Windows, so this must be applied at every boundary where a path becomes text a
/// person reads — breadcrumbs, error messages, tooltips. Never apply it before a
/// filesystem call; that is what [`extended`] is for.
#[cfg(windows)]
pub fn simplify(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if let Some(rest) = s.strip_prefix(EXT_UNC_PREFIX) {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = s.strip_prefix(EXT_PREFIX) {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

/// Identity on non-Windows platforms. See the Windows implementation for details.
#[cfg(not(windows))]
pub fn simplify(path: &Path) -> PathBuf {
    path.to_path_buf()
}
```

- [ ] **Step 4: Re-export it**

In `crates/noceur-fsmodel/src/lib.rs`, the `path` module is already public. Add to the re-export list so callers can use `noceur_fsmodel::simplify`:

```rust
pub use path::{extended, simplify};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-fsmodel`
Expected: PASS, 31 tests (26 existing + 5 new).

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-fsmodel/src/
git commit -m "feat(fsmodel): add simplify() to strip extended-length prefixes for display"
```

---

## Task 2: Provisional volume resolution

`scan_dir` needs a `volume_id`. The CLI hardcodes a fake one, which is fine for a debug tool and wrong for an app. Real filesystem identity (volume serial numbers on Windows, UUIDs elsewhere) belongs to the identity-and-freshness milestone; this task provides a documented placeholder so the shell can index real drives now.

**Files:**
- Modify: `crates/noceur-index/src/volume.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `crates/noceur-index/src/volume.rs`:

```rust
    #[test]
    fn resolves_a_volume_for_a_path() {
        let db = db();
        let p = if cfg!(windows) {
            std::path::PathBuf::from(r"C:\Music\House")
        } else {
            std::path::PathBuf::from("/music/house")
        };
        let id = for_path(&db, &p).unwrap();
        assert!(id > 0);
    }

    #[test]
    fn two_paths_on_one_volume_resolve_to_the_same_row() {
        let db = db();
        let (a, b) = if cfg!(windows) {
            (r"C:\Music\House", r"C:\Other\Place")
        } else {
            ("/music/house", "/other/place")
        };
        let ia = for_path(&db, std::path::Path::new(a)).unwrap();
        let ib = for_path(&db, std::path::Path::new(b)).unwrap();
        assert_eq!(ia, ib);
        assert_eq!(all(&db).unwrap().len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn different_drive_letters_are_different_volumes() {
        let db = db();
        let c = for_path(&db, std::path::Path::new(r"C:\Music")).unwrap();
        let e = for_path(&db, std::path::Path::new(r"E:\music")).unwrap();
        assert_ne!(c, e);
        assert_eq!(all(&db).unwrap().len(), 2);
    }

    #[cfg(windows)]
    #[test]
    fn a_unc_share_is_its_own_volume() {
        let db = db();
        let local = for_path(&db, std::path::Path::new(r"C:\Music")).unwrap();
        let share = for_path(&db, std::path::Path::new(r"\\nas\sets\friday")).unwrap();
        assert_ne!(local, share);

        let v = get(&db, share).unwrap().unwrap();
        assert_eq!(v.kind, VolumeKind::Network);
    }

    #[test]
    fn resolution_is_case_insensitive_on_windows_only() {
        let db = db();
        if cfg!(windows) {
            let lower = for_path(&db, std::path::Path::new(r"c:\music")).unwrap();
            let upper = for_path(&db, std::path::Path::new(r"C:\MUSIC")).unwrap();
            assert_eq!(lower, upper);
        } else {
            // On Unix the root is always "/" regardless of case further down.
            let a = for_path(&db, std::path::Path::new("/Music")).unwrap();
            let b = for_path(&db, std::path::Path::new("/music")).unwrap();
            assert_eq!(a, b);
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-index volume`
Expected: FAIL with `cannot find function 'for_path' in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `crates/noceur-index/src/volume.rs`, above the `#[cfg(test)]` module:

```rust
use std::path::Path;

/// Registers (or finds) the volume containing `path` and returns its id.
///
/// # Provisional
///
/// Identity is derived from the path's root — the drive letter on Windows, the
/// share root for UNC paths, `/` on Unix — **not** from a real filesystem
/// identifier. That means a removable drive reassigned from `S:` to `E:` currently
/// reads as a different volume, which is exactly what `fs_uuid` was introduced to
/// prevent.
///
/// This is deliberate and bounded: real identity (volume serial numbers on Windows,
/// UUIDs elsewhere) lands in the identity-and-freshness milestone, and only this
/// function changes when it does. Everything downstream already keys on the
/// returned id.
pub fn for_path(db: &Db, path: &Path) -> Result<i64, IndexError> {
    let (uuid, kind, label, mount) = root_of(path);
    upsert(db, &uuid, kind, &label, &mount)
}

/// Splits a path into its volume root and a provisional identity for it.
fn root_of(path: &Path) -> (String, VolumeKind, String, String) {
    let s = path.to_string_lossy();

    #[cfg(windows)]
    {
        // UNC share: \\server\share\... -> the \\server\share pair is the volume.
        if let Some(rest) = s.strip_prefix(r"\\") {
            let mut parts = rest.split(['\\', '/']).filter(|p| !p.is_empty());
            let server = parts.next().unwrap_or("");
            let share = parts.next().unwrap_or("");
            let mount = format!(r"\\{server}\{share}");
            return (
                format!("provisional:unc:{}", mount.to_lowercase()),
                VolumeKind::Network,
                share.to_string(),
                mount,
            );
        }
        // Drive letter.
        if let Some(letter) = s.chars().next().filter(|c| c.is_ascii_alphabetic()) {
            let letter = letter.to_ascii_uppercase();
            let mount = format!(r"{letter}:\");
            return (
                format!("provisional:drive:{letter}"),
                VolumeKind::Fixed,
                format!("{letter}:"),
                mount,
            );
        }
    }

    #[cfg(not(windows))]
    {
        let _ = &s;
    }

    (
        "provisional:root".to_string(),
        VolumeKind::Fixed,
        "/".to_string(),
        "/".to_string(),
    )
}
```

- [ ] **Step 4: Re-export it**

In `crates/noceur-index/src/lib.rs`, extend the volume re-export line:

```rust
pub use volume::{for_path, Volume, VolumeKind};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS, 47 tests (42 existing + 5 new).

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-index/src/
git commit -m "feat(index): resolve a volume from a path (provisional, root-derived)"
```

---

## Task 3: Serialize the wire types

The frontend needs `ListRow` over IPC. `serde` is a data-format concern, not a UI framework, so deriving `Serialize` in the index crate does not violate the rule that engine crates stay Tauri-free.

**Files:**
- Modify: `crates/noceur-index/Cargo.toml`
- Modify: `crates/noceur-index/src/query.rs`

- [ ] **Step 1: Add the dependency**

In `crates/noceur-index/Cargo.toml`, add to `[dependencies]`:

```toml
serde = { version = "1", features = ["derive"] }
```

- [ ] **Step 2: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `crates/noceur-index/src/query.rs`:

```rust
    #[test]
    fn list_rows_serialize_to_json() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let json = serde_json::to_string(&rows).expect("ListRow must serialize");
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"keyNorm\""), "fields must be camelCase for the UI");
    }
```

Add `serde_json = "1"` to `[dev-dependencies]` in the same manifest.

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p noceur-index list_rows_serialize`
Expected: FAIL — `the trait bound 'ListRow: Serialize' is not satisfied`.

- [ ] **Step 4: Derive Serialize**

In `crates/noceur-index/src/query.rs`, change the `ListRow` derive line from:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
```

to:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRow {
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS, 48 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): serialize ListRow as camelCase for the UI layer"
```

---

## Task 4: Display formatting

Every string the user reads is produced here and nowhere else. Putting this in Rust rather than TypeScript means one implementation, unit-tested with `cargo test`, and a frontend that cannot drift from it.

**Files:**
- Create: `crates/noceur-app/Cargo.toml`
- Create: `crates/noceur-app/src/format.rs`
- Create: `crates/noceur-app/src/main.rs` (stub, replaced in Task 5)

- [ ] **Step 1: Create the crate**

Create `crates/noceur-app/Cargo.toml`:

```toml
[package]
name = "noceur-app"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
noceur-fsmodel = { path = "../noceur-fsmodel" }
noceur-tags = { path = "../noceur-tags" }
noceur-index = { path = "../noceur-index" }
serde = { version = "1", features = ["derive"] }
```

Create `crates/noceur-app/src/main.rs`:

```rust
//! Noceur desktop shell.

mod format;

fn main() {
    println!("noceur-app: shell not wired yet");
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/noceur-app/src/format.rs`:

```rust
//! Every string the user reads is produced here.
//!
//! Formatting lives in Rust rather than the frontend so there is exactly one
//! implementation, it is unit-tested, and the UI cannot drift from it.

use noceur_index::ListRow;
use noceur_tags::{KeyNotation, MusicalKey};

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ListRow {
        ListRow {
            file_id: 1,
            name: "track.wav".into(),
            ext: "wav".into(),
            size: 1024,
            mtime: 0,
            kind: 1,
            state: "ok".into(),
            artist: Some("G76".into()),
            title: Some("it takes time".into()),
            album: None,
            bpm: Some(123.0),
            key_tag: Some("8A".into()),
            key_norm: Some(14),
            duration_ms: Some(671_000),
        }
    }

    #[test]
    fn duration_is_minutes_and_padded_seconds() {
        assert_eq!(duration(Some(671_000)), "11:11");
        assert_eq!(duration(Some(6_000)), "0:06");
        assert_eq!(duration(Some(3_600_000)), "60:00");
    }

    #[test]
    fn absent_duration_is_an_em_dash() {
        assert_eq!(duration(None), "—");
    }

    #[test]
    fn bpm_from_tags_is_marked_with_a_trailing_dot() {
        // Milestone 1 has no analysis, so every BPM is tag-derived.
        assert_eq!(bpm(Some(123.0), BpmSource::Tag), "123·");
        assert_eq!(bpm(Some(124.5), BpmSource::Tag), "124.5·");
    }

    #[test]
    fn analysed_bpm_is_shown_at_one_decimal_with_no_marker() {
        assert_eq!(bpm(Some(123.0), BpmSource::Analysed), "123.0");
    }

    #[test]
    fn absent_bpm_is_an_em_dash() {
        assert_eq!(bpm(None, BpmSource::Tag), "—");
        assert_eq!(bpm(None, BpmSource::Analysed), "—");
    }

    #[test]
    fn key_renders_in_the_requested_notation() {
        assert_eq!(key(Some(14), None, KeyNotation::Camelot), "8A");
        assert_eq!(key(Some(14), None, KeyNotation::OpenKey), "1m");
        assert_eq!(key(Some(14), None, KeyNotation::Classical), "Am");
    }

    #[test]
    fn an_unparseable_key_falls_back_to_its_original_text() {
        assert_eq!(key(None, Some("banana"), KeyNotation::Camelot), "banana");
    }

    #[test]
    fn absent_key_is_an_em_dash() {
        assert_eq!(key(None, None, KeyNotation::Camelot), "—");
    }

    #[test]
    fn size_uses_binary_units() {
        assert_eq!(size(0), "0 B");
        assert_eq!(size(1023), "1023 B");
        assert_eq!(size(1024), "1.0 KB");
        assert_eq!(size(1_048_576), "1.0 MB");
        assert_eq!(size(132_120_576), "126.0 MB");
    }

    #[test]
    fn directories_show_no_audio_columns() {
        let mut r = row();
        r.kind = 0;
        r.bpm = Some(999.0);
        r.duration_ms = Some(1000);
        let d = display(&r, KeyNotation::Camelot);
        assert_eq!(d.bpm, "");
        assert_eq!(d.key, "");
        assert_eq!(d.duration, "");
        assert_eq!(d.size, "");
    }

    #[test]
    fn audio_rows_show_every_column() {
        let d = display(&row(), KeyNotation::Camelot);
        assert_eq!(d.name, "track.wav");
        assert_eq!(d.bpm, "123·");
        assert_eq!(d.key, "8A");
        assert_eq!(d.duration, "11:11");
        assert_eq!(d.artist, "G76");
    }

    #[test]
    fn a_bad_state_is_surfaced_rather_than_hidden() {
        let mut r = row();
        r.state = "undecodable".into();
        assert_eq!(display(&r, KeyNotation::Camelot).state, "undecodable");

        let mut ok = row();
        ok.state = "ok".into();
        assert_eq!(display(&ok, KeyNotation::Camelot).state, "");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p noceur-app`
Expected: FAIL with `cannot find function 'duration' in this scope` and similar.

- [ ] **Step 4: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-app/src/format.rs`:

```rust
/// Placeholder shown wherever a value is genuinely absent.
pub const ABSENT: &str = "—";

/// Where a BPM value came from. Analysed values are shown at full precision;
/// tag-derived values are marked so they are never mistaken for our own analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BpmSource {
    Analysed,
    Tag,
}

/// One row, entirely as strings, ready to render.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowDisplay {
    pub file_id: i64,
    /// 0 directory, 1 audio, 2 other — the frontend uses this only for icons.
    pub kind: i64,
    pub name: String,
    pub bpm: String,
    pub key: String,
    pub duration: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub size: String,
    /// Empty when the file is fine; otherwise the reason it is not.
    pub state: String,
}

/// `mm:ss`, or [`ABSENT`].
pub fn duration(ms: Option<i64>) -> String {
    match ms {
        Some(ms) if ms >= 0 => {
            let total = ms / 1000;
            format!("{}:{:02}", total / 60, total % 60)
        }
        _ => ABSENT.to_string(),
    }
}

/// Analysed BPM at one decimal; tag BPM as stored with a trailing marker.
pub fn bpm(value: Option<f64>, source: BpmSource) -> String {
    let Some(v) = value.filter(|v| v.is_finite() && *v > 0.0) else {
        return ABSENT.to_string();
    };
    match source {
        BpmSource::Analysed => format!("{v:.1}"),
        BpmSource::Tag => {
            // Keep the tag's own precision: 123 stays 123, 124.5 stays 124.5.
            let text = if (v.fract()).abs() < f64::EPSILON {
                format!("{}", v as i64)
            } else {
                format!("{v}")
            };
            format!("{text}·")
        }
    }
}

/// The key in the requested notation, falling back to the raw tag text.
pub fn key(norm: Option<i64>, tag: Option<&str>, notation: KeyNotation) -> String {
    if let Some(k) = norm.and_then(|n| MusicalKey::from_canonical(n as i32)) {
        return k.format(notation);
    }
    match tag {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ => ABSENT.to_string(),
    }
}

/// Binary units, one decimal above a kilobyte.
pub fn size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes.max(0) as f64;
    if b < KB {
        return format!("{} B", bytes.max(0));
    }
    for (limit, unit) in [
        (KB * KB, "KB"),
        (KB * KB * KB, "MB"),
        (KB * KB * KB * KB, "GB"),
    ] {
        if b < limit {
            return format!("{:.1} {}", b / (limit / KB), unit);
        }
    }
    format!("{:.1} TB", b / (KB * KB * KB * KB))
}

/// Renders one indexed row into display strings.
///
/// Directories deliberately show nothing in the audio columns even if the index
/// happens to hold values for them — a folder has no BPM.
pub fn display(row: &ListRow, notation: KeyNotation) -> RowDisplay {
    let is_audio = row.kind == 1;
    let blank = String::new();

    RowDisplay {
        file_id: row.file_id,
        kind: row.kind,
        name: row.name.clone(),
        bpm: if is_audio {
            bpm(row.bpm, BpmSource::Tag)
        } else {
            blank.clone()
        },
        key: if is_audio {
            key(row.key_norm, row.key_tag.as_deref(), notation)
        } else {
            blank.clone()
        },
        duration: if is_audio {
            duration(row.duration_ms)
        } else {
            blank.clone()
        },
        artist: row.artist.clone().unwrap_or_default(),
        title: row.title.clone().unwrap_or_default(),
        album: row.album.clone().unwrap_or_default(),
        size: if row.kind == 0 { blank } else { size(row.size) },
        state: if row.state == "ok" {
            String::new()
        } else {
            row.state.clone()
        },
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-app`
Expected: PASS, 11 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-app/
git commit -m "feat(app): add display formatting with the analysed-vs-tagged BPM marker"
```

---

## Task 5: App state and the off-thread list command

The carryforward item from milestone 1: a cold SMB mount stalled 20 seconds on 31 files. Enumeration must never run on the thread that services the window.

**Files:**
- Modify: `crates/noceur-app/Cargo.toml`
- Create: `crates/noceur-app/src/state.rs`
- Create: `crates/noceur-app/src/dto.rs`
- Create: `crates/noceur-app/src/commands.rs`
- Modify: `crates/noceur-app/src/main.rs`

- [ ] **Step 1: Add Tauri to the manifest**

Replace `crates/noceur-app/Cargo.toml` with:

```toml
[package]
name = "noceur-app"
version.workspace = true
edition.workspace = true
license.workspace = true

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
noceur-fsmodel = { path = "../noceur-fsmodel" }
noceur-tags = { path = "../noceur-tags" }
noceur-index = { path = "../noceur-index" }
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
```

Create `crates/noceur-app/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 2: Write the state module**

Create `crates/noceur-app/src/state.rs`:

```rust
//! Application state.

use noceur_index::{Db, IndexError};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// Shared handle to the index.
///
/// `rusqlite::Connection` is `Send` but not `Sync`, so the database lives behind a
/// mutex and is cloned into blocking tasks rather than borrowed across await points.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Db>>,
}

impl AppState {
    /// Opens the index at the given path, creating it if needed.
    pub fn open(path: &std::path::Path) -> Result<Self, IndexError> {
        Ok(AppState {
            db: Arc::new(Mutex::new(Db::open(path)?)),
        })
    }

    /// Where the index lives: alongside the executable, so a portable install
    /// stays portable. Falls back to the working directory.
    pub fn default_db_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("noceur-index.db")))
            .unwrap_or_else(|| PathBuf::from("noceur-index.db"))
    }
}
```

- [ ] **Step 3: Write the DTO module**

Create `crates/noceur-app/src/dto.rs`:

```rust
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
```

- [ ] **Step 4: Write the commands module**

Create `crates/noceur-app/src/commands.rs`:

```rust
//! Tauri commands. Every one that touches the filesystem runs off-thread.

use crate::dto::{DirListing, ListRequest, Segment};
use crate::format;
use crate::state::AppState;
use noceur_fsmodel::path::simplify;
use noceur_index::{list, volume, ListOptions, SortColumn};
use noceur_tags::KeyNotation;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tauri::State;

/// Lists a directory, scanning and probing it first.
///
/// Runs entirely on the blocking pool. A cold SMB mount has been measured at
/// twenty seconds for thirty-one entries; doing this on the main thread would
/// freeze the window for that whole time.
#[tauri::command]
pub async fn list_dir(
    state: State<'_, AppState>,
    req: ListRequest,
) -> Result<DirListing, String> {
    let db = state.db.clone();
    let path = PathBuf::from(&req.path);
    let audio_only = req.audio_only;

    tauri::async_runtime::spawn_blocking(move || {
        let db = db.lock().map_err(|_| "index lock poisoned".to_string())?;

        let started = Instant::now();
        let volume_id = volume::for_path(&db, &path).map_err(|e| e.to_string())?;
        let dir_id = noceur_index::scan_dir(&db, volume_id, &path).map_err(|e| e.to_string())?;
        let scan_ms = started.elapsed().as_millis() as u64;

        let probe_started = Instant::now();
        noceur_index::probe_dir(&db, dir_id).map_err(|e| e.to_string())?;
        let probe_ms = probe_started.elapsed().as_millis() as u64;

        let opts = ListOptions {
            sort: SortColumn::Name,
            desc: false,
            audio_only,
            offset: 0,
            limit: -1,
        };
        let total = noceur_index::count(&db, dir_id, opts.clone()).map_err(|e| e.to_string())?;
        let rows = list(&db, dir_id, opts).map_err(|e| e.to_string())?;

        let display: Vec<_> = rows
            .iter()
            .map(|r| format::display(r, KeyNotation::Camelot))
            .collect();

        let shown = simplify(&path);
        Ok(DirListing {
            path: shown.to_string_lossy().into_owned(),
            segments: segments_of(&shown),
            rows: display,
            total,
            scan_ms,
            probe_ms,
        })
    })
    .await
    .map_err(|e| format!("listing task failed: {e}"))?
}

/// Resolves the parent of a path, or `None` at a root.
#[tauri::command]
pub fn parent_of(path: String) -> Option<String> {
    Path::new(&path)
        .parent()
        .map(|p| simplify(p).to_string_lossy().into_owned())
        .filter(|p| !p.is_empty())
}

/// Breaks a path into breadcrumb segments, root first.
///
/// On Windows a path yields a `Prefix` component (`C:`) followed by a `RootDir`
/// component (`\`). Emitting both would put an empty crumb after the drive letter,
/// and emitting only the prefix would make the first crumb navigate to `C:` — which
/// means "the current directory on C:", not the root. So the root separator is
/// folded into the prefix that precedes it.
fn segments_of(path: &Path) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut acc = PathBuf::new();

    for component in path.components() {
        acc.push(component.as_os_str());
        let raw = component.as_os_str().to_string_lossy();
        let label = raw.trim_end_matches(['\\', '/']).to_string();

        if label.is_empty() {
            if let Some(last) = out.last_mut() {
                last.path = acc.to_string_lossy().into_owned();
            }
            continue;
        }

        out.push(Segment {
            label,
            path: acc.to_string_lossy().into_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn segments_fold_the_root_into_the_drive_letter() {
        let s = segments_of(Path::new(r"C:\Music\House"));
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].label, "C:");
        assert_eq!(s[0].path, r"C:\", "first crumb must navigate to the root");
        assert_eq!(s[1].label, "Music");
        assert_eq!(s[1].path, r"C:\Music");
        assert_eq!(s[2].label, "House");
    }

    #[cfg(windows)]
    #[test]
    fn segments_never_contain_an_empty_label() {
        for p in [r"C:\", r"C:\Music", r"E:\music\artists\ricardo"] {
            assert!(
                segments_of(Path::new(p)).iter().all(|s| !s.label.is_empty()),
                "{p} produced an empty crumb"
            );
        }
    }
}
```

- [ ] **Step 5: Wire up main.rs**

Replace `crates/noceur-app/src/main.rs`:

```rust
//! Noceur desktop shell.
//!
//! The only Tauri-aware crate in the workspace. Everything below it compiles and
//! tests headless.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dto;
mod format;
mod state;

use state::AppState;

fn main() {
    let db_path = AppState::default_db_path();
    let app_state = match AppState::open(&db_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to open index at {}: {e}", db_path.display());
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::list_dir,
            commands::parent_of
        ])
        .run(tauri::generate_context!())
        .expect("failed to start Noceur");
}
```

- [ ] **Step 6: Write the Tauri config**

Create `crates/noceur-app/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Noceur",
  "version": "0.1.0",
  "identifier": "app.noceur.desktop",
  "build": {
    "frontendDist": "../../ui/dist",
    "devUrl": "http://localhost:1420",
    "beforeDevCommand": "pnpm --dir ../../ui dev",
    "beforeBuildCommand": "pnpm --dir ../../ui build"
  },
  "app": {
    "windows": [
      {
        "title": "Noceur",
        "width": 1280,
        "height": 800,
        "minWidth": 800,
        "minHeight": 500,
        "decorations": true
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": false
  }
}
```

Bundling is off because installer icons are not needed until there is something to ship.

- [ ] **Step 7: Verify it compiles**

`tauri::generate_context!()` validates `frontendDist` at compile time, and `ui/dist`
does not exist yet. Create a placeholder first:

```bash
mkdir -p ui/dist && printf '<!doctype html><title>placeholder</title>' > ui/dist/index.html
```

Run: `cargo check -p noceur-app`
Expected: compiles. It will not run yet — Task 6 builds the real frontend over this
placeholder. `ui/dist/` is gitignored, so the placeholder is never committed.

Also run: `cargo test -p noceur-app`
Expected: PASS — the 11 formatting tests from Task 4 plus the 2 breadcrumb tests
added in Step 4 of this task, 13 total.

- [ ] **Step 8: Commit**

```bash
git add crates/noceur-app/ Cargo.lock
git commit -m "feat(app): add Tauri state and an off-thread directory listing command"
```

---

## Task 6: Frontend scaffold and the Sodium theme

**Files:**
- Create: `ui/package.json`
- Create: `ui/vite.config.ts`
- Create: `ui/tsconfig.json`
- Create: `ui/index.html`
- Create: `ui/src/index.tsx`
- Create: `ui/src/theme.css`
- Create: `ui/src/api.ts`
- Create: `ui/src/App.tsx`
- Modify: `.gitignore`

- [ ] **Step 1: Create the package manifest**

Create `ui/package.json`:

```json
{
  "name": "noceur-ui",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "solid-js": "^1"
  },
  "devDependencies": {
    "typescript": "^5",
    "vite": "^6",
    "vite-plugin-solid": "^2"
  }
}
```

- [ ] **Step 2: Create the build config**

Create `ui/vite.config.ts`:

```ts
import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  // Tauri expects a fixed port and fails rather than silently using another.
  server: { port: 1420, strictPort: true },
  build: { target: "chrome110", outDir: "dist", emptyOutDir: true },
});
```

Create `ui/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "jsx": "preserve",
    "jsxImportSource": "solid-js",
    "strict": true,
    "noUnusedLocals": true,
    "skipLibCheck": true,
    "types": ["vite/client"],
    "isolatedModules": true,
    "noEmit": true
  },
  "include": ["src"]
}
```

Create `ui/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Noceur</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/index.tsx"></script>
  </body>
</html>
```

- [ ] **Step 3: Create the Sodium theme**

Create `ui/src/theme.css`:

```css
/* Sodium — monochrome interface, one warm light.
   The accent appears in exactly four places in this milestone: the active
   breadcrumb segment, the selected row edge, directory glyphs, and timings. */
:root {
  --bg: #0a0a0c;
  --panel: #0e0e11;
  --raise: #131317;
  --hair: rgba(238, 234, 228, 0.055);
  --hair2: rgba(238, 234, 228, 0.1);
  --fg: #eeeae4;
  --fg2: rgba(238, 234, 228, 0.54);
  --fg3: rgba(238, 234, 228, 0.28);
  --fg4: rgba(238, 234, 228, 0.14);
  --acc: #e8a55f;
  --row-h: 26px;
}

* {
  box-sizing: border-box;
}

html,
body,
#root {
  height: 100%;
  margin: 0;
}

body {
  background: var(--bg);
  color: var(--fg);
  font-family: Inter, -apple-system, "Segoe UI", system-ui, sans-serif;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  -webkit-font-smoothing: antialiased;
  user-select: none;
  overflow: hidden;
}

.mono {
  font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  font-size: 11px;
}

.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}

.crumbs {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 9px 14px;
  border-bottom: 1px solid var(--hair);
  flex: none;
  flex-wrap: wrap;
}
.crumb {
  color: var(--fg3);
  cursor: pointer;
}
.crumb:hover {
  color: var(--fg);
}
.crumb.last {
  color: var(--fg);
}
.sep {
  color: var(--fg4);
}
.navbtn {
  background: none;
  border: 1px solid var(--hair2);
  color: var(--fg2);
  border-radius: 5px;
  padding: 2px 8px;
  cursor: pointer;
  font: inherit;
}
.navbtn:disabled {
  color: var(--fg4);
  cursor: default;
}
.navbtn:not(:disabled):hover {
  background: var(--raise);
  color: var(--fg);
}

.hdr,
.row {
  display: grid;
  grid-template-columns: 1fr 62px 46px 62px 180px 84px;
  gap: 10px;
  padding: 0 14px;
  align-items: center;
}
.hdr {
  height: 28px;
  border-bottom: 1px solid var(--hair);
  color: var(--fg4);
  font-size: 9px;
  letter-spacing: 0.13em;
  text-transform: uppercase;
  flex: none;
}
.list {
  flex: 1;
  overflow-y: auto;
  background: var(--panel);
}
.row {
  height: var(--row-h);
  box-shadow: inset 2px 0 0 transparent;
  cursor: default;
  white-space: nowrap;
}
.row:hover {
  background: rgba(238, 234, 228, 0.02);
}
.row.sel {
  background: rgba(238, 234, 228, 0.045);
  box-shadow: inset 2px 0 0 var(--acc);
}
.row.dir {
  cursor: pointer;
}
.name {
  overflow: hidden;
  text-overflow: ellipsis;
}
.row.dir .name::before {
  content: "";
  display: inline-block;
  width: 9px;
  height: 7px;
  border-radius: 2px 2px 1px 1px;
  background: var(--acc);
  opacity: 0.7;
  margin-right: 9px;
}
.row.other .name {
  color: var(--fg3);
}
.num {
  text-align: right;
  color: var(--fg2);
}
.dim {
  color: var(--fg2);
  overflow: hidden;
  text-overflow: ellipsis;
}
.bad {
  color: var(--acc);
  font-size: 10px;
}

.status {
  flex: none;
  display: flex;
  gap: 16px;
  padding: 6px 14px;
  border-top: 1px solid var(--hair);
  color: var(--fg3);
  font-size: 10px;
}
.status .slow {
  color: var(--acc);
}
.empty {
  padding: 40px;
  text-align: center;
  color: var(--fg3);
}
```

- [ ] **Step 4: Create the typed API wrapper**

Create `ui/src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";

export type RowDisplay = {
  fileId: number;
  kind: number;
  name: string;
  bpm: string;
  key: string;
  duration: string;
  artist: string;
  title: string;
  album: string;
  size: string;
  state: string;
};

export type Segment = { label: string; path: string };

export type DirListing = {
  path: string;
  segments: Segment[];
  rows: RowDisplay[];
  total: number;
  scanMs: number;
  probeMs: number;
};

export function listDir(path: string, audioOnly = false): Promise<DirListing> {
  return invoke<DirListing>("list_dir", { req: { path, audioOnly } });
}

export function parentOf(path: string): Promise<string | null> {
  return invoke<string | null>("parent_of", { path });
}
```

- [ ] **Step 5: Create the entry point**

Create `ui/src/index.tsx`:

```tsx
/* @refresh reload */
import { render } from "solid-js/web";
import App from "./App";
import "./theme.css";

const root = document.getElementById("root");
if (!root) throw new Error("#root not found");
render(() => <App />, root);
```

- [ ] **Step 6: Ignore build output**

Add to `.gitignore`:

```
ui/dist/
ui/node_modules/
```

- [ ] **Step 7: Install dependencies**

Run: `pnpm --dir ui install`
Expected: completes and writes `ui/pnpm-lock.yaml`. Report the resolved versions of `solid-js`, `vite` and `@tauri-apps/api`.

- [ ] **Step 8: Commit**

```bash
git add ui/ .gitignore
git commit -m "feat(ui): scaffold Solid + Vite frontend with the Sodium theme"
```

`App.tsx` is created in Task 7; this task commits everything around it.

---

## Task 7: The list and navigation

**Files:**
- Create: `ui/src/App.tsx`

- [ ] **Step 1: Write the component**

Create `ui/src/App.tsx`:

```tsx
import { createSignal, createResource, For, Show } from "solid-js";
import { listDir, parentOf, type DirListing, type RowDisplay } from "./api";

/** Where the browser opens. Adjust freely — pinned folders arrive in a later milestone. */
const HOME = "E:\\music";

export default function App() {
  // Back/forward history for this pane. Index points at the current entry.
  const [history, setHistory] = createSignal<string[]>([HOME]);
  const [cursor, setCursor] = createSignal(0);
  const [selected, setSelected] = createSignal<number | null>(null);

  const current = () => history()[cursor()];
  // The fetcher must be wrapped: Solid calls it as `fetcher(source, info)`, and
  // passing `listDir` directly would hand the info object to its `audioOnly`
  // parameter, silently filtering out every directory.
  const [listing] = createResource<DirListing, string>(current, (p) => listDir(p));

  function navigate(path: string) {
    // Truncate any forward history, exactly like a browser.
    const next = history().slice(0, cursor() + 1);
    next.push(path);
    setHistory(next);
    setCursor(next.length - 1);
    setSelected(null);
  }

  const canBack = () => cursor() > 0;
  const canForward = () => cursor() < history().length - 1;

  function back() {
    if (canBack()) {
      setCursor(cursor() - 1);
      setSelected(null);
    }
  }
  function forward() {
    if (canForward()) {
      setCursor(cursor() + 1);
      setSelected(null);
    }
  }
  async function up() {
    const parent = await parentOf(current());
    if (parent) navigate(parent);
  }

  function onRow(row: RowDisplay) {
    setSelected(row.fileId);
    if (row.kind === 0) {
      const sep = current().endsWith("\\") || current().endsWith("/") ? "" : "\\";
      navigate(`${current()}${sep}${row.name}`);
    }
  }

  return (
    <div class="app">
      <div class="crumbs">
        <button class="navbtn" onClick={back} disabled={!canBack()}>
          ←
        </button>
        <button class="navbtn" onClick={forward} disabled={!canForward()}>
          →
        </button>
        <button class="navbtn" onClick={up}>
          ↑
        </button>
        <Show when={listing()}>
          <For each={listing()!.segments}>
            {(seg, i) => (
              <>
                <Show when={i() > 0}>
                  <span class="sep">›</span>
                </Show>
                <span
                  class={`crumb ${i() === listing()!.segments.length - 1 ? "last" : ""}`}
                  onClick={() => navigate(seg.path)}
                >
                  {seg.label}
                </span>
              </>
            )}
          </For>
        </Show>
      </div>

      <div class="hdr">
        <span>Name</span>
        <span class="num">BPM</span>
        <span>Key</span>
        <span class="num">Time</span>
        <span>Artist</span>
        <span class="num">Size</span>
      </div>

      <div class="list">
        <Show
          when={!listing.loading}
          fallback={<div class="empty">Reading {current()}…</div>}
        >
          <Show
            when={listing() && listing()!.rows.length > 0}
            fallback={<div class="empty">Nothing here.</div>}
          >
            <For each={listing()!.rows}>
              {(row) => (
                <div
                  class={
                    "row" +
                    (row.kind === 0 ? " dir" : row.kind === 2 ? " other" : "") +
                    (selected() === row.fileId ? " sel" : "")
                  }
                  onClick={() => onRow(row)}
                >
                  <span class="name" title={row.name}>
                    {row.name}
                    <Show when={row.state}>
                      <span class="bad"> [{row.state}]</span>
                    </Show>
                  </span>
                  <span class="num mono">{row.bpm}</span>
                  <span class="mono">{row.key}</span>
                  <span class="num mono">{row.duration}</span>
                  <span class="dim">{row.artist}</span>
                  <span class="num mono">{row.size}</span>
                </div>
              )}
            </For>
          </Show>
        </Show>
      </div>

      <div class="status">
        <Show when={listing()}>
          <span>{listing()!.total} entries</span>
          <span class={listing()!.scanMs > 500 ? "slow" : ""}>
            scan {listing()!.scanMs}ms
          </span>
          <span class={listing()!.probeMs > 2000 ? "slow" : ""}>
            probe {listing()!.probeMs}ms
          </span>
        </Show>
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm --dir ui exec tsc --noEmit`
Expected: no errors. Fix any that appear rather than loosening `tsconfig.json`.

- [ ] **Step 3: Build the frontend**

Run: `pnpm --dir ui build`
Expected: writes `ui/dist/index.html` and assets.

- [ ] **Step 4: Commit**

```bash
git add ui/src/App.tsx
git commit -m "feat(ui): list a directory with breadcrumb and back/forward navigation"
```

---

## Task 8: Run it and verify

There is no unit test for "the window opens". This task is the verification.

- [ ] **Step 1: Launch**

Run: `cargo tauri dev --config crates/noceur-app/tauri.conf.json`

If that path form is rejected, run it from the crate directory instead:
```bash
cd crates/noceur-app && cargo tauri dev
```
Expected: Vite starts on port 1420, the crate compiles, and a window titled **Noceur** opens showing `E:\music`.

- [ ] **Step 2: Verify each behaviour**

Check every one of these and report pass or fail with what you saw:

- Directories are listed before audio files, and non-audio files last
- Directory rows show an accent-coloured folder glyph and are clickable
- Clicking a directory navigates into it and the breadcrumb updates
- The breadcrumb shows `E:\music\...` — **not** `\\?\E:\music\...`
- Clicking a breadcrumb segment navigates to it
- `←` and `→` move through history and are disabled at the ends
- `↑` goes to the parent
- BPM values from tags render with a trailing `·`
- Untagged BPM, key and duration render as `—`
- A file with a bad state shows `[undecodable]` in the accent colour
- The status bar shows entry count and scan/probe timings
- Navigating into a folder with hundreds of files does not freeze the window while probing

- [ ] **Step 3: Verify the off-thread claim explicitly**

Navigate to a folder on the network share (`Z:`) after leaving it idle, so the mount is cold. Milestone 1 measured a twenty-second stall for this.

Expected: the window stays responsive — the breadcrumb buttons still highlight on hover and the window still moves — while the listing resolves. If the window freezes, `list_dir` is not actually running off-thread and that is a blocking bug to report, not to work around.

- [ ] **Step 4: Screenshot**

Capture the window showing a real folder and save it to `docs/superpowers/notes/2026-08-08-shell-first-light.png`.

- [ ] **Step 5: Record what you found**

Create `docs/superpowers/notes/2026-08-08-shell-verification.md` with the result of every check in Step 2, the observed behaviour on the cold network path, and any rough edges worth carrying into the next milestone (rendering slowness on large folders is expected and should be recorded with the folder size and how it felt).

- [ ] **Step 6: Commit**

```bash
git add docs/superpowers/notes/
git commit -m "docs: record shell foundation verification"
```

---

## Definition of Done

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `cargo fmt --all -- --check` is clean
- [ ] `pnpm --dir ui exec tsc --noEmit` is clean
- [ ] A window opens and browses the real library
- [ ] The breadcrumb never shows `\\?\`
- [ ] The window stays responsive while a cold network path is enumerated
- [ ] Back, forward, up, breadcrumb and directory clicks all navigate correctly

## What this milestone deliberately does not do

Virtualized rendering, resizable/reorderable/persisted columns, the folder tree, pinned folders, split panes, tabs, search, file operations, waveform, playback, the action toolbar, tag writing, real volume identity, and window state persistence. The list renders every row directly, so large folders will feel slow — virtualization is the next milestone and this is the baseline it improves on.
