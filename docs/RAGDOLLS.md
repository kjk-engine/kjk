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
- Beasts and droids from their mesh alone (`Template::auto`): every node
  carrying a real piece a body (28 at most, 16 for metal), each a capsule
  (flesh) or a box covering all its vertices (metal, which a capsule
  would let roll). A node's piece is its own mesh and all that hangs
  below it short of another body, and the largest pieces become bodies
  first, so a forearm comes before a tongue and a paw with its claws
  before an eyelid. The body with the most of the body below it (its own
  solid's volume and its descendants') is the root. A body hangs on the
  nearest body above it in the skeleton, where its node hangs on its
  parent's. A body with none above it hangs on the body of those already
  hung whose skin its own skin blends into most (the share of its skin
  the other's nodes move too), and the joint is at the middle of that
  blend; the body most blended into a hung one goes next. A skin that
  blends into no hung body (a droid's rigid plates) hangs on the one its
  skin comes closest to, and the joint is where the two skins meet (the
  middle of the hanging piece's vertices within half its thickness of
  that closest approach).
  The krayt dragon's skeleton is a star: its paws, its upper legs, its
  forearms, its back and its tail all hang on one node that carries no
  mesh, so rooting it there made its paws one stick that its back hung
  from 4 m up, and its corpse a folded heap (issue 558). Its root is now
  its chest, and each leg hangs on it from the upper leg through the
  forearm to the paw, each joint at the seam of the skin. Hanging a body
  on the nearest capsule instead hung the dragon's left front leg on its
  neck, since its chest's capsule misses its wide shoulders, and that leg
  stood propped on its paw. A capsule is at most 2 m thick. Only the four
  largest beasts (the krayt dragon, the rancor, the bantha and the firaxan
  shark) are thicker than 0.6 m anywhere.
  The bantha's skeleton is a star too, and its head is the one real tree
  in it, so counting vertices below made its head the root and hung its
  ten tonnes of body on its jaw: the head tossed as the body fell, and
  the bodies took 23 s to come to rest. Its skin is blended loosely (a
  knee's vertices half on each leg bone, two of the right leg's mostly on
  the left), so the closest skins hung its right lower leg on its left
  upper leg; the blend hangs each leg on its own hip. Its horns and its
  mane are bound to nodes resting at its hips, 2 m from their skin, and
  swung on that lever: a node resting farther from its skin than its
  piece is long is hung where the two skins meet instead, and drawn
  placed, not only turned about its node (a horn turned about the hips
  lay on the sand like one shed). Its
  feet, and one lower leg, hang on a node without mesh and had no body,
  so they stayed where the death animation left them while the legs went
  with the body, the skin between stretched into shards: a node carrying
  mesh with no body at or above it rides on the body its skin blends into
  (`Template::riders`), placed on it as the two are at rest, and its skin
  counts to that body's piece (a lower leg's capsule reaches the floor).
- Each body's inertia about any axis is at least a quarter of its inertia
  about the axis it is largest about (`world::ROUNDEST`). A long thin
  capsule spins about its own axis far more freely than end over end. The
  krayt dragon's parts are up to 10 m long and 0.6 m across, and the joint
  solver blew its body apart within a second of free fall, whatever its
  mass, its size or the solver's iterations (issue 558). A least share of
  a sixth still let the dragon blow apart. A person's limb has about a
  sixth of its inertia end over end about its own bone, so the least share
  raises its spin about the bone a little; the approved probes pass
  unchanged.
- `Material`: density, how far joints give (flesh 45° swing 30° twist,
  metal 12° and 5°), friction, bounce, and joint friction (a limp joint
  loses a share of its spin each step and holds fast below a speed; metal's
  servos lock where they stop).
- Ball joints swing in a cone centred on the limb's reach and twist within a
  limit (Rapier's up to 40° cones; ours, about the bone's own axis, beyond:
  Rapier's pushes about the parent's axis and fights a wide cone); elbows
  and knees are one-way hinges. Limbs touch other limbs, never the core.
  A ball joint whose bodies start outside its limits has its cone turned
  toward the bone by the excess, and its twist's middle likewise, so the
  start pose lies at the limits and never beyond (`world::eased`). The
  krayt dragon's dead clip bends its paws 60° to 100° from their rest on
  its forearms, and the solver snapping them back threw a leg 8 m into
  the air (issue 558).
- The level: the sim's brushes around the body, solid as the snapshot's
  doors are when first taken in (`collision::Level::faces_in`; a door that
  opens later stays as it was), taken in again as it travels.
- The bodies start joined up at their joints (`world::joined`): a dead
  clip can move a node that has no part, and leave the parts below it
  metres from the body (the krayt dragon's, issue 558). A root that starts
  under a floor is lifted with the whole body until its solid rests on the
  highest floor between it and the highest part (`World::lifted`). A part
  that would start behind a level face, seen from the root's middle,
  starts in front of it, a radius short of the face (`World::kept_near`).
  A KOTOR wall is a single face, and the sim's body is narrower than the
  arms, so a creature killed against a wall can have a hand through it.
  Started there, the part was held behind the face and the body stayed
  stuck in the wall (issue 416). The faces are taken in 5 m around the
  pelvis, or as far as the body's solids reach plus 2.5 m when that is
  more (`game::ragdolls::GATHER`, `REGATHER`).

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
- A dead body is posed every frame until its death has played (10 s from
  when the view first had it dead, or at once for one met dead),
  wherever the camera is, and from its last snapshot once the sim has
  taken it away (`Cast::draw`): it lies on as its death left it, never in
  the pose it had when it went.
- A body already dead when the view first meets it (after a load, a
  player joining) died before the view saw it: its death is taken as
  played, and its bodies start limp in its dead pose (`World::spawn`,
  `settled`), neither lifted nor kept out of the level's faces; a part
  that starts in the ground stays there. A humanoid's JA death clip is
  started when it died, so a body met dead is drawn at the clip's last
  frame, lying; a creature's dead clip plays from its death likewise.
  Started at the moment it was met, the clip's first frame drew the body
  standing, and a Gamorrean's ragdoll began there, its feet held in the
  floor, and sagged into a squat.
- A body let go that lies still (1 cm, 3° in 0.4 s), or has been all but
  down for 1.5 s (a cut limb's joint, held at its limit against the floor,
  can roll it on for ever), sleeps; asleep, it is neither posed nor placed
  until something wakes it. A body let go sleeps after 6 s in any case.
  Light parts hung on much heavier ones (the plates under a Czerka defense
  turret's base on Kashyyyk) can shake on the floor for ever, and each such
  body cost a physics step every frame while its wreck lay there. A
  sleeping body wakes at the touch of one awake, so two banthas dead on
  each other, each put to sleep in its turn, woke each other for 20 s: a
  body put to sleep takes with it every other body let go and all but
  down.
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
  closed by a cap fanned from its middle, lit as the body it is cut from. A
  lightsaber's is seared, glowing at its rim whatever the light and cooling
  over 3 s to a burn (metal: hot to scorched); its
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
