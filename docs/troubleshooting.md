# Rebuilding this repo on a fresh profile

**Date:** 2026-08-20

The repository was picked up under a different Windows account (`ahbh`) from the one that
built milestones 1 and 2 (`mtsbh`, whose profile no longer exists on the machine). Nothing in
the repo was wrong; three things outside it were. Recorded here because each cost time and
none of them is guessable from an error message.

## `git` refuses to read the repo

```
fatal: detected dubious ownership in repository at 'D:/projects/Brows'
```

The working tree carries the old account's SID. One-time fix:

```bash
git config --global --add safe.directory D:/projects/Brows
```

## The Rust toolchain looks absent but is not

`cargo` is not on `PATH` and `where cargo` finds nothing, but `~/.rustup` and `~/.cargo/bin`
both exist — the shims are intact and every file under
`~/.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin` is a **1-byte placeholder**. Running
the shim with `~/.cargo/bin` on `PATH` repairs it, because `rust-toolchain.toml` pins 1.97 and
rustup downloads that channel on first use:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo --version   # syncs 1.97.1, then works
```

Visual Studio Community 2022 and the Windows 10 SDK are present, so `rusqlite`'s bundled
SQLite links without further setup.

## `pnpm install` hangs forever

`ui/node_modules` from the previous account is unreadable: pnpm's default layout is a farm of
symlinks into `.pnpm`, and following those links across the account boundary is denied. Worse,
a fresh `pnpm install` **hangs silently** at the linking step — it prints
`Progress: resolved 75, reused 75, downloaded 0, added 0` and then stops, because creating
symlinks needs Developer Mode or an elevated shell and this account has neither.

The fix is to install a flat, copied tree instead of a symlink farm:

```bash
corepack pnpm install --node-linker=hoisted --package-import-method=copy
```

This is **not** committed to `pnpm-workspace.yaml`. It is a property of the account doing the
install, not of the project, and the original developer's setup works without it. Anyone who
hits the hang wants these two flags; nobody else does.

Notes that still apply from milestone 2: `corepack pnpm` works without `corepack enable`
(which fails, needing write access to `C:\Program Files\nodejs`), and `CI=1` keeps pnpm from
waiting on prompts.

## Consequence for the milestone-3 work

Everything above had to be sorted out before a single gate could run. Once it was, the full
set — `cargo fmt`, `cargo test`, `cargo clippy`, `tsc`, `vite build` — runs normally.
