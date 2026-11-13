# Releases: the player's build and the first-run setup

Status: started 2026-10-25 (`ROADMAP.md`). The player's build, its
package and the first-run setup are in; a gamepad scheme for play,
autosave, GOG's registry and runs on Linux and the Steam Deck are not
(below, "Not yet").

No game data is ever packaged: a release is the `kjk` program, a readme
and the licences. The player's own installs are found or chosen on the
first start and only ever read.

## The player's build

```bash
cargo xtask dist                 # build and pack for this machine's OS
cargo xtask dist --skip-build    # pack the last build again
cargo xtask dist --pgo <profile> # kjk built with a profile (below)
cargo xtask dist --dlss          # Windows and Linux: with DLSS (below)
```

`dist` builds `kjk` with the `dist` profile (`Cargo.toml`: release with
thin LTO, no incremental state, no symbols) and
`--no-default-features`:
no verbs that check and measure, no probes and no Steam, the game alone
with co-op by address. A release ships MIT code and the crates' own
licences only: Steam would bring Valve's library (`libsteam_api`, under
the Steamworks SDK's terms) and an app id kjk does not have yet (it opens
as Valve's test app 480), so Steam co-op is for builds made from source
(`cargo build --features steam`, the default). The package goes to
`target/packages/`:

| OS | package | inside |
|---|---|---|
| Windows | `kjk-<version>-windows-<arch>.zip` | `kjk.exe`, `README.txt`, `LICENSE.txt`, `NOTICE.txt`, `THIRD-PARTY-LICENSES.html` |
| macOS | `kjk-<version>-macos-<arch>.tar.gz` | `kjk.app` (the program in `Contents/MacOS`), the same texts |
| Linux, Steam Deck | `kjk-<version>-linux-<arch>.tar.gz` | `kjk`, the same texts |

`<version>` is `git describe --tags --match 'v*' --always --dirty`. Each OS packs its
own: a Windows package is made on Windows, a macOS one on a Mac (both
architectures need a build each), a Linux one on Linux.

### DLSS in a Windows or Linux package

No release package has DLSS yet. `release.yml` packs every package with a
plain `dist`. NVIDIA's licence asks for its written approval of its marks
before a release shows them, and that approval has not been given. To ship
DLSS, give the Windows and Linux lines of `release.yml`'s matrix
`dist: --dlss`. The rest of this section describes such a package.

`dist --dlss` packs Windows and Linux with DLSS: `kjk` built with
`--no-default-features --features dlss` against NVIDIA's DLSS SDK v310.5.3
(`DLSS_SDK`, `BUILD.md`, "DLSS"; CI checks out the files it needs through
`.github/actions/dlss-sdk`), and beside it what NVIDIA's licence and its
programming guide (sections 4.2 and 9.5) ask a program to ship:

| file | from the SDK |
|---|---|
| `nvngx_dlss.dll` (Windows), `libnvidia-ngx-dlss.so.310.5.3` (Linux) | `lib/<platform>/rel/`, the signed release library; never `dev/`'s, nor the frame generation (`dlssg`) or ray reconstruction (`dlssd`) ones, which kjk does not use |
| `NVIDIA-RTX-SDK-LICENSE.txt` | `LICENSE.txt` |
| `NVIDIA-DLSS-NOTICES.txt` | the guide's section 9.5 to 9.6 (notices, trademarks, copyright, the third-party code in the library), read out of `doc/DLSS_Programming_Guide_Release.pdf` by `pdftotext` (poppler-utils; xpdf on Windows) |

The readme gains a DLSS paragraph and `THIRD-PARTY-LICENSES.html` the
crates of the feature (`dlss_wgpu`, `ash`, ...). NVIDIA's library must keep
its own signature: nothing re-signs or strips it (`dist` signs only the
macOS app, which has no DLSS). The macOS package is as before.

