# Noceur Engine Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the headless engine that can enumerate a directory, classify and order its entries the way the UI will, read audio tags, normalise musical keys, and persist all of it into a SQLite index — verifiable end-to-end through a debug CLI.

**Architecture:** Three library crates with no dependencies on each other except downward (`noceur-index` → `noceur-fsmodel`, `noceur-tags`), plus a thin CLI binary for manual verification. No Tauri, no UI, no async runtime. Every crate is unit-testable in isolation; the CLI exists so a human can point the engine at a real music folder and see the result.

**Tech Stack:** Rust 2021 (toolchain 1.97 — see Amendment 3), `rusqlite` (bundled SQLite), `lofty` (tag reading), `xxhash-rust` (quick keys), `clap` (CLI), `tempfile` (test fixtures).

---

## Prerequisites

- Rust 1.88+ (`rustc --version`)
- On Windows, the MSVC build tools must be installed — `rusqlite`'s `bundled` feature compiles SQLite from C source. If `cargo build` fails with `error: linker link.exe not found`, install "Desktop development with C++" from the Visual Studio Build Tools installer.

**Dependency versions.** The manifests below pin the versions this plan's code was written against (`lofty 0.22`, `rusqlite 0.37`). If either fails to resolve, run `cargo add lofty` / `cargo add rusqlite --features bundled` to take the current release, then check the compile errors against that crate's changelog — the APIs used here are `Probe`/`Tag`/`Accessor` for lofty and `Connection`/`params!` for rusqlite, all of which are long-stable. Do not silently work around an API change; correct the call and note it in the commit message.

## Amendments

Changes made after a task shipped, recorded so the plan stays truthful. Tasks below this point still show their original code; where an amendment supersedes it, this section wins.

**Amendment 1 — `read_dir` returns `DirListing`, not `Vec<FsEntry>` (after Task 5 review).**
A code review found that `read_dir` silently discarded per-entry failures, so a caller could not tell a listing came back short. Skipping remains correct — one locked file must never fail a folder — but on an SMB share or a removable drive it needs to be observable, because §12 of the spec calls for surfacing exactly this in the activity panel. `enumerate.rs` now defines:

```rust
pub struct SkippedEntry { pub name: String, pub reason: String }
pub struct DirListing  { pub entries: Vec<FsEntry>, pub skipped: Vec<SkippedEntry> }
```

and `read_dir(dir: &Path) -> Result<DirListing, EnumerateError>`. Task 4's code block above predates this. Task 11's snippet has been updated in place.

**Amendment 5 — `parse_classical` sorts at declaration, not per call (after Task 7 review).**
Task 6's `parse_classical` allocated and sorted a 21-element `Vec` of roots on every call, in a path that runs once per audio file. A review proved by exhaustive check of all 36 root/accidental/mode combinations that the sort was never load-bearing: no accepted mode suffix (`""`, `maj`, `major`, `dur`, `m`, `min`, `minor`, `moll`) begins with `#` or `b`, so a short root wrongly matching the head of `c#m` leaves a remainder starting with `#`, matches no suffix, and the loop continues to the correct root. `ROOTS` is now declared two-character-first and iterated directly, with a comment recording both the ordering and the real invariant — adding a suffix that begins with an accidental would break it.

The same review found eight `TrackMeta` fields with no test coverage. All six tag-derived ones now have three-way `Id3v2`/`VorbisComments`/`Mp4Ilst` coverage, plus tests for `has_artwork` and `bit_depth`. One finding worth recording: `ItemKey::Year` has no ID3v2 or MP4 mapping, but `Accessor::year()` already falls back to `ItemKey::RecordingDate` (`TDRC`/`DATE`/`©day`) internally, so no code change was needed — the same fallback shape as Amendment 4, supplied by lofty rather than by us.

**Amendment 4 — BPM must be read from two `ItemKey`s, not one (during Task 7).**
Task 7's code used `ItemKey::Bpm`. Verified against lofty 0.22.4's own mapping tables in `src/tag/item.rs`, that is wrong for ID3v2 — `ID3V2_MAP` has only `"TBPM" => IntegerBpm` and no entry for `Bpm` at all, so writing `Bpm` to an ID3v2 tag silently drops it and reading it always yields `None`. But `IntegerBpm` alone is equally wrong: `VORBIS_MAP` has only `"BPM" => Bpm`, so FLAC and OGG would silently lose BPM. `ILST_MAP` (MP4) has both — `"tmpo" => IntegerBpm` and `"----:com.apple.iTunes:BPM" => Bpm`.

`read()` therefore tries `IntegerBpm` and falls back to `Bpm`, which covers all six supported containers. `InitialKey` needed no equivalent change: it maps cleanly to `TKEY` (ID3v2), `INITIALKEY`/`KEY` (Vorbis) and the `initialkey` freeform atom (MP4).

The bug was invisible to the suite because every fixture was a WAV carrying ID3v2. So `meta.rs` also gained `pub fn apply_tag(meta: &mut TrackMeta, tag: &lofty::tag::Tag)`, splitting tag mapping from file opening, and three tests now exercise `TagType::Id3v2`, `TagType::VorbisComments` and `TagType::Mp4Ilst` directly — no hand-authored FLAC or M4A fixtures required. Any future tag field added to `TrackMeta` should get the same three-way test.

**Amendment 3 — toolchain pinned to 1.97, not 1.88 (during Task 6).**
The original pin came from whatever `rustc --version` reported on the machine and was simply wrong: `lofty 0.22` pulls in `ogg_pager 0.7.2`, which requires rustc 1.89+. The first attempt worked around this by pinning `ogg_pager` back to 0.7.0, which is a treadmill — the next dependency to move would break it again. Rust 1.97.1 was already installed locally, so `rust-toolchain.toml` now pins `1.97`, the lockfile was regenerated clean, and no dependency is held back. Task 1's manifest block above shows the original `1.88`.

**Amendment 2 — supporting changes from the same review.**
`kind.rs` gained `extension_of(name: &str) -> String`, used by `enumerate.rs` so extension parsing exists once rather than twice. `natural_cmp` now falls back to a raw-string comparison when the natural comparison ties, so `"7"` and `"007"` get a deterministic order and rows cannot shuffle between refreshes. `FsEntry::path` and `extended()` gained doc comments recording two contracts that were previously implicit: the stored path is *not* the `\\?\`-extended form and must be passed through `extended()` before any I/O, and `extended()` requires an already-normalised fully-qualified path because verbatim paths bypass OS normalisation.

---

## File Structure

```
Cargo.toml                              workspace manifest
rust-toolchain.toml                     pin toolchain
crates/
  noceur-fsmodel/
    Cargo.toml
    src/lib.rs                          re-exports
    src/path.rs                         Windows extended-path normalisation
    src/kind.rs                         EntryKind + audio extension set
    src/entry.rs                        FsEntry struct
    src/enumerate.rs                    directory reading
    src/order.rs                        Finder/Explorer sort ordering
  noceur-tags/
    Cargo.toml
    src/lib.rs                          re-exports
    src/key.rs                          MusicalKey parse/normalise/format
    src/meta.rs                         TrackMeta + lofty reading
  noceur-index/
    Cargo.toml
    src/lib.rs                          re-exports
    src/schema.rs                       DDL + migration runner
    src/db.rs                           Db handle, open/pragmas
    src/volume.rs                       volume registry
    src/scan.rs                         scan stages 1-3
    src/query.rs                        windowed sorted queries
  noceur-cli/
    Cargo.toml
    src/main.rs                         `noceur ls` / `noceur index`
```

Each file has one responsibility. `noceur-fsmodel` owns "what is on disk and in what order"; `noceur-tags` owns "what does this file claim about itself"; `noceur-index` owns "what do we remember".

---

## Task 1: Workspace skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/noceur-fsmodel/Cargo.toml`
- Create: `crates/noceur-fsmodel/src/lib.rs`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"
repository = "https://github.com/mtsbh/noceur"

[workspace.dependencies]
thiserror = "2"
```

- [ ] **Step 2: Pin the toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.88"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Create the first crate**

Create `crates/noceur-fsmodel/Cargo.toml`:

```toml
[package]
name = "noceur-fsmodel"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true

[dev-dependencies]
tempfile = "3"
```

Create `crates/noceur-fsmodel/src/lib.rs`:

```rust
//! Filesystem model: enumeration, classification and ordering.

pub mod path;
```

Create `crates/noceur-fsmodel/src/path.rs`:

```rust
//! Path normalisation.
```

- [ ] **Step 4: Verify the workspace builds**

Run: `cargo build`
Expected: `Finished dev profile` with no errors.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml crates/
git commit -m "chore: add cargo workspace and noceur-fsmodel skeleton"
```

---

## Task 2: Extended path normalisation

Windows refuses paths over 260 characters unless they are prefixed with `\\?\`. Every filesystem call in the engine goes through this function. The prefix requires a fully-qualified path with backslash separators and no `.` or `..` components, so the function canonicalises first.

**Files:**
- Modify: `crates/noceur-fsmodel/src/path.rs`
- Test: `crates/noceur-fsmodel/src/path.rs` (inline `#[cfg(test)]` module)

- [ ] **Step 1: Write the failing tests**

Replace the contents of `crates/noceur-fsmodel/src/path.rs`:

```rust
//! Path normalisation for long-path-safe filesystem access.

use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn prefixes_absolute_drive_path() {
        let p = extended(Path::new(r"C:\Music\track.wav"));
        assert_eq!(p, PathBuf::from(r"\\?\C:\Music\track.wav"));
    }

    #[cfg(windows)]
    #[test]
    fn is_idempotent() {
        let once = extended(Path::new(r"C:\Music"));
        let twice = extended(&once);
        assert_eq!(once, twice);
    }

    #[cfg(windows)]
    #[test]
    fn rewrites_unc_paths() {
        let p = extended(Path::new(r"\\nas\sets\friday"));
        assert_eq!(p, PathBuf::from(r"\\?\UNC\nas\sets\friday"));
    }

    #[cfg(windows)]
    #[test]
    fn leaves_relative_paths_alone() {
        let p = extended(Path::new(r"Music\track.wav"));
        assert_eq!(p, PathBuf::from(r"Music\track.wav"));
    }

    #[cfg(not(windows))]
    #[test]
    fn is_identity_on_unix() {
        let p = extended(Path::new("/music/track.wav"));
        assert_eq!(p, PathBuf::from("/music/track.wav"));
    }

    #[test]
    fn survives_a_very_long_path() {
        let long = "a".repeat(300);
        let input = if cfg!(windows) {
            PathBuf::from(format!(r"C:\{long}"))
        } else {
            PathBuf::from(format!("/{long}"))
        };
        // Must not panic and must preserve the tail.
        assert!(extended(&input).to_string_lossy().ends_with(&long));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-fsmodel`
Expected: FAIL — `cannot find function 'extended' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-fsmodel/src/path.rs`:

```rust
/// Windows extended-length path prefix.
const EXT_PREFIX: &str = r"\\?\";

/// Returns a path safe to pass to filesystem APIs regardless of length.
///
/// On Windows, absolute paths gain the `\\?\` prefix (`\\?\UNC\` for network
/// shares), which lifts the 260-character `MAX_PATH` limit. Relative paths and
/// already-prefixed paths are returned unchanged. On other platforms this is
/// the identity function.
#[cfg(windows)]
pub fn extended(path: &Path) -> PathBuf {
    let s = path.as_os_str().to_string_lossy();

    if s.starts_with(EXT_PREFIX) {
        return path.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"{EXT_PREFIX}UNC\{rest}"));
    }
    if path.is_absolute() {
        return PathBuf::from(format!("{EXT_PREFIX}{s}"));
    }
    path.to_path_buf()
}

/// Identity on non-Windows platforms. See the Windows implementation for details.
#[cfg(not(windows))]
pub fn extended(path: &Path) -> PathBuf {
    path.to_path_buf()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p noceur-fsmodel`
Expected: PASS, 5 tests on Windows (4 `#[cfg(windows)]` + the long-path test), 2 elsewhere.

- [ ] **Step 5: Commit**

