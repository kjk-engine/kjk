# kjk

A standalone game runtime in Rust (Bevy and wgpu) that plays the worlds,
story and dialogue of **Knights of the Old Republic** and **KOTOR II: The Sith
Lords** with **Jedi Academy**-style real-time movement, saber and Force
combat, and native co-op. KOTOR 1 comes first.

It reads the games from installs you already own. **No game data is included
here or in any release.**

## Status

KOTOR 1 plays from the Endar Spire to the credits, light side and dark. The
whole campaign is checked headless as a chain of routes, one per stretch of
the story and its branches (`data/routes/k1`, `docs/PLAYTESTS.md`).

What works:

- **The world.** KOTOR's modules drawn and walked with Jedi Academy's
  movement and third-person camera (or first person); doors, triggers and
  transitions; rooms', placeables' and models' own effects; grass; TSL's
  weather; area music, ambient sound and voice-over; the games' movies
  (through `ffmpeg`).
- **The story.** The games' own compiled scripts run on our NWScript VM:
  object events, action queues, conversations with their cameras and
  voice, the journal, XP and levels, the party table, items, stores, locks
  and Security, traps, stealth, the galaxy map.
- **Combat.** KOTOR's character sheets (feats, powers, weapons, factions)
  under JA's blasters, sabers and Force powers; grenades, fists, creatures
  with minds and navigation, companions, ragdolls, dismemberment.
- **The screens.** KOTOR's menus and HUD in real time, in TSL's style: main
  menu, character creation, inventory, equipment, character sheet, journal,
  map and minimap, stores, level-up, options, the target and use menu.
- **Minigames.** Pazaak, the swoop races and the Ebon Hawk's turret.
- **Saves.** Save and load from the menus, quick save (F5) and quick load
  (F9), in our own format.
- **Co-op** over UDP (`kjk host`, `kjk join`): guests drive companions,
  share conversations, loot and level up, with prediction and lag
  compensation.

Not yet: TSL's campaign (its modules load and draw; its scripts and systems
are next in [`ROADMAP.md`](ROADMAP.md)) and finished platform releases (the
player's build and its first-run setup are in, [`docs/RELEASE.md`](docs/RELEASE.md)). Co-op has been
played between two machines.

**Before reporting a bug**, read [`KNOWN-ISSUES.md`](KNOWN-ISSUES.md):
janky cutscenes, unfinished menus, odd-looking bodies, approximate
collision, combat rules still in flux, and how much has been played by hand.

## Install

Two ways, both needing your own installs of the games (below):

- **Download a release** (recommended): the archive for your OS from the
  repository's Releases page holds the `kjk` program (on macOS
  `kjk.app`), a readme and the licences. Unpack it anywhere and run
  it; the first start finds your games or lets you point at them. The
  builds are not signed: on macOS, the first time, System Settings →
  Privacy & Security → "Open Anyway" (right-click → Open up to macOS 14);
  on Windows, SmartScreen's "More info" → "Run anyway".
- **Build from source**: Rust and the steps under "First run".

## What you need

- Rust (building from source only). The toolchain is pinned in `rust-toolchain.toml` and rustup
  installs it on the first `cargo` call ([`docs/BUILD.md`](docs/BUILD.md)
  has Windows and Linux specifics).