What a DLSS build changes for every player, card or not: Bevy makes the
renderer through a Vulkan instance alone (so it can add NVIDIA's
extensions), where wgpu would otherwise pick among Vulkan and DX12 (on
Windows, Vulkan first when the driver has it). When no Vulkan instance
can be made (no Vulkan driver), it falls back to wgpu's own choice, DX12
on Windows; `WGPU_BACKEND` is then read as before. Without an RTX card, or
a driver without DLSS, the DLSS choices are hidden and a saved one plays as
TAA (`temporal::choose`).

NVIDIA's licence, as it bears on kjk (the NVIDIA RTX SDKs licence and its
supplement, v. 14 March 2024, in the SDK's `LICENSE.txt`): the SDK's
distributable parts may ship in object code inside an application that does
more than the SDK, under terms at least as protective as NVIDIA's, and
never on their own; they may not be reverse engineered, nor put under an
open source licence, so they ship beside the MIT code under NVIDIA's own
licence (`NOTICE`). DLSS is licensed for systems with NVIDIA GPUs. The
supplement also asks for NVIDIA to be told before a commercial release
(developer.nvidia.com/sw-notification) and, for a game using DLSS, the
SDK's use and NVIDIA's marks credited on its splash screen, about box and
credits, with NVIDIA approving each use of its marks; kjk has neither yet
(owner's call, below).

The app's `Info.plist` carries the release tag's numbers (`v0.2.1` is
0.2.1), 0.0.0 on a build that is not a release.
`THIRD-PARTY-LICENSES.html` is written by `cargo about` (`about.toml`,
`about.hbs`; `cargo install --locked cargo-about --features cli`), for
the crates the player's build is made of. The icons are the logo,
`docs/logo/kjk.svg` (our own design, no game's art), rendered by `xtask`
(`xtask/src/icon.rs`): the macOS app's as it packs (made into an
`.icns` by `iconutil`); `kjk.exe`'s (`crates/launcher/kjk.ico`, built in
through `kjk.rc`) and the window's (`crates/game/icon.png`) are kept in
the repo and rendered again by `cargo xtask icon` when the logo changes.

Sizes, macOS arm64 (2026-10-27):

| profile | `kjk` | packed | build |
|---|---|---|---|
| release (development, with probes) | 129 MB, 86 MB stripped | 30 MB | |
| **dist, thin LTO, 16 codegen units** (in use) | **99 MB** | **33.7 MB** | 7.5 min |
| dist, fat LTO, 1 codegen unit | 67 MB | 28.6 MB | 16.5 min |

Fat LTO was in use until v0.18.2. Its last step runs on one core, and on
GitHub's Windows runner that step alone took 29.5 minutes of a 64 minute
release build.

### A profile-guided build

```bash
rustup component add llvm-tools          # once: llvm-profdata
cargo xtask pgo                          # target/pgo/kjk.profdata (~1 h)
cargo xtask dist --pgo target/pgo/kjk.profdata
```

`pgo` builds `kjk` as `dist` does but counting what it runs (its own target
folder, `target/pgo/gen`), plays the bench's scenes with it on the default
and low presets (`kjk bench --exe`, driven by a development build, so it
needs the games), and merges the counts. `dist --pgo` then builds `kjk`
laid out by them: hot code together and inlined, cold code apart. The
picture is the same; only where the time goes changes. A function changed
since the profile was made is built as without one, so a profile goes stale
slowly but should be made again for each release.

Measured on Windows (Ryzen 7 3700X, RTX 4070 Ti SUPER, 2026-11-07), the
same commit's `dist` build without and with a profile, `kjk bench`, back to
back:

| fps | low | default |
|---|---|---|
| Javyar's cantina | 360 → 430 (+19%) | 229 → 262 (+14%) |
| Harbinger | 274 → 332 (+21%) | 215 → 246 (+14%) |
| Taris Undercity | 323 → 377 (+17%) | 312 → 348 (+12%) |
| Arena, 8 foes | 354 → 412 (+16%) | 288 → 320 (+11%) |
| Citadel Station (not trained on) | 328 → 378 (+15%) | 249 → 284 (+14%) |
| Kashyyyk's Great Walkway (not trained on) | 335 → 387 (+16%) | 216 → 245 (+13%) |