```bash
git add crates/noceur-fsmodel/src/path.rs
git commit -m "feat(fsmodel): add extended path normalisation for long Windows paths"
```

---

## Task 3: Entry classification

The list groups rows as directories, then audio files, then everything else. Classification drives both that grouping and the audio-only filter.

**Files:**
- Create: `crates/noceur-fsmodel/src/kind.rs`
- Modify: `crates/noceur-fsmodel/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-fsmodel/src/kind.rs`:

```rust
//! Entry classification: directory, audio file, or other.

use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directories_classify_as_directory() {
        assert_eq!(EntryKind::classify(Path::new("Unsorted"), true), EntryKind::Directory);
        // Extension is irrelevant for directories.
        assert_eq!(EntryKind::classify(Path::new("weird.wav"), true), EntryKind::Directory);
    }

    #[test]
    fn known_audio_extensions_classify_as_audio() {
        for name in [
            "a.wav", "a.aiff", "a.aif", "a.aifc", "a.flac", "a.mp3",
            "a.m4a", "a.mp4", "a.ogg", "a.oga", "a.opus", "a.wv", "a.ape",
        ] {
            assert_eq!(
                EntryKind::classify(Path::new(name), false),
                EntryKind::Audio,
                "{name} should be audio"
            );
        }
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(EntryKind::classify(Path::new("A.WAV"), false), EntryKind::Audio);
        assert_eq!(EntryKind::classify(Path::new("A.FlAc"), false), EntryKind::Audio);
    }

    #[test]
    fn other_files_classify_as_other() {
        for name in ["cover.jpg", "tracklist.txt", "notes", "a.wave", "a.mp"] {
            assert_eq!(
                EntryKind::classify(Path::new(name), false),
                EntryKind::Other,
                "{name} should be other"
            );
        }
    }

    #[test]
    fn group_order_is_directories_then_audio_then_other() {
        assert!(EntryKind::Directory.group() < EntryKind::Audio.group());
        assert!(EntryKind::Audio.group() < EntryKind::Other.group());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-fsmodel kind`
Expected: FAIL — `cannot find type 'EntryKind' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-fsmodel/src/kind.rs`:

```rust
/// Extensions treated as audio, lowercase, without the leading dot.
///
/// `mp4` is included because DJ libraries contain ALAC and AAC audio in `.mp4`
/// containers. Video in `.mp4` is rare enough in a music tree that mis-classifying
/// it costs a wasted probe, not a wrong answer.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "aiff", "aif", "aifc", "flac", "mp3", "m4a", "mp4", "ogg", "oga",
    "opus", "wv", "ape",
];

/// How an entry is grouped in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    Directory,
    Audio,
    Other,
}

impl EntryKind {
    /// Classifies an entry from its name and whether it is a directory.
    pub fn classify(path: &Path, is_dir: bool) -> Self {
        if is_dir {
            return EntryKind::Directory;
        }
        match path.extension().and_then(|e| e.to_str()) {
            Some(ext) if is_audio_extension(ext) => EntryKind::Audio,
            _ => EntryKind::Other,
        }
    }

    /// Sort group. Lower sorts first, and this always outranks the user's
    /// chosen sort column so directories stay on top.
    pub fn group(self) -> u8 {
        match self {
            EntryKind::Directory => 0,
            EntryKind::Audio => 1,
            EntryKind::Other => 2,
        }
    }

    pub fn is_audio(self) -> bool {
        matches!(self, EntryKind::Audio)
    }
}

/// Case-insensitive audio extension check.
pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}
```

- [ ] **Step 4: Register the module**

Replace `crates/noceur-fsmodel/src/lib.rs`:

```rust
//! Filesystem model: enumeration, classification and ordering.

pub mod kind;
pub mod path;

pub use kind::{is_audio_extension, EntryKind, AUDIO_EXTENSIONS};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-fsmodel`
Expected: PASS, all tests green.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-fsmodel/src/kind.rs crates/noceur-fsmodel/src/lib.rs
git commit -m "feat(fsmodel): classify entries as directory, audio or other"
```

---

## Task 4: Directory enumeration

**Files:**
- Create: `crates/noceur-fsmodel/src/entry.rs`
- Create: `crates/noceur-fsmodel/src/enumerate.rs`
- Modify: `crates/noceur-fsmodel/src/lib.rs`

- [ ] **Step 1: Define the entry type**

Create `crates/noceur-fsmodel/src/entry.rs`:

```rust
//! The unit of a directory listing.

use crate::kind::EntryKind;
use std::path::PathBuf;

/// One row of a directory listing, straight from the filesystem.
///
/// Deliberately carries no tag or analysis data — that is layered on by
/// `noceur-index`. This struct is what stage 1 of the scan produces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsEntry {
    /// Full path to the entry.
    pub path: PathBuf,
    /// File name including extension.
    pub name: String,
    /// Lowercase extension without the dot; empty for directories and
    /// extensionless files.
    pub ext: String,
    /// Size in bytes. Always 0 for directories.
    pub size: u64,
    /// Modification time as seconds since the Unix epoch. 0 if unavailable.
    pub mtime: i64,
    pub kind: EntryKind,
}

impl FsEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == EntryKind::Directory
    }
}
```

- [ ] **Step 2: Write the failing tests**

Create `crates/noceur-fsmodel/src/enumerate.rs`:

```rust
//! Reading a directory into `FsEntry` rows.

use crate::entry::FsEntry;
use crate::kind::EntryKind;
use crate::path::extended;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, thiserror::Error)]
pub enum EnumerateError {
    #[error("cannot read directory {path}: {source}")]
    ReadDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn fixture() -> TempDir {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Unsorted")).unwrap();
        fs::write(d.path().join("track.wav"), b"0123456789").unwrap();
        fs::write(d.path().join("cover.jpg"), b"xx").unwrap();
        d
    }

