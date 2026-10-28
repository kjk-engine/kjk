# Movement and the third-person camera (`sim::motion`, `kjk play`)

Players move as JA multiplayer's `Pmove` moves them, in the sim (8 ms
steps), on any `collision::CollisionWorld` (specified: `MOVEMENT-SPEC.md`,
`GROUND-SPEC.md`); the view draws a KOTOR body in JA's animations and camera.
## In the sim (`sim::motion`, `sim::math`)
- A pure function of (`MotionState`, `Command`, msec, time, level):
  friction, acceleration, air control, gravity, the slide and step-slide
  moves (18-unit steps, `g_stepSlideFix` on), slopes to `WALKABLE_NORMAL_Z`
  0.7 (45.6°), crouching (box top 40 → 16; standing needs room), jumps
  (225 units/s), landings and their timers, rolls (crouch at 200+ units/s,
  or land crouched), slope stances, and the legs' and torso's animations
  (as JA's footstep and animation-set rules do), with a lit saber's stances and gaits.
  With the character's fight (`motion::Arms`) it also runs JA's weapon
  step: weapon changes, knockdowns and the saber's moves, which change the
  command and the speed before the move (docs/COMBAT.md). Force jumps
  (`motion::force_jump`, I26): a character with a jump level that keeps
  jump held while rising is lifted toward its level's height (96 / 192 /
  384 units). Not yet: water.
- Bodies (`world::move_actors`): a body's own moves use JA's box; what
  others meet of a creature is narrowed to its `perspace`. A box standing
  inside another body (not the level) is not held by it (`world::clear_of`):
  its moves pass through that body while they take it no deeper across the
  face of the other's box it overlaps least (`Inside::hold_off`, the side a
  trace stopped by the box would slide along), so nothing nudges or moves
  it, and it walks out of the other or along it, never into it. A
  seat's player meets a body it only touches so too (`world::met_as`): it
  slides along it, as K1's player slides along a creature (Q-0267), rather
  than being held by its box's corner against a slanting wall. A
  creature walking a route that runs into a body meets it as KOTOR's walker
  does (`walk::Walker::meet`), on move_body or on KOTOR's
  walker alike: an idle creature not hostile to it (nothing queued, no walk,
  no animation playing now, out of fights and conversations; never a player or the
  party's) is pushed aside onto a free spot beside its way
  (`walk::pushed_aside`: the two personal spaces and 0.1 m out) and the
  walk goes on; any other is a bump, gone round at once (`Walker::bump`:
  given up on the 6th, on the same body from the same spot, or with no side
  free). The player pushes no one and is pushed by no one (Q-0267).
- While a conversation holds the world (scene mode,
  `DIALOGUE.md`), a body walking a route moves on KOTOR's
  walker instead (`walk::kotor_step`): toward its walker's corner (on
  reaching one short of the goal, the step plans the next itself from the
  level and the doors, so a client predicting it goes on round the route as
  the authority does) at its `creaturespeed.2da` walk or run (a seat's
  body's the table's `PLAYER` row, a guest's companion or the companion
  leading too; a creature's its `module::Body` rates, the `DEFAULT` row
  where they read as nothing; `walk::rates`), times its speed multiplier
  (`run_speed` over move_body's, clamped to 0.125–1.5; a walk under 0.1 m/s is
  none, a run at least 1 m/s). Its soles are held an eighth of a unit over
  the navigation mesh's height (no collision traced), its yaw snaps to the
  step, its velocity is the step over the time and its legs play
  `BOTH_WALK1` or `BOTH_RUN1`, `BOTH_STAND1` once it arrives. It meets
bodies by its own rule only: the box rule that holds a move_body move off a
body it stands inside (`world::clear_of`) is not laid over its step (a
walker going round a body it overlapped stopped and started step by step). `motion::carry`
  runs the rest of move_body's step for it: the command's angles and saber
  moves, the timers, the weapon. Where the mesh has no floor ahead (too
  steep for move_body), that step is move_body's. A body it carried
  (`motion_flags::CARRIED`) that no longer walks starts its next step still:
  KOTOR's bodies carry no momentum. A walk the conversation outlasts carries
  on through move_body from where it is, at its velocity. Out of a
  conversation, a script's move of a body no seat drives, out of a fight,
  steps on KOTOR's walker the same way from the moment it stalls on move_body
  to its next corner (`docs/NAV.md`, "Walking"); a door on that stretch is
  met as its state stands (no held party then).
