# Skeleton: JA animation on KOTOR characters

Decision 2 (`ROADMAP.md`): the hybrid. KOTOR bodies, heads, hilts and proportions
stay; JA's movement and combat clips are retargeted onto them; KOTOR clips
serve idles, dialogue and cutscenes; the weapon is hidden out of combat.

## Pipeline (`asset_ja`)
- `g2::Player` plays `animation.cfg` sequences as Ghoul2 does (timing,
  frame lerp, blends from the captured pose, body on `model_root`, torso on
  `lower_lumbar`); `g2::bake` samples a cue list every 5 ms into a clip with
  one track per GLA bone. `chain` times moves by the game's hold.
- Blends follow JA multiplayer's cgame: 100 ms on every change, 200 into
  or out of a flip (`asset_ja::blend_ms`; deaths snap only with quirks,
  Q-0009). The fast and strong styles play transitions at 1.5× and 0.75×,
  held by the move timer's `2 − speed` (`saber_anim_speed`,
  `move_hold_ms`, Q-0008). Frames slerp along
  Ghoul2's arc; blends turn on the side that was the shorter when they
  began (so a pose swinging past half a turn does not flip), at the lerped
  matrix's pace, evened out as the gap nears half a turn (where Ghoul2's
  bone shrinks to nothing and ours would whip). A key turns its bone about
  the model's origin, so each mix lerps the joint's place and derives the
  translation from the mixed turn (`g2::joint`): the joint stays on its
  parent, as Ghoul2's lerped matrix keeps it. A translation lerped apart
  from a slerped turn pulled joints off by the chord: the dual set's left
  shoulder a metre from the chest, its upper arm 1.2 m long, for JA's
  hilts and blade overlay (the drawn body takes rotations only and never
  showed it). The sim's skeleton (`sim::skeleton::turn`) mixes its own.
- `kjk play --trace` writes JA's shoulders, elbows and wrists
  (`rig::JA_ARMS`, `ja_*` columns) beside the drawn arm nodes, in the same
  world frame.
- `Player::quirks` (off) reproduces Q-0003 (stray backward frame),
  Q-0004 (inverted captured weights), Q-0010, Q-0017 and Q-0028; all jolt
  visibly. Timing and blends: `G2-SPEC.md`.
- `rig::retarget_with` turns a baked JA clip into a KOTOR clip: mapped nodes
  (`HUMANOID`) take their JA bone's model-space turn from an aligned rest;
  thighs aim at JA's knee and roll with the shin (`HINGED`, JA's femur twist
  is noise); the root takes scaled pelvis travel, then the supporting foot
  is held at KOTOR ankle height plus JA's lift from standing (JA's base
  pose stands 3.1 units up: `Options::ja_sole`) plus the jump's height,
  soft-minimum over both feet; JA clips put the floor 24 units under the
  origin (`ORIGIN_HEIGHT`).
- `rig::Grip`: the hilt hangs at KOTOR's `rhand` as K1 hangs weapons (blade
  +Z) and the hand turns so the blade points as JA's; fingers take KOTOR's
  `g2r1` pose (`Options::hold`). `rig::rebind` is probe (b), kept.