    #[test]
    fn reads_every_entry() {
        let d = fixture();
        let entries = read_dir(d.path()).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn populates_name_ext_and_kind() {
        let d = fixture();
        let entries = read_dir(d.path()).unwrap();

        let track = entries.iter().find(|e| e.name == "track.wav").unwrap();
        assert_eq!(track.ext, "wav");
        assert_eq!(track.kind, EntryKind::Audio);
        assert_eq!(track.size, 10);

        let dir = entries.iter().find(|e| e.name == "Unsorted").unwrap();
        assert_eq!(dir.ext, "");
        assert_eq!(dir.kind, EntryKind::Directory);
        assert_eq!(dir.size, 0);

        let cover = entries.iter().find(|e| e.name == "cover.jpg").unwrap();
        assert_eq!(cover.kind, EntryKind::Other);
    }

    #[test]
    fn lowercases_the_extension() {
        let d = TempDir::new().unwrap();
        fs::write(d.path().join("LOUD.WAV"), b"x").unwrap();
        let entries = read_dir(d.path()).unwrap();
        assert_eq!(entries[0].ext, "wav");
        assert_eq!(entries[0].name, "LOUD.WAV", "name keeps original case");
    }

    #[test]
    fn records_a_plausible_mtime() {
        let d = fixture();
        let entries = read_dir(d.path()).unwrap();
        // Any file written during this test is after 2020-01-01.
        assert!(entries.iter().all(|e| e.mtime > 1_577_836_800));
    }

    #[test]
    fn errors_on_a_missing_directory() {
        let err = read_dir(Path::new("./definitely-not-here-9f2a")).unwrap_err();
        assert!(matches!(err, EnumerateError::ReadDir { .. }));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p noceur-fsmodel enumerate`
Expected: FAIL — `cannot find function 'read_dir' in this scope`.

- [ ] **Step 4: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-fsmodel/src/enumerate.rs`:

```rust
/// Reads one directory into `FsEntry` rows.
///
/// Individual entries that cannot be stat'd are skipped rather than failing the
/// whole listing — a single locked or vanished file must never cost you the
/// folder. Only failure to open the directory itself is an error.
pub fn read_dir(dir: &Path) -> Result<Vec<FsEntry>, EnumerateError> {
    let iter = std::fs::read_dir(extended(dir)).map_err(|source| EnumerateError::ReadDir {
        path: dir.display().to_string(),
        source,
    })?;

    let mut out = Vec::new();
    for entry in iter.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = meta.is_dir();

        let ext = if is_dir {
            String::new()
        } else {
            Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default()
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        out.push(FsEntry {
            kind: EntryKind::classify(Path::new(&name), is_dir),
            path: dir.join(&name),
            name,
            ext,
            size: if is_dir { 0 } else { meta.len() },
            mtime,
        });
    }
    Ok(out)
}
```

- [ ] **Step 5: Register the modules**

Replace `crates/noceur-fsmodel/src/lib.rs`:

```rust
//! Filesystem model: enumeration, classification and ordering.

pub mod entry;
pub mod enumerate;
pub mod kind;
pub mod path;

pub use entry::FsEntry;
pub use enumerate::{read_dir, EnumerateError};
pub use kind::{is_audio_extension, EntryKind, AUDIO_EXTENSIONS};
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p noceur-fsmodel`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/noceur-fsmodel/src/
git commit -m "feat(fsmodel): read directories into FsEntry rows"
```

---

## Task 5: Explorer ordering

Directories first, then audio, then everything else — regardless of which column the user sorts by. Within a group, the chosen column applies. Name comparison is case-insensitive and natural, so `track2` sorts before `track10`.

**Files:**
- Create: `crates/noceur-fsmodel/src/order.rs`
- Modify: `crates/noceur-fsmodel/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-fsmodel/src/order.rs`:

```rust
//! Sort ordering for directory listings.

use crate::entry::FsEntry;
use std::cmp::Ordering;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kind::EntryKind;
    use std::path::PathBuf;

    fn e(name: &str, kind: EntryKind, size: u64, mtime: i64) -> FsEntry {
        FsEntry {
            path: PathBuf::from(name),
            name: name.to_string(),
            ext: String::new(),
            size,
            mtime,
            kind,
        }
    }

    #[test]
    fn groups_before_columns() {
        let mut v = vec![
            e("zzz.wav", EntryKind::Audio, 1, 1),
            e("aaa.txt", EntryKind::Other, 1, 1),
            e("mmm", EntryKind::Directory, 0, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(
            v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["mmm", "zzz.wav", "aaa.txt"]
        );
    }

    #[test]
    fn grouping_survives_descending_order() {
        let mut v = vec![
            e("b.wav", EntryKind::Audio, 1, 1),
            e("a", EntryKind::Directory, 0, 1),
            e("c.txt", EntryKind::Other, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Desc);
        assert_eq!(v[0].kind, EntryKind::Directory, "dirs stay first when reversed");
        assert_eq!(v[2].kind, EntryKind::Other);
    }

    #[test]
    fn name_sort_is_case_insensitive() {
        let mut v = vec![
            e("Banana.wav", EntryKind::Audio, 1, 1),
            e("apple.wav", EntryKind::Audio, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(v[0].name, "apple.wav");
    }

    #[test]
    fn name_sort_is_natural_for_numbers() {
        let mut v = vec![
            e("track10.wav", EntryKind::Audio, 1, 1),
            e("track2.wav", EntryKind::Audio, 1, 1),
            e("track1.wav", EntryKind::Audio, 1, 1),
        ];
        sort(&mut v, SortKey::Name, SortDir::Asc);
        assert_eq!(
            v.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["track1.wav", "track2.wav", "track10.wav"]
        );
    }

    #[test]
    fn sorts_by_size_and_mtime() {
        let mut v = vec![
            e("a.wav", EntryKind::Audio, 300, 10),
            e("b.wav", EntryKind::Audio, 100, 30),
            e("c.wav", EntryKind::Audio, 200, 20),
        ];
        sort(&mut v, SortKey::Size, SortDir::Asc);
        assert_eq!(v[0].name, "b.wav");

        sort(&mut v, SortKey::Modified, SortDir::Desc);
        assert_eq!(v[0].name, "b.wav");
    }

    #[test]
    fn ties_break_on_name_so_order_is_stable() {
        let mut v = vec![
            e("b.wav", EntryKind::Audio, 100, 1),
            e("a.wav", EntryKind::Audio, 100, 1),
        ];
        sort(&mut v, SortKey::Size, SortDir::Asc);
        assert_eq!(v[0].name, "a.wav");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-fsmodel order`
Expected: FAIL — `cannot find function 'sort' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-fsmodel/src/order.rs`:

```rust
/// Column the user is sorting by. Tag-derived columns are added by
/// `noceur-index`, which sorts in SQL rather than here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Modified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Sorts entries the way Explorer and Finder do: group first, column second.
///
/// The group ordering (directories, audio, other) is never reversed — flipping
/// to descending reverses order *within* each group only.
pub fn sort(entries: &mut [FsEntry], key: SortKey, dir: SortDir) {
    entries.sort_by(|a, b| {
        a.kind
            .group()
            .cmp(&b.kind.group())
            .then_with(|| apply_dir(compare_by(a, b, key), dir))
    });
}

fn apply_dir(ord: Ordering, dir: SortDir) -> Ordering {
    match dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}

fn compare_by(a: &FsEntry, b: &FsEntry, key: SortKey) -> Ordering {
    match key {
        SortKey::Name => natural_cmp(&a.name, &b.name),
        SortKey::Size => a.size.cmp(&b.size).then_with(|| natural_cmp(&a.name, &b.name)),
        SortKey::Modified => a
            .mtime
            .cmp(&b.mtime)
            .then_with(|| natural_cmp(&a.name, &b.name)),
    }
}

/// Case-insensitive comparison that orders embedded digit runs numerically,
/// so `track2` precedes `track10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut x = a.chars().peekable();
    let mut y = b.chars().peekable();

    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) => {
                if ca.is_ascii_digit() && cb.is_ascii_digit() {
                    let na = take_number(&mut x);
                    let nb = take_number(&mut y);
                    match na.cmp(&nb) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                } else {
                    x.next();
                    y.next();
                    let la = ca.to_ascii_lowercase();
                    let lb = cb.to_ascii_lowercase();
                    match la.cmp(&lb) {
                        Ordering::Equal => continue,
                        other => return other,
                    }
                }
            }
        }
    }
}

/// Consumes a run of digits, saturating on absurdly long runs.
fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut n: u128 = 0;
    while let Some(c) = it.peek().copied() {
        if !c.is_ascii_digit() {
            break;
        }
        it.next();
        n = n.saturating_mul(10).saturating_add((c as u8 - b'0') as u128);
    }
    n
}
```

- [ ] **Step 4: Register the module**

In `crates/noceur-fsmodel/src/lib.rs`, add `pub mod order;` after `pub mod kind;` and extend the re-exports:

```rust
pub use order::{natural_cmp, sort, SortDir, SortKey};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-fsmodel`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-fsmodel/src/
git commit -m "feat(fsmodel): sort listings dirs-first with natural name ordering"
```

---

## Task 6: Musical key normalisation

Files carry keys in Camelot (`8A`), Open Key (`1m`) or classical (`Am`, `F#`) notation depending on what wrote them. The index stores a canonical integer so the column sorts in harmonic-wheel order regardless of source, while preserving the original string.

Canonical value: `(camelot_number - 1) * 2 + side`, where side is 0 for `A` (minor) and 1 for `B` (major). Range 0–23.

**Files:**
- Create: `crates/noceur-tags/Cargo.toml`
- Create: `crates/noceur-tags/src/lib.rs`
- Create: `crates/noceur-tags/src/key.rs`

- [ ] **Step 1: Create the crate**

Create `crates/noceur-tags/Cargo.toml`:

```toml
[package]
name = "noceur-tags"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true
lofty = "0.22"

[dev-dependencies]
tempfile = "3"
```

Create `crates/noceur-tags/src/lib.rs`:

```rust
//! Reading and normalising audio file tags.

pub mod key;

pub use key::{KeyNotation, MusicalKey};
```

- [ ] **Step 2: Write the failing tests**

Create `crates/noceur-tags/src/key.rs`:

```rust
//! Musical key parsing and notation conversion.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_camelot() {
        let k = MusicalKey::parse("8A").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(k.is_minor);
        assert_eq!(k.canonical(), 14);

        let k = MusicalKey::parse("12B").unwrap();
        assert_eq!(k.camelot_number, 12);
        assert!(!k.is_minor);
    }

    #[test]
    fn camelot_parsing_is_case_and_space_insensitive() {
        assert_eq!(MusicalKey::parse(" 8a ").unwrap(), MusicalKey::parse("8A").unwrap());
    }

    #[test]
    fn parses_open_key() {
        // Open Key 1m is A minor, which is Camelot 8A.
        let k = MusicalKey::parse("1m").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(k.is_minor);

        // Open Key 1d is C major, which is Camelot 8B.
        let k = MusicalKey::parse("1d").unwrap();
        assert_eq!(k.camelot_number, 8);
        assert!(!k.is_minor);
    }

    #[test]
    fn parses_classical_minor_and_major() {
        let am = MusicalKey::parse("Am").unwrap();
        assert_eq!(am.camelot_number, 8);
        assert!(am.is_minor);

        let c = MusicalKey::parse("C").unwrap();
        assert_eq!(c.camelot_number, 8);
        assert!(!c.is_minor);
    }

    #[test]
    fn accepts_classical_spelling_variants() {
        for s in ["Am", "A min", "A minor", "amin", "A-moll"] {
            assert_eq!(
                MusicalKey::parse(s).map(|k| k.canonical()),
                Some(MusicalKey::parse("Am").unwrap().canonical()),
                "{s} should parse as A minor"
            );
        }
        for s in ["C", "C maj", "C major", "Cmaj"] {
            assert_eq!(
                MusicalKey::parse(s).map(|k| k.canonical()),
                Some(MusicalKey::parse("C").unwrap().canonical()),
                "{s} should parse as C major"
            );
        }
    }

    #[test]
    fn treats_enharmonics_as_equal() {
        assert_eq!(
            MusicalKey::parse("F#m").unwrap().canonical(),
            MusicalKey::parse("Gbm").unwrap().canonical()
        );
        assert_eq!(
            MusicalKey::parse("Db").unwrap().canonical(),
            MusicalKey::parse("C#").unwrap().canonical()
        );
    }

    #[test]
    fn rejects_nonsense() {
        for s in ["", "  ", "13A", "0A", "H", "banana", "8C", "9x"] {
            assert!(MusicalKey::parse(s).is_none(), "{s} should not parse");
        }
    }

    #[test]
    fn formats_in_every_notation() {
        let k = MusicalKey::parse("8A").unwrap();
        assert_eq!(k.format(KeyNotation::Camelot), "8A");
        assert_eq!(k.format(KeyNotation::OpenKey), "1m");
        assert_eq!(k.format(KeyNotation::Classical), "Am");

        let k = MusicalKey::parse("C").unwrap();
        assert_eq!(k.format(KeyNotation::Camelot), "8B");
        assert_eq!(k.format(KeyNotation::OpenKey), "1d");
        assert_eq!(k.format(KeyNotation::Classical), "C");
    }

    #[test]
    fn canonical_round_trips_through_every_notation() {
        for n in 1..=12u8 {
            for minor in [true, false] {
                let src = MusicalKey { camelot_number: n, is_minor: minor };
                for notation in [KeyNotation::Camelot, KeyNotation::OpenKey, KeyNotation::Classical] {
                    let text = src.format(notation);
                    let back = MusicalKey::parse(&text)
                        .unwrap_or_else(|| panic!("failed to reparse {text} ({notation:?})"));
                    assert_eq!(back, src, "round trip failed for {text}");
                }
            }
        }
    }

    #[test]
    fn canonical_values_are_unique_and_bounded() {
        let mut seen = std::collections::HashSet::new();
        for n in 1..=12u8 {
            for minor in [true, false] {
                let c = MusicalKey { camelot_number: n, is_minor: minor }.canonical();
                assert!((0..24).contains(&c));
                assert!(seen.insert(c), "duplicate canonical value {c}");
            }
        }
        assert_eq!(seen.len(), 24);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p noceur-tags`
Expected: FAIL — `cannot find type 'MusicalKey' in this scope`.

- [ ] **Step 4: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-tags/src/key.rs`:

```rust
/// Notation used to display a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyNotation {
    Camelot,
    OpenKey,
    Classical,
}

/// A musical key, stored as its position on the Camelot wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MusicalKey {
    /// 1..=12
    pub camelot_number: u8,
    /// `true` for the A (minor) side of the wheel.
    pub is_minor: bool,
}

/// Classical spelling for each Camelot position: (minor, major).
/// Index 0 is Camelot 1.
const CLASSICAL: [(&str, &str); 12] = [
    ("Abm", "B"),  //  1
    ("Ebm", "F#"), //  2
    ("Bbm", "Db"), //  3
    ("Fm", "Ab"),  //  4
    ("Cm", "Eb"),  //  5
    ("Gm", "Bb"),  //  6
    ("Dm", "F"),   //  7
    ("Am", "C"),   //  8
    ("Em", "G"),   //  9
    ("Bm", "D"),   // 10
    ("F#m", "A"),  // 11
    ("Dbm", "E"),  // 12
];

/// Pitch-class index (0 = C) for every accepted root spelling.
const ROOTS: &[(&str, u8)] = &[
    ("c", 0), ("b#", 0),
    ("c#", 1), ("db", 1),
    ("d", 2),
    ("d#", 3), ("eb", 3),
    ("e", 4), ("fb", 4),
    ("f", 5), ("e#", 5),
    ("f#", 6), ("gb", 6),
    ("g", 7),
    ("g#", 8), ("ab", 8),
    ("a", 9),
    ("a#", 10), ("bb", 10),
    ("b", 11), ("cb", 11),
];

/// Camelot number for each pitch class, minor side. `Am` (pitch class 9) is 8A.
const MINOR_PC_TO_CAMELOT: [u8; 12] = [5, 12, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10];
/// Camelot number for each pitch class, major side. `C` (pitch class 0) is 8B.
const MAJOR_PC_TO_CAMELOT: [u8; 12] = [8, 3, 10, 5, 12, 7, 2, 9, 4, 11, 6, 1];

impl MusicalKey {
    /// Canonical sort value, 0..=23. Orders by wheel position, minor before major.
    pub fn canonical(&self) -> i32 {
        (self.camelot_number as i32 - 1) * 2 + if self.is_minor { 0 } else { 1 }
    }

    /// Rebuilds a key from its canonical value. Returns `None` if out of range.
    pub fn from_canonical(v: i32) -> Option<Self> {
        if !(0..24).contains(&v) {
            return None;
        }
        Some(MusicalKey {
            camelot_number: (v / 2) as u8 + 1,
            is_minor: v % 2 == 0,
        })
    }

    /// Open Key number, 1..=12. Open Key 1 is Camelot 8.
    fn open_key_number(&self) -> u8 {
        ((self.camelot_number + 4) % 12) + 1
    }

    /// Parses Camelot, Open Key or classical notation. Whitespace-tolerant and
    /// case-insensitive. Returns `None` for anything unrecognised.
    pub fn parse(input: &str) -> Option<Self> {
        let s = input.trim();
        if s.is_empty() {
            return None;
        }
        parse_wheel(s).or_else(|| parse_classical(s))
    }

    /// Renders the key in the requested notation.
    pub fn format(&self, notation: KeyNotation) -> String {
        let idx = (self.camelot_number - 1) as usize;
        match notation {
            KeyNotation::Camelot => {
                format!("{}{}", self.camelot_number, if self.is_minor { 'A' } else { 'B' })
            }
            KeyNotation::OpenKey => {
                format!("{}{}", self.open_key_number(), if self.is_minor { 'm' } else { 'd' })
            }
            KeyNotation::Classical => {
                let (minor, major) = CLASSICAL[idx];
                (if self.is_minor { minor } else { major }).to_string()
            }
        }
    }
}

/// Parses `8A` / `8a` (Camelot) and `1m` / `1d` (Open Key).
fn parse_wheel(s: &str) -> Option<MusicalKey> {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let (digits, suffix) = compact.split_at(compact.find(|c: char| !c.is_ascii_digit())?);

    let n: u8 = digits.parse().ok()?;
    if !(1..=12).contains(&n) {
        return None;
    }

    match suffix.to_ascii_lowercase().as_str() {
        "a" => Some(MusicalKey { camelot_number: n, is_minor: true }),
        "b" => Some(MusicalKey { camelot_number: n, is_minor: false }),
        // Open Key: number is offset by 7 positions from Camelot.
        "m" => Some(MusicalKey { camelot_number: ((n + 6) % 12) + 1, is_minor: true }),
        "d" => Some(MusicalKey { camelot_number: ((n + 6) % 12) + 1, is_minor: false }),
        _ => None,
    }
}

/// Parses `Am`, `A minor`, `C`, `Cmaj`, `F#m`, `A-moll` and similar.
fn parse_classical(s: &str) -> Option<MusicalKey> {
    let lower = s.to_ascii_lowercase();
    let cleaned: String = lower
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_' | '.'))
        .collect();

    // Longest root first so `c#` wins over `c`.
    let mut roots: Vec<&(&str, u8)> = ROOTS.iter().collect();
    roots.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));

