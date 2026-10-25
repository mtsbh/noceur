# Building a release

```bash
npm --prefix ui run build
cargo build --release -p noceur-app --features custom-protocol
```

## The feature is not optional

`--features custom-protocol` is what switches Tauri from loading the front end
off the dev server to serving the assets embedded in the executable.

Without it the binary starts up pointing at `http://localhost:1420` and shows

> Hmmm… can't reach this page — localhost refused to connect.

to anyone who does not happen to have `npm run dev` running. The Tauri CLI turns
this on for `tauri build`; a plain `cargo build --release` does not, and nothing
warns about it. The failure is invisible on a machine that has a dev server up,
which is exactly the machine a release usually gets built on.

## Checking a build

The frontend is embedded when the hashed asset name appears in the binary:

```bash
grep -c "$(ls ui/dist/assets/*.js | head -1 | xargs basename)" target/release/noceur-app.exe
```

`1` means the assets are in there. `0` means it is a dev build wearing a release
build's name.

Searching the binary for `localhost:1420` proves nothing either way — the whole
`tauri.conf.json` is embedded regardless, `devUrl` included.

## Building the installer

```bash
npm --prefix ui run build
cd crates/noceur-app && node ../../ui/node_modules/@tauri-apps/cli/tauri.js build --bundles nsis
```

The installer comes out at `target/release/bundle/nsis/`. Build the UI first:
`tauri build` bundles whatever is in `ui/dist`, with no step of its own to
refresh it.

`windows/installer.nsi` is Tauri's NSIS template from CLI 2.11.5 with one
addition: a third tick on the Finish page for "Show in Noceur" on audio
files' right-click menu. It runs `noceur-app.exe --context-menu on|off`, and
`windows/hooks.nsh` runs `off` on uninstall. When the Tauri CLI is upgraded,
take its template again and carry the `Noceur:` parts across.
