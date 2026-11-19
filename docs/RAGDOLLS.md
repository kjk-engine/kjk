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
- Beasts and droids are fitted from their mesh alone (`Template::auto`).
  Every node that carries a real piece of mesh becomes a body. A flesh
  creature has at most 28 bodies, and a metal one at most 16. A flesh body
  is a capsule. A metal body is a box that covers all its vertices,
  because a capsule would let it roll.
- A node's piece is its own mesh and all the mesh that hangs below it,
  down to the next node that is a body. The largest pieces become bodies
  first. A forearm therefore comes before a tongue, and a paw with its
  claws comes before an eyelid.
- The root is the body with the most body below it. That is its own
  solid's volume plus the volume of every body below it.
- A body hangs on the nearest body above it in the skeleton. The joint is
  where its node hangs on its parent node.
- Some bodies have no body above them. Such a body hangs on the body,
  among those already hung, that its skin shares most with. Two skins
  share a vertex when the nodes of both bodies move it. The joint is at
  the middle of the shared vertices. The body that shares most with a
  hung body is hung next.
- A skin that shares no vertex with a hung body, such as a droid's rigid
  plates, hangs on the body its skin comes closest to. The joint is where
  the two skins meet. That is the middle of the hanging piece's vertices
  that lie within half its thickness of the closest point.
- A node can rest farther from its skin than its piece is long. Such a
  body hangs where its skin meets its neighbour's skin. It is drawn moved
  to that place, and not only turned about its node.
- A node that carries mesh but has no body at or above it rides on the
  body its skin shares most with (`Template::riders`). It keeps the
  place it has on that body at rest. Its skin counts as part of that
  body's piece, so a lower leg's capsule reaches down to the floor.
- A capsule's radius is at most 2 m. Only the four largest beasts have a
  part with a radius over 0.6 m. They are the krayt dragon, the rancor,
  the bantha and the firaxan shark. The largest radius is 1.98 m, on the
  krayt dragon's chest.
- The krayt dragon's skeleton is shaped like a star. Its paws, its upper
  legs, its forearms, its back and its tail all hang on one node that
  carries no mesh. When that node was the root, the dragon's paws became
  one stick, its back hung from that stick 4 m up, and its corpse folded
  into a heap (issue 558). Its root is now its chest. Each leg hangs on
  the chest, from the upper leg through the forearm to the paw, and each
  joint is at the seam of the skin. Hanging each body on the nearest
  capsule did not work. It hung the dragon's left front leg on its neck,
  because the chest's capsule does not reach its wide shoulders, and that
  leg stood propped on its paw.
- The bantha's skeleton is also a star, and its head is the only real
  tree in it. Counting vertices below each node made the head the root,
  so ten tonnes of body hung on the jaw. The head tossed about as the body
  fell, and the bodies took 23 s to come to rest.
- The bantha's skin is shared loosely between bones. A knee's vertices
  are half on each leg bone, and two vertices of the right leg are mostly
  on the left leg. Hanging on the closest skin hung its right lower leg on
  its left upper leg. Hanging on the most shared skin hangs each leg on
  its own hip.
- The bantha's horns and mane are bound to nodes that rest at its hips,
  2 m from their skin. Each horn swung on that 2 m lever. Turned about the
  hips, a horn lay on the sand as if it had been shed.
- The bantha's feet and one lower leg hang on a node that carries no mesh,
  so they had no body. They stayed where the death animation left them
  while the legs moved with the body, and the skin between them stretched
  into shards.
- Each body's inertia about any axis is at least a quarter of its inertia
  about the axis where it is largest (`world::ROUNDEST`). A long, thin
  capsule spins about its own axis far more freely than end over end. The
  krayt dragon's parts are up to 10 m long and 0.6 m across. Without this
  limit, the joint solver threw the dragon's body apart within a second of
  free fall. Changing its mass, its size or the solver's iterations did
  not help (issue 558). A limit of a sixth still let the dragon fly apart.
- A person's limb has about a sixth of its end-over-end inertia about its
  own bone. The limit of a quarter makes a person's limbs spin about their
  own bones a little less freely. The approved probes pass unchanged.
- `Material`: density, how far joints give (flesh 45° swing 30° twist,
  metal 12° and 5°), friction, bounce, and joint friction (a limp joint
  loses a share of its spin each step and holds fast below a speed; metal's
  servos lock where they stop).
- Ball joints swing in a cone centred on the limb's reach and twist within a
  limit (Rapier's up to 40° cones; ours, about the bone's own axis, beyond:
  Rapier's pushes about the parent's axis and fights a wide cone); elbows
  and knees are one-way hinges. Limbs touch other limbs, never the core.
- A ball joint whose bodies start outside its limits is eased
  (`world::eased`). Its cone is turned toward the bone by the amount the
  bodies are past the limit, and the middle of its twist is turned the
  same way. The starting pose then lies at the limits and never beyond
  them. The krayt dragon's dead clip bends its paws 60° to 100° from their
  rest on its forearms. When the solver snapped them back, it threw a leg
  8 m into the air (issue 558).
- The level: the sim's brushes around the body, solid as the snapshot's
  doors are when first taken in (`collision::Level::faces_in`; a door that
  opens later stays as it was), taken in again as it travels.