- `rig::Reach` (`kjk play`): the gripping hand turns onto the sim's blade
  and the upper arm and forearm carry the wrist up to `REACH` (0.3 m)
  toward it (`game::cast`'s share). One solver (`rig::bend`): the arm's own
  plane turned with the line from the shoulder, a nearly straight arm's
  elbow fading toward hanging down, so the elbow never picks between
  answers. The share is 1 only while the saber move cuts (an attack, a
  special, a parry), eased in and out over `REACH_EASE_S` (0.08 s); in a
  held stance `STANCE_REACH`, eased in over `STANCE_EASE_S` (0.3 s); 0
  otherwise (a start, a return, a transition, a flip, a run: nothing is
  cut; JA's arm is the truth): KOTOR's longer arm, pulled toward a blade
  JA's shorter arm placed, flipped its elbow there.
- `blade_lengths` (ignition, 60 fps), `saber_trail_ms` (trail life per move,
  from JA's saber move table: style moves, defensive moves, special trails).

## Looking and strafing (`asset_ja::angles`)
- `angles::player_angles` is multiplayer's player-angles rule, a pure
  function of (`State`, `Moment`: view angles, velocity, `movementDir`,
  ground, what legs and torso play, the `Motion` bolt; ms): the legs' yaw
  (toward the line of travel, at most 60°, swung at 1.3°/ms), the torso's
  aim (for effects), and angle overrides for `lower_lumbar`,
  `upper_lumbar`, `thoracic`, `cervical` and `cranium`. The player
  controller (I13) feeds it real input.
- `g2::bake_steered` runs it every sample before the skeleton is built, as
  the cgame does: the model turns to the legs' yaw about its origin
  (bone 0), the overrides apply as Ghoul2's `BONE_ANGLES_POSTMULT` in each
  bone's base-pose frame (`Player::turn`), and `Player::bolt_angles` reads
  the `Motion` bolt as `G2API_GetBoltMatrix_NoRecNoRot` does. Retargeting
  then carries it all onto KOTOR bodies.
- JA multiplayer has no strafing or turning animations: strafes and
  diagonals run `BOTH_RUN2` with the legs turned, backward runs
  `BOTH_RUNBACK2` (JA's footsteps rule; the builder's `press`, `release`).
- Quirks: Q-0011 kept; Q-0012 to Q-0014 jolt and are reproduced only with
  `--quirks`.

## Force effects (`asset_ja::fx`, `game::force_fx`, `scene_fx`)
- `fmt_ja::efx` reads effect files (GenericParser2 text), `fmt_ja::md3`
  MD3 meshes. `fx::effect` turns an effect into primitive templates (the
  `Particle` and `Electricity` kinds; everything else is listed as
  unsupported in the notes), `fx::play` spawns them as the FX scheduler
  does (counts, ranges, origins on the axis, `org2fromTrace`), and
  `Spawned::shown` evaluates one in a client frame (size, colour and alpha
  transitions, motion, rotation); `fx::sprite` and `fx::bolt` lay out the
  renderer's sprite and electricity (seeded path, bends, taper, growth).
- `force_fx::effects` is the cgame's side, every 60 fps client frame from
  the left hand (KOTOR's `lhand`): lightning (`lightning.efx` at levels 1-2,
  `lightningwide.efx` at 3, along the torso's aim from `angles`), grip's
  red puffs, push and pull's distortion
  (`testboom.md3` showing a copy of the screen), and
  the right hand's glow while a thrown saber flies.
- `scene_fx` draws them each frame around the camera, one mesh per shader
  blended as its first stage says (`Surface::AddOne`, `Surface::BlendVertex`),
  the distortion through Bevy's view transmission texture
  (`RefractionMaterial`). Timelines record their powers (`Power`).
- Quirks: Q-0015 (multiplayer's lightning never branches), fixed: bolts
  fork as single player's do.

## Idles (`sim::combat::idle`, `game::acting`)
- JA's: after 5 s at rest (multiplayer's idle check: standing,
  pressing nothing, view within 10°, unhurt, timers out), anyone in
  `BOTH_STAND1` or `BOTH_STAND2` plays its idle (`STAND2` picks one of two),
  held; the next comes 0 to 2 s after. Fists never idle; anything that
  ends the rest breaks one off. In the sim, so blades in an idle cut.
- KOTOR's: a script's `ActionPlayAnimation` puts `Acting` on the
  character, for as long as the sim's queue times it (`EVENTS.md`, action
  queues); a creature's humanoid body draws its own clip (reone's names,
  which `animations.2da` also lists; K1's bodies lack `pausesh` and
  `pausebrd`; the constants reone leaves out are named after the table's
  rows: SCRIPTS.md, "Animations") over the JA pose, blended over the clip's transition, its
  facing moved to the body's root. Moving, fighting, dying, a
  `ClearAllActions` or a new animation ends it, fading; meanwhile the JA
  idle waits. Seats' bodies draw none. A character a script makes and
  animates in the same step (TSL's medbay body, made lying dead) has the
  animation from its first snapshot.
- Every posed body starts each frame from its model's rest pose
  (`cast::at_rest`), then takes the JA pose and the clips over it: a node
  the JA pose does not turn (`rootdummy`, which KOTOR's lying clips lay
  down) stands at rest once no clip keys it, as KOTOR leaves a node no
  animation keys. Before, it kept what the last clip left: Taris's duel
  ring loser (`k_ptar_dueler_ud`: `ANIMATION_LOOPING_DEAD`, then
  `ANIMATION_LOOPING_PAUSE` for 0.1 s and a jump to the lobby) stood 46°
  aslant. Probe `up again` (`kjk sim probes --only "up again"`, K1's body
  and clips).
- In every conversation the line's pair turn their heads toward each
  other (`DIALOGUE.md`, "Lines"). While a conversation holds the world
  (S7), and only then:
  - a scripted clip is broken off only by dying, the rest being the
    queue's to end;
  - a droid or a beast (`cast::Clips`) draws its scripted clips over its
    own, a creature model's `c…` name first (HK-47's `ctaunt`;
    `creature::creature_model`; a retargeted humanoid of a creature model
    type, a gammorean, plays its `c…` clips too: its `cdie`), and its walk and run advance by the
    distance covered, a cycle a stride (`appearance.2da`'s `walkdist`,
    `rundist`), as the gait's do;
  - a humanoid wholly on KOTOR's clips (the JA pose's share left under
    them at most 0.001, nothing gripped or aimed) is drawn as the clips
    alone: its JA pose is not made, which makes it cost what a droid
    costs.

## Gait (`game::gait`)
- Out of a fight a player's body, and a humanoid creature's, moves as
  KOTOR's characters do (the
  owner's call, 2026-10-05: JA's male stance and walk looked wrong on
  K1's female bodies): alive, its weapons away, no script's animation, and
  its legs in JA's plain standing, its idle, walking or running
  (forwards, sideways or backwards), its own clip of the same (`pause1`, `walk`, `run`, through its supermodel chain,
  so a woman's are the female ones; a creature's read with its look,
  `CreatureClips`) is laid over the JA pose. Anything
  else (a fight, a jump, a roll, a Force move) is JA's.
- Facing (the owner's call): moving, the body turns to the way it goes,
  720° a second, as KOTOR's characters do (KOTOR has no strafing): a
  strafe runs sideways facing its way, not crabbing at JA's 60° leg turn.
  Going backwards it faces the other way and plays its walk or run in
  reverse (KOTOR has no backpedal). Standing, it faces as JA's legs do.
- Each clip's weight eases in or out over 0.25 s, laid over the JA pose as
  one weighted mean, so standing to walking cross-fades the clips and the
  gait fades back into JA's pose. Standing plays at its own pace; walking
  and running by distance, a cycle per stride (`appearance.2da`'s
  `walkdist`, `rundist`), so the feet keep to the floor.
- The view only: the sim's skeleton (JA's) still decides what a blade
  cuts. `dump`'s `characters.roster[].clip.gait` gives each clip's weight.

## Tools (`kjk rig a|b|c`)
- `--moves` a timeline (`showcase`, `acrobatics`, `specials`, `defense`,
  `force`, `staff`, `dual`, `reactions`, `melee`, `styles`, `gestures`,
  `look`; `game::rig_timeline`)
  or comma-separated sequences (`NAME:blend_ms`); `--body/--head` (default
  Carth), `--hilt kotor|ja`, `--color red,green` (the K1 hilt model per
  hand: K1 makes one per colour; the blades take JA's shaders and trail
  colour for it), `--trace out.csv`. A timeline's style picks
  the sabers (single, staff, two, none) and the fingers from the ready
  stance of KOTOR's matching weapon group (`g2r1`, `g3r1`, `g4r1`,
  unarmed `g8r1`). Every timeline is steered (`g2::bake_steered`), the
  view held ahead unless it says otherwise (`look` scripts it as a mouse
  would move it); `--quirks` reproduces the jolting quirks (playback's,
  deaths' Q-0009, the throw's Q-0007, the look's).
- Ground motion is the sim's own movement: `game::rig_ground` runs
  `sim::motion` on a flat `collision::BoxWorld` floor in 8 ms steps, fed
  commands (the forward key toward a wished velocity at its speed, the
  movement keys as the view points them for `press`, up for a jump), rolls
  set on the legs for move_body to drive, and the velocity sets move_body does not
  make yet: the saber specials' launches, the hops JA's saber-move command
  gives the dual and staff attacks while standing (`Builder::driven`), and
  held force jumps. So speeds, air time and landings are JA's steps (a
  225 jump: 33.7 units up, 600 ms in the air, `kjk move measure`); the
  scene has a checked floor at z 0. The staff's butterflies and back flip
  leap during their wind-up, as in JA (Q-0006).
