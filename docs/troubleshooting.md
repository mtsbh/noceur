# Troubleshooting a fresh checkout

Things that went wrong when this repository was picked up under a different Windows account from
the one that built it. Nothing in the repo was wrong; each problem was outside it. Recorded here
because none of them is guessable from the error message.

## `git` refuses to read the repo

```
fatal: detected dubious ownership in repository at 'D:/path/to/noceur'
```

The working tree is owned by another account's SID. One-time fix:

```bash
git config --global --add safe.directory D:/path/to/noceur
```

## The Rust toolchain looks absent but is not

`cargo` is not on `PATH` and `where cargo` finds nothing, but `~/.rustup` and `~/.cargo/bin`
both exist — the shims are intact and every file under
`~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin` is a **1-byte placeholder**. Running
the shim with `~/.cargo/bin` on `PATH` repairs it, because `rust-toolchain.toml` pins the
channel and rustup downloads it on first use:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version   # syncs the pinned toolchain, then works
```

With Visual Studio Build Tools and a Windows SDK present, `rusqlite`'s bundled SQLite links
without further setup.

## `npm install` fails on an old `node_modules`

A `ui/node_modules` left behind by another account can be unreadable. Delete it and install
from the lockfile:

```bash
rm -rf ui/node_modules
npm --prefix ui ci
```

Once that is done, the full set of checks in the README runs normally.
