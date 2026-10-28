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
```

`dist` builds `kjk` with the `dist` profile (`Cargo.toml`: release with
fat LTO in one codegen unit, no incremental state, no symbols) and
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

The app's `Info.plist` carries the release tag's numbers (`v0.2.1` is
0.2.1), 0.0.0 on a build that is not a release.
`THIRD-PARTY-LICENSES.html` is written by `cargo about` (`about.toml`,
`about.hbs`; `cargo install --locked cargo-about --features cli`), for
the crates the player's build is made of. The macOS app's icon is drawn
by `xtask` itself (`xtask/src/icon.rs`, made into an `.icns` by
`iconutil`): our own design, no game's art.

Sizes, macOS arm64 (2026-10-27):

| profile | `kjk` | packed | build |
|---|---|---|---|
| release (development, with probes) | 129 MB, 86 MB stripped | 30 MB | |
| dist, thin LTO, 16 codegen units | 99 MB | 33.7 MB | 7.5 min |
| **dist, fat LTO, 1 codegen unit** (in use) | **67 MB** | **28.6 MB** | 16.5 min |

### Releases from CI

`.github/workflows/release.yml` runs on a `v…` tag, which only the owner
pushes: it makes the packages, checks each holds the program, the
readme and the three licence texts (and on macOS the plist and icon,
and that `kjk --help` runs), then puts them on a **draft** Release with
`SHA256SUMS`; the owner reviews it and publishes. Run by hand (Actions →
release → Run workflow) it makes the packages as the run's artifacts and
no Release. Only macOS on Apple silicon so far.

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

- **The gamepad in play.** It drives the menus and the setup only; a full
  scheme (sticks, triggers, a wheel for powers and items, glyphs) is the
  Steam Deck's need.
- **Autosave** (the option is greyed).
- **GOG.** Its two default Windows folders only: no registry (a custom
  folder), no Heroic or Lutris on Linux, no macOS. The setup's Locate covers
  the rest.
- **Linux and the Steam Deck** have not run a build. The Deck's Game Mode
  folder dialog depends on its desktop portal; Type path is the fallback.
- **KOTOR II** is required for the menus' look; making it optional (KOTOR's
  own menus) would let more players in.
- **Movies** need `ffmpeg` on the `PATH`; without it they are skipped.
- **A Linux AppImage**, and Linux and Windows on ARM. `release.yml` packs
  macOS on Apple silicon (no Intel build), Windows and Linux on x86-64.
