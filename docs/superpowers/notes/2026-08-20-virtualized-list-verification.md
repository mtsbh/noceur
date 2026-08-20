# Virtualized list — verification

**Date:** 2026-08-20
**Build:** `pnpm --dir ui build`, then `cargo tauri build --no-bundle` (tauri-cli 2.11.4)
**Binary:** `target/release/noceur-app.exe`

## Confirmed

| Check | Evidence |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo test --workspace` | 123 tests, 0 failures |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `tsc --noEmit` | clean |
| `vite build` | 47.4 kB bundle, no warnings |
| Release build links | `Built application at: target\release\noceur-app.exe` |
| App starts and creates its window | process alive, `MainWindowTitle = "Noceur"`, no `noceur-startup-error.log`, `noceur-index.db` created next to the exe |

**Windows tile the listing exactly.** `query::tests::consecutive_windows_tile_the_listing_exactly`
walks a whole listing at window sizes 1, 2, 3, 5 and 8 and asserts the concatenation equals the
unwindowed listing. This is the property the frontend's page cache depends on: a row repeated at
a seam or skipped between windows would render as a duplicate or a permanent gap, with no error
anywhere. It is the one part of this milestone that can be proven headlessly, and it is proven.

## Not verified

**The session was locked**, so the window could not be photographed and nothing could be
clicked — a screen capture returned the Windows lock screen. Everything below needs a person
at the machine:

- Scrolling a folder past the first window, and whether windows arrive before the viewport
  reaches them
- That the DOM holds roughly a screenful of rows rather than the whole folder
- That a fast scroll leaves no permanently blank rows once it settles
- That navigating away mid-fetch does not file stale rows into the new listing
- Back, forward, up and breadcrumb navigation — still unverified from milestone 2 as well

**Good folder to test with:** `E:\music\downloads\T01`, 667 entries — four windows at
`PAGE = 200`, so it exercises the fetch-on-scroll path rather than just the first window.
`E:\music\downloads\derpdig` (522) and `E:\music\downloads\24-05` (429) also work. The largest
folder directly under `E:\music` is only 178 entries, which fits in the first window and would
prove nothing about paging.

To check the DOM row count, open the webview devtools on a debug build and count
`.canvas .row` — it should stay near `viewport height / 26 + 24`, not equal the folder size.

## Notes

**A large folder has never been through this engine.** The milestone-1 baseline measured a
238-file folder; 667 is the largest thing tested against the index at all, and only by
`GetChildItem`, not by Noceur. Scan and probe timings for it are unknown — the status bar
reports both, so the first run on `T01` is also the first real datapoint.

**`--no-bundle` still needs `icons/icon.ico`.** Unchanged from milestone 2; the placeholder is
still in place and still wants replacing before anything ships.