    for (root, pc) in roots {
        let Some(rest) = cleaned.strip_prefix(root) else { continue };
        let is_minor = match rest {
            "" | "maj" | "major" | "dur" => false,
            "m" | "min" | "minor" | "moll" => true,
            _ => continue,
        };
        let table = if is_minor { MINOR_PC_TO_CAMELOT } else { MAJOR_PC_TO_CAMELOT };
        return Some(MusicalKey {
            camelot_number: table[*pc as usize],
            is_minor,
        });
    }
    None
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-tags`
Expected: PASS, 10 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-tags/
git commit -m "feat(tags): normalise musical keys across Camelot, Open Key and classical notation"
```

---

## Task 7: Reading track metadata

**Files:**
- Create: `crates/noceur-tags/src/meta.rs`
- Modify: `crates/noceur-tags/src/lib.rs`

Test fixtures are built by writing a minimal valid WAV file and then using `lofty` to write tags into it. This keeps the test suite self-contained — no binary fixtures in the repo.

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-tags/src/meta.rs`:

```rust
//! Reading normalised metadata out of audio files.

use crate::key::MusicalKey;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum MetaError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: lofty::error::LoftyError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::probe::Probe;
    use lofty::tag::{Tag, TagType};
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Writes a 0.1s silent 44.1kHz mono 16-bit WAV.
    fn write_wav(path: &PathBuf) {
        let sample_rate = 44_100u32;
        let samples = sample_rate / 10;
        let data_len = samples * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes()); // PCM
        f.extend_from_slice(&1u16.to_le_bytes()); // mono
        f.extend_from_slice(&sample_rate.to_le_bytes());
        f.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        f.extend_from_slice(&2u16.to_le_bytes()); // block align
        f.extend_from_slice(&16u16.to_le_bytes()); // bits
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        std::fs::write(path, f).unwrap();
    }

    /// Writes a tagged WAV and returns its path.
    fn tagged(dir: &TempDir, name: &str, artist: &str, title: &str, bpm: &str, key: &str) -> PathBuf {
        let path = dir.path().join(name);
        write_wav(&path);

        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(title.to_string());
        tag.set_album("So Unreleased Vol.1".to_string());
        tag.insert_text(ItemKey::Bpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        path
    }

    #[test]
    fn reads_artist_title_and_album() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "G76", "it takes time", "123", "8A");
        let m = read(&p).unwrap();
        assert_eq!(m.artist.as_deref(), Some("G76"));
        assert_eq!(m.title.as_deref(), Some("it takes time"));
        assert_eq!(m.album.as_deref(), Some("So Unreleased Vol.1"));
    }

    #[test]
    fn reads_bpm_as_a_float() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "128.5", "8A");
        assert_eq!(read(&p).unwrap().bpm, Some(128.5));
    }

    #[test]
    fn normalises_the_key_and_keeps_the_original_text() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "Am");
        let m = read(&p).unwrap();
        assert_eq!(m.key_tag.as_deref(), Some("Am"));
        assert_eq!(m.key, MusicalKey::parse("8A"));
        assert_eq!(m.key_norm, Some(14));
    }

    #[test]
    fn reads_audio_properties() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "8A");
        let m = read(&p).unwrap();
        assert_eq!(m.sample_rate, Some(44_100));
        assert_eq!(m.channels, Some(1));
        assert!(m.duration_ms.unwrap_or(0) > 0);
    }

    #[test]
    fn untagged_files_yield_empty_fields_not_errors() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("bare.wav");
        write_wav(&p);
        let m = read(&p).unwrap();
        assert!(m.artist.is_none());
        assert!(m.bpm.is_none());
        assert!(m.key.is_none());
        assert_eq!(m.sample_rate, Some(44_100), "properties still readable");
    }

    #[test]
    fn unparseable_bpm_is_dropped_rather_than_failing() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "not a number", "8A");
        assert_eq!(read(&p).unwrap().bpm, None);
    }

    #[test]
    fn unparseable_key_keeps_the_text_but_has_no_canonical_value() {
        let d = TempDir::new().unwrap();
        let p = tagged(&d, "a.wav", "x", "y", "120", "banana");
        let m = read(&p).unwrap();
        assert_eq!(m.key_tag.as_deref(), Some("banana"));
        assert_eq!(m.key, None);
        assert_eq!(m.key_norm, None);
    }

    #[test]
    fn errors_on_a_file_that_is_not_audio() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("cover.jpg");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(read(&p).is_err());
    }

    #[test]
    fn probe_is_available_for_manual_checks() {
        // Guards against a lofty upgrade silently changing the probe API.
        let d = TempDir::new().unwrap();
        let p = d.path().join("a.wav");
        write_wav(&p);
        assert!(Probe::open(&p).unwrap().read().is_ok());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-tags meta`
Expected: FAIL — `cannot find function 'read' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-tags/src/meta.rs`:

```rust
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::{Accessor, ItemKey};
use lofty::probe::Probe;

/// Everything a file claims about itself, normalised.
///
/// Every field is optional: a DJ library contains plenty of files with no tags
/// at all, and a missing field is normal, not an error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub comment: Option<String>,
    pub isrc: Option<String>,
    /// BPM as written in the tag.
    pub bpm: Option<f32>,
    /// Key exactly as written in the tag.
    pub key_tag: Option<String>,
    /// Parsed key, if the text was recognised.
    pub key: Option<MusicalKey>,
    /// Canonical sort value for `key`, 0..=23.
    pub key_norm: Option<i32>,
    pub duration_ms: Option<u64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bit_depth: Option<u8>,
    /// Whether the file carries embedded artwork. The bytes are not loaded here.
    pub has_artwork: bool,
}

/// Reads and normalises the tags and audio properties of one file.
///
/// Returns `Err` only when the file cannot be parsed as audio at all. A valid
/// audio file with no tags yields a `TrackMeta` with empty fields.
pub fn read(path: &Path) -> Result<TrackMeta, MetaError> {
    let tagged = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|source| MetaError::Read {
            path: path.display().to_string(),
            source,
        })?;

    let props = tagged.properties();
    let mut meta = TrackMeta {
        duration_ms: Some(props.duration().as_millis() as u64),
        sample_rate: props.sample_rate(),
        channels: props.channels(),
        bit_depth: props.bit_depth(),
        ..Default::default()
    };

    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return Ok(meta);
    };

    meta.artist = tag.artist().map(|s| s.to_string());
    meta.title = tag.title().map(|s| s.to_string());
    meta.album = tag.album().map(|s| s.to_string());
    meta.genre = tag.genre().map(|s| s.to_string());
    meta.year = tag.year();
    meta.track_no = tag.track();
    meta.comment = tag.comment().map(|s| s.to_string());
    meta.album_artist = tag.get_string(&ItemKey::AlbumArtist).map(|s| s.to_string());
    meta.isrc = tag.get_string(&ItemKey::Isrc).map(|s| s.to_string());
    meta.has_artwork = tag.picture_count() > 0;

    meta.bpm = tag
        .get_string(&ItemKey::Bpm)
        .and_then(|s| s.trim().parse::<f32>().ok())
        .filter(|v| v.is_finite() && *v > 0.0);

    if let Some(raw) = tag.get_string(&ItemKey::InitialKey) {
        let raw = raw.trim();
        if !raw.is_empty() {
            meta.key_tag = Some(raw.to_string());
            meta.key = MusicalKey::parse(raw);
            meta.key_norm = meta.key.map(|k| k.canonical());
        }
    }

    Ok(meta)
}
```

- [ ] **Step 4: Register the module**

Replace `crates/noceur-tags/src/lib.rs`:

```rust
//! Reading and normalising audio file tags.

pub mod key;
pub mod meta;

pub use key::{KeyNotation, MusicalKey};
pub use meta::{read, MetaError, TrackMeta};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-tags`
Expected: PASS.

If any test fails with a `lofty` API mismatch, run `cargo doc -p lofty --open` and correct the call — the field names above target lofty 0.22.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-tags/
git commit -m "feat(tags): read normalised track metadata via lofty"
```

---

## Task 8: Index schema and migrations

**Files:**
- Create: `crates/noceur-index/Cargo.toml`
- Create: `crates/noceur-index/src/lib.rs`
- Create: `crates/noceur-index/src/schema.rs`
- Create: `crates/noceur-index/src/db.rs`

- [ ] **Step 1: Create the crate**

Create `crates/noceur-index/Cargo.toml`:

```toml
[package]
name = "noceur-index"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true
noceur-fsmodel = { path = "../noceur-fsmodel" }
noceur-tags = { path = "../noceur-tags" }
rusqlite = { version = "0.37", features = ["bundled"] }
xxhash-rust = { version = "0.8", features = ["xxh3"] }

[dev-dependencies]
tempfile = "3"
```

Create `crates/noceur-index/src/lib.rs`:

```rust
//! The Noceur index: what we remember about the filesystem.

pub mod db;
pub mod schema;

pub use db::{Db, IndexError};
```

- [ ] **Step 2: Write the schema**

Create `crates/noceur-index/src/schema.rs`:

```rust
//! Schema definition and migration runner.

/// Current schema version. Bump when adding a migration.
pub const SCHEMA_VERSION: i32 = 1;

/// Migrations applied in order. Index 0 upgrades version 0 → 1.
pub const MIGRATIONS: &[&str] = &[V1];

const V1: &str = r#"
CREATE TABLE volume (
    id           INTEGER PRIMARY KEY,
    kind         TEXT    NOT NULL,           -- fixed | removable | network
    fs_uuid      TEXT    NOT NULL UNIQUE,    -- stable identity across drive letters
    label        TEXT    NOT NULL DEFAULT '',
    mount_path   TEXT    NOT NULL,
    online       INTEGER NOT NULL DEFAULT 1,
    last_seen_at INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE dir (
    id         INTEGER PRIMARY KEY,
    volume_id  INTEGER NOT NULL REFERENCES volume(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES dir(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    path_lower TEXT    NOT NULL,
    mtime      INTEGER NOT NULL DEFAULT 0,
    scan_state TEXT    NOT NULL DEFAULT 'pending',
    UNIQUE (volume_id, path_lower)
);

CREATE TABLE content (
    id          INTEGER PRIMARY KEY,
    audio_hash  TEXT UNIQUE,                 -- filled by a later milestone
    quick_key   TEXT NOT NULL UNIQUE,        -- size + head/tail hash
    byte_size   INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER,
    sample_rate INTEGER,
    channels    INTEGER,
    bit_depth   INTEGER,
    codec       TEXT
);

CREATE TABLE file (
    id         INTEGER PRIMARY KEY,
    dir_id     INTEGER NOT NULL REFERENCES dir(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    ext        TEXT    NOT NULL DEFAULT '',
    size       INTEGER NOT NULL DEFAULT 0,
    mtime      INTEGER NOT NULL DEFAULT 0,
    kind       INTEGER NOT NULL,             -- EntryKind::group()
    content_id INTEGER REFERENCES content(id) ON DELETE SET NULL,
    state      TEXT    NOT NULL DEFAULT 'ok',-- ok|locked|unreadable|undecodable|missing
    UNIQUE (dir_id, name)
);

CREATE TABLE content_tag (
    content_id   INTEGER PRIMARY KEY REFERENCES content(id) ON DELETE CASCADE,
    artist       TEXT,
    title        TEXT,
    album        TEXT,
    album_artist TEXT,
    genre        TEXT,
    year         INTEGER,
    track_no     INTEGER,
    comment      TEXT,
    isrc         TEXT,
    bpm_tag      REAL,
    key_tag      TEXT,
    key_norm     INTEGER,
    has_artwork  INTEGER NOT NULL DEFAULT 0,
    read_at      INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX idx_file_dir    ON file(dir_id, kind, name);
CREATE INDEX idx_file_content ON file(content_id);
CREATE INDEX idx_dir_parent  ON dir(parent_id);
"#;
```

