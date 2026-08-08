# Engine foundation — performance baseline

**Date:** 2026-08-05
**Build:** `cargo build --release`, rustc 1.97.1, commit at end of milestone 1
**Machine:** Windows 11, library on internal HDD (`E:`), network share on `Z:`

Measured with the debug CLI, which prints `scan` (stage 1, enumerate → rows) and
`probe` (stage 2, quick key + tag read) timings to stderr. Later milestones are
measured against these numbers.

---

## Local HDD — `E:\music\artists\ricardo`, 238 entries

| Run | scan | probe | Notes |
|---|---|---|---|
| Cold disk, in-memory index | 3 ms | 12,679 ms | ~53 ms/file. Dominated by HDD seek + read. |
| Warm page cache, persistent index | 3 ms | 271 ms | ~1.1 ms/file. Same work, no physical I/O. |
| Re-run against the same persistent index | 63 ms | **0 ms** | Incremental probe skips everything already linked. |

The third row is the payoff from Amendment 7. Before that fix `probe_dir` re-read
and re-hashed every file on every call, so this row would have been another 271 ms
(or another 12.7 s cold). Browsing back into a folder now costs enumeration only.

Note the scan cost rises from 3 ms to 63 ms once the index is populated — that is
the stale-row reconciliation reading existing rows back. Still far below the 120 ms
navigate budget, and it is O(n) after Amendment 7.

## Network share — `Z:`

| Folder | Entries | scan | Notes |
|---|---|---|---|
| `Z:\LibreChat`, cold mount | 31 | **19,991 ms** | First touch after the share went idle. |
| `Z:\LibreChat`, immediately after | 31 | 1 ms | Warm. |
| `Z:\bot-env` | 6 | 84 ms | Warm-ish. |
| `D:\projects` (local, for comparison) | 44 | 1 ms | |

**This is the most important number in this document.** A cold SMB mount can stall
for twenty seconds on a folder containing thirty-one items. Warm, it is
indistinguishable from local. Two consequences for the shell milestone:

1. Stage 1 enumeration **cannot** run on the UI thread, even though it is
   sub-millisecond locally. A twenty-second freeze on navigating into a network
   folder would read as a hung application.
2. The activity panel needs a visible pending state for a folder that has been
   navigated to but not yet enumerated — not just for background analysis.

The spec (§6) already routes network trees around watchers; this says the *first*
read needs the same care as the watching.

## Long paths

A 362-character path (12 nested 29-character segments under `D:\`) listed cleanly
with tags read. `noceur-fsmodel::path::extended()` works end to end against the
real filesystem, not just in unit tests.

## Real-library observations

Sampled `E:\music\{[current]/tg, slsk, downloads, artists/ricardo}`.

- **Unicode filenames are fine.** `Rêves Cirés`, `Budō`, `Isolée`, `Masquée` all
  enumerate, sort and tag-read correctly.
- **Mixed containers work.** FLAC, WAV and AIFF in the same folder, all returning
  correct durations and artists.
- **BPM and key are mostly absent in the wild.** Across several hundred real
  tracks, only a handful carried BPM, and almost none carried key — the one that
  did was an AIFF, presumably written by Rekordbox. Soulseek and Bandcamp files
  carry neither.

That last point is direct evidence for the roadmap: the BPM and Key columns are
mostly empty on this library until the analysis engine lands. It also validates
the three-state column design (analysed / from-tags / absent) — "absent" is not
an edge case here, it is the common case.

## Known issues carried forward

**`\\?\` leaks into user-facing text.** `std::fs::canonicalize` returns the Windows
verbatim form, so an error from the CLI reads
`cannot read directory \\?\D:\projects\Brows\Cargo.toml`. Harmless in a debug tool,
unacceptable in a breadcrumb. The shell milestone needs a `path::simplify()` — the
inverse of `extended()` — applied at every display boundary.

## Budgets from the spec, checked

| Spec §14 budget | Status |
|---|---|
| Navigate into a 5k-file folder → rows visible < 120 ms | Met locally (3-63 ms at 238 entries); **not yet measured at 5k**, and cold network breaks it badly |
| Background scan never blocks input | Not applicable yet — no UI |

Remaining budgets (scroll frame rate, sort latency, cold start, idle memory) need
the shell and are deferred to milestone 2.
