<div align="center">

<img src="crates/noceur-app/icons/128x128.png" width="88" alt="">

# Noceur

**An audio-aware file browser for DJs.**

The filesystem *is* the library — no import step, no database to keep in sync with reality.
Folders are folders; they just happen to show BPM, key, duration and artist per row,
and a waveform you can scrub.

[![CI](https://github.com/mtsbh/noceur/actions/workflows/ci.yml/badge.svg)](https://github.com/mtsbh/noceur/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/mtsbh/noceur?include_prereleases&sort=semver)](https://github.com/mtsbh/noceur/releases/latest)
[![Licence](https://img.shields.io/badge/licence-GPL--3.0--only-blue)](LICENSE)
![Platform](https://img.shields.io/badge/platform-Windows-lightgrey)

[Getting it](#getting-it) · [What it does](#what-it-does) · [Design](#design) · [Building](#building)

</div>

---

<!-- HERO -->

## Why

A DJ library is a folder tree, not a database. The tools that browse one well do not tag, and
the tools that tag well do not browse — so the work happens in two programs with the filesystem
passed between them, and neither knows what the other did.

This is the union: a file browser that is natively audio-aware, with a real player and a tagger
built in.

The concrete situation being designed against: **20k–100k tracks** across local HDDs, a network
share, and an SSD carried to parties. Tracks get *copied* between them and sometimes retagged by
other software afterwards, so the same audio lives at several paths at once in several tag
states. That one fact drives the whole data model.

## Getting it

Windows 10 or 11, 64-bit. Nothing else to install — the WebView2 runtime ships with Windows.

[**Build it**](#building) — two commands, and the result is a single portable executable.
Tagged commits also publish an installer and that executable to
[Releases](https://github.com/mtsbh/noceur/releases).

Either way it stays portable: settings, window position and your own buttons live in
`noceur-config.json` beside the executable, and the index (`noceur-index.db`) is a cache of what
is on disk that is safe to delete at any time. Nothing is written to the registry or to
`AppData`.

> **Status: usable, and not finished.** Everything below is built and working. It is one person's
> tool, shared because it may be somebody else's too.

## What it does

### Browsing

A virtualized listing that does not care how large a folder is — about a screenful in the DOM,
200 rows per fetch, sorted and filtered in SQLite so the order is correct across a 40k-file
folder rather than across the rows that happen to be loaded.

Folder tree, pinned folders, breadcrumbs, back and forward. Eighteen columns, reorderable and
resizable. Full-text search across the indexed library. Optionally hides everything that is not
audio, so a release folder is the tracks in it rather than the cover scans and `.nfo`s that came
with them.

<!-- IMAGE:BROWSING -->

**Tabs and split panes.** Two panes, side by side or stacked, each with its own tabs, sort and
history — so the folder you are filing tracks *into* is on screen beside the one you are filing
them *out of*.

| Shortcut | |
| --- | --- |
| `ctrl+t` / `ctrl+w` | open / close a tab |
| `ctrl+\` | split and unsplit |
| `ctrl+1`–`9`, `ctrl+tab` | move between tabs |
| `tab` | move between panes |
| `alt+←` / `alt+→` | each tab's own history |

**A folder of folders looks like folders.** Every column but the name is empty for a directory,
so a folder holding no audio is drawn as icons instead — the shell's own folder icons, so a
folder with a custom one looks like itself. One track in it and the list comes back.

<!-- IMAGE:ICONS -->

**Filing.** Drag files in from Explorer, out to anything, between panes, onto a tab that is open
but not in front of you, or onto a folder in the tree — which springs open if you rest on it.
Copy, move and delete go through the shell, so they land on Windows' own undo stack and
`ctrl+z` in Explorer puts them back.

### Listening

Preview playback with a seekable waveform drawn from three frequency bands, in four styles, and
a Spek-style spectrogram of the playing track. Double-click the cover to edit what is playing;
double-click its name to jump to it in the folder it lives in.

<!-- IMAGE:PLAYER -->

### Tagging

Editing one file or a whole selection, with fields that disagree shown as disagreeing rather
than resolved. Star ratings on the scale DJ software reads. Custom fields written where other
taggers look for them. Metadata and cover art from MusicBrainz, Deezer, iTunes and Discogs,
always chosen by hand — never applied automatically.

<!-- IMAGE:TAGS -->

**Quick tag.** Colour-coded values applied to the playing track by keyboard, held until the
player leaves the file so tagging never interrupts what you are listening to.

**Renaming and cleanup.** Rename from a template; one-click rules for the debris a download
leaves behind — `(Original Mix)`, leading track numbers, underscores. Each file's modified date
is preserved by default, because retagging a folder should not move every track in it to the top
of a sort by date.

### Buttons of your own

One thing done to a selection: fix a tag, rename, open the files in another program, look the
track up, copy or move them into a folder.

You pick what it does and how — *Title Case*, *Remove text in brackets*, *Year only*, *One
separator between artists*, *Search Google*, *Move into a release folder* — and the expression is
written for you. It stays visible and editable underneath, because reading what a recipe produced
is how anybody learns the syntax well enough to stop needing recipes.

The syntax is foobar2000's, so anything already written for foobar, Mp3tag or OneTagger works as
it is. Buttons live in the config beside the executable, never in this repository: they are full
of one person's drive letters and installed programs.

<!-- IMAGE:BUTTONS -->

### Checking

**Is this file what it claims to be?** Spectral analysis finds the real bitrate: a track claiming
320 kbps that stops at 17 kHz was 160 kbps before somebody re-encoded it, and a FLAC with a hard
cutoff was never lossless. It also catches sample-rate upconversion and clipping.

The cutoff is found by looking for the steepest local drop in the spectrum rather than the
highest bin above some level — a level test finds the natural taper of real music and calls it a
brickwall, which is how these tools produce false accusations.

<!-- IMAGE:QUALITY -->

**Is this the same track twice?** A hash of the *decoded samples*, so nothing anybody writes into
the tags can change it. Two files with the same fingerprint hold the same audio however
differently they are labelled, wrapped or dated — which is the case the whole data model exists
for. It is not a fingerprint in the AcoustID sense and will not match a 320 kbps encode to the
FLAC it came from: those genuinely are different audio, and telling them apart is most of the
point.

## Design

**Identity is the audio, not the path.** A `content` row is distinct audio; a `file` row is a
path. Many files point at one content. Copying a track to the party SSD does not create a second
track, and retagging one copy does not silently fork it.

**A value read from tags is marked as one.** BPM and key from a file's tags render dimmed with a
trailing dot (`124·`); absent renders as `—`. Different programs write different answers, and
quietly picking one of them loses the fact that they disagreed.

**Nothing below the app crate links Tauri.** The whole engine compiles and tests headless — index
a folder, read its tags, sort it — in a unit test with no window. The debug CLI is free, and the
web view could be replaced without touching the engine.

**Everything the user reads is formatted in Rust.** Durations, BPM precision, key notation, dates
and file sizes are produced in one unit-tested place. The frontend renders strings it is handed.

**Images and measurements are computed in Rust and cached by content.** Cover art is decoded and
rasterised at the exact size it will be drawn, then served over a `thumb://` scheme the web view
fetches like any other image, so nothing image-shaped crosses the IPC bridge. Waveforms and
spectrograms travel as numbers and are drawn on a canvas, so changing how they look is a repaint
rather than a re-decode.

**Nothing is written without being shown first.** Renames, cleanup rules, tag edits and every
button that changes a file are previewed per file before anything happens.

**Opening a program is not a shell.** The argument line is split in Rust and handed to the
process directly, so a track called `; del *.*` is an argument and never a command.

[`docs/design.md`](docs/design.md) holds the data-model decisions and the reasoning behind them.

## Not planned

Nothing is outstanding from the original plan.

Deliberately not planned: **BPM and key detection**, because Mixed In Key already does it better
than this would; **Beatport and Bandcamp**, which have no usable public API and would be scrapers
that break; and **audio fingerprinting**, which needs a service this does not want to depend on.

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

> [!IMPORTANT]
> **`--features custom-protocol` is not optional.** Without it the binary still points at the
> Vite dev server and shows "can't reach this page" instead of the app — and it does so invisibly
> on any machine that happens to have `npm run dev` running, which is exactly the machine builds
> get made on. To check a binary is genuinely standalone, look for the frontend's hashed asset
> name inside it:
>
> ```bash
> grep -c "$(ls ui/dist/assets/*.js | head -1 | xargs basename)" target/release/noceur-app.exe
> ```
>
> `1` means the frontend is embedded. `0` means it is a dev build wearing a release build's name.
> CI checks this on every release.

### The installer

```bash
npx @tauri-apps/cli@^2 build --config crates/noceur-app/tauri.conf.json
```

Produces `target/release/bundle/nsis/Noceur_x.y.z_x64-setup.exe`. The Tauri CLI sets
`custom-protocol` itself. Tagging a commit `v*` runs the same command in CI and attaches the
result to a draft GitHub Release.

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

All five run in CI on every push. The frontend tests run on `node --test` with no framework,
because Node strips the type annotations itself; they cover the layout reducers — the tab and
split arithmetic, and the guards that stop a bad preference file leaving the window with no pane
in it. Rendering a pane needs a window and a music library, and is not something a unit test
should pretend to have.

Some tests are `#[ignore]`d because they need a real music library or reach the network. Run them
by hand:

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

## Layout

```
crates/
  noceur-fsmodel   enumeration, ordering, long paths, rename templates, title formatting
  noceur-tags      tag reading and writing, key normalisation, star ratings
  noceur-art       cover-art extraction, thumbnail rasterisation, shell folder icons
  noceur-audio     playback, waveform envelopes, FFT, spectrograms, quality, audio identity
  noceur-online    MusicBrainz, Deezer, iTunes, Discogs, Cover Art Archive
  noceur-index     SQLite index, scanner, windowed queries, thumbnail and waveform caches
  noceur-app       the only Tauri-aware crate: commands, state, display formatting
  noceur-cli       debug CLI for driving the engine headless
ui/                Solid + Vite frontend, TanStack Virtual, the Sodium theme
                   App owns the layout, dialogs and player; Pane owns one folder's rows
docs/              the design spec, build notes, troubleshooting
```

## Your own data

Preferences that describe the interface live in the web view's local storage. Anything that names
a place on your machine — the Discogs token, window position, action buttons — lives in
`noceur-config.json` beside the executable, so a portable install stays portable and none of it
is ever committed.

## Licence

[GPL-3.0-only](LICENSE).