The spec's full schema also defines `artwork`, `content_peaks`, `pin`, `session`, `op_log`, `content_analysis` and the `file_fts` virtual table. Those are **deliberately absent from v1** — each arrives as its own migration alongside the milestone that uses it, so the schema never carries tables nothing reads. The migration runner built in this task is what makes that safe.

- [ ] **Step 3: Write the failing tests**

Create `crates/noceur-index/src/db.rs`:

```rust
//! Database handle and migration application.

use crate::schema::{MIGRATIONS, SCHEMA_VERSION};
use rusqlite::Connection;
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("index is newer than this build: found v{found}, support v{supported}")]
    TooNew { found: i32, supported: i32 },
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tables(db: &Db) -> Vec<String> {
        let mut stmt = db
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        rows
    }

    #[test]
    fn creates_every_table() {
        let db = Db::open_in_memory().unwrap();
        let t = tables(&db);
        for expected in ["volume", "dir", "file", "content", "content_tag"] {
            assert!(t.contains(&expected.to_string()), "missing table {expected}");
        }
    }

    #[test]
    fn records_the_schema_version() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn reopening_is_idempotent() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");

        let first = Db::open(&path).unwrap();
        let before = tables(&first);
        drop(first);

        let second = Db::open(&path).unwrap();
        assert_eq!(tables(&second), before);
        assert_eq!(second.version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn enables_foreign_keys() {
        let db = Db::open_in_memory().unwrap();
        let on: i32 = db
            .conn()
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn refuses_an_index_from_a_newer_build() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("index.db");
        {
            let db = Db::open(&path).unwrap();
            db.conn()
                .pragma_update(None, "user_version", SCHEMA_VERSION + 5)
                .unwrap();
        }
        assert!(matches!(Db::open(&path), Err(IndexError::TooNew { .. })));
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p noceur-index`
Expected: FAIL — `cannot find type 'Db' in this scope`.

- [ ] **Step 5: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-index/src/db.rs`:

```rust
/// An open index database.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Opens (creating if needed) the index at `path` and applies migrations.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        // `PRAGMA journal_mode` returns a row, so it must go through
        // `execute_batch` — `pragma_update` errors on result-returning pragmas.
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        Self::init(conn)
    }

    /// Opens a throwaway in-memory index. Used by tests.
    pub fn open_in_memory() -> Result<Self, IndexError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, IndexError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Current schema version stored in `user_version`.
    pub fn version(&self) -> Result<i32, IndexError> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Applies any migrations the file has not seen yet.
    fn migrate(&self) -> Result<(), IndexError> {
        let current = self.version()?;
        if current > SCHEMA_VERSION {
            return Err(IndexError::TooNew {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = i as i32 + 1;
            if current < target {
                self.conn.execute_batch(sql)?;
                self.conn.pragma_update(None, "user_version", target)?;
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS, 5 tests.

- [ ] **Step 7: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): add SQLite schema and migration runner"
```

---

## Task 9: Volume registry

Volumes are identified by a stable id, not a drive letter — so `S:` becoming `E:` does not orphan the index, and unplugging the party SSD marks rows offline rather than deleting them.

**Files:**
- Create: `crates/noceur-index/src/volume.rs`
- Modify: `crates/noceur-index/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-index/src/volume.rs`:

```rust
//! Volume registry: stable identity for drives that come and go.

use crate::db::{Db, IndexError};

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn registers_a_new_volume() {
        let db = db();
        let id = upsert(&db, "uuid-party", VolumeKind::Removable, "PARTY SSD", r"S:\").unwrap();
        let v = get(&db, id).unwrap().unwrap();
        assert_eq!(v.fs_uuid, "uuid-party");
        assert_eq!(v.mount_path, r"S:\");
        assert!(v.online);
    }

    #[test]
    fn upserting_the_same_uuid_returns_the_same_row() {
        let db = db();
        let a = upsert(&db, "uuid-x", VolumeKind::Fixed, "data", r"D:\").unwrap();
        let b = upsert(&db, "uuid-x", VolumeKind::Fixed, "data", r"D:\").unwrap();
        assert_eq!(a, b);
        assert_eq!(all(&db).unwrap().len(), 1);
    }

    #[test]
    fn a_reassigned_drive_letter_updates_the_mount_path() {
        let db = db();
        let id = upsert(&db, "uuid-party", VolumeKind::Removable, "PARTY SSD", r"S:\").unwrap();
        let again = upsert(&db, "uuid-party", VolumeKind::Removable, "PARTY SSD", r"E:\").unwrap();
        assert_eq!(id, again, "same volume despite the new letter");
        assert_eq!(get(&db, id).unwrap().unwrap().mount_path, r"E:\");
    }

    #[test]
    fn going_offline_preserves_the_row() {
        let db = db();
        let id = upsert(&db, "uuid-z", VolumeKind::Network, "sets", r"Z:\").unwrap();
        set_online(&db, id, false).unwrap();

        let v = get(&db, id).unwrap().unwrap();
        assert!(!v.online);
        assert_eq!(all(&db).unwrap().len(), 1, "offline volumes are not deleted");

        set_online(&db, id, true).unwrap();
        assert!(get(&db, id).unwrap().unwrap().online);
    }

    #[test]
    fn get_returns_none_for_an_unknown_id() {
        let db = db();
        assert!(get(&db, 9999).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-index volume`
Expected: FAIL — `cannot find function 'upsert' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-index/src/volume.rs`:

```rust
/// What kind of storage a volume is. Drives watcher strategy in a later milestone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeKind {
    Fixed,
    Removable,
    Network,
}

impl VolumeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            VolumeKind::Fixed => "fixed",
            VolumeKind::Removable => "removable",
            VolumeKind::Network => "network",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "removable" => VolumeKind::Removable,
            "network" => VolumeKind::Network,
            _ => VolumeKind::Fixed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    pub id: i64,
    pub kind: VolumeKind,
    pub fs_uuid: String,
    pub label: String,
    pub mount_path: String,
    pub online: bool,
    pub last_seen_at: i64,
}

/// Registers a volume, or updates its label and mount path if already known.
///
/// Identity is `fs_uuid`, never the mount path — a drive letter reassignment
/// must not create a second volume or orphan existing rows.
pub fn upsert(
    db: &Db,
    fs_uuid: &str,
    kind: VolumeKind,
    label: &str,
    mount_path: &str,
) -> Result<i64, IndexError> {
    db.conn().execute(
        "INSERT INTO volume (kind, fs_uuid, label, mount_path, online, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(fs_uuid) DO UPDATE SET
             label        = excluded.label,
             mount_path   = excluded.mount_path,
             kind         = excluded.kind,
             online       = 1,
             last_seen_at = excluded.last_seen_at",
        rusqlite::params![kind.as_str(), fs_uuid, label, mount_path, now()],
    )?;

    Ok(db.conn().query_row(
        "SELECT id FROM volume WHERE fs_uuid = ?1",
        [fs_uuid],
        |r| r.get(0),
    )?)
}

pub fn get(db: &Db, id: i64) -> Result<Option<Volume>, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT id, kind, fs_uuid, label, mount_path, online, last_seen_at
         FROM volume WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], row_to_volume)?;
    Ok(rows.next().transpose()?)
}

pub fn all(db: &Db) -> Result<Vec<Volume>, IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT id, kind, fs_uuid, label, mount_path, online, last_seen_at
         FROM volume ORDER BY id",
    )?;
    let rows = stmt.query_map([], row_to_volume)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Marks a volume online or offline. Never deletes rows — offline content stays
/// browsable and searchable.
pub fn set_online(db: &Db, id: i64, online: bool) -> Result<(), IndexError> {
    db.conn().execute(
        "UPDATE volume SET online = ?1, last_seen_at = ?2 WHERE id = ?3",
        rusqlite::params![online as i32, now(), id],
    )?;
    Ok(())
}

fn row_to_volume(r: &rusqlite::Row<'_>) -> rusqlite::Result<Volume> {
    Ok(Volume {
        id: r.get(0)?,
        kind: VolumeKind::from_str(&r.get::<_, String>(1)?),
        fs_uuid: r.get(2)?,
        label: r.get(3)?,
        mount_path: r.get(4)?,
        online: r.get::<_, i32>(5)? != 0,
        last_seen_at: r.get(6)?,
    })
}

/// Seconds since the Unix epoch.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 4: Register the module**

In `crates/noceur-index/src/lib.rs`, add `pub mod volume;` and extend re-exports:

```rust
pub use volume::{Volume, VolumeKind};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): add volume registry keyed on stable filesystem id"
```

---

## Task 10: Quick key computation

The quick key is provisional content identity: file size plus a hash of the first and last 64 KiB. Cheap enough to run on every audio file during a background sweep, and strong enough to link copies until the full audio hash confirms them in a later milestone.

**Files:**
- Create: `crates/noceur-index/src/quickkey.rs`
- Modify: `crates/noceur-index/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-index/src/quickkey.rs`:

```rust
//! Provisional content identity from file size plus head and tail bytes.

use std::path::Path;

/// Bytes read from each end of the file.
pub const WINDOW: usize = 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let p = dir.path().join(name);
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn identical_files_share_a_key() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"the same bytes exactly");
        let b = write(&d, "b.wav", b"the same bytes exactly");
        assert_eq!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn different_content_gives_different_keys() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"first content here");
        let b = write(&d, "b.wav", b"other content here");
        assert_ne!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn same_length_different_bytes_still_differ() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"AAAAAAAAAA");
        let b = write(&d, "b.wav", b"BBBBBBBBBB");
        assert_ne!(compute(&a).unwrap(), compute(&b).unwrap());
    }

    #[test]
    fn is_deterministic_across_calls() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"stable");
        assert_eq!(compute(&a).unwrap(), compute(&a).unwrap());
    }

    #[test]
    fn handles_files_larger_than_two_windows() {
        let d = TempDir::new().unwrap();
        let mut big = vec![7u8; WINDOW * 3];
        big[WINDOW * 3 - 1] = 42; // differs only in the tail
        let a = write(&d, "a.wav", &vec![7u8; WINDOW * 3]);
        let b = write(&d, "b.wav", &big);
        assert_ne!(compute(&a).unwrap(), compute(&b).unwrap(), "tail must be covered");
    }

    #[test]
    fn handles_empty_files() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"");
        assert!(compute(&a).is_ok());
    }

    #[test]
    fn key_encodes_the_size() {
        let d = TempDir::new().unwrap();
        let a = write(&d, "a.wav", b"1234567890");
        assert!(compute(&a).unwrap().starts_with("10-"));
    }

    #[test]
    fn errors_on_a_missing_file() {
        assert!(compute(Path::new("./nope-4c1a.wav")).is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-index quickkey`
Expected: FAIL — `cannot find function 'compute' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-index/src/quickkey.rs`:

```rust
use noceur_fsmodel::path::extended;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use xxhash_rust::xxh3::Xxh3;

/// Computes the quick key for a file: `"{size}-{hash}"`.
///
/// Reads at most `2 * WINDOW` bytes regardless of file size, so this is safe to
/// run across a whole library in the background. Two different files sharing a
/// quick key is possible but vanishingly unlikely; the full audio hash added in
/// a later milestone is what makes identity authoritative.
pub fn compute(path: &Path) -> std::io::Result<String> {
    let mut f = File::open(extended(path))?;
    let size = f.metadata()?.len();

    let mut hasher = Xxh3::new();
    hasher.update(&size.to_le_bytes());

    let window = WINDOW.min(size as usize);
    let mut buf = vec![0u8; window];

    if window > 0 {
        f.read_exact(&mut buf)?;
        hasher.update(&buf);

        if size > window as u64 {
            f.seek(SeekFrom::End(-(window as i64)))?;
            f.read_exact(&mut buf)?;
            hasher.update(&buf);
        }
    }

    Ok(format!("{size}-{:016x}", hasher.digest()))
}
```

- [ ] **Step 4: Register the module**

In `crates/noceur-index/src/lib.rs`, add `pub mod quickkey;`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): compute provisional content identity from size and edge bytes"
```

---

## Task 11: Scanning a directory into the index

Stage 1 writes `dir` and `file` rows. Stage 2 probes tags for audio files and links them to `content` rows via the quick key. A file that fails to probe is marked, not dropped.

**Files:**
- Create: `crates/noceur-index/src/scan.rs`
- Modify: `crates/noceur-index/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-index/src/scan.rs`:

```rust
//! Scan stages: enumerate into rows, then probe tags.

use crate::db::{Db, IndexError};
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::{self, VolumeKind};
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};
    use std::fs;
    use tempfile::TempDir;

    fn write_wav(path: &Path) {
        let sample_rate = 44_100u32;
        let samples = sample_rate / 10;
        let data_len = samples * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&sample_rate.to_le_bytes());
        f.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        fs::write(path, f).unwrap();
    }

    fn tag_it(path: &Path, artist: &str, title: &str, bpm: &str, key: &str) {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(title.to_string());
        tag.insert_text(ItemKey::Bpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(path, WriteOptions::default()).unwrap();
    }

    /// Builds a tree: 1 subdir, 2 tagged wavs, 1 untagged wav, 1 txt.
    fn tree() -> TempDir {
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Unsorted")).unwrap();

        let a = d.path().join("a.wav");
        write_wav(&a);
        tag_it(&a, "G76", "it takes time", "123", "8A");

        let b = d.path().join("b.wav");
        write_wav(&b);
        tag_it(&b, "Lizz", "Codex", "124.5", "Am");

        write_wav(&d.path().join("bare.wav"));
        fs::write(d.path().join("notes.txt"), b"hello").unwrap();
        d
    }

    fn setup() -> (Db, TempDir, i64) {
        let db = Db::open_in_memory().unwrap();
        let d = tree();
        let vol = volume::upsert(&db, "uuid-test", VolumeKind::Fixed, "test", "/").unwrap();
        (db, d, vol)
    }

    fn count(db: &Db, sql: &str) -> i64 {
        db.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn stage_one_writes_a_row_per_entry() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        assert!(dir_id > 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 5);
    }

    #[test]
    fn stage_one_records_the_entry_kind() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file WHERE kind = 0"), 1, "dirs");
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file WHERE kind = 1"), 3, "audio");
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file WHERE kind = 2"), 1, "other");
    }

    #[test]
    fn rescanning_does_not_duplicate_rows() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 5);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM dir"), 1);
    }

    #[test]
    fn rescanning_drops_entries_that_disappeared() {
        let (db, d, vol) = setup();
        scan_dir(&db, vol, d.path()).unwrap();
        fs::remove_file(d.path().join("notes.txt")).unwrap();
        scan_dir(&db, vol, d.path()).unwrap();
        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 4);
    }

    #[test]
    fn stage_two_links_audio_files_to_content() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM file WHERE content_id IS NOT NULL"),
            3,
            "only audio files get content rows"
        );
    }

    #[test]
    fn stage_two_stores_normalised_tags() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let (artist, bpm, key_tag, key_norm): (String, f64, String, i64) = db
            .conn()
            .query_row(
                "SELECT ct.artist, ct.bpm_tag, ct.key_tag, ct.key_norm
                 FROM file f JOIN content_tag ct ON ct.content_id = f.content_id
                 WHERE f.name = 'b.wav'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();

        assert_eq!(artist, "Lizz");
        assert!((bpm - 124.5).abs() < 0.001);
        assert_eq!(key_tag, "Am", "original text preserved");
        assert_eq!(key_norm, 14, "Am normalises to Camelot 8A");
    }

    #[test]
    fn identical_copies_share_one_content_row() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

        let src = d.path().join("original.wav");
        write_wav(&src);
        tag_it(&src, "G76", "it takes time", "123", "8A");
        fs::copy(&src, d.path().join("copy.wav")).unwrap();

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        assert_eq!(count(&db, "SELECT COUNT(*) FROM file"), 2);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM content"),
            1,
            "one audio, one content row"
        );
    }

    #[test]
    fn untagged_audio_still_gets_a_content_row() {
        let (db, d, vol) = setup();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let state: String = db
            .conn()
            .query_row("SELECT state FROM file WHERE name = 'bare.wav'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, "ok");
    }

    #[test]
    fn a_file_that_is_not_really_audio_is_marked_undecodable() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
        fs::write(d.path().join("fake.wav"), b"this is not a wav file").unwrap();

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        let state: String = db
            .conn()
            .query_row("SELECT state FROM file WHERE name = 'fake.wav'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(state, "undecodable");
    }

    #[test]
    fn one_bad_file_does_not_stop_the_others() {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

        fs::write(d.path().join("fake.wav"), b"garbage").unwrap();
        let good = d.path().join("good.wav");
        write_wav(&good);
        tag_it(&good, "G76", "fine", "120", "8A");

        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();

        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM file WHERE state = 'ok' AND content_id IS NOT NULL"),
            1
        );
    }
}
```

- [ ] **Step 2: Add lofty as a dev-dependency**

In `crates/noceur-index/Cargo.toml`, extend `[dev-dependencies]`:

```toml
[dev-dependencies]
tempfile = "3"
lofty = "0.22"
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p noceur-index scan`
Expected: FAIL — `cannot find function 'scan_dir' in this scope`.

- [ ] **Step 4: Add the Scan error variant**

The implementation in the next step returns this variant, so add it first. In `crates/noceur-index/src/db.rs`, add to `IndexError`:

```rust
    #[error("scan failed: {0}")]
    Scan(String),
