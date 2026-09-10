# Noceur

An audio-aware file browser for DJs. The filesystem is the library — no import step, no
database to keep in sync with reality. Folders are folders; they just happen to show BPM, key,
duration and artist per row, and a waveform you can scrub.

> **Status: usable, and not finished.** It browses in tabs and split panes, plays, draws
> waveforms and spectrograms, searches, edits tags, fetches metadata and artwork, renames, finds
> the same track filed twice, and tells you whether a file is really what it claims to be. See
> [What it does](#what-it-does).

---

## Why

A DJ library is a folder tree, not a database. The tools that browse one well do not tag, and
the tools that tag well do not browse — so the work happens in two programs with the filesystem
passed between them, and neither knows what the other did.

This is the union: a file browser that is natively audio-aware, with a real player and a tagger
built in.

The concrete situation being designed against: 20k–100k tracks across local HDDs, a network
share, and an SSD carried to parties. Tracks get **copied** between them and sometimes retagged
by other software afterwards, so the same audio lives at several paths at once in several tag
states. That one fact drives the whole data model.

## Design

**Identity is the audio, not the path.** A `content` row is distinct audio; a `file` row is a
path. Many files point at one content. Copying a track to the party SSD does not create a second
track, and retagging one copy does not silently fork it.

**A value read from tags is marked as one.** BPM and key from a file's tags render dimmed with a
trailing dot (`124·`); absent renders as `—`. Different programs write different answers, and
quietly picking one of them loses the fact that they disagreed.

**Nothing below the app crate links Tauri.** The whole engine compiles and tests headless —
index a folder, read its tags, sort it — in a unit test with no window. The debug CLI is free,
and the web view could be replaced without touching the engine.

**The frontend asks for windows of rows, never rows in a loop.** Sorting and filtering happen in
SQLite, so the order is globally correct across a 40k-file folder rather than correct only across
the rows that happen to be loaded. The list is virtualized on both sides: about a screenful in
the DOM, 200 rows per fetch.

**Everything the user reads is formatted in Rust.** Durations, BPM precision, key notation and
file sizes are produced in one unit-tested place. The frontend renders strings it is handed.

**Images and measurements are computed in Rust and cached by content.** Cover art is decoded and
rasterised at the exact size it will be drawn, then served over a `thumb://` scheme the web view
fetches like any other image, so nothing image-shaped crosses the IPC bridge. Waveforms and
spectrograms travel as numbers and are drawn on a canvas, so changing how they look is a repaint
rather than a re-decode.

**Nothing is written without being shown first.** Renames, cleanup rules and tag edits are all
previewed per file before anything happens, and file operations go through the shell so they
land on Windows' own undo stack.

## What it does

**Looking at it.** The interface font is a setting: eight faces that ship with Windows, or the
exact name of any other one installed, with the ones this machine does not have marked rather
than silently drawn in the fallback.

**Browsing.** A virtualized listing that does not care how large a folder is. Folder tree,
pinned folders, breadcrumbs, back and forward. Eighteen columns, reorderable and resizable.
Optionally hides everything that is not audio, folders excepted, so a release folder is the
tracks in it rather than the cover scans and `.nfo`s that came with them.
Full-text search across the indexed library. Tabs, and a split into two panes — side by side or
stacked — each with its own tabs, sort and history, so the folder you are filing tracks *into* is
on screen beside the one you are filing them *out of*. Drag files in from Explorer, out to
anything, and between panes, onto any open tab, or onto a folder in the tree — which springs
open if you rest on it; copy, move and delete through the shell.

**Filing.** Drag a selection onto the other pane, or onto a tab that is open but not in front of
you, and it lands in that folder — copied or moved by the same rule Explorer uses, through the
same shell operation, onto the same undo stack. Tabs themselves drag: to reorder, or across the
divider to hand a folder to the other pane. `ctrl+t` opens a tab, `ctrl+w` closes one, `ctrl+\`
splits and unsplits, `ctrl+1`–`9` and `ctrl+tab` move between tabs, `tab` between panes, and
`alt+←`/`alt+→` walk each tab's own history.

**Listening.** Preview playback with a seekable waveform drawn from three frequency bands, in
four styles. A Spek-style spectrogram of the playing track.

**Tagging.** Editing one file or a whole selection, with fields that disagree shown as
disagreeing rather than resolved. Star ratings on the scale DJ software reads. Custom fields
written where other taggers look for them. Quick tag — colour-coded values applied to the
playing track by keyboard, held until the player leaves the file so tagging never interrupts
what you are listening to. Metadata and cover art from MusicBrainz, Deezer, iTunes and Discogs,
always chosen by hand. Renaming from a template, and cleanup rules for the debris a download
leaves behind. Each file's modified date is preserved by default, because retagging a folder
should not move every track in it to the top of a sort by date.

**Buttons of your own.** One thing done to a selection, defined as a kind, a target and a title-
formatting expression: fix a tag, rename, open the files in another program, look the track up,
copy or move it into a folder. Anything that changes a file is previewed first. They live in the
config beside the executable, because they are full of one person's drive letters.

**Checking.** Spectral analysis that finds a file's real bitrate: a track claiming 320 kbps that
stops at 17 kHz was 160 kbps before somebody re-encoded it, and a FLAC with a hard cutoff was
never lossless. Also catches sample-rate upconversion and clipping. Separately, a hash of the
decoded samples finds the same recording filed twice however differently the two copies are
tagged — which is the case the whole data model exists for.

## Not planned

Nothing is outstanding from the original plan: tabs and split panes, user-defined buttons and
audio identity were the last three, and they are in.

Deliberately not planned: BPM and key detection, because Mixed In Key already does it better
than this would; Beatport and Bandcamp, which have no usable public API and would be scrapers
that break; and audio fingerprinting, which needs a service this does not want to depend on.

## Layout

```
crates/
  noceur-fsmodel   enumeration, ordering, long paths, rename templates, title formatting
  noceur-tags      tag reading and writing, key normalisation, star ratings
  noceur-art       cover-art extraction and thumbnail rasterisation
  noceur-audio     playback, waveform envelopes, FFT, spectrograms, quality analysis
  noceur-online    MusicBrainz, Deezer, iTunes, Discogs, Cover Art Archive
  noceur-index     SQLite index, scanner, windowed queries, thumbnail and waveform caches
  noceur-app       the only Tauri-aware crate: commands, state, display formatting
  noceur-cli       debug CLI for driving the engine headless
ui/                Solid + Vite frontend, TanStack Virtual, the Sodium theme
                   App owns the layout, dialogs and player; Pane owns one folder's rows
docs/              the design spec, build notes, troubleshooting
```

[`docs/design.md`](docs/design.md) holds the data-model decisions and the reasoning behind them.

## Building

Windows is the tested target. Linux and macOS stay buildable but are not exercised.

**Prerequisites:** Rust (the pinned toolchain installs itself from `rust-toolchain.toml`),
Node 24+, MSVC build tools, and the WebView2 runtime.

```bash
npm --prefix ui install
```

The frontend is embedded in the binary, so it has to be built first:

```bash
npm --prefix ui run build
cargo build --release -p noceur-app --features custom-protocol
```

The binary lands at `target/release/noceur-app.exe`.

> **`--features custom-protocol` is not optional.** Without it the binary still points at the
> Vite dev server and shows "can't reach this page" instead of the app — and it does so
> invisibly on any machine that happens to have `npm run dev` running, which is exactly the
> machine builds get made on. `cargo tauri build` sets the feature for you. To check a binary is
> genuinely standalone, look for the frontend's hashed asset name inside it:
>
> ```bash
> grep -c "$(ls ui/dist/assets/*.js | head -1 | xargs basename)" target/release/noceur-app.exe
> ```
>
> `1` means the frontend is embedded. `0` means it is a dev build wearing a release build's name.

`beforeBuildCommand` is deliberately absent from `tauri.conf.json` — its relative paths resolved
against an unpredictable working directory. If you want that coupling back, work out Tauri's
actual cwd first rather than guessing.

### Checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
npx --prefix ui tsc --noEmit -p ui/tsconfig.json
npm --prefix ui test
```

The frontend tests run on `node --test` with no framework, because Node strips the type
annotations itself. They cover the layout reducers — the tab and split arithmetic, and the
guards that stop a bad preference file leaving the window with no pane in it. Rendering a pane
needs a window and a music library, and is not something a unit test should pretend to have.

Some tests are `#[ignore]`d because they need a real music library or reach the network. Run
them by hand:

```bash
cargo test --release -p noceur-audio --test real_files -- --ignored --nocapture
```

### The debug CLI

Drives the engine with no window, which is the fastest way to see what the index thinks:

```bash
cargo run -p noceur-cli -- ls "E:/music" --sort bpm --audio-only
```

If a build or install fails in an unhelpful way, check
[`docs/troubleshooting.md`](docs/troubleshooting.md).

## Settings and your own data

Preferences that describe the interface live in the web view's local storage. Anything that
names a place on your machine — the Discogs token, window position, action buttons — lives in
`noceur-config.json` beside the executable, so a portable install stays portable and none of it
is ever committed. The index itself (`noceur-index.db`) is a cache of what is on disk and is
safe to delete at any time.

## Licence

GPL-3.0-only.