The last two areas are `kjk bench --scenes telos,kashyyyk`, left out of the
training, so the gain is not the trained scenes' alone. `release.yml` has
no games to train on, so a release with a profile takes one made on the
owner's machine (committed, or the Windows package built by hand); the
releases are built without one until the owner decides.

### Before the tag

Players keep their saves across releases, so a tag waits for the owner's
machine, which has the games (`release.yml` has none) and the corpus of real
saves (`docs/superpowers/specs/2026-10-28-save-migrations-design.md`, 2b):

```bash
kjk saves capture v0.5.0 --from <slot, zip or .kjkv>...  # this release's saves
kjk saves check --mute                                    # the whole corpus
```

1. **Capture** the release's corpus under its tag: the chain's checkpoints
   for the Endar Spire, Taris, mid-Korriban and TSL's Peragus (no `--from`
   takes them from `kjk-artifacts/checkpoints/<game>/`), plus any owner save
   worth keeping. They go to `kjk-artifacts/save-corpus/<tag>/<name>/`
   (git-ignored), each with a `story.json` made if it has none.
2. **Check** the full corpus, every release's saves on this build. The table
   says for each save the formats migrated, stamps moved, what reconciling
   did, whether its story restores, and PASS, EXPECTED (its release is before
   a `breaking` entry of `bumps/BREAKING.md`) or FAIL. Any FAIL stops the tag.
3. **Mirror** `kjk-artifacts/save-corpus/` to the private archive, so the
   corpus is not lost with a disk.

### Releases from CI

`.github/workflows/release.yml` runs on a `v…` tag, which only the owner
pushes: it makes the packages, checks each holds the program, the
readme and the three licence texts (and on macOS the plist and icon)
and that its `kjk --help` runs, then puts them on a **draft** Release with
`SHA256SUMS`; the owner reviews it and publishes. Run by hand (Actions →
release → Run workflow) it makes the packages as the run's artifacts and
no Release. Each package is built on its own runner: macOS on Apple
silicon, Windows and Linux on x86-64 (Ubuntu 22.04's glibc).

The sim determinism test runs on each OS in a job of its own, beside the
package's build. The draft leaves out the package of an OS whose test did
not pass. The builds read the dependencies from a rust cache. A tag's run
cannot save a cache that a later tag could read, so the workflow also runs
on main when `Cargo.lock`, `rust-toolchain.toml` or `release.yml` change,
and saves the caches then. That run makes no Release.

#### Pre-releases

A build for testers, from any branch, goes on a **pre-release**: GitHub
marks it as one and never as the Latest release, so a player who goes to
the Releases page still finds the last real one. There are two ways:

- **From a branch, as it is**: Actions → release → Run workflow, pick the
  branch, tick **pre-release**. The packages are named by `git describe`
  as always (`kjk-v0.13.0-17-gabc1234-windows-x86_64.zip`), and the draft
  is titled `kjk v0.13.0-17-gabc1234 (<branch>)`. Its tag,
  `pre-<branch>-v0.13.0-17-gabc1234` (`/` and other odd characters in
  the branch's name made `-`), is made at the run's commit when the draft
  is published. It never starts with `v`, so `git describe --match 'v*'`
  never names a later build after it. Running it again on the same commit fails while that draft or
  pre-release is there: delete the draft (it has no tag yet) first.
- **A version's candidate**: a `v…` tag with a suffix (`v0.14.0-rc.1`,
  `v0.14.0-beta.2`), pushed by the owner like any release tag, on any
  branch. It is drafted as a pre-release; the app's version is still the
  numbers (0.14.0).

Either way it is a draft that the owner reviews and publishes, as a
release is. Publishing it announces it on Discord, its title marked
"(pre-release)", but does **not** upload it to Nexus: the page's files
are the players' releases (`nexus-release.yml` skips a pre-release; run
it by hand with the tag to upload one anyway). A pre-release skips
"Before the tag": its saves are not checked against earlier releases'.

