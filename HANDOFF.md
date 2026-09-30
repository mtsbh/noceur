# Handoff

## Current state (2026-09-30)
- Work goes straight to `master`, which is pushed and matches `origin/master`. No feature
  branches or PRs.
- Windows-only Tauri 2 app: Rust backend in `crates/`, Solid + Vite frontend in `ui/`.
- `target/release/noceur-app.exe` is built from `fc03fc1`, with the UI embedded.
- Workspace fmt, clippy and tests are clean. `npx tsc` is clean too.
- `master` is the only branch on GitHub. The stale `claude/kind-lamport-22i4sm` branch was
  deleted.

## Recent commits (newest first)
| Commit | What |
|---|---|
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
- The noise fix (see above).
- Quick Tag and list ratings staying in sync across track changes.
- Label round trip from Discogs or Beatport through the editor to the file.
- Files tagged earlier still have the Discogs label in Comment. The user fixes these by hand.

## Next steps
1. Ask the user to confirm the four fixes above.
2. Roadmap (`docs/roadmap.md`), in order. Step 4 was waiting on the user's choice:
   - **Step 4, Tidal:** needs a developer Client ID and secret in Settings → Sources. It can be
     built now and switched on later.
   - **Step 5, AcoustID fingerprints:** would use MusicBrainz recordings again, only for lookup
     by fingerprint, not in Find metadata.
3. Known smaller issues:
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
