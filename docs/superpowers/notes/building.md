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
