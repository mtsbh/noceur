# Noceur Cover Art Implementation Plan

**Milestone 4.** Follows `2026-08-20-noceur-virtualized-list.md`.

**Goal:** A cover-art thumbnail before every filename, at a size the user picks — small,
medium or large — without giving back the scrolling performance milestone 3 bought.

**Architecture:** Art is rasterised in Rust and fetched by the web view as an ordinary image
URL. This is the same rule §4 of the design spec sets for waveforms — "a 100k-row list with a
waveform per row is impossible if the UI receives peak arrays and draws them" — applied to
covers, which have exactly the same shape of problem.

---

## Scope

### In

- Extracting embedded front-cover bytes (`noceur-tags`).
- Decoding, square-cropping and re-encoding them at three fixed sizes (`noceur-art`).
- A persistent thumbnail cache in the index, keyed by content (`noceur-index`, schema v2).
- A `thumb://` URI scheme serving PNGs to the web view (`noceur-app`).
- An art column, an S/M/L control, and row heights that follow the chosen size (`ui`).

### Out

Artwork from a folder's `cover.jpg` when a file embeds none; a grid/wall view; artwork
editing (that is sub-project 3); hi-DPI variants.

---

## Design decisions

**Art travels as a URL, not as a command result.** A Tauri command would have to base64 every
image into JSON and decode it again in JavaScript, on the thread that is trying to keep a
scrolling list at refresh rate. As an `<img src>` the web view fetches it, decodes it off the
main thread, and keeps its own memory cache of what it has already drawn. The alternative is
reimplementing all three badly.

**Registered asynchronously.** `register_asynchronous_uri_scheme_protocol` lets a slow decode
delay its own image instead of the window — the same reasoning that put enumeration on the
blocking pool in milestone 2.

**The cache is keyed by content, not by file.** §5.1 of the spec: identity is the audio. A
track copied to the party SSD and into a Rekordbox folder is one `content` row, so its cover is
decoded once and both rows draw the same bytes.

**Misses are cached too.** A file with no artwork, or artwork that will not decode, records a
miss. Without that the *expensive* case — failure — would be the uncached one, re-decoded on
every scroll past the row. `has_artwork` from the probe short-circuits earlier still: a folder
of untagged WAVs does no image work at all, and the frontend does not even issue the request.

**Three sizes, not a slider.** Every distinct pixel size is another cache entry and another
decode. Three is enough to choose between a dense list and a browsable wall.

**Row height moved from CSS to JavaScript.** Milestone 3 made `--row-h` the source of truth and
read it back, because it was a constant the virtualizer also needed. The user now changes it at
runtime, so the signal has to be authoritative and the stylesheet is fed from it. The
virtualizer is explicitly re-measured on change; its cached row offsets are otherwise stale and
the rows overlap.

**`ThumbSize::px()` and the frontend's `ART` table must agree.** The backend rasterises to an
exact edge length precisely so that nothing scales the image afterwards; a mismatch would throw
that away silently.

---

## Definition of Done

Automated, all confirmed on 2026-08-20:

- [x] `cargo test --workspace` — 146 tests, 0 failures
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] `cargo fmt --all -- --check` clean
- [x] `tsc --noEmit` clean
- [x] An index written at schema v1 upgrades in place, keeping its rows

Confirmed in the running application:

- [x] Real covers render at all three sizes, square, from a 113-file FLAC folder
- [x] The S/M/L control switches size and row height together
- [x] The choice survives a restart
- [x] Folders and non-audio rows show a placeholder rather than a broken image
- [x] Column headers stay put at the large size

Not done:

- [ ] Measuring what a screen of large covers costs on a cold cache over SMB
