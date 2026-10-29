# The sim's skeleton: where the blades are (`sim::skeleton`)

JA's server finds every blade each frame from its own Ghoul2 skeleton to
decide hits, blocks and clashes (rulebook 5.1); the sim does the same, on
the same bits everywhere (`SIM.md`). It turns bones the way the view draws
them, so the blade that cuts is the blade drawn (QUIRKS.md Q-0080 to
Q-0083).

## Data (`SkeletonData`, built by `game::skeleton`)
- The `_humanoid` bones on the chains from `model_root` to the bones the
  `*r_hand` and `*l_hand` tags of `models/players/kyle` hang on, and to the
  `Motion` bone (12 of 53), parents first: parent, base pose and inverse,
  and per GLA frame the compressed bone as stored (7 `u16`; the pool cut to
  what they use).
- Each hand tag's first three vertices (position, up to four bones, the
  file's 10-bit weights); each saber's `*blade<n>` tags (three vertices on
  the hilt's one bone) and lengths. Checked once (`SkeletonError`), so
  evaluating cannot fail; `digest()` is BLAKE3 over all of it.

## Pose
A `Playback` for the legs (`model_root`, every bone not under the torso),
one for the torso (`lower_lumbar` and below, and `Motion`), the look's five
angle overrides (degrees). A `Playback` is Ghoul2's bone animation (frame
range, frames per 50 ms, start ms, looping, blend: start, length, captured
`frame` and `next`, and the blend it began in, `Under`), set and timed as
`G2-SPEC.md` says (`set`, `frames`); the pick-up rule is the caller's
(`begin`).

## A bolt (`bolt`, model space, facing +X)
- A compressed quaternion is made unit (`turn`). A part's two frames turn
  along the shorter arc at an even pace, each joint's place lerped and the
  translation taken from the mixed turn (`joint`: the joint stays on its
  parent, as Ghoul2's lerped matrix keeps it); a blend turns from
  the pose it starts from into the moving animation on the side that was
  the shorter when it began, at Ghoul2's lerped matrix's pace within a
  quarter turn, easing to an even pace by half a turn (the renderer's
  `asset_ja::g2`, QUIRKS.md Q-0080). The pose a blend starts from is the
  captured `frame` toward `next`; set while another blend was under way, the
  pose that blend showed (`Under`, one level deep, Q-0081). Final = parent
  × local; the root's parent is Ghoul2's root matrix (−Y to +X). A look bone
  then × Base · R · Base⁻¹, R the columns forward, left, up of (pitch, roll,
  yaw).
- A tag: its vertices skinned (weights `w × (1/1023)`, the last taking what
  the others leave); axes from the long side (0→1) squared to the short
  (2→0) and their cross; columns (short, long, −cross) at vertex 2. A
  blade's tag sits on the hilt's one bone, which is the hand's bolt.
- `motion`: the `Motion` bone's bolt (its place × its base pose), which the
  look reads.
- Products summed left to right, translation last, as Ghoul2's; only
  `+ − × ÷ sqrt`, trig from `sim::math`, no allocation.

## In the world (`world`, `Placement`, `Blade`)
Rows made unit, turned to the yaw and moved to the origin, then the axes
turned a quarter as multiplayer's bolt query does (new X = −old Y, new Y =
old X). A blade runs from the origin along −Y (the tag's old −X) for its
length. `Placement::ahead` pushes the origin along the unit velocity by
Σ|vᵢ| × `push_per_speed`, at most 70 (JA: 1.6 / `sv_fps`, 0.04 at 40 Hz;
a parameter, the sim's 125 Hz tick not being JA's server frame).

## In the level (`AnimTable::skeleton`, `combat::saber::blades`)
`stage::anims` builds it with the animation table (sabers `single_1`,
`dual_1`); the level's digest covers it, and without JA's data there is
none and no blade has a place. Each fighter keeps the server's pose
(`Posed`, as JA updates a client's animations): a new animation or restart is set at the
tick's time with a 150 ms blend, the torso at its style's speed; a lock
freezes both on its frame. Posed with the server's look
(`combat::saber::look`, JA's player-angles rule as the server runs it):
the legs on the view's yaw turned toward the travel at once, the spine
taking the rest of the view's yaw and half its pitch less the torso
animation's own turn (its `Motion` bolt), the head level (Q-0082); straight
in a lock. Placed at the legs' yaw, pushed 1.6 ticks of travel ahead. A
swinging blade keeps its last place (`Trail`, stale after 100 ms) for the
next sweep.

## Divergences
- From Ghoul2's lerped matrices (what JA's server cuts with): up to 30
  units at the tip between frames, 114 blending (Q-0080); a change
  mid-blend from the pose shown (Q-0081).
- From the renderer: none but a change mid-blend twice running (it keeps
  the whole pose shown, the sim one blend under another): 2.3 units mean,
  5.6 at most.
- `G2-SPEC.md`'s playback without quirks (not Q-0003, -4, -10, -28); rage's
  speed not applied. Not modelled: model scale, hilts with a skeleton
  (refused), the server's emplaced-gun and vehicle looks.

## Seeing it (`kjk play --sim-blades`)
`kjk play` draws each fighter's blade where the sim has it: its parts play
as the sim set them, and the gripping hand reaches for the sim's blade
(`game::cast`, `game::sim_blades`, `asset_ja::rig::Reach`; Q-0083).
`kjk play`, `kjk host` and `kjk join` with `--sim-blades` (`cargo xtask
duo --sim-blades on`) draw the sim's blades beside the drawn ones: green
the sim's, worked out again in the view from the snapshots' pose (at a
tick's time equal to the trails the snapshots carry, which check it; but
for a character the tick's own hits moved after its blades were placed);
cyan JA's skeleton as the view's Ghoul2 player poses it, before the
retarget; yellow from the drawn tip to the sim's. The gaps of each pair
(hilt ends, tips, angle) go by torso animation into the dumps
(`sim_blades`) and frame by frame into `blade-gap.csv` in the run's folder;
characters not posed this frame (between their turns beyond `cast::POSED`,
or beyond `cast::FAR_RANGE`) are drawn but not
measured.

Probe: `kjk sim skeleton [--saber single_1] [--step 1]` (`--release`): the
sim against the renderer and Ghoul2's method in `f64`, the hash of every
result (per install: it covers the data's digest), a bolt's cost.
