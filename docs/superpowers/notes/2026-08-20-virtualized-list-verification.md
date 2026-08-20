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

## Confirmed later the same day, in the running app

The session was locked on the first attempt. Once unlocked, the window was driven directly.
Milestone 2's complaint that "something repeatedly relocated the window between scripted steps"
was solved by pinning the window to a fixed rect and re-asserting that pin before every click,
then capturing only the client area so every screenshot shares one coordinate frame.
`docs/superpowers/notes/uiauto.ps1` is not committed; the technique is what matters.

| Check | Evidence |
|---|---|
| Opens a 667-entry folder | `E:\music\downloads\T01` — status bar read `667 entries · scan 8ms · probe 35466ms` |
| **Window survives a 35-second probe** | the loading state painted and screenshots kept succeeding throughout; nothing froze |
| Slow probe is flagged | 35466ms rendered in the accent colour, as the >2000ms rule intends |
| **Rows past the first window load** | after a hard fling to the end of the list, rows around index 640 were fully populated — `open_dir` only returns 200, so these came from `list_rows` |
| A fast scroll leaves no blank rows | the capture taken immediately after the fling, with no settling delay, showed no placeholders |
| Tag-derived BPM keeps its marker | `124·`, `123·`, `109·` throughout, dimmed |
| Keys render in Camelot | `2A`, `8A`, `7A`, `10A`, `4B` |
| Grouping holds | in `downloads`, `desktop.ini` sorted last and dimmed as "other", after every directory |
| Back / forward / up | breadcrumb went `T01` → `downloads` → `T01` → `downloads`; the forward button enabled and disabled at the right moments |

Back, forward and up had been unverified since milestone 2. They work.

## Still not directly measured

**The DOM row count.** Release builds have no devtools, so "roughly a screenful of rows" was
not counted. The evidence is strong but indirect: rows at index ~640 render correctly although
the listing only ever shipped 200 rows up front, the scrollbar reflects all 667, and scrolling
stays smooth. Counting `.canvas .row` on a debug build would settle it properly.

**Navigating away mid-fetch.** The stale-reply guard was never exercised deliberately; it would
need a slow folder and a navigation timed inside the window request.

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