- A body its walk carries on (a route its mind walks, moving more than
  0.1 m/s) plays its walk or run: a script's or a line's animation still
  playing on it ends (`world::move_actors`), as KOTOR's movement replaces
  the animation and reone's creature plays no other while it moves; a
  death's clip and a scene's blow play on (Trask's forceful talk carried
  to the door in the Endar Spire's last stand).
- Out of a fight a body moves through move_body at KOTOR's pace (`walk::paced`,
  in `world::move_actors`): its step's base speed set so move_body's speed for
  its keys is its `creaturespeed.2da` walk or run (faster than JA's run
  where KOTOR's is: the krayt's 10 m/s), its own base speed kept
  (`walk::rates`, `kotor_speed`: a seat's body's the `PLAYER` row, K1 3.20 /
  5.40 m/s, TSL 1.70 / 5.40; a creature's its own row, 1.70 / 5.40 for
  most). It holds for a script's move of a body no seat drives while it
  engages no foe (walkways, `ActionRandomWalk`, `ActionMoveTo*`, a follow
  run; known to a client by `Mind::moving`, which the authority sets), and
  for a seat's body and a companion following one while neither it is in a
  fight (its weapons out: `Fighter::weapon_away`) nor the party is
  (`InCombat`); in a fight they keep JA's speeds. Leaving one, the pace
  eases from JA's to KOTOR's over `walk::PACE_EASE_MS` (500 ms). A paced
  body (`motion_flags::PACED`) walks or runs as its move says (the walking
  button), however long its keys, and gathers speed as at JA's stop speed
  (100 units/s) at least, so a walk slower than that gets going: JA's ground
  friction holds back any wish under about 60 units/s, and the whole-unit
  velocity snap stalls a slow diagonal one. `kjk move pace k1|tsl` measures
  every module's creatures against KOTOR's speeds; `kjk sim probes pace` the
  player's.
- A creature (not a player) in the air with no floor at all within 100 m
  below it does not move (`world::FLOOR_SEARCH_M`): KOTOR has no falling,
  and a creature placed over a room the game gives no walkable face (the
  cutscene modules' soldiers) stands where it was placed instead of
  falling forever.
- Units: JA's inches and seconds, JA's numbers. `Move::trace` converts at
  the collision boundary (× `collision::UNIT`, the fraction back, the stop
  recomputed in units); snapshots show metres. Velocity snaps to whole
  units each step, halves to even (OpenJK's Windows build rounds them away
  from zero; the original and OpenJK elsewhere to even).
- `Command::angles` are absolute view angles (JA shorts); `view_offset`
  hold the sim's corrections only (the ±16000 pitch clamp).
- Animation lengths come from JA's `animation.cfg` (`AnimTable`, loaded by
  the host: `stage::anim_table`; a stand-in for the placeholder world).
  Slope stances trace under the talus bones of the animation's first
  frame (JA reads the foot tags off the live skeleton).
- Trig: `sin_cos_deg` from `+ - * /` (quarter turns, Taylor to x⁹), within
  1.1e-7 of `f64` over all 65 536 command angles (`kjk move trig`); it is
  `collision::trig`, which the level's build uses too.

## In the view (`play_view`: presented snapshots only)
- Every tick's legs and torso change starts in `asset_ja::g2::Player` at
  its tick's time with the cgame's blend; `asset_ja::angles` turns legs and
  spine; `asset_ja::rig::Retargeter` poses the KOTOR body each frame.
- Camera as JA's third-person view offset: range 80, aim 16 over the eye,
  target damp 0.5, camera damp 0.3 per 50 ms, a 4-unit box traced.
- Smoothed by default, `--quirks` to see them: Q-0016 (camera pulled into
  the player under low ceilings), Q-0017 (Ghoul2 re-blending from a frame,
  not the pose shown). Kept: Q-0018. The body snaps up steps as in JA's
  third person; the camera's damping smooths the view.

## Tools
```bash
kjk move measure   # speeds, jumps, steps, slopes, roll, drops, tunnel (sim::measure)
kjk move run --stage course --moves course   # the course walk's track
kjk move pace k1 --only tar_m02 --seconds 30  # creatures' speeds out of a fight against KOTOR's
kjk play [--stage course|end_m01aa [--arrive TAG]] [--body p_carthbb,p_bastilabb|ja] [--camera third|first|side|arc]
kjk play --moves course --headless --trace n.csv --cmds 'anim 4.2; screenshot x; quit'
```
Keys: mouse, WASD, space, C/Ctrl (roll at a run), Shift (walk), E (the
target's chosen action, `docs/UI.md` "Targeting and use"), F1 (the
player's own eyes and back, `first_person`; the settings keep the choice),
Escape (the menu). Keys rebind in the options (`settings.rs`).
`--moves`: `2s w; 400ms w jump; 1s w turn 90; 800ms s walk; 500ms use; …`
(`sim::moves`). A module stage is installed as a game would
(`docs/LIFECYCLE.md`): its doors start closed, and one that leads out
loads the next module, where the walk goes on. After such a teleport the
commands' yaw turns from where the player arrives facing (JA's
`view_offset`).
