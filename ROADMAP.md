# kjk — roadmap

A standalone runtime in Rust on Bevy and wgpu. It reads **KOTOR 1** and
**KOTOR 2: The Sith Lords** for the world, story, dialogue, quests, items and
party, and replaces KOTOR's round-based combat with **Jedi Academy**-style
real-time movement, saber and Force combat, using JA's own animations, sabers
and effects from the player's JA install.

- **KOTOR 1 comes first**; TSL follows on the same runtime, and every format
  covers both games.
- **Co-op is a core goal**: friends play the story together as the party.
  The architecture is multiplayer-shaped, so single player is a special case
  of it.
- **PvP is not a core goal**, but combat never assumes attacker and target
  are on opposite sides, so duels fall out of the same rules.
- The player points it at games they already own. Nothing from any game
  ships in this repository or in a release.

## Principles

kjk is its own engine. KOTOR's story, world and content and JA's
movement, sabers and Force come in as content and as rules to honour, not as
engines to rebuild ([`docs/IMPORT.md`](docs/IMPORT.md)).

1. **Our engine first**: simulation, networking, rendering, audio and tools
   designed as a modern engine would be; no old engine's limit copied unless
   the content needs it to work.
2. **Import once, run on ours**: the installs are read once into our own
   formats in a shared store; the engine runs from the store.
3. **Honour the content, not the engine**: old behaviour is reproduced
   exactly where content depends on it (what a script expects a routine to
   do, event order, the games' rule numbers); storage, loading, drawing,
   networking and timing are ours.
4. **Scripts: the original bytecode, our routines**: the games' compiled
   scripts run in a VM; every routine they call is a general engine system.
5. **One model for every game**: K1, TSL and JA content map into the same
   engine schemas; differences stay in the importer.
6. **Fast, measured, testable**: every claim cites a check, a sweep or a
   trace.

**Reference parity.** Where the games or other engines show how something
behaves, ours matches it, and that is proven rather than assumed: the
behaviour is written down in our own words, implemented from that
description (never translated from anyone's code), and compared on the same
inputs. Every divergence is named and approved. The games' own odd
behaviour is kept unless it is plainly broken (network feel, smoothness, a
mechanic that misfires); each kept or fixed quirk has an entry in
[`QUIRKS.md`](QUIRKS.md).

## Decisions

1. **Licence: MIT.** Reference projects are read for behaviour only; no
   code from them, adapted or not, is in this repository ([`NOTICE`](NOTICE)).
2. **Skeletons: the hybrid.** KOTOR bodies, heads, hilts and proportions
   stay as they are; JA's movement and combat clips are retargeted onto
   them ([`docs/SKELETON.md`](docs/SKELETON.md)); KOTOR's own clips serve
   dialogue, idles and cutscenes.
3. **Tick rate: 125 Hz** (8 ms ticks, `sim::TICK_HZ`). Rendering runs at
   the display's rate, blending the last two snapshots. The sim is
   bit-identical across platforms, so sessions and replays work between
   Windows, Linux and macOS ([`docs/SIM.md`](docs/SIM.md)).
4. **Co-op shape**: up to three players; guests drive the host's
   companions; the host writes the save and guests get a copy at every
   save ([`docs/COOP.md`](docs/COOP.md)).

## Where it stands

- **KOTOR 1** plays from the Endar Spire to the end credits, both endings,
  headless (`kjk chain k1`, 29 routes) and by hand.
- **Combat**: JA's movement, saber moves, blocks, clashes, locks and the
  Force under KOTOR's rules; blasters, grenades, minds and navigation.
- **Screens, minigames, saves and co-op**: see [`README.md`](README.md).
- **TSL**: every module loads and draws, and its systems are in; its
  campaign is played as far as Peragus.
- **Releases**: the player's build and first-run setup
  ([`docs/RELEASE.md`](docs/RELEASE.md)).

## Next

- **Join codes**: co-op over the internet with no port to forward, through
  a small self-hostable server that introduces peers (UDP hole punching,
  with a relay where that fails). Releases carry no Steam; builds from
  source can join through Steam ([`docs/NET.md`](docs/NET.md)).
- **The TSL campaign**, solo and co-op, past Peragus.
- **Platforms**: Windows, Intel Mac and Linux packages, the Steam Deck, a
  gamepad scheme for play, autosave, GOG's registry.

## Later, in any order

- JA maps as arenas, with KOTOR characters.
- An optional visual upgrade, comparable with the original look.
- New saber and blaster parts, powers and stances; duel challenges.
- Compatibility with override-folder mods.
- Tools: module inspector, script debugger, dialogue graph viewer, network
  session inspector.
- Spectator and theater mode from replays.

## Out of scope

- Loading or writing original save games.
- Running or patching the original executables.
- Compatibility with Jedi Academy's multiplayer protocol or servers.
- Public matchmaking with strangers, and anti-cheat.
- Shipping, linking or attaching any game data, in the repo, in issues or
  in releases.
