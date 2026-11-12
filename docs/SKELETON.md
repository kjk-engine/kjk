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
  showed it), and the sim's blades, whose skeleton (`sim::skeleton::turn`)
  mixes the same way.
- Legs mid-blend go by position (`Player::limbs_blended`): knee and ankle
  move straight from the captured pose's places to the new animation's,
  measured from the hip in the pelvis's frame; thigh and shin turn by the
  knee's bend plane (`bend_frame`), each keeping its roll off that plane
  taken the short way from the old pose's to the new one's, and the foot
  turns the short way. Bone by bone, a thigh half a turn of twist from its old pose carried
  the bent knee round with it: the medium combo's blend into
  `BOTH_T2_BR_TR` looped the right foot 1.9 m (40 cm out, 30 cm up) at
  23 m/s in 100 ms and spun the shin 347° about its own line; now 0.25 m
  at 2.6 m/s and 20°, on JA's skeleton and every body (JA's own femur bone
  still rolls; K1's thigh takes its roll from JA's shin). Ghoul2's lerped matrices fold the shin through the middle there
  instead. View only: the sim's skeleton mixes its own way.
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
- The spine follows JA's by height (`rig::spans`, `Curve::turn`). K1's
  `torso_g` starts about where JA's `lower_lumbar` does (0.30 of the way
  from the hips to the neck on Carth, JA's 0.27) and `torsoupr_g` where
  `upper_lumbar` does (0.50, JA's 0.51); taking the turns of the bones
  named alike (`upper_lumbar`, `thoracic`) bent each stretch a joint too
  high, on a longer lever, and hunched every body: the hips-to-head line
  of JA's crouch (`BOTH_CROUCH1IDLE`) 41° forward on Carth where JA's is
  29°, the medium ready stance 29° to JA's 20°. Each node now takes the
  JA bones' turns over the heights it spans, weighed by overlap, swung so
  it points where JA's spine curve does over them: 32° and 23° on Carth,
  27° and 18° on `pfbam` (36° and 23° before). Without the thighs, both
  spine nodes or `neck_g`, the old pairing stands. View only, as the rest.
- K1 has two humanoid skeletons, a man's (every male body, Carth's) and a
  woman's (every female body, Bastila's); three rules fit JA's motion to
  both:
  - A collar that starts off the spine keeps its own rest under JA's base
    pose (`off_spine`). The woman's starts 7.7 cm out and slopes 35° down;
    lined up level with JA's clavicle it lifted her shoulders 3 to 7 cm
    into her neck (now within 1 cm of JA's). The man's starts on the spine,
    as JA's does, and is lined up as before.
  - The pelvis turns about the hips: KOTOR's `pelvis_g` sits 13 (a
    woman's) to 19 cm (a man's) above its hip joints, JA's at them, so the
    root moves across to keep the hips' middle where JA's travel puts the
    pelvis. Feet slide beyond JA's own 16 to 55% less on a man, 31 to 72% on
    a woman (`kjk rig` timelines).
  - With JA's hands together (`TWO_HANDED`), the left wrist keeps JA's own
    spacing from the right (`Retargeter::spaced`) before any reach.
  - A woman moves as K1's women do (`Woman`, the skeleton with off-spine
    collars). K1 has no female attacks, but its women's own clips against
    its men's (`g2r1`, `walk`, `pause1`) stand the feet 8 to 10 cm nearer
    together, the elbows 3 to 5 cm nearer the body, cock the hips about 8°
    standing and swing them 16° walking (a man's 5°); JA's moves on her
    skeleton kept a man's width but for 1 to 2 cm. So:
    - The hips cock `HIP_COCK` (8°) over the foot that bears the weight
      (the one nearer under the hips and lower), about their middle, and
      fade out off the ground (`Retargeter::cocked`). Standing it is the
      cock; walking it swings the hips as the weight changes feet (19°).
    - Each leg bends its ankle back where it was, `NARROW` (3.5 cm, at
      most a quarter of its distance out) nearer the other: the feet stand
      5 to 7 cm nearer together.
    - Each elbow turns in toward the spine about the shoulder-wrist line
      (`Retargeter::tucked`, `ELBOW_IN`), so the hands and blades stay
      where JA puts them: 1.5 to 2.5 cm in, about K1's gap to a man's.
    - Feet slide and jolts are as before (`kjk rig a --body pfbam`).
- `rig::Grip`: the hilt hangs at KOTOR's `rhand` as K1 hangs weapons (blade
  +Z) and the hand turns so the blade points as JA's; fingers take KOTOR's
  `g2r1` pose (`Options::hold`). Where JA's tag bone carries the hilt off
  its hand (the victory toss, `BOTH_A6_SABERPROTECT`'s orbit, the taunts and
  show-offs; past `DEPART`, 12 to 35 cm, so the 5 to 15 cm JA's swings shift
  it stay in hand), the weapon node goes with it, scaled (`Posed::weapons`,
  keyed as the root is), and the hand goes back to JA's arm: the dual
  protect's hilts came 2 m from JA's, now 0.4. A staff (`Grip::staff`) is
  held by its middle; K1's is shorter than JA's (blade starts 0.37 m apart,
  JA's 0.58), so where JA's hands hold its staff 35 to 43 cm apart
  (`STAFF_HANDS`) the left hand takes K1's beside the right fist, short of
  the far blade (`staff_hold`, `STAFF_SPAN`). `rig::rebind` is probe (b),
  kept.
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
  A saber lock counts as a cut too, under both sets of saber rules, and
  the wrist then moves up to `LOCK_REACH` (0.6 m). JA's new locks (any
  lock with two sabers or a staff on a side) leave KOTOR's hilts up to
  half a metre from the sim's, the double-bladed hilt the most, so the
  blades that meet in the sim passed by each other as drawn.
