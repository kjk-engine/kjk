# The sim funnel (`sim`)

The world changes only through `sim::step(world, tick, input, msec, reason)
-> Snapshot`, and between steps through `SimWorld::install` (a module's
commit, `docs/LIFECYCLE.md`). Authority, prediction and replay call `step`;
`StepReason::Predict` skips what only the authority runs (the placeholder
spawner, module transitions, scripts from I16), `Replay` runs exactly what
`Authority` does. Players move by JA's movement (`sim::motion`,
`docs/MOVEMENT.md`), use doors and walk through doors and triggers
(`interact`); wanderers are the placeholder world's.

## Pieces
- `SimWorld`: a bevy_ecs world the crate keeps private (world, components,
  systems); built from a `WorldConfig` or from any `Snapshot`, in a `Level`
  (a name, the collision, JA's animation lengths and a module's objects:
  static data, not in snapshots; `digest()` names them; `Level::arena()` is
  the placeholder, walls and steps). A snapshot restores only in its own
  level (a typed `InstallError`).
- `TickInput`: one `Command` (JA's `usercmd_t`: view angles, axes, buttons)
  per `Seat`, kept in seat order, so arrival order cannot matter. Who sent
  it is not sim input. A seat without a command keeps its view. A world
  holds at most `MAX_SEATS` (64) players: tenancies keep a bit per seat.
- Randomness: two PCG32 streams saved in the state: `shared` (every step)
  and `authority` (authority-only systems), so prediction stays in step.
- Time: whole milliseconds. `TICK_HZ` in `rate.rs` is the one rate constant:
  125 (8 ms, OpenJK's default movement step; Decision 3). A rate that does
  not divide 1000 gets uneven ticks (60 Hz: 16, 17, 17). `Accumulator` turns
  wall time into due ticks, at most `MAX_CATCH_UP` (8) per frame, and gives
  the blend `alpha`: frames run at the display's rate and draw between the
  last two snapshots, so smoothness does not depend on the tick.
- `Snapshot`: plain values, one byte encoding (`KJKS` v65: characters
  (players and creatures) carry their `MotionState` and `Fighter`, players
  their `Sheet`, bolts their flight (docs/COMBAT.md); the module (`place`,
  with the players it was planned for), its objects' changing state
  (doors' states, which their collision follows through
  `collision::Level::with_states`; triggers' tenants), the step's
  transition ask, the modules left; the screen's fade scripts, the engine
  and conversations' lines set, `Fade`, `WorldFadeIn`; the video effect
  scripts enabled, `video_effect`, from v59; a status may be TSL's
  influence gained or lost, from v60; a lasting effect's time of taking
  hold, KOTOR's states and TSL's effects, Fury's kills and the state
  holding a fighter, from v61; sheets TSL's bonus Force
  points, base save bonuses and Form, fighters fake combat, a status the
  most Force points raised or lowered, from v62; a fighter its look
  scripts gave it for good, the progress the feedback floating over
  objects, the tail the in-game screens as scripts leave them, from v63; a minigame's course its jump key, jump speed, held speed and
  destroyed obstacles, from v64; the screens whether TSL's Bao-Dur has been
  met, from v65), BLAKE3 `hash()`.
  The modules left
  (`Stash`) change only at an install: shared (a snapshot's copy is a
  pointer), encoded and hashed once then; the tail carries their digest,
  their bytes end the encoding, and `hash()` covers the digest, not the
  bytes, so stepping, hashing and the network's frames cost the same
  however many modules were left. `split()` cuts the encoding into head,
  one fixed-length record per entity, tail and stash (`Parts`), what the
  network diffs; `Snapshot::join` decodes them back.
  `Presented` (last two snapshots, `alpha`) is what render, UI and audio get.
- `Save` (`KJKV` v52: TSL's influence, puppets, `DisableHealthRegen`,
  `SetOrientOnClick`, mines set on doors, the workbench's components and
  chemicals, the companions' base alignments, the forfeit conditions, the
  second weapon set and a queued `ActionSwitchWeapons`, the components and
  chemicals of creatures outside the party, queued casts and surrenders,
  state effects waiting, the Demolitions a mine was set with, the
  heartbeats `ForceHeartbeat` ran, the map jammed, the tutorial windows
  shown, the areas' transit, a queued bark, the UTE encounters as they run,
  a puppet's queued follow of its owner): rate, the level it
  was made in (`LevelId`: name,
  digest), snapshot, and the script VM's state (`script::State`: globals,
  queues, conversations, goods, party, journal, XP, the goals and
  intents the scripts last gave the minds, which the next step acts on,
  and the world as the scripts' step last saw it, which its events are
  told from).
  The game keeps saves on disk with the `saves` crate
  (`UI.md`); a host's players keep a copy of each
  (`saves::copies`, `r0001`…).
- `Recording` (`KJKR` v32): the level, start snapshot, each tick's msec and
  input, a hash every 100 ticks and at the end, the installs'
  announcements (`KJKA` v4, with their seats and players); `play(level)` /
  `play_with(level, levels)` refuse another level and name the first
  checkpoint (or install) off.

## Rules every system keeps
The sim must be bit-identical on every platform (Decision 3): co-op and
shared replays run between Windows, Linux and macOS machines.
- One thread, a fixed `chain`; anything order-dependent (random draws above
  all) visits entities by `SimId` or seat, never in storage order, which a
  restore does not keep, and never through a hash map's iteration order.
- No clocks, no thread timing, no platform or build-dependent values.
- A break holds the world inside the step: a paused step moves its tick,
  not its time (`world::party`), so `time_ms` no longer follows the tick.
- Floats only through `+ - * /`, `sqrt`, comparisons, rounding to a whole
  number (`round_ties_even`: one right answer) and exact bit operations,
  which IEEE 754 rounds the same everywhere; no `mul_add`, and no `sin`,
  `cos`, `atan2`, `powf`, `exp` from the platform library, whose results
  differ by OS. One portable trigonometry, `collision::trig`, serves the
  sim (re-exported as `sim::math`: movement's `AngleVectors`) and the
  level it moves in (turned doors and placeables, the slopes of the course
  and the measures); facings read from game data go through its
  `atan2_deg` (`session`, `collision_odyssey::turn`), never
  `Instance::facing`, which is the platform's and for drawing.
- `kjk sim determinism` (`sim::determinism`) catches a break on one
  machine (checks B to G); `--expect <hash>` checks that a machine reaches
  the pinned state. Its world starts in the arena (boxes, wanderers, the
  spawner) and after a third of the run installs `proving_ground`: a
  built `collision::Level` of triangles turned 34° some 550 m from the
  origin, with ramps, a door the pilot opens (brushes per door state) and
  a trigger that asks for a transition.

## Cross-platform check
The approved test `sim_determinism` (docs/TESTS.md) runs the same run as
`kjk sim determinism` and requires its final hash to be `KJK_SIM_HASH`,
pinned in `ci.yml` and `manual.yml`: on Linux at every push to main and
pull request (`cargo test --release -p approved_tests`, no Bevy), on Windows and macOS
in the manual workflow, on the 1st of each month and when started by hand. When the sim changes on purpose, run
`kjk sim determinism` on one machine and put its `final` hash in both files
in the same commit; a change that moves the hash without meaning to is a
determinism bug. Locally, `kjk sim determinism --expect <hash>` checks it.

## Render reads no authority state
Structural: outside `sim` the world, its components and `advance` do not
compile (E0616, E0603, E0624), and a `Snapshot` cannot be forged (E0451).
In `kjk`, `sim_host` keeps the `SimWorld` in a private resource and
publishes `PresentedSnapshot`, made of the snapshots the player's own
network client rebuilt (`solo`, docs/NET.md); `sim_view` draws from that.


## Commands
```bash
kjk sim determinism [--ticks 10000] [--hz N] [--seed N] [--pilot N] [--expect HASH]
kjk sim probes [--only GROUP,CASE]   # groups: containers, drivers, switch, coop talk, coop items, scaling, encounter count, falls, party, save copies, save refusals, minigame, galaxy and end, pazaak, pazaak view, feats, destroys, sheath
kjk sim record run.kjkr [--ticks N]  ;  kjk sim replay run.kjkr
kjk sim save s.kjkv --ticks 5000 --then 5000  ;  kjk sim load s.kjkv --then 5000
kjk sim rates [--hz 20,40,60,125] [--entities 8,256,2048]   # use --release
kjk sim skeleton [--saber single_1]   # blades vs Ghoul2 and the renderer (SKELETON-SIM.md)
kjk sim view [--replay file] [--headless --cmds 'wait 3s; dump; quit']
kjk play …   # the player on a stage (docs/MOVEMENT.md); same host
```
An `anim <seconds>` command holds the host's clock: it steps up to that
time, so a scripted `kjk play` shot shows the same moment every run.
Each writes `facts.json` into a run folder; `view` also writes the session's
`inputs.kjkr` and `end.kjkv` there, and puts a `sim` block in dumps.
