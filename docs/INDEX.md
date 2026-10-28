# `docs/`: the one-page map

One file per area: what lives where, the rules it keeps, and the `kjk`
commands that run and check it. Each opens with what a newcomer needs; keep
that part short, since nobody opens a long file twice. New here: the
repository's `README.md`, then `CONTRIBUTING.md`, then the file for the
area you touch.

| file | about | when to read |
|---|---|---|
| [`BUILD.md`](BUILD.md) | toolchain pin, Windows MSVC requirement, first build, running `kjk` | before your first build |
| [`RELEASE.md`](RELEASE.md) | the player's build (`cargo xtask dist`), its packages, the first-run setup, where a player's files go | before making a release, or touching the setup |
| [`BUG-REPORTS.md`](BUG-REPORTS.md) | F12's bug report zip (what it holds, where it goes) and `kjk report show` / `replay` | before opening a player's report, or touching what a report holds |
| [`RULEBOOK.md`](RULEBOOK.md) | every number the game's rules use and what it does, generated from `data/rules/*.toml` (`data/rules/README.md`); `kjk rules` adds the game data's | any number a rule plays by: change it there, never in code |
| [`COMBAT.md`](COMBAT.md) | characters, sheets, factions, bodies, the combat systems of a step, the damage pipeline; `kjk combat` | touching fighting, sheets or the damage numbers |
| [`FALLING.md`](FALLING.md) | party members downed at 0 HP, getting up when the fight ends, reviving by holding use, the whole party down as the loss and its box (I30 P11–P13); `kjk sim probes --only falls` | touching deaths, revives or what losing does |
| [`COLLISION.md`](COLLISION.md) | the level's collision: `Level`, traces, capsules, door states, how KOTOR areas become brushes, clip-brush files, the containment audit; `kjk collision` | touching collision, movement's traces, or a module's clip file |
| [`NAV.md`](NAV.md) | the navigation mesh baked from walkmeshes, routes, doors on the way, `sim::walk::Walker`; `kjk collision nav`, `kjk collision walk` | touching routes, walkers or AI movement |
| [`TRACE-SPEC.md`](TRACE-SPEC.md) | the brush clip every world shares, and how `BoxWorld`/`BrushWorld` combine brushes (the brush clip's own rules removed, pending a clean-room rewrite) | changing `collision::brush::clip` or a list world's trace |
| [`FORMATS.md`](FORMATS.md) | GFF, 2DA, TLK, SSF readers and writers; `kjk formats`, `kjk show`; what write-back results mean | a data file reads wrong, or a new format lands |
| [`FX-SPEC.md`](FX-SPEC.md) | saber ignition and lightning (`asset_ja::blade_lengths`, `fx::bolt`); the detailed spec removed, pending a clean-room rewrite | changing how a blade lights or a bolt is drawn |
| [`FX-PRIMITIVES.md`](FX-PRIMITIVES.md) | JA's effect primitives in our words: spawning on spheres and cylinders, particles, oriented particles, tails, cylinders, decals, runners, sounds, physics | changing how an `.efx` primitive spawns or draws |
| [`G2-SPEC.md`](G2-SPEC.md) | Ghoul2 playback: the ranges, picking up the other part's frame, the blend's start pose (`asset_ja::g2`); which frames a bone shows removed, pending a clean-room rewrite | changing frame timing, loops or blends |
| [`LIFECYCLE.md`](LIFECYCLE.md) | module installs: plan, commit, announcement; GIT objects as sim entities; doors, triggers, transitions, the loading screen; `kjk lifecycle` | touching module loading, transitions, doors, or what a client needs to join a module |
| [`PARTY.md`](PARTY.md) | the party as one: in combat, the break, the ready check; `kjk sim probes` | touching pause, area exits or what "in combat" means |
| [`LOOK-SPEC.md`](LOOK-SPEC.md) | the look (`asset_ja::angles`) and how the sim runs it; the detailed spec removed, pending a clean-room rewrite | changing how legs, spine or head turn |
| [`MOVEMENT.md`](MOVEMENT.md) | JA's player movement in the sim (units, what is in, trig), the view and third-person camera, `kjk move`, `kjk play` | touching movement, the camera, or a level's walkability |
| [`MOVEMENT-SPEC.md`](MOVEMENT-SPEC.md) | the slide and step moves; the detailed spec removed, pending a clean-room rewrite | changing `sim::motion::slide` |
| [`GROUND-SPEC.md`](GROUND-SPEC.md) | the ground trace, landings, standing up; the detailed spec removed, pending a clean-room rewrite | changing `move_body`'s ground, landing or crouch code |
| [`NET.md`](NET.md) | the network: layers, frames and messages, rates and rules, measured bandwidth and prediction cost; `kjk net` | touching the network, sessions, prediction or installs over a link |
| [`COOP.md`](COOP.md) | co-op over the network (I31): seats and the bodies guests drive, joining a module planned for more players, holds, save copies, reloads, idle commands; the co-op duos | touching seats, guests' bodies, or what a hold, save or reload sends |
| [`RAGDOLLS.md`](RAGDOLLS.md) | deaths as active ragdolls: humanoids by plan, beasts and droids fitted from their mesh (flesh, metal), joints, the death animation's hold and tiring muscles, the level's faces, sleeping; saber kills cutting parts off (meshes parted and seared); `kjk play --aim` | touching how bodies die, fall and are cut |
| [`PLAYTESTS.md`](PLAYTESTS.md) | playing modules end to end headless (`kjk playthrough`): walkthrough first, nav checks, routes of tags, god mode, segments, reading BLOCKED, fixing as engine rules | before a playtest run of an area |
| [`RUN.md`](RUN.md) | `--cmds` scripts and their commands, `--headless`, the run folder (log, manifest, trace) | before a scripted or headless run |
| [`EVENTS.md`](EVENTS.md) | objects' script events (spawn, heartbeat, triggers, doors, damage, death, user-defined) and action queues; `kjk script coverage` | touching what runs scripts or what `Action…` routines do |
| [`PAZAAK.md`](PAZAAK.md) | the card game: its rules from the game's data, the match in the sim, `PlayPazaak`, the three screens, the opponent | touching pazaak |
| [`MINIGAMES.md`](MINIGAMES.md) | the swoop races and the Ebon Hawk's turret: the ARE's `MiniGame`, the course in the sim, the `SWMG_*` routines, the view, `race` and `turrets` | touching a minigame |
| [`MODEL-FX.md`](MODEL-FX.md) | models' own effects: clips keying meshes' alpha and self-illumination, MDL emitters and their particles (`model_fx`); rooms' `animloop` clips and emitters, placeables' clips by their look and emitters (`own_fx`: Tatooine's sand, Manaan's birds, the Star Forge's captives); scripts' visuals on characters and objects (`script_fx`) | touching emitters, particles, what rooms and placeables animate |
| [`GRASS-WEATHER.md`](GRASS-WEATHER.md) | the area's grass (placed and drawn as the executables do, the grass option), TSL's rain, snow, lightning and thunder, the fog, the clear colour and which rooms are drawn, and what stayed `[verify]`; the commands to see each (`kjk module tsl …`) | touching grass, weather, lightning, or the particle limit and emitter orientations |
| [`SCRIPTS.md`](SCRIPTS.md) | NWScript: the NCS bytecode, the routine catalog from `nwscript.nss`, the VM; `kjk script` | touching scripts, the VM or the engine routines |
| [`DIALOGUE.md`](DIALOGUE.md) | conversations: DLG trees, the walk, its contract with the scripts; `kjk dialogue` | touching conversations or the routines they own |
| [`IMPORT.md`](IMPORT.md) | the engine's principles, the import step (installs into our own formats, once) and the store every worktree shares; `kjk store` | touching how game data is read, imported, cached or stored |
| [`WORLD-STATE.md`](WORLD-STATE.md) | I19: items, inventories, equipment, gold, locks and Security, the party table, the journal, XP and levels as scripts drive them; `kjk script items`, `kjk script party`, `kjk script security`, `kjk script journal`, `kjk script xp`, `kjk script levels` | touching items, inventories or what scripts keep about the world |
| [`SIM.md`](SIM.md) | the sim funnel: `step`, snapshots, inputs, RNG, tick rate, saves, replay; the rules systems keep; `kjk sim` | touching game state, saves, replay or the tick rate |
| [`SKELETON.md`](SKELETON.md) | JA animation on KOTOR characters: the Ghoul2 player, retarget, grip, saber; `kjk rig`, its measures, how to check a change | touching animation, retargeting or sabers |
| [`SKELETON-SIM.md`](SKELETON-SIM.md) | the sim's skeleton: hand and blade bolts as JA's server finds them, bit-identical everywhere (`sim::skeleton`), every divergence; `kjk sim skeleton` | touching where blades are for hits, or the sim's poses |
| [`SOUND.md`](SOUND.md) | the view's sound: files, how a sound is placed (JA's rules), slots, the window's device and the headless capture, `record`, `clip` | touching what sounds or how it is heard |
| [`EQUIVALENCE.md`](EQUIVALENCE.md) | equivalence traces of a sim package through its public API, floats as bits: `kjk trace`, `kjk trace-diff`, the scenario files and test worlds, coverage of a trace run | proving a rewrite of a package behaves bit for bit as the old one |
| [`TESTS.md`](TESTS.md) | the approved tests (`crates/approved_tests`): what each checks, running them, re-pinning the sim hash and jolt ceilings | before changing what they measure, or when one fails |
| [`UI.md`](UI.md) | the game's screens: layouts, fonts, the paint and the stack, view-models, the HUD, actions and notices over the network; `kjk gui`, the `ui` console verbs | touching a screen, the menu, the HUD or what the UI sends |
| [`VFS.md`](VFS.md) | finding installs, archive formats, KOTOR and JA precedence, `ls`/`extract`/`check`, the fingerprint | a resource resolves to the wrong file, a game is not found |

Living maps outside `docs/`:

| file | about |
|---|---|
| [`../README.md`](../README.md) | what the project is, its status, what you need, first commands, the crates |
| [`../CONTRIBUTING.md`](../CONTRIBUTING.md) | working in the repository: branches, the gate, pull requests, the sim hash, walkthroughs |
| [`../ROADMAP.md`](../ROADMAP.md) | goals, principles, decisions, where it stands and what is next |
| [`../QUIRKS.md`](../QUIRKS.md) | reference bugs we reproduce on purpose |
| [`../CONTEXT.md`](../CONTEXT.md) | the maintainer's workflow: journal, slices, reports |
| [`../AGENT.md`](../AGENT.md) | rules for agents working in the repo |
