# Equivalence traces: `kjk trace`, `kjk trace-diff`

A package of the sim driven through its public API alone, one record a
step, so two builds of it can be compared exactly: today's code (the
oracle) against a rewrite that keeps the same signatures. The driver
(`sim::trace`) calls nothing a package keeps private, so the very same
driver builds against either. Both verbs come with the `probes` feature.

```bash
kjk --mute trace m1 scenarios/ --out m1.trace          # scenario files, or folders of them
kjk --mute trace m1 random --seed 1 --seeds 100 --n 10000 --out m1-random.trace
kjk --mute trace m3 random --seed 7 --emit m3-7.scn --out m3-7.trace
kjk --mute trace-diff oracle.trace new.trace           # exit 1 at the first difference
```

`--n` is the steps (queries, thinks) per seed, `--seeds` how many seeds in
a row from `--seed`. `--emit` writes the scenarios run as a scenario file:
a random run that diverges is kept as a named scenario that way (rename
it, trim it). `trace-diff` prints the first divergence: the scenario, the
step, the field, both values, and the ten records before it; it exits 0
when the traces are identical, 1 on a difference, 2 when a file cannot be
read.

## Packages

| pkg | what it drives | scenario directives |
|---|---|---|
| `m1` | `move_body` (and `carry`, `react`, `hold`, `begin_torso`, `view_of`, `stopping_distance`) in a test world, alone or with a Force pool for force jumps | below |
| `m2` | the saber move table: every row, kind, link, transition, chain angle, and `Blocked`'s order | `dump all` (or one part), `ask QUERY ARGS` |
| `m3` | `move_body`'s weapon step: the `m1` driver with a fight (saber, two, staff, fists, blasters) | as `m1` |
| `m4` | the saber rules' functions (clashes, power, defence, locks, kicks, the look, the sweep's geometry and swing, the lock's encoding) | `case KIND SEED`, `cases KIND COUNT SEED` |
| `m5` | the blade wielder's pacing (`Pace::decide`, `Pace::finish`) over many thinks, its encoding | `duel SEED THINKS [key=value]` |
| `m6` | the world's blades and the Force between players in a `SimWorld` (clashes, locks and their contests, kicks, throws, bolts at a blade; grip, push and pull, protect, drain) | below |