- Each body starts at its parent's joint (`world::joined`). A dead clip
  can move a node that has no body, and that left the bodies below it
  metres away from the rest (the krayt dragon's, issue 558).
- A body whose death animation still plays when its ragdoll starts can
  have its root under a floor. The whole body is then lifted until the
  root's solid rests on the highest floor between the root and the
  highest part (`World::lifted`). Only faces that point up count as a
  floor. A body met already dead is never lifted (see below).
- A part that would start behind a level face, seen from the root's
  middle, starts in front of it instead, a radius short of the face
  (`World::kept_near`). A KOTOR wall is a single face, and the sim's body
  is narrower than the arms. A creature killed against a wall can
  therefore have a hand through it. Started there, the part was held
  behind the face and the body stayed stuck in the wall (issue 416). A
  body met already dead is not moved this way either.
- The level's faces are taken in around the pelvis, in a box that
  reaches 6 m below it and 3 m above it (`game::ragdolls::GATHER`). To
  each side, the box reaches as far as the body's solids do plus 2.5 m,
  and at least 5 m. The faces are taken in again once the pelvis has
  moved 2.5 m from where they were last taken in (`REGATHER`).

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
- A dead body is posed every frame until its death has played. That is
  10 s after the view first had it dead, or at once for a body met dead.
  It is posed
  wherever the camera is, and from its last snapshot once the sim has
  taken it away (`Cast::draw`): it lies on as its death left it, never in
  the pose it had when it went.
- A body can already be dead when the view first meets it, after a load
  or when a player joins. Its death is then taken as played. The
  snapshots do not say when it died, so it is taken to have died 10 s
  before it was met. Its bodies start limp in its dead pose
  (`World::spawn`, `settled`). They are neither lifted nor kept out of
  the level's faces. A part that starts in the ground is held there, and
  the rest of the body sags onto the floor.
- A humanoid's JA death clip is timed from that death, so a body met dead
  is drawn at the clip's last frame, lying down. A creature's dead clip
  is timed from its death in the same way. When the clip was started at
  the moment the body was met, its first frame drew the body standing. A
  Gamorrean's ragdoll began there, with its feet held in the floor, and
  sagged into a squat.
- A body let go that lies still (1 cm, 3° in 0.4 s), or has been all but
  down for 1.5 s (a cut limb's joint, held at its limit against the floor,
  can roll it on for ever), sleeps; asleep, it is neither posed nor placed
  until something wakes it. A body let go sleeps after 6 s in any case.
  Light parts hung on much heavier ones (the plates under a Czerka defense
  turret's base on Kashyyyk) can shake on the floor for ever, and each such
  body cost a physics step every frame while its wreck lay there.
- A sleeping body wakes when an awake body touches it. Two banthas that
  died on each other were each put to sleep in turn, and they woke each
  other for 20 s. Now, when a body is put to sleep, a body touching it is
  put to sleep with it when three things are true. Its muscles are fully
  let go, with no hold and no tone. It is all but down. It has been all
  but down for at least 0.5 s (`world::SLOW_FOR_TOGETHER`). The bodies
  touching those go to sleep too, and so on.
- A body that is still dying can stand still for a moment while its
  muscles hold it up. A sleeping body is no longer driven, and only a
  driven body with some hold wakes. Put to sleep beside a corpse, a dying
  body therefore stood for ever.
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

## Seeing the loose parts (`game::loose_view`)

`kjk view` and `kjk rig` can draw the capsules and the loose bones of a
body over it, so that a person can check them by eye. The flags are the
same in both. They draw over the first model of the scene, which is the
body.

- `--capsules` draws the 14 capsules of `Template::fit` as wire frames.
  Each capsule is fixed to its node, so it follows the animated skeleton
  every frame. The right arm is cyan, the left arm blue, the right leg
  yellow, the left leg orange, the head pink and the trunk white.
- `--ghost 0.4` draws the body's surfaces at that opacity and a little
  brighter, so that the capsules and bones can be seen through a robe.
  `--ghost 0` hides the body.
- `--marks` puts a red dot on each garment vertex that starts inside a
  capsule when the body stands at rest. The dots move with the vertices.
  A garment vertex is one that a loose bone moves most, or one in the
  `legs` mesh of KOTOR 1's Jedi robes (`pmbj…`, `pfbj…`), which holds the
  whole robe. The run's log and its dump (`loose_view`) give the counts
  and the depth of the deepest vertex.
- `--bones` draws each loose bone as a line from its parent to its joint,
  with a square at the joint. A bone is loose when its name holds `robe`,
  `cape`, `cloth`, `flap`, `hood`, `sleeve`, `belt`, `sash`, `shirttie` or
  `shldr`. It is orange for a cape, cyan for a robe, yellow for cloth,
  green for a flap, pink for a hood, violet for a sleeve and white for the
  rest. A node that only carries a mesh (`robe_geo`) is not drawn.
- The camera orbits the body and follows its pelvis. The left mouse button
  turns it and the wheel zooms. `--angle` sets the first angle in degrees
  from the front, `--zoom` sets how much closer than the whole-body
  framing it starts, and a `camera` command in `--cmds` takes the camera
  from it.

`kjk rig` takes `--game tsl` for KOTOR 2 bodies and `--head none` for a
body that has a head of its own, such as Malak's. `kjk view` takes
`--clip NAME` to play one clip of the model in a loop, `--clip-from
MODEL` when another model holds the clip (a cutscene's stunt model holds
the clips of a character that another model draws), and `--slow 0.4` to
play it at 40% of its speed. A `record` command steps held time, so a slow
clip records slowly too.

The capsules are only fitted to humanoid skeletons. `--marks` and
`--capsules` do nothing for a beast or a droid, and say so in the log.