- `blade_lengths` (ignition, 60 fps), `saber_trail_ms` (trail life per move,
  from JA's saber move table: style moves, defensive moves, special trails).

## Looking and strafing (`asset_ja::angles`)
- `angles::player_angles` is multiplayer's player-angles rule, a pure
  function of (`State`, `Moment`: view angles, velocity, `movementDir`,
  ground, what legs and torso play, the `Motion` bolt; ms): the legs' yaw
  (toward the line of travel, at most 60°, swung at 1.3°/ms), the torso's
  aim (for effects), and angle overrides for `lower_lumbar`,
  `upper_lumbar`, `thoracic`, `cervical` and `cranium`. The player
  controller feeds it real input.
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
  (`cast::at_rest`), then takes the JA pose and the clips over it, but for
  the nodes its standing clip (`pause1`) does not key (`cast::held_nodes`).
  KOTOR writes only the nodes the playing clip keys (reone, xoreos): a
  node the standing clip keys is set again whenever the body stands, any
  other keeps what the last clip keying it left. So a node the JA pose
  does not turn (`rootdummy`, which KOTOR's lying clips lay down) stands
  at rest once no clip keys it; before, it kept what the last clip left:
  Taris's duel ring loser (`k_ptar_dueler_ud`: `ANIMATION_LOOPING_DEAD`,
  then `ANIMATION_LOOPING_PAUSE` for 0.1 s and a jump to the lobby) stood
  46° aslant. And a robe's hood thrown back stays back: TSL's Handmaiden
  at Atris's academy (`handmaiden.dlg`'s welcome, `Hood_Off`: `offhood`
  keys `P_HandmaidenBB`'s hood, `pause1` does not) put it up again the
  moment the clip ended while every node was posed from rest. Probe `up
  again` (`kjk sim probes --only "up again"`: K1's body and clips, TSL's
  robe).
- In every conversation the line's pair turn their heads toward each
  other (`DIALOGUE.md`, "Lines"). While a conversation holds the world
  (scene mode), and only then:
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
  (forwards or sideways), its own clip of the same (`pause1`, `walk`, `run`, through its supermodel chain,
  so a woman's are the female ones; a creature's read with its look,
  `CreatureClips`) is laid over the JA pose. Anything
  else (a fight, a jump, a roll, a Force move) is JA's.
- Facing (the owner's call): moving, the body turns to the way it goes,
  720° a second, as KOTOR's characters do (KOTOR has no strafing): a
  strafe runs sideways facing its way, not crabbing at JA's 60° leg turn.
  Going backwards is JA's own backpedal (`BOTH_WALKBACK1`,
  `BOTH_RUNBACK1`): KOTOR has none, and its walk or run played in
  reverse looks wrong. Standing, it faces as JA's legs do.
- Each clip's weight eases in or out over 0.25 s, laid over the JA pose as
  one weighted mean, so standing to walking cross-fades the clips and the
  gait fades back into JA's pose. Standing plays at its own pace; walking
  and running by distance, a cycle per stride (`appearance.2da`'s
  `walkdist`, `rundist`), so the feet keep to the floor.
- The view only: the sim's skeleton (JA's) still decides what a blade
  cuts. `dump`'s `characters.roster[].clip.gait` gives each clip's weight.

