# Noceur

An audio-aware file browser for DJs. The filesystem is the library — no import step, no
database to keep in sync with reality. Folders are folders; they just happen to show BPM, key,
duration and artist per row, and eventually a waveform you can scrub.

> **Status: early.** The engine and the window exist and browse a real library. Playback,
> waveform, search, tag writing and analysis do not exist yet. See [Roadmap](#roadmap).

---

## Why

Two tools each solve half of this problem. [Flow](https://flowdj.app/) is a native browser that
renders waveform, BPM and key inline and treats the filesystem as the truth — but it is closed
source and barely tags. [OneTagger](https://github.com/Marekkon5/onetagger) fetches metadata
from nine sources and has an excellent Quick Tag — but it is a batch-job UI with no browsing.

Neither wants to become the other. The union — a file browser that is natively audio-aware,
with a real player and a tagger built in — does not exist, so this is it.

The concrete situation being designed against: 20k–100k tracks across local HDDs, a network
share, and an SSD carried to parties. Tracks get **copied** between them and sometimes retagged
by Rekordbox afterwards, so the same audio lives at several paths at once in several tag states.
That one fact drives the whole data model.

## Design

**Identity is the audio, not the path.** A `content` row is distinct audio; a `file` row is a
path. Many files point at one content. Copying a track to the party SSD does not create a second
track, and retagging one copy does not silently fork it.

**BPM and key have three states, and never merge.** Analysed values render at full precision;
values read from file tags render dimmed with a trailing dot (`124·`); absent renders as `—`.
Analysed and Rekordbox-written values *will* disagree, and quietly picking one loses information.

**Nothing below the app crate links Tauri.** The whole engine compiles and tests headless —
index a folder, read its tags, sort it — in a unit test with no window. The debug CLI is free,
and the web view could be replaced without touching the engine.

**The frontend asks for windows of rows, never rows in a loop.** Sorting and filtering happen in
SQLite, so the order is globally correct across a 40k-file folder rather than correct only across
the rows that happen to be loaded. The list is virtualized on both sides: about a screenful in
the DOM, 200 rows per fetch.

**Everything the user reads is formatted in Rust.** Durations, BPM precision, key notation and
file sizes are produced in one unit-tested place. The frontend renders strings it is handed.

**Images are rasterised in Rust and fetched by URL.** Cover art is decoded, cropped square and
re-encoded at the exact size it will be drawn, then served over a `thumb://` scheme the web view
fetches like any other image — so it decodes off the main thread and caches what it has already
drawn. Nothing image-shaped goes through the IPC bridge. Waveforms will arrive the same way.

## Layout

```
crates/
  noceur-fsmodel   enumeration, classification, ordering, Windows long-path handling
  noceur-tags      tag reading, key normalisation (Camelot / Open Key / classical)
  noceur-art       cover-art extraction and thumbnail rasterisation
  noceur-index     SQLite index, directory scanner, windowed queries, thumbnail cache
  noceur-app       the only Tauri-aware crate: commands, state, display formatting
  noceur-cli       debug CLI for driving the engine headless
ui/                Solid + Vite frontend, TanStack Virtual, the Sodium theme
docs/superpowers/  design spec, per-milestone plans, verification notes
```

`docs/superpowers/specs/` holds the design decisions and the reasoning behind them. Each
milestone gets a plan in `plans/` and a verification note in `notes/` recording what was actually
observed — including what could not be checked.

## Building

Windows is the tested target. Linux and macOS stay buildable but are not exercised.

**Prerequisites:** Rust (the pinned toolchain installs itself from `rust-toolchain.toml`),
Node 24+, pnpm, MSVC build tools, and the WebView2 runtime.

```bash
cargo install tauri-cli --version "^2" --locked
pnpm --dir ui install
```

The frontend must be built before the app, as a separate step:

```bash
pnpm --dir ui build
cd crates/noceur-app && cargo tauri build --no-bundle
```

The binary lands at `target/release/noceur-app.exe`.

> **`cargo build --release` alone is not enough.** It produces a binary still pointing at the
> Vite dev server, which shows a "can't reach this page" error instead of the app. Use
> `cargo tauri build` or `cargo tauri dev`.

`beforeBuildCommand` is deliberately absent from `tauri.conf.json` — its relative paths resolved
against an unpredictable working directory. If you want that coupling back, work out Tauri's
actual cwd first rather than guessing.

### Checks

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
pnpm --dir ui exec tsc --noEmit
```

### The debug CLI

Drives the engine with no window, which is the fastest way to see what the index thinks:

```bash
cargo run -p noceur-cli -- ls "E:/music" --sort bpm --audio-only
```

If a build or install fails in an unhelpful way, check
[`docs/superpowers/notes/2026-08-20-toolchain-restore.md`](docs/superpowers/notes/2026-08-20-toolchain-restore.md)
— it covers a silent `pnpm install` hang and a rustup toolchain that looks absent but is not.

## Roadmap

This is sub-project 1 of three. Sub-project 2 is the analysis engine (BPM, key, spectrogram);
sub-project 3 is the tagger (writing, Quick Tag, multi-source metadata fetch).

**Done**

- Engine foundation — enumeration, tag reading, key normalisation, SQLite index, incremental
  scan and probe
- Shell foundation — a Tauri window browsing a real library, off-thread so a cold SMB mount
  cannot freeze it, breadcrumb and back/forward history
- Virtualized list — windowed IPC and virtualized rendering, so folder size stops mattering
- Cover art — embedded artwork per row at three sizes, rasterised in Rust and cached by content

**Next**

Resizable, reorderable and persisted columns · sortable columns · the folder tree · pinned
folders · search · split panes and tabs · file operations with undo · waveform rendering ·
playback · the action system · real volume identity

## Licence

GPL-3.0-only.