A package's frozen API, the list of what its driver touches, is kept with
its scenarios outside the repository (the clean-room's notes).

## The trace file

A header `#kjk-trace 1 <pkg>`, then for each scenario a line `== <name>`
and a record a step: its number and `key=value` words. Every float is its
bits in hex (`f32::to_bits`, eight digits), vectors three of them joined by
commas; integers are decimal, enums by name (their `Debug`), booleans 0 or
1, lists joined by commas (`-` when empty). A movement record holds the
step's inputs (`dt t a f r u b w fs aim`), every field of the player state,
the entities touched, and with a fight the saber's and the pool's state,
the random stream's state, how many numbers the step drew from it
(`draws`) and a digest of the fighter's whole encoding (`fenc`).

## Scenario files

Plain text, `.scn`. One directive a line, words split by spaces, `#` to
the end of the line a comment. `scenario NAME` starts a scenario; the lines
after it, up to the next, are its directives, run in order. A float may be
decimal or its bits (`0x3f800000`).

```text
scenario run_up_the_limit_ramp
world ramps                  # a test world from the library
anims varied                 # animation lengths: stand_in, varied, sparse
start -96 -64 24.125 0       # origin (units), yaw (degrees)
step 250 f=127               # 250 steps of 8 ms, forward held
step 40                      # 40 more, keys let go
```

The movement directives (`m1`, `m3`):

- `world NAME`: one of `void flat ramps stairs ledges ceilings wedges
  walls boxes crevices course`, all built from boxes, ramps and prisms in
  units (`sim::trace::geometry`): ramps at many slopes either side of the
  walkable limit, stairs either side of the step height, ledges up to 1024
  units, tunnels either side of crouching and standing room, corners of 15
  to 170 degrees, walls at many slants, crates with a non-walkable top,
  invisible walls of both kinds, actors' bodies, clefts. Shapes may be
  added: `box X0 Y0 Z0 X1 Y1 Z1 [solid|nonwalk|clip|walkclip]`, `ramp X0 Y0
  Z0 X1 Y1 Z1 x|y [down]`, `prism ZLOW ZHIGH X Y X Y X Y ...` (a convex
  polygon, counter-clockwise), `body ENTITY X0 Y0 Z0 X1 Y1 Z1`, `spawn` (a
  box random scenarios may start in).
- `start X Y Z YAW`, `time MS`, `set FIELD VALUES` (`origin velocity view
  face delta flags pm_time legs torso ground gravity base_speed maxs_z
  view_height jump_start_z movement_dir in_air bob slope_recalc use_delay`,
  and with a fight `fp hp weapon_ms hand combat_until target sheath`).
- A fight: `arms KIND [key=value]`, the hands' `none fists saber dual staff
  saber_fists pistol rifle pistols rifle_saber saber_pistol` (`jump fp pool hp flurry critical power two_weapon defense dueling
  proficient melee player`), `rng SEED STREAM`, `other ENTITY ANIM`, `enemy
  X Y Z|none`, `saber key=value` (the saber's state as the server leaves
  it: `on style move blocked blocking chain change hand hand_until getup
  quicker lock_until ...`), `force key=value`, `knockdown MS`, `standup`.
- Steps: `step [N] [key=value]` with keys `f r u` (axes, -128 to 127), `b`
  (buttons: a number or names joined by `+`: `attack alt walk use style
  ...`), `w` (weapon), `fs` (power), `msec` (default 8; extremes allowed,
  the step clamps them), `pitch yaw roll` (degrees) or `a=P,Y,R` (shorts),
  `turn` and `tilt` (degrees a step), `aim=X,Y,Z`. The command's angles
  carry over from step to step. `carry X Y Z VX VY VZ YAW LEGS [keys]` is a
  walker's step; `react ANIM torso|both`, `hold ANIM torso|both [MSEC]`,
  `start_torso ANIM SENT` and `query stopping_distance S|view_of P Y R DP
  DY DR|shrugs_off_pain|anims_digest` each record one line too. `arms`
  also takes `throw=LEVEL[:COST]`, `saber` `thrown=none|leaving|out|back|
  falling|lying`, `force` `hand_until=MS`; `anims` also `long` and `tiny`.

`m5`'s `duel` keys hold `Sight` fields think after think (`gap in_view
closing arms swinging grounded moving coming facing busy ally health smv
bounced broken parried pain foe`; `pain=+MS` after each think, `ally=flip`
a change of side every five thinks), and `approach=X` (the gap's change a
think), `step=MS`, `mask=N` (the clear sides), `scramble=1` (a made-up
pace), `sides=1` (keys of any size to `finish`), `aggression=N rated=N
roam=MS` (the pace's start).

The world directives (`m6`; `sim::trace::duels`): players on the arena's
floor, friendly fire full, posed on a skeleton made here (a torso turning
across the front through its frames, a hand, a blade, a staff's two) so
blades sweep and meet without game data.

- Before `begin`: `seed N`, `friendly off|area|full`, `anims
  full|sparse|without TEXT` (`without`: every animation whose name holds
  `TEXT` missing), `module COST|none` (a module with nothing placed pricing every power:
  the Force needs one), `fighter SEAT KIND [key=value]` (`fast medium
  strong dual staff pistol rifle ion fists`, `ion` a blaster whose bolts
  no blade turns; `x y z yaw defence jump throw
  tiers hp fp proficient powers=ROW,ROW`), `creature KIND
  [faction=hostile|friendly] [the fighter's keys]` (placed in the module,
  one made if none was asked for, fighting by its own mind, recorded as
  `c<N>`; the module's factions: hostile 1 and 3 at war with the players'
  0 and the friendly 2).
- `tick [N] [key=value]`: keys by seat, `f0 r0 u0 b0 t0 w0 p0` (the axes,
  the buttons `attack alt walk use style force grip lightning drain`
  joined by `+`, degrees turned from looking at the other, the weapon
  hand, the power row the force button casts); `until SEAT
  locked|unlocked|down N [keys]` ticks until the seat is so.
- Between ticks, the snapshot changed and the world made again from it:
  `lag SEAT MS`, `wound SEAT HP`, `place SEAT X Y Z YAW`, `saber SEAT
  key=value` (`on style move lock_until lock=none thrown`).

  Also `lock ATT DEF KIND` (a lock of `top diag_tr diag_tl diag_br diag_bl
  r l` started as a clash leaves one), and for a seat (`0`) or a creature
  (`c0`): `put WHO X Y Z YAW`, `toggle WHO absorb|protect LEVEL MS`, `stat
  WHO hp|fp N`, `force WHO key=value` (`hand throw_ready grip_ready
  drain_ready stunned immune`, ms from now), `knockback WHO MS`, `pose WHO
  ANIM MS`, `away WHO FLIGHT LEVEL`. `reason predict|authority` steps the
  ticks after as a client's prediction or the server. Tick keys also `l0`
  (degrees looked down, up negative) and `c0=N` (look at creature N);
  `creature` also `faction=ally`, `fighter` also `plot=1`.

A tick's record holds each seat's player state, saber state (lock and
thrown saber included), swing, Force state, trails, new events and a
digest of its fighter's encoding, the server's stream and its draws, and
the snapshot's hash.

## Coverage

What a trace run covers is measured on a build of the driver alone (the
`sim` crate's `trace` example, the same code without the game around it):

```bash
cargo llvm-cov run -p sim --features probes --example trace --no-report -- m1 m1.trace scenarios/
cargo llvm-cov run -p sim --features probes --example trace --no-report -- m1 m1r.trace random 1 100 10000
cargo llvm-cov report --summary-only
```

The example's words: `trace PKG OUT (random SEED SEEDS N | FILE...)`.
Branch coverage needs the compiler's unstable branch option (`--branch`
under `RUSTC_BOOTSTRAP=1`); it changes only that measuring build.
