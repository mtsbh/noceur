# Handoff

## Current state (2026-09-30)
- Work goes straight to `master`, which is pushed and matches `origin/master`. No feature
  branches or PRs.
- Windows-only Tauri 2 app: Rust backend in `crates/`, Solid + Vite frontend in `ui/`.
- `target/release/noceur-app.exe` is built from `f86d55b`, with the UI embedded.
- Workspace fmt, clippy and tests are clean. `npx tsc` is clean too.
- `master` is the only branch on GitHub. The stale `claude/kind-lamport-22i4sm` branch was
  deleted.

## Recent commits (newest first)
| Commit | What |
|---|---|
| step 9 | Tag tools. `noceur-app::tools` (`preview_tool`, enum `Tool`, pure `plan()`), applied by `cleanup::apply_cleanup` (now apply-only; `preview_cleanup`/`cleanup_rules` and the rule picker are gone; `"date"` field = `Edit.date_to_year`). `noceur-fsmodel::cleanup::{clean_file_name, strip_trailing_brackets, split_artist_title}`. `TrackMeta.date` is the raw date. UI: `Tools.tsx` (tabs Name / Tags / Move; Move is `FolderMover.tsx`, an Action move with root + pattern split from one path); broom opens it on Clean file name; toolbar "Tools" (was Functions, id still `functions`) and right-click Tools submenu list every tool, then the user's buttons. titlefmt: `$regexp` (Rust regex, `[[` inside a class escaped for Mp3tag compatibility), `$cutleft`, `$cutright`, `$eql`, `$neql`, balanced `( )` inside arguments as text. New recipes in `recipes.ts` (fill if empty, remove text, brackets at end, cut N). |
| step 8 | Key detection. `noceur-audio::key`: librosa-compatible CQT (decimated to ~11 kHz, `fft::Plan` with a twiddle table) + KeyNet (9 convs, BN folded, im2col, threaded). Weights `models/openkeyscan2.bin` from OpenKeyScan's MusicalKeyCNN fork (MIT; `models/README.md`, `models/export.py`); chosen over MusicalKeyCNN's `keynet.pt` and the 10× `openkeyscan3.pt` on agreement with Mixed In Key (300 MIK-tagged tracks from X:\ssd: 65.0% vs 61.0% / 61.7%). GiantSteps: `NOCEUR_GIANTSTEPS=<dataset> cargo test -p noceur-audio --release --test key_giantsteps -- --ignored --nocapture` (76.46 weighted, same as PyTorch on 603/604). Library tags: `tests/key_library.rs` (`NOCEUR_MIK=1` keeps only files MIK analysed; `NOCEUR_KEY_MODELS` compares other weights). App: `analysis::analyse_tracks` (one decode, BPM + key) → `ui/src/Analysis.tsx` (was `Tempo.tsx`). |
| step 18 | Background BPM & key for played tracks only. `background.rs`: one low-priority worker (THREAD_MODE_BACKGROUND), queue fed by `commands::play` → `Background::prioritise`; `process()` measures via `analysis::measure_file`, stores in `noceur_index::analysis` (V10 table, carried in `reprobe`), applies `plan()` (Fill default / Show / Overwrite), re-reads the file before writing; the loaded track is written via `write_beside` (copy with the extension kept, tag the copy, rename over the original — the player's handle keeps reading the old bytes) and `NowPlaying` is updated, falling back to holding the write until the player moves on if the rename is refused. Config `analysis` (`enabled` off by default, `mode`, `min`/`max`). Commands `analysis_settings`, `set_analysis_settings`, `analysis_status`; events `analysis-status`, `analysis-changed` (Pane refreshes visible rows). List: `RowDisplay.detectedBpm/detectedKey/bpmDiffers/keyDiffers`; dimmed stand-in + click-to-write dot (`.measured`, `.measuredmark`). Settings → Files → BPM & key. |
| `f86d55b` | BPM detection. `noceur-audio::tempo` (mel spectral flux → autocorrelation → coarse pick with a 125 BPM log-normal prior → 24-beat comb refine; `rounded()` writes whole within 0.1). `noceur-app::tempo::detect_tempo(fileIds, min, max)` measures a batch of 4 in parallel; `ui/src/Tempo.tsx` shows tagged → detected, differing rows ticked, writes via `writeTags` per file. Range preset in `localStorage` `noceur.tempoRange`. Accuracy: `cargo test -p noceur-audio --release --test tempo_real -- --ignored --nocapture` (`NOCEUR_SCAN`, `NOCEUR_LIMIT`). Many library tags are 1 BPM low; a whole-track kick-grid check confirmed the detection every time. `examples/bpm.rs` prints one file's tempo. |
| `272d846` | Roadmap step 6. `match_track` loops `By::Tags` → `By::Sound` → `By::Name` until one returns matches, skipping empty and repeated queries. `Matched.query` (was `heard`) carries `by` + artist/title when not tags; picker says "From filename:". |
| `addde45` | Roadmap step 4, Tidal (`noceur-online/src/tidal.rs`). User's own client ID + secret in Settings → Metadata sources (`tidalClientId` / `tidalClientSecret` in `noceur-config.json`). Search `/searchResults?filter[query]` then `/tracks?include=albums,artists,genres,albums.coverArt`. Live-tested: album, date, ISRC, 640 px cover. BPM/key/genre are withheld from third-party apps. Live test reads `NOCEUR_TIDAL_ID` / `NOCEUR_TIDAL_SECRET`. |
| `ca86927` / `c2eef3b` | Roadmap step 5, AcoustID. `noceur-audio::fingerprint` (rusty-chromaprint, first 120 s, ~0.3 s) → `noceur-online::acoustid::lookup`. Find metadata uses it when the file has no artist/title tag, or when its tags find nothing, before the file-name guess. The picker shows "By sound: artist — title". The app's key is in `acoustid.rs`. The live test identifies a real Burial FLAC at 0.90. |
| `fc03fc1` | MusicBrainz removed from Find metadata. Its recording search is deleted. MusicBrainz still finds Cover Art Archive covers in the cover picker. |
| `c3c1c2d` | Find metadata: matches without art are dropped, 3 per source (backend keeps 6), rows whose art fails to load are hidden. Deezer and Bandcamp details are fetched in parallel. |
| `88ae63f` | Audio decoded off the device callback. Ratings stay in sync between the list and Quick Tag. Label is its own tag field. |
| `9bd341c` / `16f6709` | Bandcamp and Beatport as metadata sources (roadmap steps 2 and 3). |

## How the fixes from this session work
- **Playback noise on lossless (`noceur-audio/src/prefetch.rs`).** rodio pulls samples inside
  the WASAPI callback, so the decoder used to read the file there too. On a cold cache, large
  FLAC/WAV/AIFF files stalled the callback, and Windows looped the stale buffer as a loud buzz.
  - `Prefetch` runs the decoder on a `noceur-decode` thread, up to 2 s ahead.
  - It prefills 300 ms before playback starts.
  - On an underrun it plays whole frames of silence.
  - Seeks are tagged with a generation number, so stale chunks are dropped.
  - `source.rs` uses a 256 KB `BufReader`.
  - An offline capture through the real chain was clean before the fix, so the bug was
    timing-only. **The user has not yet confirmed the fix by ear.**
- **Rating came back after a track change.** The file write was fine. The Quick Tag panel
  wrote back every field it had read when the track started, which overwrote a star cleared
  in the list.
  - Quick Tag now writes only the fields in its `touched` set.
  - The list and the panel share changes through `announceRating` / `onRating` in `api.ts`,
    a window CustomEvent.
- **Label.** `Edit.label` and `TrackMeta.label` use lofty `ItemKey::Label` (`TPUB` in ID3,
  `LABEL` in Vorbis).
  - It is in the editor form (`TAG_FIELDS` in `api.ts`, `TagForm` / `TagEdit` in `dto.rs`).
  - Actions accept `label` and `publisher`, and syntax help lists `%label%`.
  - Matches fill Label, not Comment.
  - The user asked for no catalog-number field.
  - The label is not in the index or a list column yet.
- **Find metadata filtering.** `autotag::trim` keeps matches with art, 6 per source.
  `MatchPicker` hides any whose image fails and shows `PER_SOURCE = 3`.

## Not yet confirmed in the real app
- Confirmed by the user on 2026-09-30: the lossless noise is gone, ratings stay cleared,
  and the cover filter works.
- Label round trip from Discogs or Beatport: confirmed working.
- AcoustID in the app: try Find metadata on a file with no tags.
- The credits text in Comment on some slsk files ("Written By – …") came with the downloads.
  Noceur never wrote it. Matches have no comment field.

## Next steps
1. Confirm AcoustID and the filename fallback in the app. Picker wording is settled:
   "By sound:" and "From filename:".
2. Try Detect BPM & key in the app (toolbar metronome, or right-click). Not yet seen by the
   user. Range presets 70–180 / 60–120 / 85–175 / 100–200 are open wording.
3. The user wants to pick 3–5 of their tracks and compare Noceur, rekordbox and Mixed In Key on
   BPM and key. `cargo run --release -p noceur-audio --example bpm|key -- <files>` prints ours.
4. Roadmap: step 10, converters on Shift+1–5. Steps 1–9 and 18 are done. Step 19 (track sidebar) is a maybe. The user has not tried the Tools dialog yet; its name ("Tools") is open wording.
5. Settings copy for sources is open: current "Discogs and Tidal need your own keys. Each is
   sent only to its service." (user not yet asked to pick).
6. Known smaller issues:
   - The converter does not carry ID3 `POPM` ratings.
   - Stars in an unfocused pane do not move focus.
   - Sorting by Rating does not re-sort until the next refetch.

## User preferences
- Push straight to `master`. No branches or PRs.
- UI copy should be short and terse. When wording is open, offer 3–4 choices.
- "Rebuild" means the embedded-UI build below. A plain `cargo build` shows "localhost refused
  to connect".

## Commands
```bash
# Rebuild the app (close a running noceur-app.exe first)
npm --prefix ui run build
cargo build --release -p noceur-app --features custom-protocol

# Checks (what CI runs)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npx --prefix ui tsc --noEmit -p ui/tsconfig.json

# Live network tests for the metadata sources (ignored by default)
cargo test -p noceur-online --test live -- --ignored
```
Details: `docs/building.md`, `docs/troubleshooting.md`.
