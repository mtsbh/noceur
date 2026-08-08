# Shell foundation — verification

**Date:** 2026-08-08
**Build:** `pnpm --dir ui build` then `cargo tauri build --no-bundle`
**Binary:** `target/release/noceur-app.exe` (native Windows executable, 1400×900 window)

Screenshots: `2026-08-08-shell-first-light.png` (E:\music), `2026-08-08-shell-audio-rows.png`
(a folder of tracks).

---

## Confirmed working

Observed directly in the running application against the real library on `E:`.

| Behaviour | Evidence |
|---|---|
| Launches as a native window | Process `noceur-app`, `MainWindowTitle = "Noceur"`, no console, no dev server |
| Directories listed first | `E:\music` showed nine folders and no files interleaved |
| Sodium theme renders | Bone-on-black, accent only on folder glyphs, active crumb, selected-row edge |
| **Breadcrumb has no `\\?\`** | Reads `E: › music › [current] › tg`. `path::simplify()` doing its job |
| Navigating into a folder works | Breadcrumb and listing both updated to the child folder |
| Audio rows render every column | Name, BPM, key, duration, artist, size all populated |
| **Tagged BPM carries its marker** | `105·` and `86·` rendered; the `·` distinguishes tag-derived from analysed |
| Absent values render as a dash | Untagged BPM and key showed `–`, not blank or `0` |
| Durations and sizes format correctly | `12:32`, `4:27`, `5:11`; `126.7 MB`, `25.0 MB` |
| Unicode filenames render | `04 Rêves Cirés.flac`, `A - Shcaa - Masquée.flac`, `Uchitoshi - Budō.wav` |
| Row selection | Accent left edge on the clicked row |
| Status bar reports timings | `12 entries · scan 0ms · probe 559ms` |

## Not verified

GUI automation in this environment proved unreliable — something repeatedly relocated the
window between scripted steps (observed at `963,158`, then `1124,207`, then `-56,140`), so
scripted clicks landed outside it, and a stray click eventually hit the close button. These
need a human with a mouse; each is a few seconds of clicking:

- Back and forward buttons, including being disabled at the ends of history
- The up button
- Clicking a breadcrumb segment to jump to it
- Window responsiveness while a **cold** `Z:` network path is enumerated — the milestone-1
  baseline measured a twenty-second stall there, and this is the check that proves
  `spawn_blocking` actually holds. **This is the most important outstanding check.**

The navigation *logic* was read and is correct (`ui/src/App.tsx`: `canBack = cursor() > 0`,
history truncated on branch, `HOME = "E:\\music"`), and forward navigation into a child
folder was observed working. What is unproven is the button wiring, not the model.

## Notes for the next milestone

**`cargo build` is not a valid way to build this app.** A plain `cargo build --release`
produces a binary that still points at the Vite dev server and shows an Edge
"can't reach this page" error. Use `cargo tauri build --no-bundle` (or `cargo tauri dev`).
The UI must be built first — see below.

**`beforeBuildCommand` / `beforeDevCommand` were removed from `tauri.conf.json`.** Their
relative paths resolved against a working directory that could not be predicted; Tauri tried
`D:\projects\ui` rather than `D:\projects\Brows\ui`. The frontend is now built as an explicit
step:

```bash
pnpm --dir ui build
cd crates/noceur-app && cargo tauri build --no-bundle
```

If that coupling is wanted back, work out Tauri's actual cwd first rather than guessing.

**pnpm blocks esbuild's postinstall until approved.** In a non-interactive shell it writes a
`set this to true or false` stub into `ui/pnpm-workspace.yaml` and waits, which hangs the
build. `allowBuilds.esbuild = true` answers it once; deleting the stub only makes it ask again.

**`tauri-build` requires `icons/icon.ico` on Windows even with `bundle.active = false`.** A
placeholder is committed at `crates/noceur-app/icons/icon.ico`; it should be replaced with a
real icon before anything ships.

**Rendering is unvirtualized, as planned.** The folders tested held 9 and 12 entries, so this
milestone provides no evidence about large-folder behaviour. The 238-file folder from the
milestone-1 baseline is the first place to look once virtualization lands.