```

- [ ] **Step 5: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-index/src/scan.rs`:

```rust
use crate::quickkey;
use crate::volume::now;
use noceur_fsmodel::{read_dir, EntryKind};
use rusqlite::params;

/// Stage 1: enumerate `path` and write `dir` and `file` rows.
///
/// Returns the `dir.id`. Entries that have vanished since the last scan are
/// removed; entries that are new are inserted; the rest are updated in place so
/// `content_id` links survive a rescan.
pub fn scan_dir(db: &Db, volume_id: i64, path: &Path) -> Result<i64, IndexError> {
    // `read_dir` returns a `DirListing` carrying both entries and any that were
    // skipped (see Amendment 1). Stage 1 ignores skips; the UI surfaces them later.
    let listing = read_dir(path).map_err(|e| IndexError::Scan(e.to_string()))?;
    let entries = listing.entries;

    let path_lower = path.to_string_lossy().to_ascii_lowercase();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path_lower.clone());

    db.conn().execute(
        "INSERT INTO dir (volume_id, parent_id, name, path_lower, mtime, scan_state)
         VALUES (?1, NULL, ?2, ?3, ?4, 'scanned')
         ON CONFLICT(volume_id, path_lower) DO UPDATE SET
             mtime = excluded.mtime, scan_state = 'scanned'",
        params![volume_id, name, path_lower, now()],
    )?;

    let dir_id: i64 = db.conn().query_row(
        "SELECT id FROM dir WHERE volume_id = ?1 AND path_lower = ?2",
        params![volume_id, path_lower],
        |r| r.get(0),
    )?;

    let tx = db.conn().unchecked_transaction()?;

    for e in &entries {
        tx.execute(
            "INSERT INTO file (dir_id, name, ext, size, mtime, kind)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(dir_id, name) DO UPDATE SET
                 ext = excluded.ext, size = excluded.size,
                 mtime = excluded.mtime, kind = excluded.kind",
            params![dir_id, e.name, e.ext, e.size as i64, e.mtime, e.kind.group() as i64],
        )?;
    }

    // Remove rows for entries that no longer exist.
    let names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    let mut stmt = tx.prepare("SELECT id, name FROM file WHERE dir_id = ?1")?;
    let stale: Vec<i64> = stmt
        .query_map([dir_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(Result::ok)
        .filter(|(_, n)| !names.contains(n))
        .map(|(id, _)| id)
        .collect();
    drop(stmt);

    for id in stale {
        tx.execute("DELETE FROM file WHERE id = ?1", [id])?;
    }

    tx.commit()?;
    Ok(dir_id)
}

/// Stage 2: probe tags for every audio file in `dir_id` that has no content link.
///
/// Failures mark the individual row and continue — one corrupt file must never
/// cost the folder.
pub fn probe_dir(db: &Db, dir_id: i64) -> Result<(), IndexError> {
    let mut stmt = db.conn().prepare(
        "SELECT f.id, f.name, d.path_lower
         FROM file f JOIN dir d ON d.id = f.dir_id
         WHERE f.dir_id = ?1 AND f.kind = ?2",
    )?;
    let targets: Vec<(i64, String, String)> = stmt
        .query_map(params![dir_id, EntryKind::Audio.group() as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .filter_map(Result::ok)
        .collect();
    drop(stmt);

    for (file_id, name, dir_path) in targets {
        let full = Path::new(&dir_path).join(&name);

        let quick = match quickkey::compute(&full) {
            Ok(q) => q,
            Err(_) => {
                mark_state(db, file_id, "unreadable")?;
                continue;
            }
        };

        let meta = match noceur_tags::read(&full) {
            Ok(m) => m,
            Err(_) => {
                mark_state(db, file_id, "undecodable")?;
                continue;
            }
        };

        db.conn().execute(
            "INSERT INTO content (quick_key, byte_size, duration_ms, sample_rate, channels, bit_depth)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(quick_key) DO UPDATE SET
                 duration_ms = excluded.duration_ms,
                 sample_rate = excluded.sample_rate,
                 channels    = excluded.channels,
                 bit_depth   = excluded.bit_depth",
            params![
                quick,
                full.metadata().map(|m| m.len() as i64).unwrap_or(0),
                meta.duration_ms.map(|v| v as i64),
                meta.sample_rate.map(|v| v as i64),
                meta.channels.map(|v| v as i64),
                meta.bit_depth.map(|v| v as i64),
            ],
        )?;

        let content_id: i64 = db.conn().query_row(
            "SELECT id FROM content WHERE quick_key = ?1",
            [&quick],
            |r| r.get(0),
        )?;

        db.conn().execute(
            "INSERT INTO content_tag (content_id, artist, title, album, album_artist, genre,
                                      year, track_no, comment, isrc, bpm_tag, key_tag,
                                      key_norm, has_artwork, read_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(content_id) DO UPDATE SET
                 artist=excluded.artist, title=excluded.title, album=excluded.album,
                 album_artist=excluded.album_artist, genre=excluded.genre,
                 year=excluded.year, track_no=excluded.track_no, comment=excluded.comment,
                 isrc=excluded.isrc, bpm_tag=excluded.bpm_tag, key_tag=excluded.key_tag,
                 key_norm=excluded.key_norm, has_artwork=excluded.has_artwork,
                 read_at=excluded.read_at",
            params![
                content_id,
                meta.artist, meta.title, meta.album, meta.album_artist, meta.genre,
                meta.year.map(|v| v as i64), meta.track_no.map(|v| v as i64),
                meta.comment, meta.isrc,
                meta.bpm.map(|v| v as f64), meta.key_tag, meta.key_norm.map(|v| v as i64),
                meta.has_artwork as i64, now(),
            ],
        )?;

        db.conn().execute(
            "UPDATE file SET content_id = ?1, state = 'ok' WHERE id = ?2",
            params![content_id, file_id],
        )?;
    }
    Ok(())
}

fn mark_state(db: &Db, file_id: i64, state: &str) -> Result<(), IndexError> {
    db.conn().execute(
        "UPDATE file SET state = ?1 WHERE id = ?2",
        params![state, file_id],
    )?;
    Ok(())
}
```

