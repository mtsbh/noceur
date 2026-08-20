# Noceur Virtualized List Implementation Plan

**Milestone 3.** Follows `2026-08-08-noceur-shell-foundation.md`, which ended with a plain
scrolling list and the note that it "will visibly struggle above a few thousand rows and that
is expected — virtualization is next."

**Goal:** Browse a 40k-file folder as smoothly as a 12-file one. Only about a screenful of
rows exists in the DOM at any moment, and only about a screenful crosses the IPC bridge at a
time.

**Architecture:** The design spec settles both halves of this. §4 — "The frontend requests
windows of rows, never per-file calls in a loop. Sorting and filtering happen in SQLite, so
sort order is globally correct across a 40k-file folder rather than correct only across loaded
rows." §9.2 — "Virtualized; only ~100 rows realised at any time." So this milestone is not a
rendering trick layered over a fully-loaded array; the array is never fully loaded.

---

## Scope

### In

- Split `list_dir` into `open_dir` (scan, probe, count) and `list_rows` (one window).
- Page cache and windowed fetching in the frontend, keyed by directory.
- Virtualized rendering over the *total* row count, with placeholder rows for windows that
  have not arrived.
- Row height moved into a single source of truth shared by CSS and the virtualizer.

### Out — later milestones

Sorting by column, resizable/reorderable/persisted columns, the folder tree, pinned folders,
split panes, tabs, search, file operations, waveform, playback, the action toolbar, tag
writing. Sort stays fixed at name-ascending, exactly as milestone 2 left it: the windowing
contract does not need a sort parameter until there is a UI that can change it, and adding
unused API surface now would be speculative.

---

## Design decisions

**`open_dir` carries the first window.** A listing that returned only a count would paint a
full screen of placeholders on every navigation, then replace them a round trip later — a
visible flash on a folder of twelve files, which is the common case. The first 200 rows ride
along with the listing and become page 0 of the cache. The cost is a `LIMIT 200` read the
frontend was going to issue anyway.

**Windows echo their own offset.** Windows are requested while the user is scrolling and can
complete out of order. `RowWindow.offset` lets a late reply be filed against the range it was
actually cut from rather than the range that happens to be visible when it lands.

**One `options()` constructor in `commands.rs`.** `open_dir` and `list_rows` must cut from
identically ordered and identically filtered lists, or a window will be a slice of a different
list than the one on screen — which shows up as duplicated and missing rows, not as an error.
Building `ListOptions` in one place makes that drift impossible rather than merely unlikely.

**Replies are matched against the current `dirId`.** Navigating away while a window is in
flight must not file those rows into the new directory's cache.

**Page size 200, one constant on each side.** `FIRST_WINDOW` in `commands.rs` and `PAGE` in
`App.tsx` must agree, because the listing's first window is cached verbatim as page 0. Both
carry a comment saying so. `MAX_LIMIT = 1000` bounds what a single request can pull, so a bug
cannot turn windowing back into "fetch the whole folder".

**Row height is duplicated deliberately.** The virtualizer needs the height in JavaScript;
the stylesheet needs it in CSS. `--row-h` stays the source of truth and `ROW_H` is read from
it at runtime rather than retyped, so the two cannot diverge.

---

## Tasks

1. **Wire types** — `DirListing` loses `rows`, gains `dirId`, `total`, `first`; add
   `RowsRequest` and `RowWindow`. (`dto.rs`)
2. **Commands** — `open_dir` and `list_rows`, both off-thread, sharing `options()` and
   `window()`. (`commands.rs`, `main.rs`)
3. **Typed API** — `openDir` / `listRows`. (`ui/src/api.ts`)
4. **Virtualized list** — page cache, in-flight tracking, stale-reply rejection, placeholder
   rows, virtualizer over `total`. (`ui/src/App.tsx`, `ui/src/theme.css`)
5. **Verify** — build and browse a large folder.

## Definition of Done

Automated, all confirmed on 2026-08-20:

- [x] `cargo test --workspace` passes — 122 tests, 0 failures
- [x] `cargo clippy --workspace --all-targets -- -D warnings` is clean
- [x] `cargo fmt --all -- --check` is clean
- [x] `tsc --noEmit` is clean
- [x] `vite build` produces a bundle

Needs the running application, and a person with a mouse:

- [ ] A folder of several thousand files scrolls without stalling
- [ ] The DOM holds roughly a screenful of rows at any scroll position, not the whole folder
- [ ] Scrolling fast and stopping leaves no permanently blank rows
- [ ] Navigating away mid-fetch does not put stale rows in the new listing
- [ ] Back, forward, up and breadcrumb navigation still work

The last one is a re-check: milestone 2 could not verify the buttons either (see
`notes/2026-08-08-shell-verification.md` — the window kept relocating under scripted clicks).
The navigation model is unchanged by this milestone, but the list underneath it is not.