Each package job also attests its package (`actions/attest-build-provenance`):
a Sigstore-signed record, kept in the public transparency log, that the
file with this SHA-256 was built by `release.yml` in this repository at
this commit and ref. Anyone checks a download with
`gh attestation verify kjk-<tag>-<os>.tar.gz --repo kjk-engine/kjk`; a
package rebuilt elsewhere, or changed by one byte, fails. It proves where
a package was built, not that the commit is safe. A package built on a Mac
by hand has no attestation; one from a re-run job does.

The draft is made with the packages that built: GitHub's macOS arm64
runners are at times not given at all (the job is cancelled after about 15
minutes, "not acquired by Runner of type hosted even after multiple
attempts"), and one OS missing does not hold back the others. The draft's
notes name the missing build. To add it later, re-run the failed job
(Actions → the run → Re-run failed jobs) and upload its artifact, or build
it on a Mac at the tag (`cargo xtask dist`), then `gh release upload <tag>
target/packages/kjk-<tag>-macos-aarch64.tar.gz` and add its line to
`SHA256SUMS`. A package added after publishing reaches Nexus with
nexus release → Run workflow and the tag.

Publishing the draft announces it in the Discord server's #releases
channel (`.github/workflows/discord-release.yml`, the message in
`.github/discord-release.jq`): the repository secret
`DISCORD_RELEASES_WEBHOOK` holds the channel's webhook, and the variable
`DISCORD_RELEASE_ROLE`, a role's id, is pinged when set. Progress goes to
#progress by hand: `cargo xtask discord "caption" clip.mp4 shot.png`
(its webhook in `KJK_DISCORD_PROGRESS_WEBHOOK` or the git-ignored
`context/discord-progress-webhook`; larger files are shrunk to Discord's
20 MB, `--dry-run` sends nothing).

Publishing it also puts the packages on the Nexus Mods page,
nexusmods.com/kotor/mods/1911 (`.github/workflows/nexus-release.yml`,
the calls in `.github/nexus-upload.sh`): the repository secret
`NEXUS_API_KEY` holds the owner's key (nexusmods.com/settings/api-keys);
without it nothing is uploaded. Each OS's package becomes a new main
version of its own file on the page, the file named by the variable or
secret `NEXUS_FILE_<PLATFORM>` (`NEXUS_FILE_MACOS_AARCH64`,
`NEXUS_FILE_WINDOWS_X86_64`, `NEXUS_FILE_LINUX_X86_64`): the API's id for
the file, or the `file_id` in a download link of any of its versions. The
mod's version follows. Nexus takes no `.tar.gz`, so the macOS and Linux
packages go up repacked as `.zip` (modes and symlinks kept). A package with no such variable is added as a new file,
and the run's notice names its id to save as the variable. Actions →
nexus release → Run workflow with a tag uploads that Release again.

The macOS app is **not signed or notarised** (the owner's decision,
2026-10-25: no Apple developer account). `dist` seals it with an ad-hoc
signature (`codesign --sign -`): left as the linker signs it, the plist
and icon break the program's signature and a downloaded app is
"damaged", which nothing in Gatekeeper's dialogs gets past. Sealed,
Gatekeeper refuses it on the first open as from an unknown developer;
the player's readme says how through: System Settings → Privacy &
Security → Open Anyway (right-click → Open up to macOS 14), or `xattr
-dr com.apple.quarantine kjk.app`.

What differs in the player's build (without `probes`):

- **Runs' folders** (log, manifest, a script's screenshots) go to the
  user's folder's `logs/` (beside `settings.json` and `saves/`), and only
  the last 10 are kept (`runtime::KEPT_RUNS`). A development build keeps
  every run in `kjk-artifacts/runs/` under the working folder, as before.