- [ ] **Step 6: Register the module**

In `crates/noceur-index/src/lib.rs`, add `pub mod scan;` and re-export:

```rust
pub use scan::{probe_dir, scan_dir};
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS, all scan tests green.

- [ ] **Step 8: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): scan directories into rows and probe tags into content"
```

---

## Task 12: Windowed sorted queries

The UI never asks for a whole folder — it asks for a window of a sorted, filtered view. Sorting happens here in SQL so order is globally correct across a 40k-file folder.

**Files:**
- Create: `crates/noceur-index/src/query.rs`
- Modify: `crates/noceur-index/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/noceur-index/src/query.rs`:

```rust
//! Windowed, sorted reads of a directory listing.

use crate::db::{Db, IndexError};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{probe_dir, scan_dir};
    use crate::volume::{self, VolumeKind};
    use lofty::config::WriteOptions;
    use lofty::prelude::{Accessor, ItemKey, TagExt};
    use lofty::tag::{Tag, TagType};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn write_wav(path: &Path) {
        let sr = 44_100u32;
        let data_len = sr / 10 * 2;
        let mut f = Vec::new();
        f.extend_from_slice(b"RIFF");
        f.extend_from_slice(&(36 + data_len).to_le_bytes());
        f.extend_from_slice(b"WAVEfmt ");
        f.extend_from_slice(&16u32.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&sr.to_le_bytes());
        f.extend_from_slice(&(sr * 2).to_le_bytes());
        f.extend_from_slice(&2u16.to_le_bytes());
        f.extend_from_slice(&16u16.to_le_bytes());
        f.extend_from_slice(b"data");
        f.extend_from_slice(&data_len.to_le_bytes());
        f.extend_from_slice(&vec![0u8; data_len as usize]);
        fs::write(path, f).unwrap();
    }

    fn make(dir: &Path, name: &str, artist: &str, bpm: &str, key: &str) {
        let p = dir.join(name);
        write_wav(&p);
        let mut tag = Tag::new(TagType::Id3v2);
        tag.set_artist(artist.to_string());
        tag.set_title(name.to_string());
        tag.insert_text(ItemKey::Bpm, bpm.to_string());
        tag.insert_text(ItemKey::InitialKey, key.to_string());
        tag.save_to_path(&p, WriteOptions::default()).unwrap();
    }

    fn setup() -> (Db, TempDir, i64) {
        let db = Db::open_in_memory().unwrap();
        let d = TempDir::new().unwrap();
        fs::create_dir(d.path().join("Zebra folder")).unwrap();
        make(d.path(), "c.wav", "Carl", "128", "8A");
        make(d.path(), "a.wav", "Alice", "124", "5A");
        make(d.path(), "b.wav", "Bob", "126", "11B");
        fs::write(d.path().join("aaa.txt"), b"x").unwrap();

        let vol = volume::upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();
        let dir_id = scan_dir(&db, vol, d.path()).unwrap();
        probe_dir(&db, dir_id).unwrap();
        (db, d, dir_id)
    }

    fn names(rows: &[ListRow]) -> Vec<&str> {
        rows.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn groups_directories_first_then_audio_then_other() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        assert_eq!(
            names(&rows),
            ["Zebra folder", "a.wav", "b.wav", "c.wav", "aaa.txt"]
        );
    }

    #[test]
    fn grouping_survives_descending_order() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions { desc: true, ..Default::default() },
        )
        .unwrap();
        assert_eq!(rows[0].name, "Zebra folder", "dir stays first");
        assert_eq!(names(&rows)[1..4], ["c.wav", "b.wav", "a.wav"]);
    }

    #[test]
    fn sorts_by_bpm() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions { sort: SortColumn::Bpm, ..Default::default() },
        )
        .unwrap();
        let audio: Vec<&str> = rows.iter().filter(|r| r.kind == 1).map(|r| r.name.as_str()).collect();
        assert_eq!(audio, ["a.wav", "b.wav", "c.wav"]);
    }

    #[test]
    fn sorts_by_normalised_key_not_alphabetically() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions { sort: SortColumn::Key, ..Default::default() },
        )
        .unwrap();
        let audio: Vec<&str> = rows.iter().filter(|r| r.kind == 1).map(|r| r.name.as_str()).collect();
        // 5A (canonical 8) < 8A (14) < 11B (21)
        assert_eq!(audio, ["a.wav", "c.wav", "b.wav"]);
    }

    #[test]
    fn audio_only_hides_directories_and_other_files() {
        let (db, _d, dir_id) = setup();
        let rows = list(
            &db,
            dir_id,
            ListOptions { audio_only: true, ..Default::default() },
        )
        .unwrap();
        assert_eq!(names(&rows), ["a.wav", "b.wav", "c.wav"]);
    }

    #[test]
    fn windows_return_a_slice_of_the_same_global_order() {
        let (db, _d, dir_id) = setup();
        let all = list(&db, dir_id, ListOptions::default()).unwrap();
        let window = list(
            &db,
            dir_id,
            ListOptions { offset: 1, limit: 2, ..Default::default() },
        )
        .unwrap();
        assert_eq!(names(&window), names(&all)[1..3]);
    }

    #[test]
    fn rows_carry_tag_values() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let a = rows.iter().find(|r| r.name == "a.wav").unwrap();
        assert_eq!(a.artist.as_deref(), Some("Alice"));
        assert_eq!(a.bpm, Some(124.0));
        assert_eq!(a.key_tag.as_deref(), Some("5A"));
        assert!(a.duration_ms.unwrap_or(0) > 0);
    }

    #[test]
    fn directories_have_no_tag_values() {
        let (db, _d, dir_id) = setup();
        let rows = list(&db, dir_id, ListOptions::default()).unwrap();
        let dir = rows.iter().find(|r| r.kind == 0).unwrap();
        assert!(dir.bpm.is_none());
        assert!(dir.artist.is_none());
    }

    #[test]
    fn counts_the_full_view_not_the_window() {
        let (db, _d, dir_id) = setup();
        assert_eq!(count(&db, dir_id, ListOptions::default()).unwrap(), 5);
        assert_eq!(
            count(&db, dir_id, ListOptions { audio_only: true, ..Default::default() }).unwrap(),
            3
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p noceur-index query`
Expected: FAIL — `cannot find function 'list' in this scope`.

- [ ] **Step 3: Write the implementation**

Insert above the `#[cfg(test)]` module in `crates/noceur-index/src/query.rs`:

```rust
/// Column the list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    Name,
    Size,
    Modified,
    Bpm,
    Key,
    Duration,
    Artist,
    Title,
    Album,
}

impl SortColumn {
    /// SQL expression to sort by. Tag columns are NULL for directories, which
    /// SQLite orders first — acceptable because the group clause runs first.
    fn sql(self) -> &'static str {
        match self {
            SortColumn::Name => "f.name COLLATE NOCASE",
            SortColumn::Size => "f.size",
            SortColumn::Modified => "f.mtime",
            SortColumn::Bpm => "ct.bpm_tag",
            SortColumn::Key => "ct.key_norm",
            SortColumn::Duration => "c.duration_ms",
            SortColumn::Artist => "ct.artist COLLATE NOCASE",
            SortColumn::Title => "ct.title COLLATE NOCASE",
            SortColumn::Album => "ct.album COLLATE NOCASE",
        }
    }
}

/// How to slice and order a directory listing.
#[derive(Debug, Clone)]
pub struct ListOptions {
    pub sort: SortColumn,
    pub desc: bool,
    pub audio_only: bool,
    pub offset: i64,
    /// Rows to return. `-1` means no limit.
    pub limit: i64,
}

impl Default for ListOptions {
    fn default() -> Self {
        ListOptions {
            sort: SortColumn::Name,
            desc: false,
            audio_only: false,
            offset: 0,
            limit: -1,
        }
    }
}

/// One row as the UI sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ListRow {
    pub file_id: i64,
    pub name: String,
    pub ext: String,
    pub size: i64,
    pub mtime: i64,
    /// `EntryKind::group()`: 0 directory, 1 audio, 2 other.
    pub kind: i64,
    pub state: String,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub album: Option<String>,
    pub bpm: Option<f64>,
    pub key_tag: Option<String>,
    pub key_norm: Option<i64>,
    pub duration_ms: Option<i64>,
}

const SELECT: &str = "
SELECT f.id, f.name, f.ext, f.size, f.mtime, f.kind, f.state,
       ct.artist, ct.title, ct.album, ct.bpm_tag, ct.key_tag, ct.key_norm,
       c.duration_ms
FROM file f
LEFT JOIN content     c  ON c.id  = f.content_id
LEFT JOIN content_tag ct ON ct.content_id = f.content_id
WHERE f.dir_id = ?1";

/// Reads a window of a sorted, filtered directory listing.
///
/// Grouping (directories, audio, other) is applied before the sort column and is
/// never reversed by `desc`.
pub fn list(db: &Db, dir_id: i64, opts: ListOptions) -> Result<Vec<ListRow>, IndexError> {
    let sql = format!(
        "{SELECT}{audio}
         ORDER BY f.kind ASC, {col} {dir}, f.name COLLATE NOCASE ASC
         LIMIT ?2 OFFSET ?3",
        audio = if opts.audio_only { " AND f.kind = 1" } else { "" },
        col = opts.sort.sql(),
        dir = if opts.desc { "DESC" } else { "ASC" },
    );

    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![dir_id, opts.limit, opts.offset], |r| {
        Ok(ListRow {
            file_id: r.get(0)?,
            name: r.get(1)?,
            ext: r.get(2)?,
            size: r.get(3)?,
            mtime: r.get(4)?,
            kind: r.get(5)?,
            state: r.get(6)?,
            artist: r.get(7)?,
            title: r.get(8)?,
            album: r.get(9)?,
            bpm: r.get(10)?,
            key_tag: r.get(11)?,
            key_norm: r.get(12)?,
            duration_ms: r.get(13)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Total rows in the view, ignoring `offset` and `limit`.
pub fn count(db: &Db, dir_id: i64, opts: ListOptions) -> Result<i64, IndexError> {
    let sql = format!(
        "SELECT COUNT(*) FROM file f WHERE f.dir_id = ?1{}",
        if opts.audio_only { " AND f.kind = 1" } else { "" }
    );
    Ok(db.conn().query_row(&sql, [dir_id], |r| r.get(0))?)
}
```

- [ ] **Step 4: Register the module**

In `crates/noceur-index/src/lib.rs`, add `pub mod query;` and re-export:

```rust
pub use query::{count, list, ListOptions, ListRow, SortColumn};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p noceur-index`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/noceur-index/
git commit -m "feat(index): add windowed sorted directory queries"
```

---

## Task 13: Debug CLI

The engine is finished but invisible. This gives you a way to point it at a real folder.

**Files:**
- Create: `crates/noceur-cli/Cargo.toml`
- Create: `crates/noceur-cli/src/main.rs`

- [ ] **Step 1: Create the crate**

Create `crates/noceur-cli/Cargo.toml`:

```toml
[package]
name = "noceur-cli"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "noceur"
path = "src/main.rs"

[dependencies]
noceur-index = { path = "../noceur-index" }
noceur-tags  = { path = "../noceur-tags" }
clap = { version = "4", features = ["derive"] }
```

- [ ] **Step 2: Write the CLI**

Create `crates/noceur-cli/src/main.rs`:

```rust
//! Debug CLI for the Noceur engine. Not the shipping interface.

