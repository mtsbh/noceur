# Handoff

## Current state
- Branch `claude/kind-lamport-22i4sm`, draft PR mtsbh/noceur#4 against `master`. Not merged.
- Windows-only Tauri 2 app (Rust backend in `crates/`, Solid + Vite frontend in `ui/`).
- Before the last push, CI (`.github/workflows/ci.yml`, windows-latest) was red on two clippy
  lints, now fixed. Workspace clippy is clean on Linux with a local stub (see "Commands").
- No Windows binary built from this branch yet. Build one with the Release workflow (manual
  trigger) or locally on Windows.

## Implemented on this branch
| Feature | Where |
|---|---|
| Meters in the transport (peak LEDs, VU needles, scope, vectorscope + correlation, spectrum), click to cycle | `noceur-audio/src/scope.rs`, `commands::scope_frames`, `ui/src/Meters.tsx` |
| 7 themes (Sodium default, Cobalt, Frost, Velvet, Phosphor, Graphite, Paper light) | `ui/src/themes.ts`, token blocks at top of `ui/src/theme.css` |
| Transport: title/artist on two lines, no `—` placeholders, mute on volume icon | `ui/src/Transport.tsx` |
| Icons for user buttons; buttons with an icon also sit in the toolbar | `actions.rs` (`icon` field), `icons.tsx` (`ACTION_ICONS`), `ActionEditor.tsx`, `Toolbar.tsx` |
| Grouped top bar; icons + grouping in the row context menu | `Toolbar.tsx`, `App.tsx`, `Menu.tsx` |
| Lossless converter WAV/AIFF/FLAC/ALAC via ffmpeg, bit-exact MD5 check, tags/art copied | `noceur-app/src/convert.rs`, `ui/src/Converter.tsx`, Settings → Converter |
| MP4 custom fields written as `----:com.apple.iTunes:NAME` (were silently dropped) | `noceur-tags/src/edit.rs::custom_key` |
| New folder: tree menu, listing background menu, row menu, Ctrl+Shift+N | `noceur-app/src/newfolder.rs`, `ui/src/NewFolder.tsx`, `Tree.tsx`, `Pane.tsx` |
| Star ratings: indexed, sortable Rating column, click to set, Alt+0..5 | `noceur-tags/src/meta.rs`, `noceur-index` (schema V9, query, scan), `format.rs`, `ui/src/Stars.tsx` |
| Spectrogram click/drag to seek with hover time | `ui/src/Spectrogram.tsx` |
| Tags from file name (`%artist% - %title%`, presets, `%dummy%`, `\` for folders), preview then write | `noceur-app/src/fromname.rs`, `ui/src/FromName.tsx`, toolbar Metadata group + row menu |

## Architectural decisions
- **Meters are pre-fader.** A `Tap<S: Source>` sits between the decoder and `rodio::Player` and
  shows the file's level, not the playback volume. The UI polls `scope_frames` every animation
  frame. The reply is raw bytes (`u32` rate + interleaved `i16` stereo) via `tauri::ipc::Response`.
- **Themes are CSS custom-property overrides** on `:root[data-theme=…]`. Colours derive from
  `--fg-rgb`/`--acc-rgb`. Canvases read tokens at draw time and redraw on the `themeTick()` signal.
- **Converter uses an external ffmpeg** (not bundled). It is looked up in this order: config
  `ffmpegPath` (file or folder) → beside exe (`./`, `ffmpeg/`, `ffmpeg/bin/`) → `PATH`.
  - The encoding recipe keeps the source depth and rate. Float input going to FLAC/ALAC becomes
    24-bit. Narrowing adds TPDF dither.
  - Output is written to a `*.noceur-tmp.*` file and renamed into place. A failed verify deletes it.
  - Tags are copied by lofty item by item (not `re_tag`), so custom fields survive.
    WAV/AIFF get ID3v2.
- **Ratings live in the index** (`content_tag.rating`). Migration V9 sets `read_at = 0` for all
  rows, so tags get re-read lazily as folders are opened. The UI updates optimistically and rolls
  back if the write fails. Clicking the current star clears it.
- **Filename → tags** matches left to right: a field ends at the first occurrence of the text
  after it, and the last field takes the rest. Files that don't fit are skipped, never guessed.
  Unknown `%fields%` become custom tags (upper-cased).
- **A re-read of the folder on screen keeps its scroll.** `Pane` tells a refresh (same `dirId`)
  from a navigation. On a refresh it keeps the rows up (no "Reading…" swap, which collapsed the
  scroll box) and replaces the visible pages in place. Refreshes come from drags, tag writes,
  ratings and conversions. On Windows a click with a few pixels of movement starts a native drag,
  whose end refreshes every pane.
- **Per-file IPC for long jobs** (convert): the dialog loops over files, so it can show progress
  and stop between files. There are no background events.

## Files changed (vs `master`)
- Rust:
  - `noceur-audio/src/{lib.rs,scope.rs}`
  - `noceur-app/src/{main.rs,commands.rs,config.rs,actions.rs,convert.rs,newfolder.rs,fromname.rs,format.rs}`
  - `noceur-app/Cargo.toml` (+`lofty`)
  - `noceur-index/src/{schema.rs,scan.rs,query.rs}`
  - `noceur-tags/src/{meta.rs,edit.rs}`
- UI:
  - New: `Meters.tsx`, `themes.ts`, `Converter.tsx`, `NewFolder.tsx`, `Stars.tsx`, `FromName.tsx`
  - Changed: `App.tsx`, `Transport.tsx`, `Waveform.tsx`, `Spectrogram.tsx`, `Toolbar.tsx`,
    `Menu.tsx`, `Tree.tsx`, `Pane.tsx`, `ActionEditor.tsx`, `SettingsPanel.tsx`, `settings.ts`,
    `columns.ts`, `api.ts`, `icons.tsx`, `theme.css`

## Known bugs / limitations
- ID3 `POPM` star ratings are not carried over by the converter (lofty's generic tag omits POPM).
- Clicking stars in an unfocused pane does not move pane focus (stars stop `pointerdown`).
- After rating while sorted by Rating, rows keep their position until the next refetch.
- Vectorscope is only 34 px square (the meter box height is fixed at 34 px).
- On Linux, `noceur-app` does not compile (`fileops` is Windows-only) and 6 `actions` path
  tests fail under the stub. Both are expected; Windows is the real target.
- Not yet exercised in the real Windows app: the new-folder prompt, the tree context menu, the
  converter against user files, and meter timing on WASAPI.

## Next steps
1. Confirm CI on PR #4 is green on the latest push. Fix anything Windows-specific it reports.
2. Build and run on Windows (commands below). Smoke-test:
   - Meters, all modes
   - Theme switch
   - Convert FLAC→WAV with verify on
   - New folder from the tree, listing and row menu
   - Rate a selection, then sort by Rating
   - Spectrogram seek
   - Tags from file name on a folder of "Artist - Title" downloads
3. Put `ffmpeg.exe` beside `noceur-app.exe` (e.g. from gyan.dev builds) before testing the converter.
4. Mark PR #4 ready for review and merge. Then tag `v*` to publish a release.
5. Optional follow-ups:
   - Carry ID3 POPM in the converter.
   - A resizable or larger meter panel.
   - "New file…" if wanted (the user was asked and has not answered).

## Commands
```bash
# Frontend
npm --prefix ui ci
npm --prefix ui run build
npx --prefix ui tsc --noEmit -p ui/tsconfig.json
npm --prefix ui test

# Rust (Windows: exactly what CI runs)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Rust on Linux (app crate is Windows-only): test the portable crates
cargo test -p noceur-audio -p noceur-tags -p noceur-index
# converter test needs ffmpeg on PATH (skips if absent): sudo apt install ffmpeg

# Build the Windows app locally (on Windows) -> target/release/noceur-app.exe + NSIS installer
npm --prefix ui run build
npx @tauri-apps/cli@^2 build --config crates/noceur-app/tauri.conf.json

# Or build on GitHub: Actions → Release → "Run workflow" on this branch.
# A branch run attaches noceur-app.exe + *-setup.exe to the run as the
# "noceur-windows" artifact; pushing a v* tag also publishes a draft Release.
```
Details: `docs/building.md`, `docs/troubleshooting.md`.
