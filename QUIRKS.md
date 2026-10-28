# Quirks

Behaviour of a reference (OpenJK, reone, xoreos or the original games) that
looks like a bug, but that kjk reproduces on purpose. The rule is in
`ROADMAP.md` under "Reference parity": by default we keep a quirk, and we fix one
only when the owner approves it.

This file lists quirks in our own words. It never quotes reference code.

## Entry format

```markdown
### Q-0001 — short name

- Reference:  which one (OpenJK, reone, xoreos, K1, TSL, JA)
- Behaviour:  what the reference does, in our words
- Expected:   what a "correct" implementation would do instead
- Status:     kept | fixed (approved YYYY-MM-DD)
- Ours:       the crate and module that reproduces it
- Evidence:   the differential probe and the journal artifact that proved parity
```

Numbers are never reused. A quirk that turns out not to be one is marked
`withdrawn` with a reason; it is not deleted.

## Entries

### Q-0001 — PK3 load order folds letters to upper case

- Reference:  OpenJK (JA filesystem, both single player and multiplayer)
- Behaviour:  `base/*.pk3` are sorted by comparing names byte by byte after
  folding `a`–`z` to upper case and reading `\` and `:` as `/`. So `_`
  (after `Z`) sorts after every letter: `z.pk3` loads before `_x.pk3`, and
  `_x.pk3` wins.
- Expected:   a lower-case fold (as most case-insensitive sorts do) would put
  `_x.pk3` before `a.pk3`.
- Status:     kept
- Ours:       `vfs::ja::pak_order`
- Evidence:   I01.5 journal, `negative-ja.log`: load order
  `a < assets0 < B < bad < z < _x` on archives we built ourselves

### Q-0002 — Saber blade axis: single player and multiplayer agree

- Reference:  OpenJK (JA cgame, game and Ghoul2)
- Behaviour:  both read the blade's direction from the hilt's `*blade1` bolt.
  Single player takes the bolt's negative X axis, with a note that the GLM
  exporter turned the tag. Multiplayer asks for its negative Y axis, but
  multiplayer's `G2API_GetBoltMatrix` (the server's and the renderer's)
  turns the axes it returns a quarter unless `G2API_BoltMatrixSPMethod` was
  called, which the game and cgame never do: multiplayer's −Y is the raw
  bolt's −X, single player's axis. On `saber_1.glm` that axis runs up the
  hilt.
- Expected:   one axis for both, the one that runs along the hilt.
- Status:     not a divergence (this entry once said multiplayer's axis
  pointed sideways, from reading the axis name without the bolt query's
  turn; kept here so the number stays taken).
- Ours:       `launcher::viewer_ja::hilt` (the raw bolt's −X);
  `sim::skeleton` (the bolt query's turn, then −Y), docs/SKELETON-SIM.md
- Evidence:   I10.2b journal (the `*blade1` vertices and axes,
  `kyle-saber-fixed.png`); `kjk sim skeleton`: the sim's blade, turned as
  multiplayer's query turns it, agrees with Ghoul2's method in `f64` within
  0.2°.

### Q-0003 — Backward animation sequences start one frame past their range

- Reference:  OpenJK (JA cgame and Ghoul2, single player and multiplayer)
- Behaviour:  a sequence with a negative rate in `animation.cfg` is handed to
  Ghoul2 as running from `first + count` down to `first`, and Ghoul2 shows
  frames from the start down to one above the end. So it opens on the frame
  just after its range (the first frame of the next sequence in the GLA) and
  never shows its own first frame. Forward sequences show `first` to
  `first + count − 1`. Frame times are `ceil(1000 / fps)` ms, or `floor` for
  a negative rate, so a 132 fps sequence runs at 8 ms a frame.
- Expected:   a backward sequence shows the same frames as the forward one,
  last to first.
- Status:     reproduced only on request (`asset_ja::g2::Player::quirks`,
  `kjk rig --quirks`, `kjk play --quirks`).
  The stray frame is the first of the next sequence, and running from it
  into the sequence's own last frame can swing a bone half a turn in one
  frame: in the medium style's `BOTH_T2_BR_TR` the right thigh turns 150°
  in 34 ms. Multiplayer's cgame blends every change over 100 ms, which only
  softens it; single player does not blend backward sequences at all. By
  default a backward sequence shows its own frames, last to first (the
  owner's no-jank rule, 2026-09-30). `asset_ja::playback` still lists the
  game's frames.
- Ours:       `asset_ja::playback`, `asset_ja::g2::Player`
- Evidence:   I10.3 journal (`BOTH_T2_BR_TR`, 7 frames at −30 fps in the
  medium-style combo)

### Q-0004 — Ghoul2 weighs the captured blend pose's two frames the wrong way round

- Reference:  OpenJK (Ghoul2's bone animation setting and bone transform)
- Behaviour:  when a new animation blends in, the bone keeps the frame its
  old animation was on (say 10.3) and the next one (11.3). The frozen pose
  mixes the two whole frames with the fraction (0.3) on the first and the
  rest (0.7) on the second, so it sits nearer frame 11 than frame 10.
- Expected:   0.7 on frame 10 and 0.3 on frame 11.
- Status:     reproduced only on request (`asset_ja::g2::Player::quirks`,
  `kjk rig --quirks`, `kjk play --quirks`).
  Caught exactly on a frame, the frozen pose is the next frame, a whole
  frame ahead of where the body was, so every blend starting on a frame
  boundary jolts (3300 m/s² at the left hand when a swing starts on the
  run, 1300 without). By default the weights are the right way round.
- Ours:       `asset_ja::g2::Player::pose`
- Evidence:   I10.3 journal

### Q-0005 — A saber lights and goes out at a speed that depends on frame rate

- Reference:  OpenJK (JA multiplayer's gradual blade length, run once per
  client frame)
- Behaviour:  each frame the blade grows (or shrinks) by 0.01 × the
  milliseconds since it started moving, at least 0.2 units, after a first
  one-unit step. The step is per frame, not per millisecond, so a faster
  client lights its saber sooner (a 40-unit blade takes about 0.37 s at
  60 fps).
- Expected:   the same speed at any frame rate.
- Status:     kept, evaluated at 60 fps so every machine and every shot sees
  the same blade
- Ours:       `asset_ja::blade_lengths`
- Evidence:   I10.3 journal

### Q-0006 — The staff's leaping attacks leave the ground before their animations do

- Reference:  OpenJK (JA multiplayer's movement-driven attack choice and the
  back-flip attack start the move; the saber-move command logic drives
  it)
- Behaviour:  the butterflies set the player flying at once (190 units/s
  sideways; right 350 up, left 250) and the back-flip attack at 500 up,
  while their animations still stand and wind up. The body floats through
  the wind-up (the right butterfly about 1.9 m up), lands, and the
  butterflies hop again for the spin they animate (250 or 350 up, only if
  standing within a stretch of the legs timer). Landing does not interrupt
  them (special jumps).
- Expected:   the leap timed to the animation, as the dual attack and the
  forward staff attack wait for theirs.
- Status:     kept
- Ours:       `launcher::rig_timeline` (`Builder::driven`), `launcher::rig_ground`
  hops
- Evidence:   I10.5 journal

### Q-0007 — A thrown saber jumps back when it turns for home

- Reference:  OpenJK (JA multiplayer's thrown-saber first-throw and touch
  handling)
- Behaviour:  on the way out the saber's worked-out origin is refreshed
  only when it thinks, every 100 ms. When it turns back, its trajectory
  restarts from that origin, where it was at its previous think, and stays
  there for one more think: it jumps back up to 50 units (about 1.3 m at
  500 units/s) and hangs for 100 ms before flying home.
- Expected:   turning back from where it is.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): by default it turns back from where it is and flies home at once,
  with no hang; `quirks` keeps JA's jump back and hang. (The sim's thrown
  saber, `sim::combat::saber::throw`, always turned home at once.)
- Ours:       `launcher::saber_flight`
- Evidence:   I10.5 journal

### Q-0008 — A sped-up transition's move timer is not its played length

- Reference:  OpenJK (JA multiplayer's saber transition-animation start and final
  animation setting)
- Behaviour:  the fast style plays transitions at 1.5× and the strong
  style at 0.75×. The move timer adds `hold − hold × speed` to the hold,
  i.e. scales it by `2 − speed`, not `1 / speed`: a fast transition hands
  over to the next move after half its hold while it takes two thirds of it
  to play (cut three quarters of the way through); a strong one after 1.25
  of it while it takes 1.33.
- Expected:   the hold divided by the speed.
- Status:     kept (it is the fast and strong styles' rhythm)
- Ours:       `asset_ja::move_hold_ms`, `launcher::rig_timeline`
- Evidence:   I10.5 journal

### Q-0009 — Deaths start without a blend

- Reference:  OpenJK (JA multiplayer cgame's animation blending and its
  in-a-death-animation test)
- Behaviour:  every other animation change blends in (100 ms, 200 for
  flips); into or out of a death the blend is dropped, so the body snaps
  from whatever it was doing to the death's first frame (26 000 m/s² at the
  left hand going from the ready stance into `BOTH_DEATH1`).
- Expected:   a blend like any other change.
- Status:     reproduced only on request (`asset_ja::blend_ms`'s `quirks`,
  and a steered bake's `g2::Look::quirks`: `kjk rig --quirks`); by default
  deaths blend in over 100 ms
- Ours:       `asset_ja::blend_ms`
- Evidence:   I10.5 journal

### Q-0010 — A backward sequence's blend starts from a whole frame

- Reference:  OpenJK (Ghoul2's bone animation setting)
- Behaviour:  when a new animation blends in over one playing backward, the
  pose it starts from is the whole frame below where the old one was, not
  the pose it showed: the body jumps up to a frame ahead at the switch (15
  cm at the hand when the fast style's attack cuts into `BOTH_T1_BL_T_`,
  which plays backward at 1.5×).
- Expected:   the pose the bone was showing.
- Status:     reproduced only on request (`asset_ja::g2::Player::quirks`,
  with Q-0003 and Q-0004; `kjk rig --quirks`, `kjk play --quirks`); by
  default the shown pose
- Ours:       `asset_ja::g2::Player::set`
- Evidence:   I10.5 journal

### Q-0011 — The spine turns only 90% of the way to the view

- Reference:  OpenJK (JA multiplayer's client spine and neck angles)
- Behaviour:  the yaw between the legs and the view is shared up the spine
  as 45% (lower lumbar), 35% (upper lumbar) and 20% (thoracic), which the
  code says must add up to the whole. The neck's share is then averaged
  into the thoracic's: with no look target the neck adds nothing to the
  yaw, so the average halves the thoracic's 20% to 10%, and the upper body
  stops 10% short of the view (6° while strafing, the legs 60° off).
- Expected:   the whole yaw up the spine.
- Status:     kept (smooth, and small)
- Ours:       `asset_ja::angles::player_angles`
- Evidence:   I10.6 journal

### Q-0012 — Running backward diagonally, the legs turn by the keys, not the travel

- Reference:  OpenJK (JA multiplayer's player angles)
- Behaviour:  the legs turn toward the line the velocity runs along, and
  for the backward diagonals (`movementDir` 3 and 5, from the keys) the
  other way, so their back faces the travel. The flip follows the keys at
  once while the angle follows the velocity, which takes a fifth of a
  second to come round: pressing back-right while running ahead-left turns
  the legs 90° the wrong way and back again.
- Expected:   the flip once the velocity points behind.
- Status:     reproduced only on request (`angles::Moment::quirks`,
  `kjk rig --quirks`); by default the legs flip while the keys hold
  backward (directions 3 to 5) and the velocity points behind
- Ours:       `asset_ja::angles`
- Evidence:   I10.6 journal (`look`, back right from ahead left)

### Q-0013 — Legs whose aim jumps past 90° snap

- Reference:  OpenJK (JA multiplayer's swing angles for the legs' yaw)
- Behaviour:  the legs swing toward their aim at 1.3°/ms but are never left
  more than 89° behind it. Their aim jumps 120° when a strafe reverses (60°
  one side of the view to 60° the other, as the velocity passes zero), so
  the legs snap 31° in one frame, then swing the rest.
- Expected:   the swing the whole way.
- Status:     reproduced only on request (`angles::Moment::quirks`,
  `kjk rig --quirks`: 9 700 m/s² at the right foot in `look`'s strafe
  reversal, 2 000 without)
- Ours:       `asset_ja::angles`
- Evidence:   I10.6 journal

### Q-0014 — The spine's motion correction switches on and off at once

- Reference:  OpenJK (JA multiplayer's client spine angles)
- Behaviour:  while legs and torso play different animations, the spine is
  turned back by the torso animation's `Motion` bolt (the legs' pelvis
  against the one the torso animation was made on). The test is on the
  animations' names, so the correction vanishes in the frame the two
  become the same, while the pelvis still blends: the lower lumbar snaps
  16° when a swing on the run ends in the ready stance (9 100 m/s² at the
  right hand).
- Expected:   the correction easing out as the animations blend.
- Status:     reproduced only on request (`angles::Moment::quirks`,
  `kjk rig --quirks`); by default it eases in and out over 100 ms (the
  cgame's blend), the last correction held while it eases out
- Ours:       `asset_ja::angles`
- Evidence:   I10.6 journal (`showcase` at 4 020 ms)

### Q-0015 — Multiplayer's lightning never branches

- Reference:  OpenJK (JA multiplayer renderer's electricity surface and its
  bolt segments)
- Behaviour:  an electricity primitive flagged to branch (`usePhysics` in an
  effect file, as the force lightning's are) forks off side bolts only
  while a budget of forks lasts. Single player's renderer fills the budget
  (three) for each bolt; multiplayer's never sets it, so its lightning
  never forks.
- Expected:   the forks the effect files ask for.
- Status:     fixed (approved 2026-10-03, the owner): a branching bolt
  gets single player's budget of three forks; multiplayer's other rules
  (its 20-unit steps, the 0.94 draw, the width left) decide where they
  fall (docs/FX-SPEC.md, "Lightning").
- Ours:       `asset_ja::fx::bolt` (`Walk::wander`)
- Evidence:   I10.6 journal (`force` timeline's lightning); the fix's
  journal: `kjk rig a --moves force`, the lightning before and after

### Q-0016 — The third-person camera is pulled into the player under a low ceiling

- Reference:  OpenJK (JA multiplayer cgame's third-person view offset and
  its damping)
- Behaviour:  the camera's place is found by backing off from where its aim
  point would be with nothing in the way (16 units above the eye), and the
  trace from the aim point as it is (stopped under the ceiling) to that
  place runs into the ceiling. Crouching into a tunnel lower than the aim
  point brings the camera into the player's back for as long as the
  tunnel lasts, and the camera shows the inside of the body.
- Expected:   the camera backing off below the ceiling.
- Status:     reproduced only on request (`kjk play --quirks`); by default
  the camera backs off from the aim point as far as the ceiling lets it
  rise, level with it and then up or down (a ceiling stops the rise, not
  the way back)
- Ours:       `launcher::play_view` (`View::third_person`)
- Evidence:   I13 journal (the course's tunnel, 6 to 7.5 s)

### Q-0017 — A blend set while another is under way pops

- Reference:  OpenJK (Ghoul2's bone animation setting)
- Behaviour:  a new animation blends in from the frame the bone's previous
  animation is on, even while that animation is itself still blending in:
  the pose on screen (mostly the animation before) is dropped for a frame
  of the one barely started. The torso follows the legs a movement step
  later, so landing from a fall while running (the torso plays the landing
  for one 8 ms step, then the run) pops the arms: 68 cm at the left hand in
  10 ms.
- Expected:   the blend starting from the pose shown.
- Status:     reproduced only on request (`g2::Player::quirks`,
  `kjk play --quirks`, `kjk rig --quirks`); by default a blend set mid-blend starts from the
  pose shown
- Ours:       `asset_ja::g2::Player::set`; the sim's skeleton blends from the
  pose shown too (Q-0081)
- Evidence:   I13 journal (the course's 1.7 m drop, 4 390 ms)

### Q-0018 — The slope stances step one level, then jump to the rest

- Reference:  OpenJK (JA multiplayer's stand-animation slope adjustment)
- Behaviour:  standing on uneven ground picks a stance with one foot up by
  4 to 20 units. Changing stance was meant to step a level at a time, one
  level per 100 ms; but a move that is not yet due to step sets the stance
  wanted outright, so the stepping lasts one step. Counting runs through
  the list, so a change of side first steps the wrong way for that step.
- Expected:   a level every 100 ms, toward the stance wanted.
- Status:     kept (each change blends over 100 ms: no jolt)
- Ours:       `sim::motion` (`Move::stand_on_slope`)
- Evidence:   I13 journal (turning on the course's 30° ramp)

### Q-0019 — A capsule meets a brush's edges square

- Reference:  OpenJK (JA multiplayer collision model, a capsule's position
  test in a brush; Quake 3 sweeps the same way, see Q-0022)
- Behaviour:  each of a brush's planes is pushed out by the capsule's
  radius, and the tested point moved to the capsule's nearer sphere.
  Against a face that is exact; at an edge or corner the pushed planes
  meet square, so a capsule stops at a box's vertical edge exactly where
  the box it replaces would.
- Expected:   a round body would roll closer round the edge (by up to
  √2 − 1 of its radius running at it diagonally).
- Status:     kept
- Ours:       `collision::brush` (`Swept::Capsule`), through `Level::sweep`
- Evidence:   `kjk collision probes`, `capsule edge` (and `capsule slope`
  for the faces); I12 journal

### Q-0020 — A trace that stops short of a brush's bounds is not pulled back

- Reference:  OpenJK (JA multiplayer collision model, traces through
  brushes)
- Behaviour:  a brush is skipped unless the box the trace sweeps overlaps
  the brush's bounds. So a trace that ends in front of a face, closer than
  the clip epsilon (0.125 units) but not touching it, goes the whole way;
  a player can end up nearer a wall than the epsilon, and a shallow move
  just under a ceiling is not cut short.
- Expected:   Quake 3's rule, which the epsilon exists for: any trace
  ending within the epsilon of a face stops the epsilon short of it.
- Status:     reproduced in every collision world (`BoxWorld`,
  `BrushWorld`, `Level`: `collision::reaches`); decided 2026-09-30
- Ours:       `collision::boxes`, `collision::level`
- Evidence:   I12 journal, `jadiff` (OpenJK's own trace code run over our
  brushes): 4 to 6 in 20 000 random traces through boxes, 1 to 2 in 3 000
  through the Endar Spire

### Q-0021 — A trace stops looking once it has nowhere to go

- Reference:  OpenJK (JA collision model, both games)
- Behaviour:  brushes are tried one by one, and the first that leaves the
  trace no distance at all (fraction 0) ends the search. A later brush the
  trace starts inside is never tried, so `startsolid` and `allsolid`
  depend on the order the map's leaf lists brushes in.
- Expected:   every brush tried; start solid whenever the trace starts
  inside any brush.
- Status:     not reproduced: every brush is tried. KOTOR levels have no
  map brush order to match; decided 2026-09-30
- Ours:       `collision::boxes`, `collision::level`
- Evidence:   I12 journal, `jadiff`: 4 to 49 in 20 000 random box traces,
  2 in 3 000 through the Endar Spire

### Q-0022 — Multiplayer sweeps a capsule through brushes as its box

- Reference:  OpenJK (JA multiplayer collision model)
- Behaviour:  a capsule trace honours the capsule only when it tests a
  position (start equals end); swept through a brush it clips exactly as
  the box it was made from. Single player has no capsule traces at all.
- Expected:   the capsule the trace asked for (Quake 3 sweeps it, pushing
  the planes out by the radius from the nearer sphere).
- Status:     not reproduced: `Level::sweep` sweeps the capsule, as the
  position test and Quake 3 do; JA's game code never traces the world
  with a capsule. Decided 2026-09-30
- Ours:       `collision::brush` (`Swept::Capsule`)
- Evidence:   I12 journal, `jadiff`: OpenJK's capsule answers as our box in
  19 938 to 19 995 of 20 000 random traces, 2 979 of 3 000 through the
  Endar Spire

### Q-0023 — A trigger fires whatever the height

- Reference:  reone (KOTOR triggers: entering, leaving, module transitions)
- Behaviour:  a creature is in a trigger when its position, seen from
  above, is inside the trigger's outline; the height is never looked at.
  A trigger on one floor also fires for a player on the floor above or
  below it.
- Expected:   the outline as a prism of the trigger's height (or at least
  within a step or two of its points' heights).
- Status:     fixed (approved 2026-10-03, the owner): a trigger counts a
  character over its outline and on its floor there: of the walkable
  floors over the spot (the navigation mesh), the one nearest the outline's
  points' heights; the character's is the highest at or under its soles (a
  step's slack). A floor alone over a spot is the trigger's however high
  the outline floats (the Dantooine ruins' star map, 7.5 m over its floor)
  or far the ground dips under it (Tatooine's dunes, 5 m): a plain prism of
  the points' heights would have lost those. Off the mesh, the prism, 2 m
  (`TRIGGER_REACH_M`) under and over the points.
- Ours:       `sim::module::ObjectData::contains` (through
  `sim::Level::in_trigger`): `sim` `interact`, the scripts' enter and exit,
  arrivals (`install`), `kjk playthrough`
- Evidence:   `context/artifacts/2026-10-03-quirk-fixes/1-TRIGGER-VOLUMES-READY/`:
  every trigger of K1's and TSL's entry areas against the walkable floor
  under it (`shipped-triggers.txt`, 991 and 794 triggers): none has a
  second floor under its outline, so none changes by height; a plain 2 m
  prism would have lost the star map's trigger whole and parts of 13
  more. Floors made there (`trigger-floors.txt`): 12 of 12, the floor above
  and below left out. `kjk sweep all --skip render,audit` before and after
  differs in 9 of 199 modules, all by the outline (Q-0024; the same with the
  height test off).

### Q-0024 — A trigger's outline is a fan from its centre

- Reference:  reone
- Behaviour:  the outline is cut into triangles from the average of its
  points, and a point is inside when it is in one of them. For an outline
  that is convex, or that its centre sees whole, that is the outline; for
  any other concave outline some of it is missed and some outside is
  counted.
- Expected:   the polygon itself (an even-odd or winding test).
- Status:     fixed (approved 2026-10-03, the owner): the outline is the
  polygon itself, an even-odd test (`sim::module::inside_outline`, which
  the containment audit's exits use too).
- Ours:       `sim::module::inside_outline`, `ObjectData::contains`
- Evidence:   `context/artifacts/2026-10-03-quirk-fixes/1-TRIGGER-VOLUMES-READY/`:
  the fan is never smaller than the polygon on shipped outlines; it
  counts more of 44 of K1's 991 triggers and 20 of TSL's 794
  (`shipped-triggers.txt`: Tatooine's dune edges most, 208 000 of 274 000
  spots outside). The sweep loses 1 to 10 script runs in 9 modules
  (`sweep-diff.txt`), each a creature placed outside a concave outline but
  inside its fan entering at once (`trigger-runs-diff.txt`: the Endar
  Spire's `end_cut2_soldier1` in `end_reinfrc`, TSL's Vaklu and Talia in
  `tr_end_riiken`, …); the Endar Spire route still meets 57 of 57 goals,
  its fights shorter by that start (`endar-spire-route-*.txt`).

### Q-0025 — A door always opens the same way

- Reference:  reone and xoreos (xoreos marks it as not done)
- Behaviour:  opening a door always plays its first opening and takes its
  first open state (`opening1`, the `<model>1.dwk` walkmesh), from
  whichever side it is opened.
- Expected:   the door opens away from whoever opens it (its second way
  from the other side), as the two open walkmeshes suggest.
- Status:     fixed (approved 2026-10-03, the owner): a door opens away
  from its opener (a player's use, a walker running into it, a key or
  Security, a script's action with its creature): state 2 when the opener
  stands on the side its first way swings to, else 1. A door opens both
  ways when its open walkmeshes (`<model>1.dwk`, `<model>2.dwk`) stand on
  opposite sides of its closed one, each its middle 0.25 m or more off it,
  and its model has `opening2`; any other door, and one a script opens
  with no opener, takes 1. The view plays a way's clip, or the first where
  the model lacks it. Saves need no change (the snapshot's door state was
  already a byte holding 0, 1 or 2; the module digest names a door's ways
  only when it has two). No door model of K1 or TSL opens both ways (114
  models: each slides or vanishes in place; three slide their second
  opening a different length, none to another side), so every
  shipped door still takes 1.
- Ours:       `sim::module::ObjectData::opening` (`Ways`), `session::doors`
  (from the walkmeshes and clips), `sim` `interact`, `uses::use_object`,
  `world::locks`, the scripts' door changes; `launcher::lifecycle_view`
  (`doors`)
- Evidence:   `context/artifacts/2026-10-03-quirk-fixes/2-DOOR-WAYS-READY/`:
  the proving ground's door made two-way, opened from each side by a use,
  takes 1 and 2 as it should, one-way 1 from both, each state the same
  through a snapshot's bytes (`door-ways.txt`, 6 of 6); every K1 and TSL
  door model's walkmeshes and clips (`door-models.txt`, `door-sides.txt`:
  0 of 114 open both ways). `kjk sim determinism` keeps its pinned hash.

### Q-0026 — reone never loads a module's stores

- Reference:  reone (area loading)
- Behaviour:  the GIT's store list is never read, so a module's stores do
  not exist in the area (xoreos reads it).
- Expected:   stores exist like the other instances.
- Status:     not reproduced: stores are entities like every other GIT
  instance (the Endar Spire has none). Decided in I14.
- Ours:       `sim::module` (`ObjectKind::Store`), `session`
- Evidence:   read, not run: I14 journal

### Q-0027 — reone takes a door's and a trigger's tag from the template

- Reference:  reone (door and trigger loading)
- Behaviour:  the tag comes from the blueprint (UTD, UTT) and the GIT
  instance's own `Tag` is ignored; xoreos loads the blueprint and then the
  instance over it, so the instance's tag wins.
- Expected:   the instance's tag (its GIT struct is the instance).
- Status:     not reproduced: the instance's tag, else the template's
  (xoreos). A transition arriving by tag finds the door the GIT names.
  Decided in I14.
- Ours:       `asset_odyssey::module` (`ModuleIr::build`)
- Evidence:   read, not run: I14 journal; `end_m01aa`'s transition door is
  tagged `end_door10` in both

### Q-0028 — A backward loop's first wrap starts a frame lower than the rest

- Reference:  OpenJK (Ghoul2's frame timing model)
- Behaviour:  a looping animation playing backward shows, as it runs off
  the end of its range for the first time, a mix from the range's end
  bound toward its start frame; every later wrap mixes from one frame above
  the bound. With the reference's backward range (Q-0003) the bound is the
  sequence's own first frame, so only the first wrap ever shows it.
- Expected:   every wrap alike.
- Status:     reproduced only on request (`asset_ja::g2::Player::quirks`,
  with Q-0003's range; `kjk rig --quirks`, `kjk play --quirks`). By default
  a backward sequence runs over its own frames, one lower than the
  reference's, where the bound is the neighbour sequence's frame: every wrap
  mixes from the sequence's own first frame. Before 2026-09-30 the first
  wrap took the neighbour frame without quirks too (13 601 m/s² at the right
  hand at each wrap of `BOTH_WALKBACK_STAFF`, 463 since)
- Ours:       `asset_ja::g2::BoneAnim::timing` (docs/G2-SPEC.md)
- Evidence:   2026-09-30 anim-fixes journal (`kjk rig a --moves
  BOTH_WALKBACK_STAFF,BOTH_WALKBACK_STAFF,BOTH_WALKBACK_STAFF`; the timing
  probe against the earlier code)

### Q-0029 — A legs restart can pick up the torso's frame, then the torso restarts

- Reference:  OpenJK (multiplayer cgame's animation blending)
- Behaviour:  when the legs and torso play the same animation and the legs
  restart it (a new flip, or a restart flag), the legs pick up the frame
  the torso's bone shows, because the test meant for a speed change
  compares the new rate (frames per 50 ms, scaled) with the last set's bare
  multiplier: they differ for any sequence not at 20 frames a second, or
  scaled by a style. The torso's own restart is never picked up (its test
  cancels it), so it starts over from its first frame: the legs carry on
  mid-animation while the torso starts again, the "wobbly spine" the rule
  was written to prevent.
- Expected:   a restart starts both parts over (or picks up only on a real
  change of speed).
- Status:     reproduced only on request (`asset_ja::g2::Player::quirks`:
  `kjk rig --quirks`, `kjk play --quirks`); by default a restart starts
  over, as the torso's does. Found by the macOS fix pass's review
  (2026-09-30).
- Ours:       `asset_ja::g2::Player::set_part` (`restart_picks_up`)
- Evidence:   read in the reference; no differential probe yet.

### Q-0030 — The front of the torso's middle is "no location"

- Reference:  OpenJK (the hit location, the box method)
- Behaviour:  a hit whose direction from the body's centre lands in the
  middle band of the torso, in front and within ±0.3 to either side, falls
  through every case and reports no location; it deals × 1, as the chest
  does, but a death there picks among all the deaths at random
  (`BOTH_DEATH1` to `BOTH_DEATH25`). Ours picks among the nineteen the
  location rules use (`BOTH_DEATH1` to `BOTH_DEATH19`); the other six join
  when the sim's `Anim` gains them.
- Expected:   the chest.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): the front of the torso's middle is the chest, so a death there
  picks by the chest's rules. A hit with no point (a power's, a blast's) or
  at the body's very centre still has no location.
- Ours:       `sim::combat::location::locate`
- Evidence:   read in the reference (I23 rulebook quirk candidate 10); no
  differential probe yet.

### Q-0031 — A hit that harms nothing still pushes

- Reference:  OpenJK (damage dealing, multiplayer)
- Behaviour:  the knockback is worked out from the damage and added to the
  target's velocity before the checks that can stop the damage (a teammate
  with friendly fire off, a god-moded or protected target): a blocked hit
  pushes as hard as a harmful one.
- Expected:   the damage that cannot land to push nothing either (arguably;
  Quake 3, which JA inherits it from, says it is on purpose).
- Status:     fixed (approved 2026-10-03, the owner): a hit the share
  refuses (friendly fire off, a plot target, the party's on a neutral
  outsider) pushes nothing; one that may harm pushes by its damage before
  the share, as before.
- Ours:       `sim::combat::damage::assess` (the push from the located
  damage, only with a share)
- Evidence:   read in the reference (I23 rulebook 3.1, the pipeline's
  order); `kjk combat force k1`: 20 JA points shove a hostile, not the
  party's neutral outsider.

### Q-0032 — Lightning and drain leave from the feet's side of the body

- Reference:  OpenJK (the lightning and drain powers, multiplayer)
- Behaviour:  levels 1 and 2 trace their line from the caster's origin (the
  middle of its movement box), not its eye, along the view; the cone of
  level 3 is measured from the origin too. Aimed at a head from close by,
  the line leaves from the waist and can pass under a ledge the eye sees
  over.
- Expected:   from the eye, as aimed shots and the other powers are.
- Status:     fixed (approved 2026-10-03, the owner): levels 1 and 2 trace
  from the eye along the view, and level 3's cone (its reach and its
  front) is measured from the eye; the bolts are drawn from the hand as
  before.
- Ours:       `sim::world::powers` (`Cast::caught`, `Scene::around_from`)
- Evidence:   read in the reference (I23 rulebook 4.6.5, quirk candidate
  8); `kjk combat force k1` strikes a standing target 20 times a second,
  and, with Force Lightning (level 2), strikes one aimed at the shins
  from 100 units (from the waist the line meets the floor first).

### Q-0033 — Protect stops everything above its cap for nothing

- Reference:  OpenJK (damage dealing, Force Protect)
- Behaviour:  protect counts a hit only up to its level's cap (100, 200,
  400) for its share and its FP cost, then prevents the whole rest of the
  hit as well: a hit far above the cap is stopped almost entirely, paid as
  if it were the cap.
- Expected:   the share and the cost of the whole hit, or the rest let
  through.
- Status:     fixed (approved 2026-10-03, the owner): the share and the
  cost of the hit up to the cap, the rest let through.
- Ours:       `sim::combat::force::protect`
- Evidence:   read in the reference (I23 rulebook 4.6.10, quirk candidate
  3); `kjk combat force k1` checks 150 points on level 1: 40 prevented
  (JA: 90).

### Q-0034 — A timesynced roll never reaches its top

- Reference:  OpenJK (the time-synced random roll)
- Behaviour:  a roll between a and b is the floor of a + r × (b − a) with r below 1, so b itself never comes up; JA's random draw there also changes the command's server time as it draws.
- Expected:   every value from a to b.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): a synced roll comes up anywhere from a to b, both included; the
  server-time change is still not ours.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0035 — Readying from a pure attack returns the wrong way

- Reference:  OpenJK (the saber transition animations)
- Behaviour:  the transition to READY from a move that is a pure attack picks S_R2L whatever the attack's quadrant.
- Expected:   the return for the attack's own quadrant.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): readying from a pure attack takes that attack's own return.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0036 — Only the fast style counts as both parts playing

- Reference:  OpenJK (setting the saber move)
- Behaviour:  the check whether the legs play the same move, and the restart check, compare the move's unshifted animation with the playing one, which is shifted by style, so they match only for the fast style.
- Expected:   the style-shifted animation compared.
- Status:     kept (parity).
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0037 — A bounce never re-rolls to bottom left

- Reference:  OpenJK (the bounce chosen for an attack)
- Behaviour:  the random quadrant a bounce picks for the next attack comes from a range that stops one short, so the bottom-left quadrant is never chosen.
- Expected:   any quadrant.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): the re-roll covers bottom right to bottom left, bottom left
  included (with Q-0034's roll).
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0038 — Footsteps override a pain on the legs

- Reference:  OpenJK (the footsteps and pain handling in player movement)
- Behaviour:  the footstep code sets the legs' run or stand animation over a pain animation that is still playing on them.
- Expected:   the pain to play out.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): a pain on the legs plays out under a stand, a walk or a crouch;
  only running sets the run over it.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0039 — A bounce with no transition ends the move

- Reference:  OpenJK (the movement-driven attack choice and bounce handling)
- Behaviour:  an attack bounce whose table transition is none sets the move to LS_NONE instead of a ready or return move.
- Expected:   a return to ready.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): an attack bounce with no transition to the new quadrant returns
  from where it was, to ready.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0040 — Reacting to a block cancels a weapon change

- Reference:  OpenJK (the react-to-block path)
- Behaviour:  reacting to a block sets the weapon state to ready, which cancels a weapon being dropped.
- Expected:   the change to go on.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): reacting to a block leaves a weapon change under way alone.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0041 — The staff's back-flip attack is free

- Reference:  OpenJK (the staff back-flip special)
- Behaviour:  the special costs no force points, where the other specials of its kind do.
- Expected:   its share of the pool.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): it costs the forward and back specials' quarter of the pool, and
  needs it to start.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0042 — The dual jump attack starts in the air

- Reference:  OpenJK (the dual jump attack)
- Behaviour:  it starts while a jump is already under way, without waiting for the ground.
- Expected:   from the ground.
- Status:     kept (parity).
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0043 — A level-2 jump getup never happens after lying

- Reference:  OpenJK (the get-up checks in player movement)
- Behaviour:  after the 4 s lie time a getup with jump held and jump level 2 or more is checked, but holding jump already got the fighter up before that.
- Expected:   reachable.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): after the down time, jump with jump level 2 or more gets up with
  the acrobatic get-up (`BOTH_FORCE_GETUP_B3`, 300 units/s up); jump below
  that the force get-up, as before.
- Ours:       `sim::motion::saber` (the move step), `sim::combat::saber`
- Evidence:   read in the reference (I24 part 1 journal); `kjk combat
  saber` exercises the moves around it (299 checks).

### Q-0044 — A raging character cannot be killed by another

- Reference:  OpenJK (damage dealing, Force Rage, multiplayer)
- Behaviour:  a hit on a raging client from another client is halved, then
  divided by the rage level plus one, and cannot take its health below 1
  while rage runs: only rage's own cost (which stops at 1 and ends the
  rage) and world damage remain. The two stacked factors look unintended.
- Expected:   one reduction, and death by damage as ever.
- Status:     fixed (approved 2026-10-03, the owner): a character's hit
  on a raging one, player or creature, is divided by the rage level plus
  one (not halved first) and can kill. Rage's own cost still stops at 1
  HP and ends the rage; scripts' damage (`Source::Script`, no attacker)
  is not reduced.
- Ours:       `sim::world` (`hurt`), `sim::combat::force::ForceState::raging`
- Evidence:   read in the reference (I23 rulebook 4.6.9, quirk candidate 4);
  `kjk combat force k1` runs rage (its speed, cost and recovery), and
  the saber kills a target raging at level 1 with 1 HP (JA: its blows
  land for nothing).

### Q-0045 — A blade's extrapolated trace overshoots, and stays off for the frame

- Reference:  OpenJK (the server's saber sweep, rulebook I24 5.2)
- Behaviour:  when a blade's sweep is extended past its old point, the extra trace reaches a fixed 16 units from the old point rather than just past the new one, and once the extension switches off it stays off for every later step and chunk of that frame.
- Expected:   an extension measured from the new point, decided per step.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review, chosen over an extension past the new point after the
  before/after video): every trace of a sweep runs from its point's old
  place to its new one, all the way, so a cut lands only where the blade
  went.
- Ours:       `sim::combat::saber::sweep`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0046 — A sweep is cut by the size of its angle, not its direction

- Reference:  OpenJK (the server's saber sweep, the arc's chunks)
- Behaviour:  how finely a blade's arc between two frames is cut into traces is taken from the absolute cosine between the blade's old and new directions, so a swing past a right angle is cut as coarsely as a small one.
- Expected:   the signed angle, cutting a wide swing finer.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): a turn is cut into chunks when the directions' dot is below 0.67,
  signed, so a swing past a right angle is cut finer.
- Ours:       `sim::combat::saber::sweep`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0047 — Bolt defence falls to a level that reflects

- Reference:  OpenJK (bolt blocking, rulebook I24 7.5)
- Behaviour:  level-1 bolt defence drops to level 0 while rising or backpedalling, and level 0 reflects a bolt straight back at the shooter, whether it met the holder's body (a block by state) or its blade; so a defence-0 lightsaber reflects every bolt that strikes it, and a level-1 defender jumping or backing off reflects perfectly. (An earlier version of this entry said a level raised past the top by a deflection property wraps to 0; JA has no such property, and ours caps it at 3: `defence::bolt_level`.)
- Expected:   level 0 never turning a bolt.
- Status:     fixed (approved 2026-10-01, the owner): level 0 turns nothing by state (a defence-0 holder, or level 1 rising or backing off, takes the bolt on the body), and a bolt that strikes the blade itself at level 0 is deflected along the holder's view (as level 2), not reflected; reflecting at the shooter is level 3's.
- Ours:       `sim::combat::saber::defence` (`bolt_level_now`, `bolt`), `sim::world::sabers::bolt_block`
- Evidence:   `kjk combat duel --only bolts,blade_bolts`: level 0 backing off
  blocks none by state; a defence-0 saber's blade deflects 18 of 18 aimed
  at it, none back at the shooter, where defence 3 reflects them (saber
  defence journal, slice 2).

### Q-0048 — Half the clash tree is dead in practice

- Reference:  OpenJK (the saber clash decision, rulebook I24 8.2)
- Behaviour:  the branches of the clash decision that need an attacking blade's power below 3 never run, because an attacking blade's power is always 3 or more.
- Expected:   reachable branches for weak attacks.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): the dead branch (A deflecting off D's blade below power 3, which
  needs a true clash, always at power 3 or more) is gone. The weak-attack
  knockaway roll stays: a blow held for a lagging player's grace (C19) and
  blocked late reaches the tree after the attack ended.
- Ours:       `sim::combat::saber::clash`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0049 — When both attack, a tie knocks the swinger away

- Reference:  OpenJK (the saber clash decision, both attacking, rulebook I24 8.2 branch 7)
- Behaviour:  two attacking blades compare their power; a tie knocks the swinging side away, and the outcomes where one or both bounce can never be reached.
- Expected:   a tie bouncing both blades.
- Status:     kept (parity).
- Ours:       `sim::combat::saber::clash`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0050 — The deflection routine's quadrant arithmetic

- Reference:  OpenJK (the saber deflection routine, rulebook I24 8.3)
- Behaviour:  picking the quadrant a deflected blade goes to measures the first quadrant difference without mirroring it, its wrap for negative values does nothing, and it wraps below 0 with 7 (not 8) and not at all above 7; a result past the bottom plays move 0.
- Expected:   a symmetric difference wrapping with 8 both ways.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): the deflection's quadrants are measured the short way round both
  ways (D's end seen from A's side for both differences) and wrap with 8, so
  it never falls past the bottom.
- Ours:       `sim::combat::saber::clash`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0051 — A saber lock's delay before pushing is cleared at once

- Reference:  OpenJK (saber locks, rulebook I24 9.2)
- Behaviour:  a lock sets a one-to-three-second delay before either side may push, then clears it in the same step.
- Expected:   the delay holding.
- Status:     kept (parity).
- Ours:       `sim::combat::saber::lock`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0052 — A no-winner top-lock break knocks down one side

- Reference:  OpenJK (saber locks, rulebook I24 9.4)
- Behaviour:  when a top lock breaks with no winner, the side in the BF1 role plays a knockdown animation all the same.
- Expected:   both sides parting evenly.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): a top lock breaking with no winner plays the break on both sides;
  neither is knocked down.
- Ours:       `sim::combat::saber::lock`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0053 — The lock-loss disarm is almost unreachable

- Reference:  OpenJK (saber locks, rulebook I24 9.4)
- Behaviour:  the loser of a lock is disarmed only if it cannot be knocked down, which ordinary fighters always can.
- Expected:   a disarm chance for every loser.
- Status:     kept (parity).
- Ours:       `sim::combat::saber::lock`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0054 — A circle lock's loser forgets it was blocked

- Reference:  OpenJK (saber locks, the circle locks)
- Behaviour:  when a circle lock ends, the loser's blocked state is cleared along with the lock, so it recovers as if nothing had held its blade.
- Expected:   the loser kept blocked for its recovery.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): a circle lock's loser does nothing (its weapon timer held) until
  its broken move has played.
- Ours:       `sim::combat::saber::lock`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0055 — Air kicks all guess forward, and the spin kick's names disagree

- Reference:  OpenJK (kicks, rulebook I24 10.2)
- Behaviour:  a kick in the air takes its direction as forward whatever the movement keys say, and the spin kick's labels and the turns it makes do not match.
- Expected:   air kicks by the keys; matching labels.
- Status:     fixed (approved 2026-10-03, the owner's saber combat quirks
  review): air kicks go the way their animation (picked from the keys) goes;
  the spin kick turns clockwise 45° a slice, round the compass.
- Ours:       `sim::combat::saber::kick`, the step's `sim::world::sabers`
- Evidence:   read in the reference (I24 part 2 journal); `kjk combat
  duel` exercises it among its 38 checks.

### Q-0060 — A block by state is crushed by any stronger attack

- Reference:  OpenJK (the legacy block feeding the clash tree, rulebook I24 7.2 and 8.2 branch 8)
- Behaviour:  when a defender turns a blow by state (not attacking, facing it), the tree's blocking branch smashes its parry at once whenever the attacker's animation power and offence beat the defender's style and defence; the swing goes on and lands through the broken parry. Below defence 3 nearly every block against a proficient attacker is a broken parry: an idle defence-1 or defence-2 defender facing 13 strong medium swings took 10 hits, the block reading on screen as a hit.
- Expected:   a block holding the blow it meets.
- Status:     fixed (approved 2026-10-01, the owner): a block by state holds the blow it meets; the crush applies only where the blades truly met (a clash) or to a special. A second blow while the parry still plays still breaks it (the tree's fifth branch, JA's "a parry stops one hit"), so defence 1 still gives way to fast chains: 4 hits of 13, defence 2 and 3 none.
- Ours:       `sim::combat::saber::clash::resolve`
- Evidence:   `kjk combat duel --only blocks` (the check "holds every blow no clash crushed" at defence 2 and 3: blows after a true clash in the same swing may land); saber defence journal, slice 4 (`slice4-crush-variants.mp4`).

### Q-0070 — A player's own bolt shows only when the server's snapshot has it

- Reference:  JA (OpenJK multiplayer cgame)
- Behaviour:  the client predicts its own fire event (the muzzle flash and
  the sound play at once), but the bolt is a server entity: it is drawn
  only once a snapshot carries it, a round trip after the shot. With
  kjk's C18 fast-forward the host's bolt is also already a view lag
  along its way when it first shows (about 350 units at 150 ms), and a
  target nearer than that is hit before any bolt is drawn.
- Expected:   the bolt leaving the gun when the trigger is pulled.
- Status:     fixed (owner policy on net feel, 2026-10-01)
- Ours:       `launcher::shots` (the own bolts from the predictions, their
  handover to the host's copy, their end on a body or wall), drawn by
  `launcher::combat_fx`; the sim is unchanged.
- Evidence:   NET-FEEL slice 2 journal: the same scene before and after at
  80, 150 and 250 ms round trip, and the joined player's counts (own bolts
  drawn, handed over, met, unfired).

### Q-0071 — A player's own Force effects wait for the server

- Reference:  JA (OpenJK multiplayer)
- Behaviour:  a power is started by the server's game code, not by the
  movement code the client predicts, so the client's own player shows the
  power's effects (lightning from the hand, push's distortion, grip's
  puffs) only once a snapshot carries the power, a round trip after the
  key; the hand comes up late with them.
- Expected:   the effect at the key, as the cast's animation is.
- Status:     fixed (owner policy on net feel, 2026-10-01)
- Ours:       `launcher::force_view`, from the own character's predictions
  (`shots::OwnFire::own`); its effect on others stays the authority's.
- Evidence:   NET-FEEL slice 2b journal: push and lightning from the joined
  player's own view at 80, 150 and 250 ms round trip, and on main (none
  drawn).

### Q-0072 — A player's own block or hit starts late, from its first frame

- Reference:  JA (OpenJK multiplayer)
- Behaviour:  the server decides blocks, bounces and hits; the client's
  own player shows them when a snapshot carries the new animation, and
  starts it then from its first frame, a round trip after the server did,
  so the pose plays the verdict late and out of step with it.
- Expected:   the verdict's animation where the server has it.
- Status:     fixed (owner policy on net feel, 2026-10-01)
- Ours:       `cast::Figure::adopt` (Q-0083): the own figure plays its
  predictions' skeleton, whose parts start where the host started them,
  so a verdict a rewind brings shows as far in as the host has it.
  `launcher::play_view` counts the rewrites (`play.own_rewrites`). The
  ~100 ms the wrong animation was shown stays: the client predicts no
  verdict (I22 C20).
- Evidence:   NET-FEEL journal: the duel at 150 ms before, with slice 4's
  own start-in, and on the merged tree (`kjk join --trace`, the own
  body's jerk p95 0.184, 0.180 and 0.123 m; at most 0.905, 0.821 and
  0.446).

### Q-0080 — The server's skeleton shrinks a bone turned far between keys

- Reference:  OpenJK (Ghoul2's bone transform on the server)
- Behaviour:  a bone's two frames, and a blend's two poses, are mixed by
  lerping their 3×4 matrices value by value. A bone turned far between them
  shrinks (to nothing at half a turn) and its axes whip round; the hand's
  tag, built from three vertices skinned by the shrunken chain, then points
  the blade almost anywhere. Against the renderer's arcs the blade's tip is
  up to 30 units off between frames and 114 in a blend (162° apart), so the
  blade that cuts is not the one drawn.
- Expected:   bones turned along arcs at full size, as the renderer draws
  them.
- Status:     fixed (owner policy 2026-10-01, CHARACTER-LOOK): the sim turns
  frames and blends as `asset_ja::g2` does, with `sim::math`'s trigonometry.
- Ours:       `sim::skeleton::turn`, docs/SKELETON-SIM.md
- Evidence:   `kjk sim skeleton`: the sim is the renderer within 0.0005
  units on frames, between frames and blending (before: 30 and 114);
  CHARACTER-LOOK journal, `probe-skeleton.txt`.

### Q-0081 — The server's skeleton pops on a change mid-blend too

- Reference:  OpenJK (Ghoul2's bone animation setting on the server)
- Behaviour:  as Q-0017, on the skeleton the server finds the blades with:
  a torso animation played for one tick (a jump's flip replaced by an
  attack) makes the next blend start from that animation's own frames, and
  the blade swings about 114° in one frame and back over 150 ms.
- Expected:   the blend starting from the pose shown.
- Status:     fixed (owner policy 2026-10-01, CHARACTER-LOOK): a blend set
  mid-blend keeps the blend it began in (`skeleton::Under`), one level deep
  (a third change within the blend falls back to the frames).
- Ours:       `sim::skeleton::Playback::set`, `sim::skeleton::turn`
- Evidence:   CHARACTER-LOOK journal (the `BOTH_FLIP_F` tick in
  `1-solo-swings.mp4`); `kjk sim skeleton`'s "blending from a blend": 2.3
  units mean, 5.6 at most from the renderer, which keeps the whole pose.

### Q-0082 — The server's head looks at the player's position

- Reference:  OpenJK (the player angles, multiplayer game)
- Behaviour:  with no look target the server hands the player-angles routine the
  player's origin (a position) as the head's look angles. Clamped, a tenth
  of that "yaw" and roll is averaged into the thoracic's turn, so the arms,
  and the blades the server cuts with, turn by up to about 3° depending on
  where the player stands on the map.
- Expected:   the head looking level, as the cgame has it.
- Status:     fixed (owner policy 2026-10-01, CHARACTER-LOOK).
- Ours:       `sim::combat::saber::look`
- Evidence:   read in the reference; the look's spine and neck shares are
  the cgame's (`asset_ja::angles`, docs/LOOK-SPEC.md).

### Q-0083 — The blade drawn is not the blade that cuts

- Reference:  OpenJK (multiplayer cgame against the server)
- Behaviour:  the client animates every player its own way (blends of 100
  ms from the pose shown, the legs swinging after the view) while the
  server finds the blades on its own skeleton (150 ms blends, the legs at
  once): a hit can land where the drawn blade is not, and a drawn blade
  pass through a body without harm.
- Expected:   the drawn blade where the blade that cuts is.
- Status:     fixed (owner policy 2026-10-01, CHARACTER-LOOK): `kjk play`
  plays each living fighter's parts as the sim set them (`cast::Figure`,
  `g2::Player::adopt`) and turns its gripping hand onto the sim's blade (a
  reach of the upper arm and forearm, at most 30 cm at the wrist,
  `asset_ja::rig::Reach`). With `--quirks` the view keeps the cgame's way.
- Ours:       `launcher::cast`, `launcher::sim_blades`, `asset_ja::rig`
- Evidence:   `kjk play --sim-blades`: the drawn blade within 1 unit of the
  sim's on average and 0° apart (duel, solo), within 4 on the module's
  saber creatures; CHARACTER-LOOK journal.

### Q-0084 — A block by state judges where the blade met a box

- Reference:  OpenJK (the saber can-block test from the saber damage check, rulebook
  I24 5.3 and 7.2)
- Behaviour:  the block arc is judged at the hit point: in JA the point on
  the Ghoul2 model the blade's sweep met first (a per-polygon trace). The
  sim has no model to trace, only the player box (±15 units, about twice
  the body's width), so its hit point is where the sweep entered the box.
  With the blades posed where the server poses them (Q-0082), a medium
  top-down swing at arm's length reaches past the defender's middle and
  enters the box through a side face: behind the defender's middle when it
  faces the attacker (the blow lands, 5 of 11 at defence 2), ahead of it
  when it has turned away (a blow from behind blocked, 1 to 3).
- Expected:   a defender blocking the blows of those it faces, never one
  from behind.
- Status:     fixed (owner's delegation 2026-10-01, the merge of
  CHARACTER-LOOK and SABER-DEFENCE; owner policy: defence readable over
  faithful): a blade's arc is judged from where its attacker stands (the
  sweep and C19's held blows). The hit point still picks the parry and
  places the contact; bolts and thrown sabers keep their points.
- Ours:       `sim::world::sabers` (`contacts`, `turns`, `graces`)
- Evidence:   `kjk combat duel --only blocks`: from behind none blocked at
  any level; facing, defence 2 and 3 hold every blow no clash crushed.

### Q-0075 — A shot is heard three times over

- Reference:  OpenJK (JA multiplayer cgame's weapon firing and player weapon drawing, the effects' `sound` primitive)
- Behaviour:  a shot plays its weapon's fire sound in the weapon slot, and its
  muzzle flash effect is played on every client frame up to 30 ms after the
  shot; each flash plays the effect's own copy of the fire sound, nobody's,
  in no slot. At 60 frames a second a shot is heard three times at once
  (more at higher frame rates).
- Expected:   one fire sound a shot.
- Status:     fixed (approved 2026-10-03, the owner): a shot plays its
  muzzle flash once, at the shot, and its weapon's fire sound once, in the
  shooter's weapon slot; the flash's own sound primitive (its copy of the
  fire sound) is left out. The one sound plays at gain 1.4
  (`combat_fx::FIRE_GAIN`), as loud as JA's stack at 60 frames a second,
  at any frame rate.
- Ours:       `launcher::combat_fx` (`flash`, `place_flashes`); shots found
  by `shots::shots` (`launcher::shot_fx` draws the ends and beams)
- Evidence:   `context/artifacts/2026-10-03-quirk-fixes/3-ONE-FIRE-SOUND-READY/`:
  the player's Bryar fired five times on the course, recorded at 60 and 120
  frames a second (`shots.sh.txt`): `sound.asked` 15 fire sounds for 5
  shots before, 5 after, at both rates (`sound-asked.txt`). Loudness of
  the 150 ms after each shot (`loudness.txt`): before RMS 0.170 at 60 and
  0.223 at 120 (the stack grew with the rate), after RMS 0.125 at gain 1,
  0.176 at gain 1.4 at both rates. Videos `base-shots*.mp4`,
  `g14-shots*.mp4`.

### Q-0076 — Effect particles marked `usePhysics` fly through walls

- Reference:  OpenJK (JA multiplayer client effects: particle movement and the template flag parsing)
- Behaviour:  a particle collides with the level and bounces only when its
  template also says `expensivePhysics` (or `ghoul2Collision`); `usePhysics`
  alone does nothing, so the saber's blood sparks (`saber/blood_sparks_mp`,
  `usePhysics` with gravity) fall through the floor for their whole life.
- Expected:   `usePhysics` particles bounce off the level.
- Status:     fixed (approved 2026-10-03, the owner): a particle, oriented
  particle or tail with `usePhysics` (or `expensivePhysics`) collides with
  the level: each 60 Hz client frame's move is one point trace through the
  collision level (`Level`'s BVH, doors as they stand), done lazily as the
  particle is drawn and kept as legs of free flight, so its path is the
  same at any frame rate and under held time. Meeting a face it bounces,
  its velocity mirrored in the face and scaled by its own `bounce`
  (`elasticity`; blood sparks 0.3 to 0.5); leaving a floor slower than 10
  units a second it rests. A trace was chosen over a depth-buffer test: the
  level's traces are already at hand in the view, cost about a microsecond,
  and need no render readback. Bodies are not collided with.
- Ours:       `asset_ja::fx` (`Spawned::collide`, `Path`), called by
  `launcher::combat_fx` (`draw`, its impacts)
- Evidence:   `context/artifacts/2026-10-03-quirk-fixes/4-BOUNCING-PARTICLES-READY/`:
  a `kjk combat duel --only blocks` bout (saber defence 0) drawn back from
  a low camera: `blood-before-after.png` (the same moment: the sparks gone
  under the floor before, lying and hopping on it after) and the two
  recordings. Cost, the bout's 4 hits each forced to the heaviest blood
  (`saber/blood_sparks_mp` three times, 90 to 150 particles a hit):
  28 806 traces, at most 358 in a frame, the slowest frame's traces
  0.10 ms at 60 frames a second, 0.25 ms at 120 (the same 28 806 traces
  at both rates); the bout as played (its own sparks): 2 510 traces, at
  most 30 a frame, 0.04 ms.

### Q-0085 — A bolt leaves the shooter's eye, not its gun

- Reference:  OpenJK (the muzzle point, the cgame's missile drawing, the
  weapon ready and attack animation tables)
- Behaviour:  a shot starts at the muzzle point, a few units ahead of the
  eye along the view, and the cgame draws the bolt where it is; JA's own
  player model holds its gun up by the eye, so the two look the same. A
  KOTOR body holds the gun out at arm's length: drawn from that point, a
  bolt leaves from about the shooter's wrist, behind the gun. JA's one
  rifle pose, retargeted, holds KOTOR's longer rifles across the chest,
  tilted up and off the line of fire, the second hand short of the barrel;
  and the torso holds that pose while KOTOR has the gun put away.
- Expected:   the bolt leaving the barrel along it; a rifle held in both
  hands and aimed; no ready pose round a gun not shown.
- Status:     fixed (owner, 2026-10-01): each bolt is drawn leaving the
  gun's muzzle as drawn (the KOTOR model's `bullethook`), off its way by
  the difference and flying straight to its way 256 units out, where the
  barrel is aimed; the muzzle flash plays there too. A gun out is turned
  to that aim and kept upright, the other hand under it at the foregrip,
  each shot kicking as its blaster does; with it put away the torso
  plays the legs' part. Where bolts fly and hit is the sim's, unchanged.
- Ours:       `launcher::combat_fx` (`Path::launch`), `cast::Guns`,
  `cast::gun_aim`, `weapons::GunShape`, `asset_ja::rig::Aim`
- Evidence:   blaster animations journal: hold fire from the side, before
  and after, at quarter speed (`closeup-hold-fire-quarter-speed.mp4`);
  rifle animations journal: every rifle held and fired, moving, close.

### Q-0120 — The games name scripts they do not ship, and nothing runs

- Reference:  TSL (the shipped data), reone and xoreos (how a missing script
  is run)
- Behaviour:  TSL's objects and areas name event scripts that are in no
  layer of the install, the module's own archives included
  (`k_def_calospn` on Nar Shaddaa, `k_def_spawn_01`, the Dxun heartbeats
  `k_401_heart` and `k_402_heart`, `k_403_oh`). Asked to run one, both
  references find no compiled script and run nothing: reone's runner
  returns at once, xoreos's fails to open it, warns, and goes on. The same
  holds for `ExecuteScript` naming one.
- Expected:   a game that ships every script it names.
- Status:     kept: a missing script runs nothing, costs the step no run,
  and is no fault. It is counted apart (the script report's `missing`, the
  sweep's "scripts named but missing"), as a gap is to a player. A
  conversation's condition that is missing does not hold (xoreos; reone
  holds it), as before.
- Ours:       `sim::script` (`Report::program`, `Step::run_as`,
  `Engine::execute`)
- Evidence:   `kjk sweep tsl --only 30 --skip render,audit` (and `--only 35`,
  `--only 40`): no faults; the names under "Scripts named but missing".
  Journal `context/artifacts/2026-10-02-sweep-missing-scripts/`.

### Q-0124 — An arrival stands on the floor drawn, not on the walkmesh under it

- Reference:  reone (landing an object, testing elevation),
  xoreos (the party leader's height from the walkmesh)
- Behaviour:  an arriving party member is set on the topmost walkable
  walkmesh face under its x and y, looked for from far above; the entry's
  own height is not used, and drawn meshes do not collide. K1's swoop
  track (`tat_m17mg`) puts its entry on a walkmesh that lies half a metre
  under the track drawn over it.
- Expected:   the same, where the level is the walkmesh only.
- Status:     not reproduced: our level is solid where the rooms are drawn
  (I12), so a player standing on that walkmesh would start inside the
  drawn floor. The arrival's drop starts a step above the spot, and where
  no spot near it leaves room, from a step higher, up to four steps
  (1.8 m): the player stands on the floor drawn above the entry. A drop
  from far above is not taken, as it would land on roofs.
- Ours:       `sim::install` (`spots`)
- Evidence:   sweep arrival-search journal: `kjk sweep k1 --only
  tat_m17mg` before (`no room to stand near (88.69, 286.44, 0.00)`) and
  after (installed); `kjk collision trace k1 tat_m17mg` shows the
  walkmesh at z 0 and the drawn floor at z 0.5 there.

### Q-0140 — No one leaves the walkmesh, not even by jumping

- Reference:  K1, TSL (reone, xoreos)
- Behaviour:  a character only ever steps to a point whose topmost
  walkmesh face is walkable; the outline of the walkable faces stops
  everyone, whatever is drawn beyond it. KOTOR has no jumping, so that is
  also where its world ends.
- Expected:   a JA player jumping onto furniture, ledges and terrain off
  the walkmesh where it is safe to stand.
- Status:     kept (owner, 2026-10-02): walls along the walkmesh's open
  edges reach to the sky (or a walkable floor overhead), so neither
  walking nor jumping gets past them. Known follow-up: open the world up
  for jumps (lower walls, or jump-only openings, with clip files where a
  jump would leave the level).
- Ours:       `collision_odyssey::edges` (`Contents::WALK_CLIP`, which only
  walkers' moves meet: the camera, shots and sight lines pass)
- Evidence:   sweep walk containment journal
  (`context/artifacts/2026-10-02-sweep-walk-containment`): audits before
  and after, `danm14ac-river-walk.gif`, `danm14ac-bank-camera.gif`.

### Q-0136 — Droids built with `pause1` stand frozen as creature models

- Reference:  reone (the pause animation lookup)
  and xoreos (the default animations)
- Behaviour:  a creature model (`appearance.2da` model type S or L) is asked
  only for its `c…` clips: `cpause1`, `cwalk`, `crun`, `cdie`. TSL's tank
  droid (`c_tankdroid`, S), construction boss droid (`c_condrdboss`, L)
  and large mine droid (`c_drdminelg`) ship their idle only as `pause1`
  and `pause2`, beside `c…` motion clips, so both references hold them in
  their rest pose. The game's own models would not carry an idle it never
  plays.
- Expected:   those droids idle.
- Status:     fixed (divergence, for the owner to confirm): a creature
  model whose supermodel chain lacks the `c…` clip plays the shared name
  (`pause1`, `walk`, …). Every other creature model of both games is
  unchanged: none carries both names. A model with neither (the turrets
  `c_turret01`…`03`, `stuntsaberr`) has no idle; it stands at rest and is
  noted, not reported as a missing clip.
- Ours:       `launcher::viewer` (`creature_clip`), `launcher::clips`
- Evidence:   `context/artifacts/2026-10-02-sweep-creature-idles/`

### Q-0137 — A unique appearance row draws nobody in armour

- Reference:  reone (the body model name) and xoreos
  (the part models)
- Behaviour:  a character row (model type B) gives its body as
  `model<letter>`, the letter from the armour's body variation; an empty
  cell is an empty model name and nothing is drawn. TSL's unique rows fill
  only `modela`/`texa`: Gerevick (row 628, 602dan, heavy armour `g`) would
  be invisible, though the game shows him; a Malachor dark Jedi template
  (`g_darkjedi006`, row 564, robe `n`) is the same case.
- Expected:   drawn in the row's one body.
- Status:     fixed (divergence, for the owner to confirm): an empty
  `model<letter>` uses the row's `a` columns, model and texture (with the
  armour's texture variation, falling back to `01` as before). In both
  rows `race` names the same model, so `race` would draw the same body.
- Ours:       `asset_odyssey::creature` (`resolve`)
- Evidence:   `context/artifacts/2026-10-02-sweep-creature-idles/`

### Q-0132 — xoreos puts on a body texture the game does not have

- Reference:  xoreos (KOTOR and TSL creatures), against reone
- Behaviour:  a creature's body texture (`appearance.2da` `racetex`, or
  `tex<v>` and its variation) replaces every body mesh's own texture,
  whether or not the install has it. TSL names `n_talia` and `n_gand`, K1
  `n_comm_low_f1`, which neither game ships: every body mesh then has no
  texture that loads and is not drawn, leaving a head or weapon floating.
  reone puts the body texture on only once it loads; otherwise the model
  keeps its own (`N_taliaA`, `N_gand_D`).
- Expected:   the body drawn with its model's own textures, as reone does
  (Talia and the Gand are seen in the games).
- Status:     not reproduced: reone's rule.
- Ours:       `asset_odyssey::creature::resolve` (`missing_body_texture`),
  `launcher::viewer::module_scene`
- Evidence:   read, not run: sweep-textures journal
  (`context/artifacts/2026-10-02-sweep-textures/`), TSL 506ond's Talia
  and 352nar's Gand before and after.

### Q-0133 — reone does not draw a mesh whose diffuse is missing

- Reference:  reone (mesh drawing), against xoreos
- Behaviour:  a mesh whose diffuse texture does not load is skipped, even
  when its lightmap loads. xoreos draws whatever of the two loads, so such
  a mesh shows its lightmap alone; both skip a mesh none of whose textures
  load. TSL room and placeable models name K1 textures TSL does not ship
  (`lda_wall02`, `lta_blast03`, `dantflag`, `mal_rk01aa`), on lightmapped
  meshes: reone leaves holes there.
- Expected:   the surface shown, lit, rather than a hole in a wall.
- Status:     not reproduced: xoreos's rule (`launcher::scene`, `mesh`).
- Ours:       `launcher::scene` (`Spawner::mesh`), `launcher::viewer::stand_in`
- Evidence:   read, not run: sweep-textures journal; the sweep's missing
  textures list says what is drawn in each one's place.

### Q-0090 — A loud moment clips

- Reference:  JA (its mixer adds every channel and clamps the sum), and
  ours before I20
- Behaviour:  sounds that add up past full scale are cut flat: the Endar
  Spire's fight clipped 255 samples in 18 s with no music, over 1,000 in
  30 s once the battle music played under it.
- Expected:   loud moments held down, not distorted.
- Status:     fixed (owner policy 2026-10-01, objectively bad): the mix
  goes through a fast limiter (−3 dB, 20:1, 0.1 ms attack, 150 ms
  release) after its volume; the same fight peaks at −0.07 dB, none
  clipped.
- Ours:       `audio::mixer` (`Mixer::new`)
- Evidence:   `context/artifacts/2026-10-02-i20-audio/` (the fight's
  `audio.wav` measured before and after)

### Q-0091 — xoreos takes a generic door's sounds from the wrong row

- Reference:  xoreos (KOTOR doors)
- Behaviour:  a door's `SoundAppType` is read from `genericdoors.2da` at
  the row of its appearance, not of its generic type; KOTOR's doors have
  appearance 0, so every generic door sounds like row 0.
- Expected:   the row of its generic type, the row its model comes from.
- Status:     not reproduced: the generic type's row (owner policy
  2026-10-01); `doortypes.2da` by a non-zero appearance as xoreos.
- Ours:       `asset_odyssey::sound::area`
- Evidence:   read, not run: I20 journal; the Endar Spire's door plays
  `dr_lhr01_open`.

### Q-0092 — xoreos plays the night ambience at full volume

- Reference:  xoreos (KOTOR areas)
- Behaviour:  the night ambience's volume is read from a field the GIT
  does not have (`AmbientSndNightVol`; the GIT's is `AmbientSndNitVol`),
  so it is always the default, 127.
- Expected:   the GIT's `AmbientSndNitVol`.
- Status:     not reproduced: the GIT's field.
- Ours:       `asset_odyssey::sound::area`
- Evidence:   read, not run: I20 journal.

### Q-0093 — Footsteps follow the gait, not the animation's events

- Reference:  KOTOR (reone reads it, unwired; xoreos has none)
- Behaviour:  a footstep is an event of the walk and run animations
  (`snd_footstep` at its time in the clip).
- Expected:   a step where a foot lands.
- Status:     different by design: our characters move and are drawn by
  JA's gait (`move_body`), not KOTOR's clips, so a step is when its footstep
  cycle passes a quarter (Quake's footstep rule), 3 a second running,
  1.5 walking. The sound is KOTOR's: `footstepsounds.2da` by appearance
  and floor.
- Ours:       `launcher::ambience` (`footsteps`)
- Evidence:   I20 journal, the Taris walk video.

### Q-0086 — The turret's loss hands every fighter but the first 2000 HP

- Reference:  K1's own `k_pebo_hawkhit` (`m12ab`, the hull's `OnDamage`)
- Behaviour:  once the hull falls below 2000 it sets the fighters' hit
  points with `SWMG_SetFollowerHitPoints(SWMG_GetEnemy(n), 2000)` for `n`
  from 1 to `SWMG_GetEnemyCount()`: the first fighter (`n` 0) is left as it
  is, and the last call names no fighter.
- Expected:   every fighter set, so none dies during the game-over fade.
- Status:     kept (the game's script runs as it is)
- Ours:       `script::minigame` (`GetEnemy` past the last is no object)
- Evidence:   the script as the game ships it

### Q-0087 — The swoop's wind and reflux sounds are never heard

- Reference:  K1's swoop scripts (`heartbeat`, `reflux`, all three tracks)
- Behaviour:  they set the volume of, and play, the sound objects tagged
  "Wind" and "Reflux", which no swoop module's GIT places: the calls act on
  no object.
- Expected:   a wind louder with speed, a reflux on a boost.
- Status:     kept
- Ours:       the scripts' sound routines (no object, nothing played)
- Evidence:   `context/artifacts/2026-10-03-minigames/` (GIT dumps)

### Q-0141 — reone's animated camera never takes a line's field of view

- Reference:  reone (the animated camera's field of view)
- Behaviour:  the setter returns early when the new field of view differs
  from the current one, so the animated camera keeps its default, 55
  degrees, whatever a line's `CamFieldOfView` says (the Endar Spire's
  opening asks 34.5).
- Expected:   the line's `CamFieldOfView`, 55 when it is 0.
- Status:     not reproduced: the line's (owner, 2026-10-04).
- Ours:       `launcher::play_view::animated_shot`
- Evidence:   read, not run: `context/artifacts/2026-10-04-dialogue-cameras/`.

### Q-0142 — A mine laid in combat with a badly failed check is laid anyway

- Reference:  K1's executable, its set-trap action (read for behaviour)
- Behaviour:  laying a trap kit is d20 (take 20 out of combat) +
  Demolitions (+2 with five ranks) against the row's `setdc`. A failure
  makes no mine, except in combat when the sum falls more than 10 short:
  then the mine is made all the same, with DCs from that low sum.
- Expected:   a failure makes no mine (or springs it, as a failure on an
  object does).
- Status:     kept
- Ours:       `traps::lay`
- Evidence:   `context/artifacts/2026-10-04-stealth-traps/` (the reading)

### Q-0155 — Garouk's pazaak lesson ends with no word from him

- Reference:  K1's Upper City cantina (`tar_m02ae`): Garouk's lesson's end
  script `k_ptar_garouktlk`
- Behaviour:  it starts his conversation again through the object tagged
  "Garouk21", but he is "Garouk021": no object answers, and the lesson
  ends in silence.
- Expected:   Garouk speaking once the lesson is over.
- Status:     kept
- Ours:       the scripts' `GetObjectByTag` (no object, nothing started)
- Evidence:   the route's transcript

### Q-0156 — The Lower City cantina's pazaak names a missing end script

- Reference:  K1's Lower City cantina (`tar_m03ae`): `k_ptar_pazup`
- Behaviour:  its `PlayPazaak` names the end script `k_ptar_nikpazend`,
  which only the Upper City cantina's module has: nothing runs after that
  match (the wager is still settled).
- Expected:   the opponent reacting to the result.
- Status:     kept
- Ours:       `script::pazaak` queues the end script; the library counts it
  missing
- Evidence:   `context/artifacts/2026-10-04-pazaak/` (`kjk ls k1
  '*pazend*' --module tar_m03ae`)

### Q-0157 — The commoners' AI loops for good on a hostile beyond 20 m

- Reference:  K1's `k_ai_master`, its commoner path (Korriban's Dreshdae,
  `korr33_citizen3` while `kor33b_murderer` fights the player 24 m off)
- Behaviour:  it walks the creatures it holds neutral, nearest first; one of
  a hostile faction within 20 m makes it flee, but one beyond 20 m never
  moves the walk on, so the loop never ends. KOTOR's script machine stops a
  run past 131,071 instructions, and the game goes on.
- Expected:   the walk moving on to the next creature.
- Status:     kept
- Ours:       `script_ncs::vm`'s instruction limit (KOTOR's), the run listed
  among `Report::runaways`, not faults
- Evidence:   the K1 chain's `korriban` route (`runaways 1`)

### Q-0158 — A weapon's slay-race on-hit takes another property's chance

- Reference:  K1's and TSL's executables, the on-hit handler (item property
  32, `iprp_onhit.2da` row 8 Slay Racial Group)
- Behaviour:  a hit's on-hit chance starts at none and is set only by a
  property of a timed kind (sleep to slow, `iprp_onhitdur.2da`); a slay of
  a race reads whatever chance such a property before it on the weapon
  left on this hit, so alone it never fires.
- Expected:   its own chance.
- Status:     kept
- Ours:       `combat::gear::on_hits`
- Evidence:   `context/artifacts/2026-10-05-item-properties/README.md`

### Q-0159 — Improved Saving Throws against an element reads it as a save

- Reference:  K1's and TSL's executables, item property 26
  (`ImprovedSavingThrows`, its subtype an `iprp_saveelement.2da` row)
- Behaviour:  its subtype is taken as a save (1 Fortitude, 2 Reflex, 3 Will,
  else every save), as property 27's is: a bonus against acid (row 1) is a
  Fortitude bonus, one against the Force (row 15) a bonus to every save.
- Expected:   a bonus to saves against that element only.
- Status:     kept
- Ours:       `session::items::read_property` (`Property::Save`)
- Evidence:   `context/artifacts/2026-10-05-item-properties/README.md`

### Q-0160 — A Jedi mind's sidestep reads its enemy's side off its own angles

- Reference:  OpenJK (the Jedi AI's attack decision)
- Behaviour:  attacking and not strafing, 1 think in 4 it steps aside for
  that think, away from the side its enemy is on, that side read as the dot
  of its right with the enemy's origin less its own angles (pitch, yaw,
  roll taken for a point), not less its own origin: the side comes out by
  where the enemy stands in the world and which way it faces, not where
  the enemy is from it.
- Expected:   the enemy's origin less its own origin.
- Status:     kept
- Ours:       `world::minds::fight` (`from_foe`), `combat::pace::Pace::finish`
- Evidence:   `kjk combat duel`, the pace probes

### Q-0230 — reone's dialogue camera ignores a line's CameraAngle

- Reference:  reone (the dialog camera); K1's executable, its
  dialog camera (read for behaviour)
- Behaviour:  reone reads the DLG's `CameraAngle` and never uses it: every
  shot of a line without a placed or animated camera is picked at random
  (close, far or both) between the `talkdummy` nodes, and the eye is
  pulled in to the first wall on its sight line.
- Expected:   the executable's meanings: 0 the conversation's default
  sequence (walked from the DLG name's hash, its first entry and first
  reply set 2), 1 the close-up of the speaker, 2 over the listener's
  shoulder, 3 the wide two-shot (3 after 3 keeps the shot), 4 no shot of
  its own, 5 the shot kept, 6 the placed camera; each pair filmed from one
  side of the line between them, and a wall on the sight line dropping
  the line to the close-up. A line of a DLG with a camera model and no clip
  of its own takes these shots too (reone holds the last clip's frame).
- Status:     not reproduced: the executable's camera (design S6), its own
  quirk too: a line of angle 6 whose `CameraID` the area lacks is filmed
  from the area's origin at 55 degrees [likely], looking straight down,
  its orientation unturned and no pitch [verify]. A script's cut to a
  camera the area lacks leaves the line to the dialog camera [verify]. A
  line of angle 5 or 4 after an animated or a placed line keeps our last
  dialog shot before it, where the executable likely has the previous
  line's pair face each other again [verify].
- Ours:       `sim::script::dialogue` (`Conversation::angle`, `angle_walk`),
  `launcher::play_view::talk_camera` (`DialogShot`, `Sides::side`,
  `dialog_shot`)
- Evidence:   `context/artifacts/2026-10-16-scene-mode/exe.md` (Q7); the
  scene probes' `default_angles` and the `dialog camera` probes
  (`kjk sim probes`); `context/artifacts/holistic-view/SURVEY.md` (§6).

### Q-0231 — reone hangs no visual effect on a creature

- Reference:  reone (applying a visual effect does nothing)
- Behaviour:  an `EffectVisualEffect` draws nothing.
- Expected:   the row's models on the creature's `impact`, `headconjure`
  and `handconjure` nodes (the K1 executable's names for its attach points).
- Status:     not reproduced: the models hang from those nodes.
- Ours:       `launcher::script_fx` (`Node::name`, `body_node`)
- Evidence:   `context/artifacts/holistic-view/SURVEY.md` (§1).

### Q-0232 — TSL adds a joining creature's own components at every join

- Reference:  TSL's executable (`CSWPartyTable::TransferInventory`, called
  by every way a creature joins the party)
- Behaviour:  the components and chemicals a creature outside the party
  holds of its own (what was made on it or given it) are added to the
  party's as it joins, and its own count is left as it was (the party's
  is read while it is a member). It adds them again the next time it
  joins.
- Expected:   moved to the party once.
- Status:     reproduced: `State::takes_in` adds `State::hoards` at each
  join and keeps it. The gold the routine moves the same way is not moved
  in ours (a creature's purse stays its own) [verify K1's].
- Ours:       `sim::script::workbench` (`takes_in`, `pool_carried`)
- Evidence:   `context/artifacts/i35/crafting.md` (§1); the workbench probe
  "it joins" (`kjk sim probes --only workbench`).

### Q-0237 — TSL's PlayMovieQueue keeps its nAllowSkips and never reads it

- Reference:  TSL's executable (`PlayMovieQueue`, its movie player's
  `PlayMoviesAsync` and `CancelMovie`; read for behaviour)
- Behaviour:  `PlayMovieQueue(nAllowSkips)` stores the flag in the player
  and nothing reads it: whether a movie of the queue may be skipped is its
  own `QueueMovie` flag alone, and a skip of a skippable one drops the rest
  of the queue. The endings' `PlayMovieQueue(FALSE)` (904MAL) are
  skippable, each movie queued `TRUE`.
- Expected:   `nwscript.nss` documents `nAllowSkips` as allowing skips for
  the queue.
- Status:     reproduced: the queue's flag is ignored
  (`script::presentation::play_movie_queue`).
- Ours:       `sim::script::presentation`, `game::movie_view::Queued`
- Evidence:   the presentation probes' `movie_queue` (`kjk sim probes`).

### Q-0238 — RoundsToSeconds counts a round as 3 seconds

- Reference:  both games' executables (`TimeConversions`; read for
  behaviour)
- Behaviour:  `RoundsToSeconds(n)` is `n` × 3.0 and `TurnsToSeconds(n)` `n`
  × 30.0, in K1 and TSL alike.
- Expected:   `nwscript.nss` says "a round is always 6.0 seconds" (NWN's);
  reone divides by 6.
- Status:     reproduced: the executables' 3.0 (`k_sp1_generic`'s 10-round
  buffs last 30 s, `k_trp_generic`'s 3-round effects 9 s).
- Ours:       `sim::script::presentation` (`seconds`)
- Evidence:   the presentation probes' `time_and_angles`.

### Q-0239 — ShowTutorialWindow takes its row as a byte

- Reference:  TSL's executable (`ShowTutorialWindow`, `TutorialReasonWillShowWindow`)
- Behaviour:  the row is cut to its low byte (`tutorial.2da` rows past 255
  wrap), and a window refused (a conversation running, the option off) is
  not marked shown: a later call may show it.
- Expected:   the row as given.
- Status:     reproduced (the row a byte; with the option off our sim marks
  it shown all the same, the option being each player's view's: owner's
  call).
- Ours:       `sim::script::presentation` (`tutorial`), `script::party::Tutorials`
- Evidence:   the presentation probes' `tutorial_window`.

### Q-0242 — A vitality regeneration modifier doubles healing whatever its percent

- Reference:  TSL's executable, its natural healing (`CSWSCreature`'s
  hit point regeneration, read for behaviour)
- Behaviour:  each `EffectVPRegenModifier` on a party member doubles its
  healing rate; the effect's percentage is fetched and thrown away, so
  Battle Meditation's 50 % doubles, and two overlapping ones quadruple.
- Expected:   the rate raised by the percentage (+50 %).
- Status:     kept
- Ours:       `world::powers::regenerate` (`Effects::healing_doubled`)
- Evidence:   `context/artifacts/i35/effects-actions.md` (2.13); the
  `tsl effects` probe `healing_doubled`.

### Q-0243 — Scramble Droid and Droid Trick describe each other's effect

- Reference:  TSL's `spells.2da` and `k_inc_force` (rows 201 and 269)
- Behaviour:  row 201 (`FORCE_POWER_DROID_TRICK`) is named Scramble Droid
  and described as turning the droid to fight for the caster, but its
  script gives the distracting `EffectDroidScramble`; row 269
  (`FORCE_POWER_DROID_CONFUSION`) is named Droid Trick and described as a
  distraction, but its script changes the droid's faction.
- Expected:   each power's description matching what it does.
- Status:     kept (the scripts decide; the descriptions are the game's)
- Ours:       the powers' impact scripts (`EffectDroidScramble`,
  `EffectFactionModifier`)
- Evidence:   `context/artifacts/i35/effects-actions.md` (2.12; TLK 49070,
  108458, 114571, 114572).

### Q-0244 — True Seeing's end marks the wrong bit while another runs

- Reference:  TSL's executable, its true seeing effect's removal (read for
  behaviour)
- Behaviour:  taking a true seeing off clears its bearer's true seeing bit
  (0x4) and, when another true seeing is left, sets bit 0x2 instead of
  keeping 0x4: the one left no longer sees as true seeing does.
- Expected:   the bit kept while any true seeing runs.
- Status:     not reproduced: no invisibility or darkness here for the bit
  to see through; `EffectTrueSeeing` is kept and walked only (no shipped
  script applies one, Q-0245).
- Ours:       `combat::effects::Modifies::TrueSeeing`
- Evidence:   `context/artifacts/i35/effects-actions.md` (2.6).

### Q-0245 — Enhanced Senses makes True Seeing and never applies it

- Reference:  TSL's `k_inc_force` (Juhani's `SPECIAL_ABILITY_ENHANCED_SENSES`,
  row 59)
- Behaviour:  the ability makes an `EffectTrueSeeing` and links only a
  defence penalty and Awareness (twice) into what it applies: the true
  seeing is never on her.
- Expected:   the true seeing linked in.
- Status:     kept (the game's script)
- Ours:       the script as it runs
- Evidence:   `context/artifacts/i35/effects-actions.md` (2.6).

### Q-0246 — Body Fuel takes no vitality for its refills

- Reference:  TSL's executable (Body Fuel's effect and the combat round's
  end); TSL's talk table for the ability
- Behaviour:  while Body Fuel runs its bearer's Force points are set full at
  each of its combat rounds' ends, for nothing: the description's
  "takes Vitality damage equal to the Force Point cost" has no code.
- Expected:   the hit points paid for the Force points.
- Status:     kept
- Ours:       `combat::effects::regenerate` (`Modifies::BodyFuel`)
- Evidence:   `context/artifacts/i35/effects-actions.md` (2.10).

### Q-0247 — TSL tells a lowered bonus by the bonus left

- Reference:  TSL's executable (`ExecuteCommandSetBonusForcePoints`,
  `CGuiInGame::UpdateStatus`, `CSWGuiStatusSummary::AddMaxFPLost`)
- Behaviour:  a set that lowers a player's bonus Force points tells the
  status summary the new value, not the change: `AddMaxFPLost` adds it to
  the lost count, and the summary shows "Maximum Force Points Decreased"
  only while that count is not 0, so a bonus set down to 0 is never told.
  An add tells its own amount (`AddBonusForcePoints(0)` tells nothing).
- Expected:   the change told.
- Status:     reproduced: `StatusKind::MaxForcePoints` carries what the
  executable passes; the summary adds it up as KOTOR 2's does.
- Ours:       `sim::script::force` (`AddBonusForcePoints`,
  `SetBonusForcePoints`), `launcher::progress` (`Summary`)
- Evidence:   `context/artifacts/i35/combat-force.md` (2.5); the `tsl force`
  probes' `bonus_force_points`.

### Q-0248 — TSL's room Force rating outside every room reads before its table

- Reference:  TSL's executable (`CSWSArea::GetRoomForceRating`,
  `GetRoomIndex`)
- Behaviour:  a place under no room's walkable floor gets room index −1,
  which the rating lookup uses as is: the byte before its table.
- Expected:   no room's rating: 0.
- Status:     not reproduced (an undefined read): 0.
- Ours:       `sim::module::ForceRules::rating`
- Evidence:   `context/artifacts/i35/combat-force.md` (3.6); the `tsl
  force` probes' `room_ratings`.

### Q-0249 — TSL's GetRandomDestination divides by its range

- Reference:  TSL's executable (`ExecuteCommandGetRandomDestination`)
- Behaviour:  each offset is `rand() % (2 × range) − range`: a range of 0
  divides by zero (the game faults), a negative one picks from a mirrored
  span.
- Expected:   the creature's own place.
- Status:     not reproduced: a range of 0 or less is the creature's place.
- Ours:       `sim::script::random_destination`
- Evidence:   `context/artifacts/i35/combat-force.md` (7); the `tsl force`
  probes' `random_destination`.

### Q-0250 — TSL's AdjustCreatureAttributes wraps the score

- Reference:  TSL's executable (`ExecuteCommandAdjustCreatureAttributes`,
  `CSWSCreatureStats::SetSTRBase` …)
- Behaviour:  the base score moves as an unsigned byte: 3 lowered by 5 is
  254.
- Expected:   the score held within its range.
- Status:     reproduced (no shipped script lowers a score past 0).
- Ours:       `sim::script::force` (`AdjustCreatureAttributes`)
- Evidence:   `context/artifacts/i35/combat-force.md` (5).

### Q-0251 — TSL's AdjustCreatureSkills keeps a signed rank

- Reference:  TSL's executable (`ExecuteCommandAdjustCreatureSkills`,
  `CSWSCreatureStats::SetSkillRank`)
- Behaviour:  the rank is a signed byte: lowered past 0 it is negative,
  raised past 127 it turns negative.
- Expected:   a rank of 0 to 127.
- Status:     not reproduced: the sheet's ranks are unsigned, held within 0
  and 127 (no shipped script lowers one: `a_adjust_skill`, `a_hk_skill`
  add).
- Ours:       `sim::script::force` (`AdjustCreatureSkills`)
- Evidence:   `context/artifacts/i35/combat-force.md` (5).

### Q-0257 — SWMG_SetPlayerInvincibility stops the script that calls it

- Reference:  K1's and TSL's executables, the handlers of routines 642
  (`SWMG_GetPlayerInvincibility`) and 648 (`SWMG_SetPlayerInvincibility`)
- Behaviour:  both are handled as a follower's float (the routines that
  take a follower first): the handler pops an object the prototype never
  pushed, the typed stack refuses it, the handler fails and the script
  machine stops the script there. TSL's mine script (`mine.ncs`, all three
  races) calls it just before `SWMG_StartInvulnerability` on the player,
  which therefore never runs: a mine leaves the swoop open to the next.
- Expected:   the player's invulnerable period set, the script going on.
- Status:     kept
- Ours:       `script::minigame::aborts`, `script::engine` (`Host::action`),
  `script::finish` (no fault)
- Evidence:   `context/artifacts/i35/minigames.md` (§2.9); the probe
  `minigame tsl` `routines`

### Q-0258 — SWMG_AdjustFollowerHitPoints never reads its third argument

- Reference:  K1's and TSL's executables, the handler of routines 590 and
  604
- Behaviour:  it pops the follower and the change only: `nAbsolute` stays
  on the stack (where the script's drop of the int the prototype returns,
  which the handler never pushes, takes it off again). The change is always
  added, through the follower's damage hook.
- Expected:   the hit points set to the number when `nAbsolute` is true.
- Status:     kept (the stack's balance needs nothing here: ours pops by
  the prototype)
- Ours:       `script::minigame` (`AdjustFollowerHitPoints`),
  `minigame::Course::adjust_hp`
- Evidence:   `context/artifacts/i35/minigames.md` (§2.8); the probe
  `minigame tsl` `hit_points`

### Q-0259 — SWMG_SetFollowerPosition puts the player at its start

- Reference:  TSL's executable, the handler of routine 790
- Behaviour:  it makes no use of its vector: whatever it is given, the player
  goes back to the start of its track (its clip at 0), its offset and
  drift none; declared to return a vector, it returns nothing. Only dead
  code in the races' crash script calls it.
- Expected:   the follower at the given position.
- Status:     kept
- Ours:       `minigame::Course::restart`
- Evidence:   `context/artifacts/i35/minigames.md` (§3)

### Q-0260 — A bump pushes the swoop by the world's axes on its track's

- Reference:  TSL's executable, the player's follower hit check (its bumps)
- Behaviour:  the push out of a bumped enemy is worked out between the two
  models' places in the world, then added to the player's offset, which
  turns with its track: on a track not running up +y the push goes another
  way than away from the enemy. The direction of the plane the minigame's
  `Bump_Plane` names is worked out first and thrown away.
- Expected:   the push turned into the track's frame (or kept to the bump
  plane).
- Status:     kept
- Ours:       `minigame::Course::bump`
- Evidence:   `context/artifacts/i35/minigames.md` (§2.6); the probe
  `minigame tsl` `bump`

### Q-0261 — A save forgets which encounter made a creature

- Reference:  K1's and TSL's executables, `CSWSEncounter` and its save
  (read for behaviour)
- Behaviour:  a creature an encounter made keeps a link to it (what
  `GetIsEncounterCreature` reads, and the flag that counts its death once),
  but neither game's save writes that link (no field for it in either
  executable). After a load those creatures are ordinary:
  `GetIsEncounterCreature` answers 0 for them, and their deaths no longer
  count against the encounter's `NumberSpawned`, which the save does keep.
  An encounter whose creatures were alive at the save never exhausts and
  never resets.
- Expected:   the link kept across a save.
- Status:     kept (the owner's call, 2026-10-19: follow the executables)
- Ours:       `script::State::encounter_creatures` (not encoded),
  `spawner::Spawn::number_spawned` (encoded)
- Evidence:   `context/artifacts/i35/encounters.md` (1.2); the probe
  `ute encounters` `save_keeps_the_list_not_the_links`.

### Q-0262 — TSL piles an encounter's creatures on its second spawn point

- Reference:  TSL's executable, `CSWSEncounter::SpawnCreatureFromSpawnList`
  (read for behaviour)
- Behaviour:  TSL deals an encounter's creatures round its spawn points,
  `list count / point count` (whole) to each. With more points than
  creatures that share is 0: the first creature moves the round to the
  second point and every later one stays there (506ond's `ls_vaklu_enc`
  and `ls_va_sith_enc`: four creatures, six points, all at the second, each
  pushed off the one before by the safe-place search). The round is not
  saved and carries over a reset. Whoever last entered gone (not a
  creature here), the point is the world's origin facing +X, the round
  moving on all the same; K1 likewise for its farthest point.
- Expected:   the creatures spread over the points.
- Status:     kept (the owner's call, 2026-10-19)
- Ours:       `world::spawners::point`, `spawner::Cursor`
- Evidence:   `context/artifacts/i35/encounters.md` (2.5); the probe
  `ute encounters` `tsl_points_outnumber_the_list`.

### Q-0263 — An encounter's "out of the players' sight" test sees no player

- Reference:  K1's and TSL's executables, `CSWSArea::CountVisibleToPlayers`
  (read for behaviour; K1's twin read too)
- Behaviour:  placing a creature at the encounter's own place (no spawn
  point, or none safe), the engine means to pick a spot no player sees.
  Its count of the players who see a spot asks the server for the client
  of the area's own id, not the creature's, finds none, and counts 0 for
  every spot: the encounter's place itself if free, else the first safe
  spot of the rings round it within 30 m with a clear line from it, in
  view or not. K1's executable makes the same call with the same id, so
  both games behave alike [verify by a test in each].
- Expected:   a spot out of the players' sight (creatures within 30 m with
  a clear line at 1.5 m over the floor).
- Status:     kept in both games
- Ours:       `spawner::Ground::out_of_sight`
- Evidence:   `context/artifacts/i35/encounters.md` (2.5); K1's twin of
  `ComputeNonVisibleLocation` and its count read 2026-10-19 (the call
  passes the area's id in both).

### Q-0264 — K1's budget pick reads a float from before its creature list

- Reference:  K1's executable, `CSWSEncounter::BuildInitialSpawnList`
  (read for behaviour)
- Behaviour:  before drawing from a run of creatures sharing a challenge
  rating, it compares the remaining budget with a float read 16 bytes
  before the creature array (the heap's header), not with a creature's
  points. In practice that reads as a tiny or negative number and the run
  is drawn from [verify].
- Expected:   the run's points.
- Status:     taken as passing (the owner's call, 2026-10-19)
- Ours:       `spawner::k1_initial`
- Evidence:   `context/artifacts/i35/encounters.md` (2.4).

### Q-0265 — K1's encounter budget grows with every list it builds

- Reference:  K1's executable, `CSWSEncounter::ComputeSpawnPool`,
  `SpawnCreatureFromSpawnList` (read for behaviour)
- Behaviour:  each list built adds the party's budget to the encounter's
  pool, and making a creature spends the spawn list entry's points, which
  the list never fills in (0): nothing is spent. An encounter activated
  again by a script after exhausting builds against the old pool and the
  new budget together; only a reset (after its list is built) and a pool
  under the cheapest creature's points empty it. The pool is not saved.
- Expected:   the pool spent by the creatures made.
- Status:     kept
- Ours:       `spawner::Spawn::pool`, `spawner::spawn_if_appropriate`,
  `world::spawners::reset`
- Evidence:   `context/artifacts/i35/encounters.md` (2.4, 2.5).

### Q-0266 — Bao-Dur "cannot equip" a sonic grenade

- Reference:  TSL's executable, `CSWSItem::AddCNPCRequirements` (read for
  behaviour)
- Behaviour:  once Bao-Dur's influence is set, an item whose base item is
  35 to 37, 97 to 99 or 102 begins its description with "Cannot Equip:"
  and "Bao-Dur". The middle range takes base item 97, Sonic Grenade Type
  2, besides the armoured flight suit (98) and the restrictive robe (99),
  so that grenade says Bao-Dur cannot equip it.
- Expected:   the Jedi robes and the armour he cannot wear only.
- Status:     kept
- Ours:       `sim::cannot_equip::BASE_ITEMS`, `game::character::described`
- Evidence:   `context/artifacts/i35/influence-party.md` (1.5); the range
  test reads the item's base item (`CSWItem::GetBaseItem` reads the same
  field).

### Q-0267 — the player pushes no one aside

- Reference:  K1's executable, the bump routine and its "can bump" test
  (read for behaviour)
- Behaviour:  a walker running into an idle creature that is not hostile
  to it pushes it aside onto a free spot beside its way. The client's own
  move of the party leader asks the same routine when the player runs into
  a creature, but the test refuses a leader as the one bumping: its only
  path for a leader returns the bumped creature's flag that an earlier
  check has already required to be clear. The player is stopped and slides
  along the creature; nobody is ever pushed by the player.
- Expected:   unclear: the client asking the routine suggests the player
  was meant to push idle creatures as walkers do.
- Status:     kept
- Ours:       `world::move_actors` (`walk::Met::push` only for a creature
  walking), `walk::Walker::meet`; the player's slide along a body it
  touches, `world::met_as` and `Inside::hold_off`
- Evidence:   `context/artifacts/2026-10-21-npc-collision/SURVEY.md` (K2,
  K3).

### Q-0279 — a fountain's births follow the frame rate

- Reference:  K1 (the executable's fountain update, read for behaviour)
- Behaviour:  a fountain waits until a clock reaches one birth's interval,
  then bears the whole part of clock × rate (wrapped by the rate's whole
  part plus one) and starts the clock again, dropping the rest. A step
  longer than one interval bears several, a shorter one exactly one, so a
  fast fountain bears about one particle a frame: 100 a second gives some
  60 at 60 frames a second, 90 at 30.
- Expected:   births a second at the birth rate, whatever the frame rate.
- Status:     not reproduced (divergence, for the owner to confirm): births
  are owed by the second and the fraction kept, so the rate holds at any
  frame rate. Kept from the same routine: no births under a rate of one a
  second, whatever the random birth rate, and the random birth rate moving
  the rate up or down by a whole number below it.
- Ours:       `game::model_fx::Particles::fountain_births`
- Evidence:   `context/artifacts/2026-10-21-taris-fixes/SURVEY.md`
  (follow-up 2).

### Q-0280 — a saber's burn mark goes at once

- Reference:  JA (OpenJK's saber mark creation and mark drawing; the
  install's `shaders/marks.shader`)
- Behaviour:  the burn a blade leaves along a wall is a mark that fades
  its vertex alpha over its last second (`alphaFade`), but its shader
  (`gfx/damage/rivetmark`, `blendFunc GL_DST_COLOR GL_SRC_COLOR`, no
  `rgbGen` or `alphaGen`) reads neither the vertex colour nor its alpha:
  the burn stays whole for ten seconds and then goes at once.
- Expected:   the burn fading out over its last second, as the glow does.
- Status:     reproduced
- Ours:       `game::saber_marks::Marks::draw`
- Evidence:   `context/artifacts/2026-10-22-saber-marks/NOTES.md`.

### Q-0305 — a room the VIS leaves out is not drawn, even in plain sight

- Reference:  K1 (the executable's VIS rooms and its clear colour)
- Behaviour:  only the rooms the VIS file links with the viewer's room
  (either way round) are drawn; the rest of the screen keeps the clear
  colour, which is the area's fog colour. Where the designers counted on
  something hiding a room and it does not (the Taris cantinas' hub: four
  quadrant rooms round the bar, each leaving out the archway rooms across
  it), the archway shows flat fog colour, with the room's people drawn
  against it, until the viewer reaches a room whose VIS lists it.
- Expected:   the room past the archway drawn, as far as the fog lets it be
  seen.
- Status:     fixed (divergence, approved by the owner 2026-10-24): a room
  the VIS leaves out is drawn too while some part of it is nearer the
  camera than the area's fog end (`SunFogFar`), where the fog does not yet
  hide it all; in an area without fog, always (each mesh's frustum test
  then draws only what is in view). The people and objects in it were
  never left out.
- Ours:       `game::scene::Culling::room_visible`, `game::scene::cull_rooms`
- Evidence:   `context/artifacts/2026-10-24-cf2-vis/README.md` (before and
  after at the same poses, the walk gif, frame cost in six areas).

### Q-0323 — a line's speaker search keeps a plain distance as its bar

- Reference:  K1 and TSL (the executables' dialogue speaker search, a
  nearest-object-by-tag search from the conversation's owner)
- Behaviour:  looking for the object a line's `Speaker` names, the search
  walks the owner's area in its order, keeping a bar that starts as the
  search radius squared and against which each match's squared distance is
  tested; but a match taken stores its plain distance as the new bar. After
  the first match, a later one is taken only when its squared distance is
  within the first's plain distance: past a metre the nearer of two bodies
  sharing a tag can lose, under a metre the farther can win. (TSL also
  takes a match failing the test when a flag of the object is set; the
  flag is not identified, not reproduced [verify].)
- Expected:   the nearest object with that tag.
- Status:     reproduced
- Ours:       `sim::script::dialogue::Step::speaker_for`
- Evidence:   `context/artifacts/2026-10-25-interjections/SURVEY.md`; the
  probe `speaker_search_keeps_its_first_bar` (`kjk sim probes --only
  speakers`).
