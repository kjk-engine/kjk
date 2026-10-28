# Ragdolls (`ragdoll`, `game::ragdolls`)

Every character's death is an active ragdoll, view-side only: the sim's
corpse (origin, hits, snapshots) never moves with it, so it need not be
deterministic and costs the sim nothing. Rapier, 120 steps a second.

## The body (`ragdoll::Template`, fitted once per body model)
- Humanoids by `humanoid::PLAN`: 14 capsules (pelvis, belly, chest, head,
  upper arms, forearms with the hand, thighs, shins, feet), each body's
  frame its node's, radii fitted to the skin (65% of its vertices inside).
  A body whose head is a model of its own (appearance model type B: the
  commoners, the Republic's soldiers, the player's bodies) has no `neck_g`
  or `head_g`: its `headhook` stands in for both (`Plan::stand_ins`), the
  head model hung there going with the head part. `kjk clips k1` counts
  how every creature's body lies when dead (ragdoll or not and why, its
  dying and dead clips, the bodies left standing): K1 1073 of 1074
  humanoids fall.
- Beasts and droids from their mesh alone (`Template::auto`): rooted at the
  deepest node all the mesh hangs from (else a piece above the root stays
  where the clip had it), every node carrying a real piece a body (20 at
  most, 16 for metal), each a capsule (flesh) or a box covering all its
  vertices (metal, which a capsule would let roll).
- `Material`: density, how far joints give (flesh 45° swing 30° twist,
  metal 12° and 5°), friction, bounce, and joint friction (a limp joint
  loses a share of its spin each step and holds fast below a speed; metal's
  servos lock where they stop).
- Ball joints swing in a cone centred on the limb's reach and twist within a
  limit (Rapier's up to 40° cones; ours, about the bone's own axis, beyond:
  Rapier's pushes about the parent's axis and fights a wide cone); elbows
  and knees are one-way hinges. Limbs touch other limbs, never the core.
- The level: the sim's brushes around the body, solid as the snapshot's
  doors are when first taken in (`collision::Level::faces_in`; a door that
  opens later stays as it was), taken in again as it travels.

## The death
- On the death frame the bodies start in the drawn pose, moving as the sim
  moved it (capped: JA flings its dead at its knockback's speed), struck by
  the killing hit at its location's part from the striker's side.
- The death animation (a humanoid's JA death, a creature's KOTOR dying clip)
  stays the target. `hold` pulls each body toward its place, less their
  sum: it shapes the body and turns it upright, never carries it, so the
  centre of mass is the animation's velocity once (the blow's push kept),
  then momentum and gravity alone. `tone`, the
  muscles, turns each body toward its pose on its parent (450 N·m at most).
  Hold goes by 60% of the animation; tone tires to 30% by its end, gone
  0.7 s after.
- A dead body is posed every frame until its death has played (10 s),
  wherever the camera is, and from its last snapshot once the sim has
  taken it away (`Cast::draw`): it lies on as its death left it, never in
  the pose it had when it went.
- A body let go that lies still (1 cm, 3° in 0.4 s), or has been all but
  down for 1.5 s (a cut limb's joint, held at its limit against the floor,
  can roll it on for ever), sleeps; asleep, it is neither posed nor placed
  until something wakes it.
- A character alive again has its fall let go (`Ragdolls::rise`): its
  bodies leave the world and it is posed as the living are. The sim raises
  only the downed (`EffectResurrection`, `FALLING.md`), who never fall as
  ragdolls, so none does yet.
- Drawn: each part's node turned as its body is (placed too where it does
  not hang below its ragdoll parent: KOTOR's thighs); other nodes keep the
  animation's turn. Dumps: `characters.ragdolls`, `ragdoll_ms`, `falls`.

## Dismemberment (`ragdoll::cut`, `game::severing`)
- A blade's killing blow cuts a part off about one time in three, as JA's
  saber cuts (our numbers): a lightsaber's, or (the owner's addition) an
  edged melee weapon's, one that slashes or pierces (a sword, a vibroblade;
  never a staff or a baton). It cuts the head, the waist (the body in half), an arm, the
  right hand (the forearm) or a leg, by the hit's location; on a body fitted
  without a plan, the part nearest where the hit landed. Which deaths cut,
  and how, is a number of the character and its killing hit's tick, so
  every client draws the same cut and the sim never knows. A hit older than
  the view's history (350 ms: a body first seen dead by a player joining)
  cuts nothing.
- The ragdoll lets the part go (`World::sever`): its joint goes, it and all
  that hangs on it go limp and touch what they were cut from. Thrown across
  the striker's line and up (thrown along it, from behind the striker, a
  limb hides behind its own body until it clears it), the way the blade went
  through it, tumbling about the blade's line; a piece over 5 kg as much
  slower as it is heavier (a head flies, half a body slumps off).
- Where and when the blade went through: the striker's blade in the
  snapshots of the 250 ms up to the hit, nearest the joint. The sim pays a
  saber's hit when the blade has left the body, up to 80 ms later; the limb
  leaves as far on as it would be since (50 ms at most).
- The meshes (`SkinnedMesh::cut`): each triangle goes with the side its
  skin leans to, each side skinned again so nothing stretches across the
  cut (the stump's weights on the limb's bones to the joint, the limb's on
  the body's to its root). The stump's slot is the nearest node above the
  cut the skin has, or the body's bone the limb's skin blends into most (a
  neck skinned beside the chest). A mesh all the limb's goes with it whole.
- Every open loop of edges at the joint, the cut's or the model's own (a
  sleeve, an armhole; two pinched through one corner each closed), is
  closed by a cap fanned from its middle. A lightsaber's is seared, glowing
  at its rim and cooling over 3 s to a burn (metal: hot to scorched); its
  cut puffs JA's `saber/saber_cut` and both wounds smoulder (black smoke)
  for 0.9 s. A blade's is raw (metal: bare) and puffs nothing. A droid of
  rigid pieces needs no cutting: its pieces go with their nodes.
- A model hung on a node nearest the cut part (KOTOR's head, its own model
  at `headhook`, which hangs off the torso) goes with the part, its own
  wound closed, and is hung back when the world starts over.
- Every client cuts the same (the killing hit's tick): filmed with a host
  and two joined players at 100 ms round trip, all three dumped the same
  cut (`characters.falls[].cut`).
- The meshes are parted the frame the limb is let go, before the renderer
  takes the frame's new meshes (`AssetEventSystems`): a limb flying off
  under the whole mesh, for even a frame, stretches it. A world started
  over in the same scene puts the whole meshes back (`severing::uncut`).

## Filming deaths
`kjk play --aim TAG` turns the scripted player's view each tick to the
chest of the nearest creature with that tag (the living first, then the
body): scripted fights hit, and the camera stays on the fall. Approved
test: `ragdoll_physics` (docs/TESTS.md).

Journals (the owner's own; `context/` is not in git):
`context/artifacts/2026-10-02-ragdolls/`,
`context/artifacts/2026-10-02-dismemberment/`.
