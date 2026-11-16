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

A new worktree's `target/` starts as a copy of the shared build seed,
before its first `cargo` command. The build seed is one build folder made
from `origin/main`, kept in `~/Library/Caches/kjk-seed/target`. On macOS
the copy is a clone (APFS copy-on-write). It takes about a second and no
disk. Cargo then rebuilds our crates alone (80 s), never Bevy and the
other dependencies (~20 minutes cold). The dependencies are on disk once,
in the seed, however many worktrees there are. From the new worktree, on
macOS:

```bash
cp -c -R ~/Library/Caches/kjk-seed/target target
```

To build the seed, run this from a checkout of `origin/main`:

```bash
CARGO_TARGET_DIR=~/Library/Caches/kjk-seed/target cargo build --release -p launcher --bin kjk
```

Build it again when `Cargo.lock` or `rust-toolchain.toml` on `origin/main`
change. Delete the seed's `release/incremental` and `debug/incremental`
folders afterwards. Incremental state is tied to the folder it was built
in, so a clone never reuses it, and it is most of a build folder's size.

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
A build with DLSS (below) makes its renderer through Vulkan alone, and
falls back to wgpu's own choice when no Vulkan instance can be made.

## Linux and macOS

Linux needs `pkg-config libwayland-dev libxkbcommon-dev libasound2-dev
libudev-dev` (what CI installs; udev for gamepads).
macOS needs nothing extra. Game paths go in `.env` (from `.env.example`).

The macOS Steam ports keep their data inside the `.app` bundle
(`Contents/Assets` for KOTOR, `Contents/GameData` for TSL, `Contents/base`
for Jedi Academy). Discovery looks there, so a variable may name the Steam
folder or the `.app` itself.

## DLSS

The `dlss` feature (`cargo build -p launcher --features dlss`) adds DLAA
and DLSS to the anti-aliasing choices (`crates/game/src/temporal.rs`),
through Bevy's `dlss` and the `dlss_wgpu` crate. It is for Windows and
Linux on x86-64, an NVIDIA RTX card and the Vulkan backend; on macOS the
feature builds nothing, so `--all-features` builds there as before. The
release packages are built without it until NVIDIA has approved the use
of its marks (`RELEASE.md`).

Building it needs, besides the toolchain:

- **NVIDIA's DLSS SDK** at the tag `dlss_wgpu` pairs with: **v310.5.3**
  for `dlss_wgpu` 4.0.0 (Bevy 0.19, wgpu 29; the crate's readme has the
  chart, `Cargo.lock` the version). Read its licence first
  (`LICENSE.txt`, the NVIDIA RTX SDKs licence), then
  `git clone --depth 1 --branch v310.5.3 https://github.com/NVIDIA/DLSS.git`
  and set `DLSS_SDK` to that folder's absolute path. The build links its
  static NGX library (`lib/<platform>/`); a run finds the DLSS library in
  `$DLSS_SDK/lib/<platform>/rel/` while `DLSS_SDK` is set, or beside the
  program.
- **The Vulkan headers**, in `$VULKAN_SDK/include/vulkan` (`Include` on
  Windows): the [Vulkan SDK](https://vulkan.lunarg.com/sdk/home) sets
  `VULKAN_SDK` itself; a clone of
  [Vulkan-Headers](https://github.com/KhronosGroup/Vulkan-Headers) does
  as well, which is what CI uses (v1.4.335), and on Linux the distribution's
  headers too (`libvulkan-dev`, `VULKAN_SDK=/usr`).
- **clang**, for the bindings (bindgen): `libclang-dev` on Linux, LLVM on
  Windows (`LIBCLANG_PATH` naming its `bin` folder if it is not found).

```bash
export DLSS_SDK=/abs/path/to/DLSS VULKAN_SDK=/abs/path/to/Vulkan-Headers
cargo run -p launcher --features dlss
```

The log says whether the card offers DLSS (Bevy's "DLSS is not supported
on this system" when not); `KJK_AA=dlss_quality` (or `dlaa`,
`dlss_balanced`, `dlss_performance`, `dlss_ultra_performance`) picks it for
a run. Without the card the choices are hidden and a saved one plays as
TAA. The SDK's `dev/` libraries (NVIDIA's on-screen debug overlay) are
for development alone and never ship.

Bevy's `force_disable_dlss` (`dlss_wgpu`'s mock) builds without the SDK
but leaves Bevy's DLSS module out, so it type-checks none of kjk's DLSS
code: that needs the SDK, on Linux or Windows. On a Mac, a Linux container
does (`rust:bookworm` with `libclang-dev` and the packages above,
`DLSS_SDK` and `VULKAN_SDK` mounted in, `cargo clippy -p launcher
--features dlss`); it checks, it does not link (the NGX library is
x86-64).

## CI

`ci.yml` runs on pushes to main and on pull requests (not when only `docs/`
or `*.md` files change): Linux only, two jobs side by side (tests:
publish-check, licences (`cargo deny`), fmt, the approved tests; clippy: both lints), and on main,
beside them, `kjk` built and started with `--help`, and again with DLSS
(`--no-default-features --features dlss`, as the releases build it). Only main saves the
build caches. clippy's `--all-features` takes in `dlss`, so it checks out
NVIDIA's SDK and the Vulkan headers (`.github/actions/dlss-sdk`: the files
a build uses, cached by their tags).
Windows/macOS builds and the screenshot runs are `manual.yml`: Actions →
manual → Run workflow, with a checkbox for each. It also runs on the 1st of each month
with the approved tests alone on Windows and macOS (the sim's hash).