## Duel stances (`game::duel_stances`)
- Movie Duels keeps each character look as a skeleton of its own beside
  `_humanoid` (`models/players/_humanoid_kotor/`, `_humanoid_maul/`, …:
  its own `_humanoid.gla` and `animation.cfg`, JA's sequence names played
  its way; `docs/SABER-RULES.md` section 11), and each of its models names
  the one it plays. With the setting `game.duel_stances` = `true` (off by
  default; read when a run loads), a character with its blade lit is drawn
  in its row's: while the sim plays one of JA's lit stances, their
  fidgets or walks (`BOTH_STAND2`, `BOTH_STAND2IDLE1`/`2`, the four
  `BOTH_SABER…_STANCE`s, `BOTH_WALK2`, `BOTH_WALK_DUAL`,
  `BOTH_WALK_STAFF`), the body plays the sequence its row of
  `data/duel_stances.toml` finds: in the row's `skeleton` (the same name,
  or one named outright), else in the stack's own `animation.cfg` (one
  named outright, or JA's name with each of the row's suffixes, for a
  mod that keeps its looks in one skeleton). A skeleton is read once and
  the frames taken from it are added after the view's skeleton's own,
  when its bones are the stack's, in order (one whose are not is said in
  the notes and lent nothing). The rows follow Movie Duels' own models:
  KOTOR's Jedi and Sith as `_humanoid_kotor`, Sion and Nihilus as
  `_humanoid_md`, Vandar as `_humanoid_yoda`. Rows go by template or tag
  (`p_bastilla*`), the player's own character, and a default; the first
  that matches, of those that found a sequence, wins. The run's notes (a
  dump's `scene.creature_notes`) say what each row found (`duel stances,
  Bastila Shan (_humanoid_kotor): BOTH_STAND2 as BOTH_SABERSTAFF_STANCE, …`).
- Picks: with the setting on, the character page's Stance gives the member
  shown any of Movie Duels' skeletons instead of its row's (every
  `models/players/_humanoid_*/_humanoid.gla` of the stack, but the
  droids' and creatures' `_bdroid`, `_sbd`, `_deka`, `_jabba`), for
  roleplay. The pick is kept in the player's settings, `saber.stance.<who>`
  = the folder (`who` as the headgear's choice: `seat0` for a seat's own
  character, else the creature's tag): a look of their game alone, never
  the sim's nor another seat's, the same in every save. A skeleton picked
  in play is read then (`DuelStances::repick`, from the run's stack, kept
  for it), its frames added to a copy of the view's skeleton that every
  figure moves to (`Cast::repick`, `Player::set_gla`; the copy before is
  left alive, its frames all in the new one); each is read once a run.
- A held block (the duel rules' button, `Duel::block_held`) shows its
  pose over a lit stance, its fidgets or (the torso alone) a walk, forward
  or back (`docs/SABER-RULES.md` 11.5), toward where its holder moves by
  the command the snapshot carries (`Fighter::last_command`): back left
  `BOTH_P1_S1_TL`, back right `TR`, back `T_`, forward left `BL`, forward
  right `BR`, forward `T_`, left `TL`, right `TR`. Standing still, with
  attack held the top parry `BOTH_P1_S1_T_`; without, the standing guard
  `BOTH_STAND_BLOCKING_ON` from the character's Movie Duels skeleton (its
  pick's or its row's) whenever Movie Duels is installed, the stances on or
  off, else the top parry. Two blades play `P6_S6`, a staff `P7_S7`, and
  `_DUAL`, `_STAFF` guards. The look's skeleton's own first, else the
  stack's (JA's has the parries, Movie Duels' the guards too).
- The other sequence is played from the sim's moment, at the sim's share
  of its rate (a walk sped by the pace keeps its speed), looping as its
  `animation.cfg` line says. A part going to, from or between such
  sequences blends from the pose it shows, over the sim's blend's length;
  JA's held-stance breathing (`stance`) stays off while one plays.
- A reverse grip: Galen Marek's stances, walks and guards turn JA's tag
  bone half a turn in JA's hand, about a line across the blade, and leave
  the hand on the arm as it was. Under the duel rules the hilt then turns
  in the KOTOR fist (`rig::Grip::fist`). The hand stays where JA's arm
  carries it and the weapon node takes the turn, from 60 degrees between
  the blade and where the hand would carry it, wholly by 120 (`REVERSED`).
  The rule applies by the share of a duel stance's pose in the torso's
  pose (`Figure::duel_share`), so it comes and goes with the blend to and
  from the stance. JA's own sequences twirl the hilt in the hand too (as
  much as 173 degrees in a dual attack), and there the hand turns with the
  hilt as before, as it always does under JA's rules.
- Nothing but the held block's guard changes without the setting, nothing
  at all without a mod (no such sequence),
  with no blade lit, or for a body drawn the cgame's way (`--quirks`) or
  dead. The view only, as `stance` and the gait: the sim's skeleton holds
  JA's sequence, the blades cut from it, and no digest moves.
  `kjk sim probes --only "duel stances"` checks the table, a pick, the
  held block's ways and the replay; `kjk gui probes` the character page's
  Stance (`character_stance`).

## Tools (`kjk rig a|b|c`)
- `--moves` a timeline (`showcase`, `acrobatics`, `specials`, `defense`,
  `force`, `staff`, `dual`, `reactions`, `melee`, `styles`, `gestures`,
  `look`; `game::rig_timeline`)
  or comma-separated sequences (`NAME:blend_ms`; `--saber none` holds
  nothing, as unarmed); `--body/--head` (default
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
the measures; try Bastila and `pmbam`/`pmha01`, `pfbam`/`pfha01`: every
change on a man's body and a woman's, the two skeletons K1 has. `--trace`
carries the hips and spine (`pelvis_g`, `torso_g`, the thighs) for foot
slide and hands in the body.