- Cues follow JA's final animation set: a part already playing an animation keeps
  playing it unless the set restarts it (a combo repeating a move).
- `force` ends with a saber throw: `game::saber_flight` flies the hilt
  as JA's saber code does (out along the view, back to the hand) on the
  retargeted hand; a second copy of the hilt follows the path (`Shown::path`)
  while the hand's is hidden, and the load runs twice so the torso's return
  to ready (`BOTH_SABERPULL` held till then) falls on the flown catch.
- Facts: `creature_notes` (natural speeds, foot slide, top jolts, the
  sharpest steps of the look's angles per sample, ground track; jolt peaks
  off cues and landings are bugs, and an angle step past the legs' swing
  of 6.5° a sample is a snap), `blade_ends` (to aim close-ups). `kjk view ja <glm> --clip A,B --saber single_1` is JA itself.

## Checking a change
Compare b (JA exact) or JA with c at the same times; film c with a camera
following the ground track and look at every frame; hilt close-ups; re-read
the measures; try Bastila and `pmbam`/`pmha01`, `pfbam`/`pfha01`.

Journal (the owner's own; `context/` is not in git):
`context/artifacts/2026-09-29-i10-skeleton/` (3-PROBES, 4-MOVES,
5-DRIVEN, 6-ANGLES-FX).
