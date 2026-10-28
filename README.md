<p align="center"><img src="docs/logo/kjk.svg" alt="kjk" width="160"></p>

# kjk

A Rust engine (Bevy, wgpu) that plays **KOTOR** and **KOTOR II** with
**Jedi Academy**'s real-time movement, sabers and Force, plus native co-op.
KOTOR 1 comes first.

It reads the games from installs you own. **No game data is included here or
in any release.**

## Install

You need your own installs of **KOTOR 1**, **Jedi Academy** and **KOTOR II**
(Steam and GOG are found automatically; others go in `.env`). `ffmpeg` on the
`PATH` is optional, for the movies.

- **Release:** download your OS's archive from Releases, unpack, run. Builds
  are unsigned: macOS needs "Open Anyway" in Privacy & Security, Windows
  "More info" → "Run anyway".
- **From source:** `cargo run --release -p launcher` (Rust is installed via
  `rust-toolchain.toml`; [`docs/BUILD.md`](docs/BUILD.md)).

The first start finds your games, imports what it needs and opens the main
menu.

## Co-op

Up to three players; guests play the host's companions and the host keeps the
save.

```bash
kjk host --stage end_m01aa                 # host (UDP port 29170)
kjk join 192.0.2.20                        # join by IP
kjk join "Friend's Steam name"             # or through Steam (source builds), no port to forward
```

In play: Z takes a break, Y says ready at an exit, N follows the next party
member while watching. More in [`docs/COOP.md`](docs/COOP.md).

## Controls

Every key rebinds in the options; fighting in detail is in
[`docs/COMBAT.md`](docs/COMBAT.md).

| key | does |
|---|---|
| mouse, W A S D | look, move |
| Space / C / Shift | jump / crouch or roll / walk |
| left / right button | attack / weapon's other mode |
| E, mouse wheel | use the target, choose the action |
| 1 / 2 | right / left hand weapon |
| L | saber style |
| F, [ ] | cast / select a power |
| G / R / Q | grip / lightning / drain |
| T | grenade (hold to charge) |
| 3 / 4 / 5 | medpac / stim / energy shield |
| H / B / V | holster / weapon set / Form |
| X / O / U | stealth / mine / Security |
| I J M P K | inventory, journal, map, party, abilities |
| Tab | switch party leader |
| Escape | menu |
| F5 / F9 | quick save / load |
| F1 | first / third person |
| Alt+Enter | borderless / windowed |
| 1–9 | dialogue reply |

## Status

KOTOR 1 plays from the Endar Spire to the credits, light and dark side.

- **World:** KOTOR's modules with JA's movement and camera, doors, effects, music, voice and movies.
- **Story:** the games' own scripts run on our NWScript VM: dialogue, journal, XP, party, items, stores.
- **Combat:** KOTOR's character sheets under JA's sabers, blasters and Force.
- **Screens:** KOTOR's menus and HUD in TSL's style.
- **Minigames:** pazaak, swoop racing, the Ebon Hawk's turret.
- **Saves:** from the menus, F5 / F9 quick save and load.

Not yet: TSL's campaign past Peragus. **Before reporting a bug, read
[`KNOWN-ISSUES.md`](KNOWN-ISSUES.md).**

## Development

```bash
kjk play --stage end_m01aa                 # one module (--class jedi for the Force)
kjk chain k1                               # the whole campaign, headless
```

| path | what |
|---|---|
| `crates/` | the engine, one crate per part (`sim`, `collision`, `script_ncs`, `net`, `ui`, `game`, `launcher`, …) |
| `data/` | our own data: rules, clip brushes, routes |
| `docs/` | how each part works ([`docs/INDEX.md`](docs/INDEX.md)) |
| `ROADMAP.md` | goals, decisions, what's next |
| `QUIRKS.md` | original behaviour kept on purpose |
| `KNOWN-ISSUES.md` | known bugs and unfinished parts |

Contributing: [`CONTRIBUTING.md`](CONTRIBUTING.md).

## License

MIT ([`LICENSE`](LICENSE)). No game data, ever; behaviour references are
credited in [`NOTICE`](NOTICE).

Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their owners; kjk is not affiliated with or endorsed by them.
