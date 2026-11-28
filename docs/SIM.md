# The sim funnel (`sim`)

The world changes only through `sim::step(world, tick, input, msec, reason)
-> Snapshot`, and between steps through `SimWorld::install` (a module's
commit, `docs/LIFECYCLE.md`). Authority, prediction and replay call `step`;
`StepReason::Predict` skips what only the authority runs (the placeholder
spawner, module transitions, scripts), `Replay` runs exactly what
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
  125 (8 ms, OpenJK's default movement step; Decision 3, `ROADMAP.md`). A rate that does
  not divide 1000 gets uneven ticks (60 Hz: 16, 17, 17). `Accumulator` turns
  wall time into due ticks, at most `MAX_CATCH_UP` (8) per frame, and gives
  the blend `alpha`: frames run at the display's rate and draw between the
  last two snapshots, so smoothness does not depend on the tick.
- `Snapshot`: plain values, one byte encoding (`KJKS` v79: characters
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
  met, from v65; the later steps are named in `sim::since`, the full list is
  `snapshot.rs`'s), BLAKE3 `hash()`.
  The modules left
  (`Stash`) change only at an install: shared (a snapshot's copy is a
  pointer), encoded and hashed once then; the tail carries their digest,
  their bytes end the encoding, and `hash()` covers the digest, not the
  bytes, so stepping, hashing and the network's frames cost the same
  however many modules were left. `split()` cuts the encoding into head,
  one fixed-length record per entity, tail and stash (`Parts`), what the
  network diffs; `Snapshot::join` decodes them back.
  `Presented` (last two snapshots, `alpha`) is what render, UI and audio get.
- `Save` (`KJKV` v72: TSL's influence, puppets, `DisableHealthRegen`,
  `SetOrientOnClick`, mines set on doors, the workbench's components and
  chemicals, the companions' base alignments, the forfeit conditions, the
  second weapon set and a queued `ActionSwitchWeapons`, the components and
  chemicals of creatures outside the party, queued casts and surrenders,
  state effects waiting, the Demolitions a mine was set with, the
  heartbeats `ForceHeartbeat` ran, the map jammed, the tutorial windows
  shown, the areas' transit, a queued bark, the UTE encounters as they run,
  a puppet's queued follow of its owner; the later steps are named in
  `sim::since`, the full list is `save.rs`'s): rate, the level it
  was made in (`LevelId`: name,
  digest), snapshot, and the script VM's state (`script::State`: globals,
  queues, conversations, goods, party, journal, XP, the goals and
  intents the scripts last gave the minds, which the next step acts on,
  and the world as the scripts' step last saw it, which its events are
  told from).
  The game keeps saves on disk with the `saves` crate
  (`UI.md`); a host's players keep a copy of each
  (`saves::copies`, `r0001`…).
  A save holds the canonical numbers of `spells.2da` and `feat.2da` rows
  (`rules::rows`, `docs/VFS.md`). A save made before rows by label, on an
  install whose mods made the map other than the identity, holds the
  install's numbers instead. Such a save is not migrated, so a character
  in it may know other powers and feats than it did. A save made before a
  mod was installed or removed may likewise name other rows, since the map
  depends on the install's tables.
- `Recording` (`KJKR` v39): the level, start snapshot, each tick's msec and
  input, a hash every 100 ticks and at the end, the installs'
  announcements (`KJKA` v4, with their seats and players); `play(level)` /
  `play_with(level, levels)` refuse another level and name the first
  checkpoint (or install) off.

## Rules every system keeps
The sim must be bit-identical on every platform (Decision 3, `ROADMAP.md`): co-op and
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
`kjk sim determinism` and requires its final hash to be the one pinned in
`bumps/SIM_HASH`: on Linux at every push to main and
pull request (`cargo test --release -p approved_tests`, no Bevy), on Windows and macOS
in the manual workflow, on the 1st of each month and when started by hand. When the sim changes on purpose, run
`kjk sim determinism` on one machine and put its `final` hash in both files
in the same commit; a change that moves the hash without meaning to is a
determinism bug. Locally, `kjk sim determinism --expect <hash>` checks it.

That run builds no level from the games' data, so the code that does is
guarded by clippy instead: every crate that decides sim state or a level's
digest (`sim`, `collision`, `collision_odyssey`, `session`, `script_ncs`,
`asset_odyssey`, `fmt_odyssey`) has a `clippy.toml` refusing the
platform's `sin`, `cos`, `tan`, `acos`, `atan2`, `hypot`, `powf`, `exp` and
`ln`, which round their last bits differently between C libraries. A
level built with them had another digest on Windows than on macOS, and
the Mac's saves were refused there. `collision::trig` is the portable
trigonometry; `sqrt` and `+ - * /` are exact everywhere.

## Render reads no authority state
Structural: outside `sim` the world, its components and `advance` do not
compile (E0616, E0603, E0624), and a `Snapshot` cannot be forged (E0451).
In `kjk`, `sim_host` keeps the `SimWorld` in a private resource and
publishes `PresentedSnapshot`, made of the snapshots the player's own
network client rebuilt (`solo`, docs/NET.md); `sim_view` draws from that.


## The step thread
A live game's frame hands every tick that is due to a thread of its own
(`game::sim_worker`), and the main thread draws while the sim steps. This
is the default since 2026-11-18
(https://github.com/kjk-engine/kjk/issues/395). It puts the view a frame
later, which is about 8 ms at 120 frames a second. The owner judged it in
a blind test on 2026-11-18. They played the same fight in Anchorhead with
it on and with it off. They felt the same responsiveness both ways, and
they noticed a higher frame rate with it on. On the owner's PC (an RTX
4080 and an i7-8700K), big fights ran 37% to 71% faster with it on. The
Jekk'Jekk Tarr cantina went from about 49 to 80 frames a second, the
Refugee Quad from 49 to 84, and Anchorhead from 51 to 70. Those fights
were measured before `kjk bench` could repeat a fight, so the fight with
the step thread on was not the same as the fight with it off.

`KJK_STEP_BATCH=off` turns it off for the run. Every frame then steps all
but its last tick itself, and hands only the last to the thread. Every
frame did this before 2026-11-18. At 40 frames a second that is 2 ticks
of every frame on the main thread.

With it on, a live game's frame does not step the world itself. The frame
makes the commands of every tick that is due, at most 8, and hands the
ticks with the world to the thread. The thread steps them in order
while the frame draws. The next frame takes the
world back first, and then records each tick, tells the session of it and
shows it, in order, as if it had stepped them itself. Whatever must read
the world sooner (a save, a dump, the session's end) waits for the thread
(`Authority::settle`).

- **What the player sees** is the time of the frame before, less one tick.
  The frame blends the two newest ticks by the clock of the frame that
  handed them over, so the view moves evenly whatever number of ticks each
  frame has. The view is one frame and 8 ms behind the clock, which is
  33 ms at 40 frames a second. With `KJK_STEP_BATCH=off`, a slow frame
  shows the ticks it stepped itself, 16 ms behind the clock.
- **A tick that holds the world** ends the thread's work: a tick that calls
  a break, or one that asks for a transition when the session installs
  modules. The thread steps no tick after it. The clock goes back to that
  tick, and the players' actions taken for the ticks after it wait for the
  next tick (`Solo::unstepped`).
- **A frame steps all but its last tick itself** when a tick's command
  depends on the tick before it, as every frame does with
  `KJK_STEP_BATCH=off`. This happens when a pilot or a minigame's bot
  drives, when a break holds at the frame's start, or when a transition
  failed to plan and the steps still ask for it. Only the last tick goes
  to the thread then. A host that players can join over the network does
  the same in every frame. Its server takes a guest's command for a tick
  when the tick's input is made, so the guests' commands keep the time
  they had to arrive.
- **A replay, or a time held by `anim`,** steps every tick in the frame, so
  the frame shows the moment it holds. A guest steps its own world beside
  its frames (docs/NET.md).
- A human player's keys are read once a frame, so every tick of a frame
  gets the same keys either way. One thing is decided once a frame:
  whether a shared Block and alternate attack button blocks, by what the
  character held when the frame began.
- A dump's `sim` fact counts the ticks stepped
  each way: `ticks_on_main_thread` and `ticks_on_step_thread`.

With `KJK_STEP_ASYNC=1` (while `KJK_STEP_BATCH` is not `off`), the main thread keeps the `SimWorld` checked into `game::sim_worker`, queues up to `MAX_CATCH_UP` (8) ticks, drains completed ticks without blocking, and interpolates between neighbouring landed ticks along a monotonic `ViewClock` delayed by one smoothed frame plus one tick rather than waiting in `Authority::settle`. Co-op hosts use the same async queue: the step thread pulls live guest commands right before each tick, publishes each finished snapshot directly into the shared server, and wakes `net_thread` immediately rather than holding the batch until the host's next frame settles. When leaving async queue mode (breaks, menus, holds, transitions, saves, or shutdown), it checks the world back out, processes any late or unstepped ticks in order so replays stay bit-exact, and slews smoothly up to settled time. Dumps then also report `step_async`, `wait_ms_total`, `wait_ms_max`, `wait_frames`, `frames_without_new_tick_while_due`, `view_held_frames`, `max_queue_depth`, `cap_hits`, `picture_age_ms_mean`, `picture_age_ms_max`, and `step_busy_share`, and `KJK_PERF=1` emits `sim.step` counters and `sim.wait` spans.

## Commands
```bash
kjk sim determinism [--ticks 10000] [--hz N] [--seed N] [--pilot N] [--expect HASH]
kjk sim probes [--only GROUP,CASE]   # groups: the list in crates/launcher/src/sim_probes.rs (containers, falls, party, dual pistols, …)
kjk sim record run.kjkr [--ticks N]  ;  kjk sim replay run.kjkr
kjk sim save s.kjkv --ticks 5000 --then 5000  ;  kjk sim load s.kjkv --then 5000
kjk sim rates [--hz 20,40,60,125] [--entities 8,256,2048]   # use --release
kjk sim skeleton [--saber single_1]   # blades vs Ghoul2 and the renderer (SKELETON-SIM.md)
kjk sim view [--replay file] [--headless --cmds 'wait 3s; dump; quit']
kjk play …   # the player on a stage (docs/MOVEMENT.md); same host
```
A recording made on an arena stage (`kjk play --stage arena@302nar`) is
replayed in the level that stage makes, because the module alone does not
make that level. `KJK_REPLAY_STAGE` names the stage for `kjk sim replay`.
`KJK_REPLAY_HOSTILE` names the creatures that `--hostile` turned against
the player when the recording was made, separated by commas. With
`KJK_REPLAY_TIMES` set to a file name, the replay also writes into that
file each tick's number and the microseconds since the tick before it,
one tick a line. A tick after a checkpoint also counts the time to hash
the checkpoint.
An `anim <seconds>` command holds the host's clock: it steps up to that
time, so a scripted `kjk play` shot shows the same moment every run.
Each writes `facts.json` into a run folder; `view` also writes the session's
`inputs.kjkr` and `end.kjkv` there, and puts a `sim` block in dumps.