use clap::{Parser, Subcommand, ValueEnum};
use noceur_index::query::{count, list, ListOptions, SortColumn};
use noceur_index::volume::{upsert, VolumeKind};
use noceur_index::{probe_dir, scan_dir, Db};
use noceur_tags::{KeyNotation, MusicalKey};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "noceur", about = "Noceur engine debug CLI")]
struct Cli {
    /// Index database path. Defaults to an in-memory database.
    #[arg(long)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan a directory and print it in the app's sort order.
    Ls {
        path: PathBuf,
        #[arg(long, value_enum, default_value_t = Sort::Name)]
        sort: Sort,
        #[arg(long)]
        desc: bool,
        #[arg(long)]
        audio_only: bool,
        /// Display notation for the key column.
        #[arg(long, value_enum, default_value_t = Notation::Camelot)]
        notation: Notation,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    Name,
    Size,
    Modified,
    Bpm,
    Key,
    Duration,
    Artist,
    Title,
    Album,
}

impl From<Sort> for SortColumn {
    fn from(s: Sort) -> Self {
        match s {
            Sort::Name => SortColumn::Name,
            Sort::Size => SortColumn::Size,
            Sort::Modified => SortColumn::Modified,
            Sort::Bpm => SortColumn::Bpm,
            Sort::Key => SortColumn::Key,
            Sort::Duration => SortColumn::Duration,
            Sort::Artist => SortColumn::Artist,
            Sort::Title => SortColumn::Title,
            Sort::Album => SortColumn::Album,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Notation {
    Camelot,
    OpenKey,
    Classical,
}

impl From<Notation> for KeyNotation {
    fn from(n: Notation) -> Self {
        match n {
            Notation::Camelot => KeyNotation::Camelot,
            Notation::OpenKey => KeyNotation::OpenKey,
            Notation::Classical => KeyNotation::Classical,
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let db = match &cli.db {
        Some(p) => Db::open(p)?,
        None => Db::open_in_memory()?,
    };

    match cli.command {
        Command::Ls { path, sort, desc, audio_only, notation } => {
            let path = path.canonicalize()?;
            let vol = upsert(&db, "cli-local", VolumeKind::Fixed, "local", "/")?;

            let started = std::time::Instant::now();
            let dir_id = scan_dir(&db, vol, &path)?;
            let scanned = started.elapsed();
            probe_dir(&db, dir_id)?;
            let probed = started.elapsed();

            let opts = ListOptions {
                sort: sort.into(),
                desc,
                audio_only,
                ..Default::default()
            };
            let total = count(&db, dir_id, opts.clone())?;
            let rows = list(&db, dir_id, opts)?;

            for r in &rows {
                let marker = match r.kind {
                    0 => "/",
                    2 => " ",
                    _ => " ",
                };
                let bpm = r.bpm.map(|v| format!("{v:.1}")).unwrap_or_else(|| "—".into());
                let key = r
                    .key_norm
                    .and_then(|v| MusicalKey::from_canonical(v as i32))
                    .map(|k| k.format(notation.into()))
                    .or_else(|| r.key_tag.clone())
                    .unwrap_or_else(|| "—".into());
                let dur = r
                    .duration_ms
                    .map(|ms| format!("{}:{:02}", ms / 60_000, (ms / 1000) % 60))
                    .unwrap_or_else(|| "—".into());
                let state = if r.state == "ok" { String::new() } else { format!("  [{}]", r.state) };

                println!(
                    "{marker} {:<44} {:>7} {:>5} {:>7}  {}{}",
                    truncate(&r.name, 44),
                    bpm,
                    key,
                    dur,
                    r.artist.clone().unwrap_or_default(),
                    state
                );
            }

            eprintln!(
                "\n{total} entries · scan {:.0}ms · probe {:.0}ms",
                scanned.as_secs_f64() * 1000.0,
                (probed - scanned).as_secs_f64() * 1000.0
            );
        }
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}
```

- [ ] **Step 3: Build and run against a real folder**

Run: `cargo build --release`
Expected: builds clean.

Run: `cargo run --release -p noceur-cli -- ls "D:\Music"`
Expected: directories listed first, then audio files with BPM/key/duration/artist, then other files. Timing on stderr.

- [ ] **Step 4: Verify sorting and notation work**

Run: `cargo run --release -p noceur-cli -- ls "D:\Music" --sort bpm --audio-only`
Expected: audio only, ascending BPM, files with no BPM first.

Run: `cargo run --release -p noceur-cli -- ls "D:\Music" --notation classical`
Expected: key column shows `Am`, `C`, `F#m` rather than `8A`, `8B`, `11A`.

- [ ] **Step 5: Commit**

```bash
git add crates/noceur-cli/
git commit -m "feat(cli): add debug CLI for listing a folder through the engine"
```

---

## Task 14: End-to-end integration test

**Files:**
- Create: `crates/noceur-index/tests/end_to_end.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/noceur-index/tests/end_to_end.rs`:

```rust
//! Full pipeline: build a tree, scan it, mutate it, rescan, verify convergence.

use lofty::config::WriteOptions;
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::tag::{Tag, TagType};
use noceur_index::query::{list, ListOptions, SortColumn};
use noceur_index::volume::{upsert, VolumeKind};
use noceur_index::{probe_dir, scan_dir, Db};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_wav(path: &Path) {
    let sr = 44_100u32;
    let data_len = sr / 10 * 2;
    let mut f = Vec::new();
    f.extend_from_slice(b"RIFF");
    f.extend_from_slice(&(36 + data_len).to_le_bytes());
    f.extend_from_slice(b"WAVEfmt ");
    f.extend_from_slice(&16u32.to_le_bytes());
    f.extend_from_slice(&1u16.to_le_bytes());
    f.extend_from_slice(&1u16.to_le_bytes());
    f.extend_from_slice(&sr.to_le_bytes());
    f.extend_from_slice(&(sr * 2).to_le_bytes());
    f.extend_from_slice(&2u16.to_le_bytes());
    f.extend_from_slice(&16u16.to_le_bytes());
    f.extend_from_slice(b"data");
    f.extend_from_slice(&data_len.to_le_bytes());
    f.extend_from_slice(&vec![0u8; data_len as usize]);
    fs::write(path, f).unwrap();
}

fn make(dir: &Path, name: &str, artist: &str, bpm: &str, key: &str) {
    let p = dir.join(name);
    write_wav(&p);
    let mut tag = Tag::new(TagType::Id3v2);
    tag.set_artist(artist.to_string());
    tag.set_title(name.to_string());
    tag.insert_text(ItemKey::Bpm, bpm.to_string());
    tag.insert_text(ItemKey::InitialKey, key.to_string());
    tag.save_to_path(&p, WriteOptions::default()).unwrap();
}

#[test]
fn scan_mutate_rescan_converges() {
    let db = Db::open_in_memory().unwrap();
    let d = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    fs::create_dir(d.path().join("Unsorted")).unwrap();
    make(d.path(), "a.wav", "Alice", "124", "5A");
    make(d.path(), "b.wav", "Bob", "126", "Am");
    fs::write(d.path().join("notes.txt"), b"x").unwrap();

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(&db, dir_id, ListOptions::default()).unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].name, "Unsorted", "directory first");
    assert_eq!(rows.last().unwrap().name, "notes.txt", "other last");

    // Mutate: delete one, add one.
    fs::remove_file(d.path().join("a.wav")).unwrap();
    make(d.path(), "c.wav", "Carl", "128", "11B");

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(&db, dir_id, ListOptions::default()).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Unsorted", "b.wav", "c.wav", "notes.txt"]);
}

#[test]
fn a_copy_in_another_folder_reuses_the_content_row() {
    let db = Db::open_in_memory().unwrap();
    let root = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    let promos = root.path().join("promos");
    let party = root.path().join("party");
    fs::create_dir(&promos).unwrap();
    fs::create_dir(&party).unwrap();

    make(&promos, "track.wav", "G76", "123", "8A");
    fs::copy(promos.join("track.wav"), party.join("track.wav")).unwrap();

    for dir in [&promos, &party] {
        let id = scan_dir(&db, vol, dir).unwrap();
        probe_dir(&db, id).unwrap();
    }

    let files: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM file WHERE kind = 1", [], |r| r.get(0))
        .unwrap();
    let contents: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM content", [], |r| r.get(0))
        .unwrap();

    assert_eq!(files, 2, "two paths");
    assert_eq!(contents, 1, "one piece of audio");
}

#[test]
fn key_sort_is_harmonic_not_alphabetical() {
    let db = Db::open_in_memory().unwrap();
    let d = TempDir::new().unwrap();
    let vol = upsert(&db, "u", VolumeKind::Fixed, "t", "/").unwrap();

    make(d.path(), "x.wav", "X", "120", "10A");
    make(d.path(), "y.wav", "Y", "120", "2A");
    make(d.path(), "z.wav", "Z", "120", "9A");

    let dir_id = scan_dir(&db, vol, d.path()).unwrap();
    probe_dir(&db, dir_id).unwrap();

    let rows = list(
        &db,
        dir_id,
        ListOptions { sort: SortColumn::Key, ..Default::default() },
    )
    .unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    // Alphabetically "10A" < "2A"; harmonically 2A < 9A < 10A.
    assert_eq!(names, ["y.wav", "z.wav", "x.wav"]);
}
```

- [ ] **Step 2: Confirm the dev-dependencies**

`crates/noceur-index/Cargo.toml` must contain exactly this `[dev-dependencies]` section (added in Task 11):

```toml
[dev-dependencies]
tempfile = "3"
lofty = "0.22"
```

- [ ] **Step 3: Run the integration tests**

Run: `cargo test -p noceur-index --test end_to_end`
Expected: PASS, 3 tests.

- [ ] **Step 4: Run the whole suite and lint**

Run: `cargo test --workspace`
Expected: PASS, all tests.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings. Fix anything reported.

Run: `cargo fmt --all`
Expected: reformats in place.

- [ ] **Step 5: Commit**

```bash
git add crates/
git commit -m "test(index): add end-to-end scan, mutate and rescan coverage"
```

---

## Task 15: Milestone verification

- [ ] **Step 1: Run against your real library**

Run: `cargo run --release -p noceur-cli -- ls "D:\Music\House" --sort bpm`

Verify by eye:
- Directories appear before audio files, and `.txt`/`.jpg` files appear last.
- BPM and key columns are populated for files that carry those tags.
- Keys that were written as `Am` or `1m` display as Camelot.
- Files with no tags show `—` rather than crashing or being skipped.

- [ ] **Step 2: Check a folder on the network share**

Run: `cargo run --release -p noceur-cli -- ls "Z:\sets"`
Expected: listing completes. Note the scan and probe timings printed on stderr — this is your baseline for the network-drive work in a later milestone.

- [ ] **Step 3: Check a folder with a very long path**

Build a directory chain past the 260-character `MAX_PATH` limit, drop a track into it, and list it. In PowerShell:

```powershell
$deep = "D:\noceur-longpath-test"; 1..12 | ForEach-Object { $deep = Join-Path $deep ("segment-" + "x" * 20) }; New-Item -ItemType Directory -Force $deep; Copy-Item "D:\Music\House\*.wav" $deep -ErrorAction SilentlyContinue | Out-Null; $deep.Length
```

Confirm the printed length is above 260, then run:

```bash
cargo run --release -p noceur-cli -- ls "<paste the path printed above>"
```

Expected: the listing succeeds with no `The system cannot find the path specified` or `path too long` error. This is Task 2's `extended()` working end to end. Remove the test tree afterwards with `Remove-Item -Recurse -Force "D:\noceur-longpath-test"`.

- [ ] **Step 4: Record the baseline**

Create `docs/superpowers/notes/2026-08-05-engine-baseline.md` with the timings observed in Steps 1–2 and the folder sizes they came from. Later milestones are measured against this.

- [ ] **Step 5: Commit**

```bash
git add docs/
git commit -m "docs: record engine foundation performance baseline"
```

---

## Definition of Done

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [ ] `noceur ls` lists a real music folder in the app's sort order with BPM, key and duration from tags
- [ ] A copy of a track in a second folder produces two `file` rows and one `content` row
- [ ] A corrupt file is marked `undecodable` and does not stop the scan
- [ ] A path over 260 characters lists without error on Windows

## What this milestone deliberately does not do

Watchers, audio hashing, offline reconciliation, FTS search, file operations, undo, decoding, peaks, waveform rendering, playback, the action system, and the Tauri shell. Each has its own plan.
