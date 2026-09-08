# Noceur — Sub-project 1: Browser Core

**Date:** 2026-08-05
**Status:** Design approved, ready for planning
**Scope:** Sub-project 1 of 3

---

## 1. Context

Two existing tools each solve half of a problem DJs have:

- **[Flow](https://flowdj.app/)** — a native file browser that treats the filesystem as the source of truth and renders waveform, BPM and key inline per row. No library import. Closed source, no metadata fetching, no tag writing beyond basics.
- **[OneTagger](https://github.com/Marekkon5/onetagger)** — GPL-3.0, Rust + Vue, ~11 crates. Fetches metadata from Beatport, Discogs, MusicBrainz, Traxsource, Juno, iTunes, Beatsource, Spotify and Shazam; Quick Tag with keyboard bindings; template-based renaming. Batch-job UI with no browsing.

Neither wants to become the other. The union — a file browser that is natively audio-aware, with a proper player and a tagger built in — does not exist.

The user currently runs Directory Opus (with a music metadata panel) and foobar2000 side by side, plus Mp3tag, and copies tracks between HDDs, network shares and a party SSD by hand.

### Constraints given

| Constraint | Consequence |
|---|---|
| Not for sale; open source on GitHub, personal-to-community scope | GPL-3.0 is free to adopt. libKeyFinder, aubio, Essentia and OneTagger's own code are all available to link or read. |
| Spotify Audio Features not wanted | Avoids a dead dependency — Spotify [deprecated `/audio-features` and `/audio-analysis`](https://developer.spotify.com/blog/2024-11-27-changes-to-the-web-api) on 2024-11-27 for all apps without prior extended quota. |
| Beatport / Juno / Traxsource not required; 4–5 sources with fallback is enough | Removes the scraper-maintenance tax entirely. Sub-project 3 can be built on real APIs (MusicBrainz, Discogs, Deezer, AcoustID, iTunes). |

### Storage reality

20k–100k tracks. Most on local HDDs. Some sets on a network share (`Z:`). Tracks are **copied** into Rekordbox folders and onto an SSD for parties — so the same audio exists at several paths simultaneously, sometimes retagged by Rekordbox after copying.

This single fact drives the core data-model decision in §5.

---

## 2. Decomposition

The full product is three subsystems, each with its own spec → plan → build cycle:

1. **Browser core** *(this spec)* — filesystem browsing, index, search, file operations, waveform, playback, action system.
2. **Analysis engine** — BPM detection, key detection, spectrogram. The trust layer.
3. **Tagger** — tag writing, Quick Tag, custom fields, artwork, multi-source metadata fetch with fallback.

Playback and waveform rendering were moved from (2) into (1): a seekable waveform requires a player, peak extraction requires a decoder, and neither is hard. This leaves (2) as purely the difficult DSP.

### Non-goals for sub-project 1

- BPM detection, key detection, spectrograms — sub-project 2.
- Tag **writing**, Quick Tag, custom tag fields, artwork editing — sub-project 3.
- Any network metadata fetching — sub-project 3.
- A conditional rule/scripting engine for templates — sub-project 3. Simple placeholder substitution only (§10).
- Playlist formats, DJ-software library sync, cue point parsing.
- Polished macOS/Linux builds. Both stay buildable; Windows is the tested target.

---

## 3. Product decisions

| Decision | Choice |
|---|---|
| Name | **Noceur** — French, one who stays up all night. Verified free on crates.io (0 matches) and effectively free on GitHub (one dormant 2020 blog). |
| License | GPL-3.0 |
| Primary platform | Windows 11 x64. Linux/macOS buildable, untested. |
| Stack | Tauri v2, Rust engine, TypeScript frontend |
| Layout | Single pane by default; splits to dual pane on `Ctrl+\` |
| Primary waveform | Full-width transport bar at window bottom |
| Secondary waveform | Compact, in the right inspector under the artwork |
| Theme | **Sodium** — bone-on-black monochrome, single warm accent `#e8a55f`. Alternates: bone, ice, moss, violet, ember. |
| BPM/key in this sub-project | Read from existing tags, visually distinguished from analysed values (§9.3) |

---

## 4. Architecture

Seven crates in four tiers. All dependencies point downward; no cycles.

```
ui/                          Frontend (TypeScript)
  │  Tauri commands (batched) ▲ / events ▼
noceur-app                   Shell — the ONLY Tauri-aware crate
  │
noceur-index  noceur-render  noceur-actions      Engine
  │
noceur-fsmodel  noceur-tags  noceur-audio        Leaves (mutually independent)
```

### Crate responsibilities

**`noceur-fsmodel`** — Directory enumeration. Windows long-path (`\\?\`) and UNC handling. Volume detection and identity. All mutating operations: copy, move, rename, delete, mkdir — each with conflict policy, progress reporting and undo records.

**`noceur-tags`** — Tag reading via `lofty` into one normalised `TrackMeta`. Key-notation conversion (Camelot ⇄ Open Key ⇄ classical). BPM parsing. Artwork extraction. Read-only in this sub-project.

**`noceur-audio`** — Decode via `symphonia`. Format/duration probe. Peak extraction. Playback engine on `cpal` — play, pause, seek.

**`noceur-index`** — SQLite + FTS5. Content identity and hashing. Directory scanner. Filesystem watchers. Volume mount/unmount reconciliation. Job queue. Windowed list queries. Search.

**`noceur-render`** — Rasterizes peaks into waveform sprites via `tiny-skia`. Major/minor tiers, played-vs-unplayed split, themeable colour pairs, sprite atlas for row-sized renders.

**`noceur-actions`** — Action registry and config format. Placeholder substitution. Executes open-with, reveal, web-search, copy-to, move-to, rename, run-command.

**`noceur-app`** — Tauri commands, app state, event emission, settings persistence, session restore. Translation only; no logic worth testing.

**`ui/`** — Solid + TanStack Virtual. Holds ~100 rows at a time. Owns layout, panes, tabs, columns, keyboard map. Never sees a peak array or a raw path — only view models and image blobs.

### The rule

Nothing below `noceur-app` links Tauri. The whole engine compiles and tests headless: index a folder, hash it, rasterize a waveform, rename a file — all in a unit test with no window. A CLI is free. The web view can be replaced without touching the engine.

### Two performance rules that follow

1. **Waveforms are rasterized in Rust.** `noceur-render` produces sprites; the frontend blits images. A 100k-row list with a waveform per row is impossible if the UI receives peak arrays and draws them.
2. **The frontend requests windows of rows**, never per-file calls in a loop. Sorting and filtering happen in SQLite, so sort order is globally correct across a 40k-file folder rather than correct only across loaded rows.

---

## 5. Data model

### 5.1 Identity is the audio, not the file

A `content` row represents distinct audio. A `file` row represents a path. Many files → one content.

The content hash covers **decoded audio frames only, skipping metadata blocks**. When Rekordbox writes BPM and cue points into the party-SSD copy, the bytes on disk change but the identity does not — the copy still resolves to the same analysed content.

Consequences:
- Copy a track to the party SSD and its waveform (and later, BPM/key) is already present.
- "Where else do I have this?" is a single indexed query, including across offline volumes.
- Re-tagging never triggers re-analysis.

Hashing the whole file instead would make every retag look like a new unknown track.

### 5.2 Schema

```sql
volume(id, kind, fs_uuid, label, mount_path, online, last_seen_at)
dir(id, volume_id, parent_id, name, path_lower, mtime, scan_state)
file(id, dir_id, name, ext, size, mtime, ctime, is_audio, content_id, quick_key, state)
     -- state ∈ {ok, locked, unreadable, undecodable, missing}; drives row indicators (§12)
content(id, audio_hash, byte_size, duration_ms, sample_rate, channels, codec, bit_depth)
content_tag(content_id, artist, title, album, album_artist, genre, year,
            track_no, disc_no, bpm_tag, key_tag, key_norm, rating, isrc,
            comment, artwork_id, read_at)
content_peaks(content_id, tier, bucket_count, data BLOB)
artwork(id, sha256, mime, width, height, bytes BLOB)
pin(id, path, label, sort_order)
session(pane_id, tab_index, path, history_json, columns_json, sort_json)
op_log(id, kind, payload_json, undo_json, at)      -- undo stack
file_fts(name, artist, title, album, path)          -- FTS5, external content
```

`content_analysis` (bpm_detected, key_detected, engine_version, analyzed_at) is added by sub-project 2. The schema is designed for it now; the table is not created yet.

**Key normalisation.** Files carry a mix of Camelot (`8A`), Open Key (`9m`) and classical (`Am`) depending on what wrote them. `key_norm` stores a canonical integer so the column sorts correctly regardless of source; `key_tag` preserves the original string. Display notation is a setting.

### 5.3 Offline volumes

Unplugging the party SSD sets `volume.online = 0`. Rows are **not** deleted. Offline content stays searchable and visible, marked as such in the tree and in search results. Reconnecting triggers a reconciliation pass keyed on `fs_uuid`, not drive letter — so `S:` becoming `E:` does not orphan the index.

---

## 6. Scanning

Five stages, ordered so the user never waits on a queue.

| Stage | Work | When |
|---|---|---|
| 1 · Enumerate | Read directory. Names, sizes, mtimes. Rows appear. | Instant, on navigate |
| 2 · Probe tags | `lofty` read → artist, title, album, BPM, key, rating, artwork ref | Milliseconds, visible folder prioritised |
| 3 · Quick key | `size` + xxh3 of head and tail | Background |
| 4 · Audio hash | Hash audio frames, skipping metadata | Background, pinned trees |
| 5 · Peaks | Decode, extract major + minor peaks, cache as BLOB | Background, job queue |

Stages 1–2 run on the current folder ahead of everything else. Stages 3–5 run on pinned trees at low priority via a `rayon` pool, are fully resumable, and are checkpointed per file — killing the app mid-scan costs the current file only.

### Watchers

- **Local NTFS:** real watchers via `notify`.
- **Network shares:** watchers are unreliable or silent over SMB. Network trees are revalidated on navigate, on window focus, and by a periodic sweep. Silent staleness on `Z:` is the explicit failure mode being designed against.
- **Removable:** volume arrival/removal events drive online/offline transitions and reconciliation.

---

## 7. Search

`Ctrl+F` opens a centred popover searching **every pinned folder, plus the current directory tree** whether or not it is pinned — not the current directory alone.

- FTS5 over `name`, `artist`, `title`, `album`, `path`.
- Results grouped: Tracks, then Folders.
- Each result shows path and BPM/key; offline results are marked, not hidden.
- Ranking: exact prefix > word prefix > substring, then recency of access.
- `↵` open · `⇧↵` reveal in list · `Ctrl+M` move.

`Ctrl+M` opens the move popover: fuzzy-filters destinations across pinned folders with the most-recently-used target preselected, and accepts a typed path.

---

## 8. Waveform and playback

### Rendering

Two tiers from one peak set. Major tier is the seek surface; minor tier is the same peaks compressed, keeping arrangement readable. Bars sit on the app background — no container fill. Unplayed bars are the foreground colour at 22%; played bars take the accent; the minor tier renders at 34% opacity.

Configurable: colour pair, bar width, gap, tier heights, linear/logarithmic amplitude scaling.

Peaks are computed once per `content` at a fixed bucket count and cached as a BLOB. Display at any width resamples from the cached buckets — no re-decode on resize.

### Placement

- **Primary:** full-width transport bar at the window bottom. Survives split mode. Collapsible.
- **Secondary:** compact, in the inspector under the artwork.

Both are live and both seek. Visibility of each is a setting.

### Playback

`cpal` output. Play/pause on `Space`, seek by click or drag on either waveform, no gapless or crossfade requirement in this sub-project. Playback is preview, not performance — it exists to audition while browsing.

---

## 9. UI

### 9.1 Structure

Top to bottom: title/menu · action toolbar · favourites bar · tab strip · breadcrumb + search · body (tree | list | inspector) · waveform transport.

### 9.2 List

- Sort groups are fixed: **directories first, then audio files, then everything else**, regardless of active sort column.
- Columns: name, waveform, BPM, key, duration, artist, title, album, rating, size, date. All sortable, reorderable, resizable; layout persisted per pane.
- Audio-only toggle filters non-audio rows.
- Virtualized; only ~100 rows realised at any time.

### 9.3 Three-state BPM and key

| State | Rendering |
|---|---|
| Analysed (sub-project 2) | Full precision, full opacity — `123.0` |
| Read from file tags | Dimmed, as stored, trailing dot — `124·` |
| Neither | `—` at 14% opacity |

Sorting works across all three states. The distinction exists because analysed values and Rekordbox-written values will disagree, and silently merging them loses information.

### 9.4 Panes and tabs

- Single pane default; `Ctrl+\` splits, `Tab` moves focus, `Ctrl+\` again collapses.
- Both panes are the same list component; split is a container concern.
- Independent tabs per pane. Back/forward history per pane, `Alt+←/→`.
- Full session — panes, tabs, paths, column layout, sort — restored on launch.

Built as specified, with three notes on what shipped:

- **Two panes, not a tree of them.** A recursive split is what a text editor needs. A file
  browser needs the folder you are filing *into* beside the one you are filing *out of*, and past
  two the panes are narrower than a filename.
- **Sort is per tab; columns stay global.** Sort belongs to the tab because the reason to hold two
  folders open is usually that they differ. Which columns exist, how wide and in what order, is
  one answer to "what is a track" and is edited in one settings panel — per-pane column widths in
  a half-width pane would be a second layout to maintain and no better to read.
- **History is restored as a path, not as a stack.** A tab comes back where it was, with its back
  button empty. Restoring the walk that got you somewhere last week is remembering a route nobody
  is still on.
- **Tabs drag**, to reorder or across the divider, and are drop targets for files: dropping a
  selection on a tab files it into that tab's folder without going there. Not in the original
  spec; it is the shortest route from the split to the reason for the split.

### 9.5 Keyboard

| Key | Action |
|---|---|
| `Ctrl+F` | Search all pinned folders |
| `Ctrl+\` | Split / unsplit |
| `Tab` | Focus other pane |
| `Ctrl+T` | New tab at current folder |
| `Ctrl+W` | Close tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+1…9` | Jump to tab, `9` being the last |
| `Space` | Play / pause selected |
| `T` | Quick tag |
| `F2` | Edit metadata |
| `Del` | Recycle selection |
| `Ctrl+R` | Rename from template |
| `F5` / `F6` | Copy / move to other pane |
| `Ctrl+M` | Move-to popover |
| `Alt+←` / `Alt+→` | Back / forward (per pane) |
| `Ctrl+D` | Pin current folder |
| `Ctrl+Z` | Undo last file operation |

`Ctrl+1…9` was originally "jump to favourite". It went to the tabs instead: it is what the key
means in every application with a tab strip, and a pinned folder is one click away in the tree
whereas a tab behind eleven others is not.

---

## 10. Action system

User-defined toolbar buttons, each bindable to a key, defined in `actions.toml`.

### Action types

| Type | Behaviour |
|---|---|
| `open_with` | Launch external app with selected paths (foobar2000, Mp3tag) |
| `reveal` | Show in Explorer |
| `web_search` | Open URL template in default browser |
| `copy_to` / `move_to` | Named destinations; dropdown, fuzzy-filterable |
| `rename` | Filename from template |
| `run` | Arbitrary command with substituted arguments |

### Templates

Placeholder substitution only: `{artist}`, `{title}`, `{album}`, `{bpm}`, `{key}`, `{year}`, `{ext}`, `{filename}`, plus `{n}` for collision counters. A small function set — `lower`, `upper`, `pad`, `sanitize`, `default` — applied as `{artist|sanitize}`.

Conditionals, per-field transforms and scripting are **out of scope**; they belong with tag writing in sub-project 3, which will share one engine.

### Example

```toml
[[action]]
id = "rename_std"
label = "Rename"
type = "rename"
template = "{artist|sanitize} — {title|sanitize}.{ext}"
key = "Ctrl+R"

[[action]]
id = "to_party"
label = "Copy to"
type = "copy_to"
destinations = [
  { label = "Party SSD",    path = "S:\\PARTY SSD" },
  { label = "Rekordbox In", path = "D:\\Rekordbox\\_in" },
]

[[action]]
id = "lookup"
label = "Search on"
type = "web_search"
targets = [
  { label = "Discogs",     url = "https://www.discogs.com/search/?q={artist}+{title}" },
  { label = "MusicBrainz", url = "https://musicbrainz.org/search?query={artist}+{title}&type=recording" },
  { label = "YouTube",     url = "https://www.youtube.com/results?search_query={artist}+{title}" },
]
```

Renaming is a first-class browser operation, not deferred: this is a file browser and it must write to disk.

---

## 11. File operations

All mutating operations live in `noceur-fsmodel` — one implementation of conflict handling, progress and undo.

### Semantics

- **Copy / move:** streamed with progress. Same-volume moves use rename where possible. Cross-volume moves are copy-then-verify-then-delete; the delete only happens after size and hash confirm.
- **Conflicts:** per-operation policy — skip, overwrite, rename with `{n}`, or ask. Batch operations offer apply-to-all.
- **Delete:** to Recycle Bin by default. Permanent delete requires explicit confirmation and is never the default.
- **Rename:** template or inline edit; collision-checked before execution.

### Undo

Every mutating operation writes an `op_log` entry with an inverse. `Ctrl+Z` reverses the last one.

- Move and rename invert exactly.
- Copy inverts by deleting the created files (only if unmodified since).
- Delete inverts by Recycle Bin restore.
- Overwrites are **not** undoable — so overwrite is never a silent default.

`op_log` persists across restarts as an operation history. The **undo stack is session-scoped** — it does not survive a restart — and is bounded at 100 operations. Undoing a persisted-but-previous-session operation is deliberately not offered, because the filesystem may have moved on beneath it.

### Index coherence

Operations update the index in the same transaction that records the op. A move updates `file.dir_id` and leaves `content_id` untouched — moving a track never discards its analysis.

---

## 12. Error handling

The engine's rule: **degrade the row, never the session.** No single bad file may stall a scan or crash a pane.

| Failure | Handling |
|---|---|
| File locked (Rekordbox/Serato holding it) | Retry with backoff, then mark row locked with an indicator. Never block the UI. |
| Permission denied | Mark the directory unreadable in the tree; continue siblings. |
| Corrupt / undecodable audio | Mark content undecodable with the decoder error; keep filesystem metadata. Do not retry every scan. |
| Path > 260 chars | `\\?\` prefixing throughout `fsmodel`. Covered by tests. |
| Network share disappears mid-scan | Mark volume offline, checkpoint the queue, resume on return. |
| Drive letter reassigned | Reconcile on `fs_uuid`; rewrite mount paths. |
| SQLite busy / WAL contention | Single writer thread, readers via connection pool. |
| Disk full during copy | Abort, delete the partial target, surface the error. Never leave a truncated audio file. |

Errors surface in a dismissible activity panel that also carries scan and job progress. Nothing about failure is modal.

---

## 13. Testing

Because the engine has no Tauri dependency, nearly everything is testable headless.

**Unit**
- `fsmodel`: long paths, UNC, conflict policies, undo inversion, cross-volume copy-verify-delete.
- `tags`: key-notation conversion across all three notations; BPM parsing edge cases; malformed tag tolerance.
- `audio`: decode and probe per format (AIFF, WAV, FLAC, MP3, M4A/ALAC, OGG); peak determinism.
- `index`: hash stability under retagging; quick-key collision behaviour; offline/online transitions; drive-letter reassignment.
- `actions`: template substitution, function application, collision counters.
- `render`: sprite output determinism and dimension correctness.

**Integration** — a fixture tree of small generated audio files: full scan, verify index state, mutate the tree, rescan, verify convergence. Includes a simulated volume-disappears-mid-scan case.

**Property** — hash identity holds across arbitrary tag rewrites; copy→move→rename→undo sequences always return the tree to its initial state.

**Performance** — benchmarks with hard budgets (§14), run against a generated 100k-file tree in CI.

**Manual** — the real library on real hardware, including the network share and the removable SSD. Some failure modes only appear on actual SMB.

---

## 14. Performance targets

| Metric | Budget |
|---|---|
| Navigate into a 5k-file folder → rows visible | < 120 ms |
| Scroll a 100k-row virtualized list | Matches display refresh (120 Hz target), no dropped frames |
| Sort a 40k-file folder by any column | < 200 ms |
| `Ctrl+F` first results | < 80 ms |
| Cold start to interactive | < 900 ms |
| Idle memory, 100k indexed files | < 350 MB |
| Background scan | Never blocks input |

---

## 15. Sequencing within this sub-project

1. Crate skeleton, `fsmodel` enumeration, Tauri shell, list + tree + breadcrumb.
2. SQLite index, scanner stages 1–2, tag reading, columns with persistence.
3. Content identity, hashing, offline volumes, watchers, reconciliation.
4. FTS5 search, `Ctrl+F` popover, `Ctrl+M` move popover.
5. File operations with conflict handling and undo.
6. Decode, peaks, `render` sprites, inline waveform column.
7. Playback, transport bar, both waveforms seeking.
8. Action system, `actions.toml`, toolbar, rename templates.
9. Tabs, split pane, session restore.
10. Theme, polish, performance pass against §14.

Each step should leave the app runnable.

---

## 16. Decisions log

| # | Decision | Reasoning |
|---|---|---|
| 1 | Three sub-projects, browser core first | A single spec would be too large to implement against; the browser is usable alone and the index schema is what everything else hangs off. |
| 2 | BPM/key read from tags in this sub-project, visually distinguished | Makes the first release genuinely useful; costs almost nothing since files are already opened for duration. Distinguishing sources preserves information when analysis and Rekordbox disagree. |
| 3 | Hybrid single/split layout | Two real modes — browsing/tagging wants width, copy sessions want two locations. Cheap if designed in from the start, expensive to retrofit. |
| 4 | Content-hash identity over audio frames only | The user maintains multiple copies of the same track, some retagged after copying. Path- or file-hash identity would break on every retag. |
| 5 | Primary waveform as bottom transport bar | A right-panel waveform is the first casualty of splitting, and would later compete with the tag editor for the same column. |
| 6 | Playback + waveform moved into sub-project 1 | A seekable waveform needs a player; peaks need a decoder. Neither is hard, and it leaves sub-project 2 as purely the difficult DSP. |
| 7 | Tauri v2 over native GUI or Electron | Native GUI means rebuilding every Explorer affordance by hand; Electron's memory and filesystem throughput are wrong for a file browser. |
| 8 | Engine crates never link Tauri | Headless testability, free CLI, replaceable shell. |
| 9 | Rename in sub-project 1, rule engine in sub-project 3 | Renaming is a browser operation and must write to disk. Only the conditional/scripting engine genuinely needs to be shared with tag writing. |
| 10 | Name: Noceur | Duende collides with an established .NET vendor; Noceur is clean on crates.io and GitHub. |
