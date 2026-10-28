# Building

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.98.1, with rustfmt
and clippy). rustup installs it on the first `cargo` call in the repo.

```bash
cargo build -p launcher      # first build compiles Bevy: ~5 min cold
cargo run -p launcher        # opens the kjk window
```

The log's first `kjk_game` line names the render backend, adapter and
driver: `render adapter backend=Vulkan adapter=… driver=…`.

## Two crates: `game` and `launcher`

- `crates/game` (package `game`, library `kjk_game`): the runtime. The
  Bevy app (window, rendering, the play view, the screens, sound and
  movies), the sim's host, and the sessions it hosts or joins. Its
  `probes` feature builds the game's own checks and the layers' probes.
- `crates/launcher` (package `launcher`, library and binary `kjk`): the
  command line, the verbs that check and measure, the playthroughs and
  chain, the probes. It builds on `game`; nothing in `game` names it.
  `launcher`'s `probes` feature (on by default) turns on `game/probes`.

Logs keep their module paths as targets: the game's read `kjk_game::…`,
the CLI's and the verbs' `kjk::…`. The default filter is Bevy's (`info`
and up, every target), so both show as before.

Both stamp the commit as `KJK_GIT_SHA` (`crates/game/stamp.rs`). The
binary's stamp watches every crate; the game's watches every crate but
`launcher` and `approved_tests`, so a change to a verb does not rebuild the
game, and a tree dirty only in a verb is stamped `-dirty` in the game's
stamp (the one the network handshake compares) once the game next builds.

Once per clone, install the pre-commit hook:

```bash
cargo xtask hooks            # git config core.hooksPath .githooks
cargo xtask publish-check    # what the hook runs
```

The `Makefile` wraps the same commands (`make run`, `make publish-check`,
`make hooks`, and `make duo` for `cargo xtask duo`, `NET.md`) for machines
that have `make`.

## Rebuild times

Our crates build incrementally in release too (`[profile.release]` in
`Cargo.toml`; cargo never does so for dependencies, which stay at
`opt-level` 3 and rebuild only when they change). Measured 2026-10-11 on
the 12-core Mac: a one-line change in `sim` to a new `kjk` took 137 s,
123 s of it the `launcher` crate, and takes 12 s so; one in `launcher`
15 s. The sim runs no slower (a playthrough's CPU time the same within
1%) and its hash is unchanged. CI sets `CARGO_INCREMENTAL=0`: it builds
once.

Splitting the game out of `launcher` (2026-10-16, same Mac, warm target,
`cargo build --release -p launcher`, a constant added to the file): a
change to a verb (`chain.rs`) went from 18 s to 3.5 s, `cargo clippy -p
launcher --all-targets` from 4.7 s to 2.1 s; a change to the game
(`scene.rs`) from 36 s to 28 s, clippy 7.3 s either way (the game is
still one crate, which the verbs then build on).

A new worktree's `target/` starts as a copy of the main checkout's,
before its first `cargo` command. On macOS that copy is a clone (APFS
copy-on-write): about a second and no disk, and cargo then rebuilds our
crates alone (80 s), never Bevy and the other dependencies (~20 minutes
cold). From the new worktree, on macOS:

```bash
cp -c -R "$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/target" target
```

Worktrees keep their own `target/`: one shared by all (`CARGO_TARGET_DIR`)
makes parallel builds wait on its lock and rebuilds our crates at every
switch of branch.

## Windows

Build with the MSVC toolchain. It needs Visual Studio Build Tools with the
"Desktop development with C++" workload (the linker and Windows SDK).

The GNU toolchain does not build this repo without MinGW (`dlltool.exe`).
rustup resolves a pinned version for its *default host*, so if
`rustup show` says `Default host: x86_64-pc-windows-gnu`:

```bash
rustup set default-host x86_64-pc-windows-msvc
```

On Windows, wgpu picks Vulkan when the driver offers it, DX12 otherwise.

## Linux and macOS

Linux needs `pkg-config libwayland-dev libxkbcommon-dev libasound2-dev
libudev-dev` (what CI installs; udev for gamepads).
macOS needs nothing extra. Game paths go in `.env` (from `.env.example`).

The macOS Steam ports keep their data inside the `.app` bundle
(`Contents/Assets` for KOTOR, `Contents/GameData` for TSL, `Contents/base`
for Jedi Academy). Discovery looks there, so a variable may name the Steam
folder or the `.app` itself.

## CI

`ci.yml` runs on pushes to main and on pull requests (not when only `docs/`
or `*.md` files change): Linux only, two jobs side by side (tests:
publish-check, fmt, the approved tests; clippy: both lints), and on main,
beside them, `kjk` built and started with `--help`. Only main saves the
build caches.
Windows/macOS builds and the screenshot runs are `manual.yml`: Actions →
manual → Run workflow, with a checkbox for each. It also runs on the 1st of each month
with the approved tests alone on Windows and macOS (the sim's hash).