- Your own installs of **KOTOR 1** (the campaign), **Jedi Academy** (the
  animations, sabers and effects) and **KOTOR II** (the screens' look).
  Steam and GOG installs are found by themselves; anything else goes in
  `.env` (copy `.env.example`). Installs are only ever read.
- Optional: `ffmpeg` and `ffprobe` on the `PATH`, for the movies.

## First run

```bash
cargo run --release -p launcher                 # the setup the first time, then the main menu
cargo run --release -p launcher -- games        # what was found, and why a folder was refused
```

The first start opens the setup: it shows the three games as found (Steam,
GOG) and lets you locate the others, then imports KOTOR's shared data into
the store once and goes on to the main menu (`kjk setup` opens it again;
[`docs/RELEASE.md`](docs/RELEASE.md)). Paths in `.env` (from
`.env.example`) or `KJK_KOTOR1`, `KJK_JA`, `KJK_KOTOR2` still win.

The binary is `kjk` (`target/release/kjk`); the examples below use that
name. The first build compiles Bevy (several minutes). Imported data lives in
the store, outside the repository (`~/Library/Caches/kjk`,
`~/.cache/kjk` or `%LOCALAPPDATA%\kjk`; `KJK_STORE` moves it),
shared by every clone and worktree ([`docs/IMPORT.md`](docs/IMPORT.md)).

```bash
kjk                                        # the main menu
kjk --load quick                           # straight into the quick save
kjk play --stage end_m01aa                 # one module, alone (--class jedi for the Force)
kjk host --stage end_m01aa --class jedi    # a session others join (UDP 29170)
kjk join 192.0.2.20                        # join it
kjk join "Friend's Steam name"             # or through Steam: no port to forward (docs/NET.md)
kjk chain k1                               # the whole campaign, headless
```

## Controls

The defaults; every key rebinds in the options.

| key | does |
|---|---|
| mouse, W A S D | look, move |
| Space | jump (held while rising, a Force user jumps higher) |
| C or Ctrl | crouch; while running, roll |
| Shift | walk |
| left / right button | attack / the weapon's other mode (a feat or saber special, a throw) |
| E, mouse wheel | the target's action (open, talk, use, revive), and choosing it |
| 1 / 2 | right-hand / left-hand weapon |
| L | cycle saber style; in a co-op conversation that may be left, leave it |
| F, [ and ] | cast the selected power, select one |
| G / R / Q | grip / lightning / drain |
| T | grenade: hold to charge, release to throw |
| 3 / 4 / 5 | medpac / stim / energy shield |
| H | holster |
| B | switch weapon sets (TSL's second set, in both games) |
| V | next Form (TSL's lightsaber and Force Forms) |
| X / O / U | stealth / lay a mine / Security on the nearest lock |
| I J M P K | inventory, journal, map, party, abilities |
| Escape | the in-game menu |
| F5 / F9 | quick save / quick load |
| Tab | switch party leader |
| Z | take a break (co-op) |
| Y | ready to go at an exit (co-op); the host, while a join request's card is up and no ready check is: accept it |
| N | while watching, follow the next party member (co-op); the host, while a join request's card is up and no ready check is: end that conversation |
| F1 | first person / third person |
| Alt+Enter | borderless / windowed |
| 1–9 | a reply, in conversation |

Fighting in detail: [`docs/COMBAT.md`](docs/COMBAT.md).

## The repository

| path | what |
|---|---|
| `crates/vfs` | finding installs, archives, precedence, case-insensitive lookup |
| `crates/fmt_odyssey`, `crates/fmt_ja` | pure parsers for KOTOR's and JA's file formats |
| `crates/asset_odyssey`, `crates/asset_ja` | models, textures and animations as the runtime draws them |
| `crates/store` | the per-machine store of imported and derived data |
| `crates/collision`, `crates/collision_odyssey` | JA's trace model; KOTOR areas as collision levels |
| `crates/sim` | the deterministic simulation: every change to the world goes through `step` |
| `crates/script_ncs` | NWScript: the routine catalog and the VM |
| `crates/session` | module installs as transactions |
| `crates/net` | UDP transport, snapshots, prediction, lag compensation |
| `crates/saves` | saved games on disk |
| `crates/ui` | KOTOR's screens as draw lists and intents, no engine |
| `crates/audio`, `crates/movie` | sound placement and mixing; movies via `ffmpeg` |
| `crates/ragdoll` | active ragdolls, view-side only |
| `crates/console` | the `--cmds` queue that scripts a run |
| `crates/game` | the runtime (library `kjk_game`): window, rendering, the play view and screens |
| `crates/launcher` | the `kjk` binary: the CLI, verbs, playthroughs and probes |
| `crates/approved_tests` | the owner-approved tests, and nothing else |
| `xtask` | `cargo xtask`: the publish check, git hooks, two-process co-op runs |
| `data/` | our own data: clip brushes, encounter overrides, links, playthrough routes |
| `docs/` | how each part works and how to run it ([`docs/INDEX.md`](docs/INDEX.md)) |
| `ROADMAP.md` | goals, principles, decisions, where it stands and what is next |
| `QUIRKS.md` | reference behaviour reproduced on purpose, bugs included |
| `KNOWN-ISSUES.md` | what is known to be broken or unfinished, so it need not be reported |

## Working on it

[`CONTRIBUTING.md`](CONTRIBUTING.md) is the door: branches, the gate before
every commit, pull requests, the sim hash and the walkthroughs.
[`docs/INDEX.md`](docs/INDEX.md) maps the docs by area.

## License

kjk is MIT; see [`LICENSE`](LICENSE). Contributions are MIT too
([`CONTRIBUTING.md`](CONTRIBUTING.md), "Licensing").

It contains **no game data**, and no release does: it reads the games from
installs you own, and only ever reads them. Projects read as behaviour
references are credited in [`NOTICE`](NOTICE); no code from them is
included. Binary releases carry `THIRD-PARTY-LICENSES.html` for the crates
they are built from.

Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their respective owners; kjk is not affiliated with or endorsed by them.