- Every verb under `kjk` that checks or measures is absent; `kjk --help`
  lists what plays.

## The first-run setup (`crates/game/src/setup.rs`)

A plain `kjk` (no script, not headless, no save to load, not back from a
game) opens on the setup when a game is not found or the setup was never
done (`setup.done` in the settings). `kjk setup` opens it at any time;
`kjk setup --start` starts the import at once (with `--headless --cmds`,
for scripted checks).

It draws with Bevy's own UI and font (the `bevy_ui`, `bevy_text` and
`default_font` features), as no game's may be found yet. For each of the
three games it shows the folder and how it was found (Steam, GOG, chosen,
or an environment variable), or why a folder was refused, or that none was
found, with:

- **Locate…**: the platform's folder dialog (`rfd`: Windows' and macOS'
  own, the XDG desktop portal on Linux and the Deck);
- **Type path**: the folder typed or pasted (Bevy's `EditableText`, with
  the system clipboard), Enter to take it, Escape to leave it.

A chosen folder goes in the settings as `install.k1`, `install.ja`,
`install.tsl`; `discover` reads them after the environment's variables
(`KJK_KOTOR1`, …) and before `.env`. A folder that is not that game is a
refusal, shown with its reason, never replaced by another install.

When all three are found, **Import and play** runs on a thread: Jedi
Academy's and KOTOR II's files opened (a broken install is told here, not
in the menu), then KOTOR's install fingerprinted and its templates and
voices made into the store (`import_verbs::prepare`, what `kjk import k1`
does before its checks). Then `setup.done` is saved and the program starts
again on its main menu (`setup::restart`; a new process, as winit makes one
event loop a process).

Mouse, keys (arrows, Tab, Enter, Space, Escape) and gamepads (D-pad or the
left stick, A, B) all drive it.

## Where a player's files are

| what | Windows | macOS | Linux |
|---|---|---|---|
| settings, saves, logs | `%APPDATA%\kjk` | `~/Library/Application Support/kjk` | `$XDG_CONFIG_HOME/kjk` (`~/.config`) |
| the store (imported data) | `%LOCALAPPDATA%\kjk` | `~/Library/Caches/kjk` | `$XDG_CACHE_HOME/kjk` (`~/.cache`) |

Nothing is written next to the program or into a game's folder.

## Not yet

- **The gamepad on real hardware.** Play, the menus (with their hint bar),
  rebinding and the Deck's keyboard are built and probed (`docs/controls`),
  but no physical pad or Deck has played a build yet.
- **Autosave** (the option is greyed).
- **GOG.** Its two default Windows folders only: no registry (a custom
  folder), no Heroic or Lutris on Linux, no macOS. The setup's Locate covers
  the rest.
- **Linux and the Steam Deck** have not been played on; CI builds the
  Linux package and starts it. The Deck's Game Mode
  folder dialog depends on its desktop portal; Type path is the fallback.
- **KOTOR II** is required for the menus' look; making it optional (KOTOR's
  own menus) would let more players in.
- **Movies** need `ffmpeg` on the `PATH`; without it they are skipped.
- **DLSS from a release package.** A local build has run DLAA and the four
  DLSS modes on an RTX 4070 Ti SUPER. No release package has been played
  with DLSS yet. CI builds and links the feature (ci.yml's `dlss` job).
- **NVIDIA's marks.** NVIDIA has been notified of the release. Its written
  approval of its marks on a splash screen and in the credits has not been
  asked for. Releases are packed without DLSS until it is given (NVIDIA's
  licence, above). The splash screen that carries the marks is not built.
- **A Linux AppImage**, and Linux and Windows on ARM. `release.yml` packs
  macOS on Apple silicon (no Intel build), Windows and Linux on x86-64.
