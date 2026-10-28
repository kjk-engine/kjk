# Collision (`collision`, `collision_odyssey`)

JA's traces through KOTOR's levels, in the sim's space (KOTOR world, metres,
+Z up; JA units convert with `collision::UNIT`, an inch).

## The contract and `Level`
- `CollisionWorld`: `trace` (a box; JA's `trace_t`, surface = walkmesh
  material), `sweep` (box or JA's `Capsule`), `point_contents`. `BoxWorld`
  is the boxes-only stand-in; `Level` answers as it does on boxes, bit for
  bit (one shared clip, `TRACE-SPEC.md`), and holds any convex brush: JA's
  bevels (axial, edge × axis), stop `CONTACT_GAP` short. A triangle is
  a slab 1 cm deep behind its face (a flat brush has no inside to clip).
- A bounding-volume tree built from the brushes in order (median splits,
  ties by index), walked nearest child first; of equal hits the lowest
  brush wins, so no answer depends on the tree (`trace_every_brush` checks
  it), but for one case: a brush whose near-level face stands in for its
  +Z bevel can report a hit before the path enters its grown bounds, and
  whether the tree clips it depends on its visiting order. `digest()`
  names the built level. Capsules are square at brush edges (Q-0019).
  Where OpenJK's own traces differ: Q-0020 to Q-0022.
- `DropIndex` (`drop_index`, `trace_down`): the brushes by columns of an
  XY grid, for one box falling straight down. It gives the tree's answer
  (it clips every brush the tree would, by each one's highest reach, and
  falls back to the tree where a near-level face could decide it) without
  walking the tree; the audit's falls go through it.
- Door and placeable brushes are solid in some of their entity's states.
  A level holds a state per entity (`set_state`, for the verbs); the sim
  keeps its own in snapshots and traces `level.with_states(&states)`, an
  overlay on the shared level (nothing is rebuilt when a door moves).
  `CollisionWorld::level()` finds the built level behind a world.

## KOTOR areas (`collision_odyssey`)
- Rooms: every drawn triangle of the room models (LYT place, node rest
  transforms) and every walkmesh face (`.wok`, world space). A drawn
  triangle on a walkmesh face takes its material; the rest have 0.
- Doors: `<model>0/1/2.dwk` in states 0 (closed), 1, 2 (open), less open
  ones that are a sheet edge-on in the doorway, which KOTOR's point walker
  never meets (`placeholder`). Placeables: `<model>.pwk`, state 0.
- Placeables' footprints (`footprints.rs`): KOTOR's walker never steps
  onto a non-walkable face of a placeable's walkmesh, and every K1
  placeable walkmesh with faces is only that, a footprint flat on the
  floor (a swoop bike's, a footlocker's, a cage's ring: 98 models, 1664
  instances, by `kjk collision build k1 all`'s census), which a JA box
  walked over. The outline of a walkmesh's non-walkable faces that face
  up gets `WALK_CLIP` walls owned by the placeable (solid in state 0),
  set back into it by the player's half width, so the box's centre stops
  at the outline as KOTOR's point does; corners meet as the outline set
  back meets, a corner sharper than a right angle (a bike's nose) gets a
  wall down its middle to the set back short of its point, and a
  footprint too small for any wall a post at its middle. Up from under
  the footprint to past any jump or a walkable floor overhead, as the
  edge walls. The sim gives a placeable state 0 in every look it takes
  (open, on, an animloop: KOTOR's walkmesh does not change with the
  look); only one gone from play lets bodies by. The navigation mesh is
  baked from the floor cut along the walled footprints' faces
  (`footprints::cut`, in `AreaCollision::nav_faces`), and marks a face
  mostly over a footprint (three of `nav::mark`'s four points), so
  routes go round the footprint and past it: uncut, a street's face a
  junked swoop stood on in part on Taris was shut whole, and a route on
  Manaan ran straight over a broken droid's footprint into its walls.
- Edge walls (`edges.rs`; the owner's pick of 2026-10-02, section 6 of the
  whole-game sweep): KOTOR's walker only steps to a point
  whose walkmesh face is walkable (reone, xoreos), so every edge of a
  walkable face that no walkable face continues (looked for 2 and 15 cm
  past it, from 0.5 m above to 1 m below) gets an upright slab 0.1 m
  thick of `WALK_CLIP` (in `PLAYER_SOLID`: only walkers' moves meet it;
  the camera, shots and sight lines pass), set back the player's half width (15 units) so the
  box's centre reaches the edge as KOTOR's point does. It runs on past
  corners the walkable faces turn away from and stops short of corners
  they wrap round, where the outline set back meets the next edge's, so
  no wall's end reaches into floor a centre may stand on; from 0.1 m below the edge
  to 100 m above it, or up to a walkable floor overhead (more than 0.5 m above the edge
  where it stands, nearer is the floor past it that continues it). What makes an edge open, and how its walls meet (each rule
  closed real gaps the whole-game sweep found, in both games; the probes
  `edge_walls_slight_bend` and `edge_walls_wide_turn` in
  `collision_odyssey::probes`, run by the approved test `surface_probes`,
  hold the third):
  - inside the floor is an edge with walkable faces on both sides of it;
    two on one side overlap (a sliver over a larger face: K1 danm14aa),
    and the side beyond is open;
  - a walkable face with no area (two corners on one vertex) is no
    floor: its doubled edge had read as continued (K1 tar_m04aa);
  - at a corner the wall runs on or stops short by the set back times the
    tangent of half the turn to the outline's next edge (the half width
    at a right angle, centimetres at a slight bend), and half a thickness
    on so the walls touch; a flat half width had stopped both walls of a
    slight bend half a box short, leaving a gap a player walked through
    (TSL 601dan, 710kor, 701kor), and run on into the floor of a wide
    turn would stick walks (TSL 710kor). Where the outline is not one
    edge on (a vertex several meet at), or goes on along an edge with no
    wall (a step down to a lower floor, K1 manm26aa), the floor just past
    the end decides, by the half width, as before;
  - a walkable floor overhead lowers only the stretch of wall under it
    (half a metre at a time), not the wall's whole run: a ramp over the
    end of an edge had lowered the whole wall to a metre, which a plain
    jump clears (TSL 002ebo, 262tel); and a floor is overhead from half a
    metre up, looked for over the edge where each stretch stands: from a
    metre, a lower floor's wall had risen through the first stretch of a
    ramp beside it, a slot by the ramp's own wall that stranded T3 in the
    TSL prologue (002ebo). `KJK_EDGE_WALLS=off` (or a height in metres) builds the
  `kjk collision` verbs' levels without them (or that high), to compare.
  Reaching the sky, they end jumping off the walkmesh too (furniture in
  its holes, ledges); opening the world up for jumps is a later step.
- Non-walkable walkmesh faces (`surfacemat.2da`'s walk 0: non-walk,
  obscuring, lava, deep water), and drawn triangles taking their material,
  are `SOLID | NONWALK`. KOTOR's walker, a point, never stands on one;
  move_body's step up does not land with the box's middle over one (a body at
  a table's edge stepped onto its lip, then its top, within the half
  width the edge walls stand back); the box may overhang one (a sill).
  Drawn surfaces no walkmesh face covers (material 0) are plain solid.
- Clip brushes: `data/clip/<game>/<module>.clip`, built in (`clip_file.rs`).
- `build_module` (entry area + built-in clip file), `set_doors`; in kjk,
  `collision_verbs::module_level(resolver, game, module)` gives the level
  with its openable doors open (the verbs'); `session::plan` builds it for
  the sim, doors as the snapshot has them (`docs/LIFECYCLE.md`).
  Built at load, about 0.2 s in release (0.5 s at most, every K1 and TSL
  module); a session keeps each area's level with its navigation mesh in
  the store (`session::Bakes`, `area-v6`; `NAV.md`), the verbs build
  afresh; not in snapshots: entity states are sim state.

## Containment audit (`collision::audit`)
Floods from where the game puts players (entry, waypoints; `--seeds
walkmesh`: every walkable spot) by walking (18-unit steps) and jumping per
reach level (plain, force 1 to 3: `forceJumpHeight` plus step and crouch
lift), tracing every move. Spots reached off the walkmesh are perches
(inside the level: under a roof, the player's box unable to rise 10 m, or
low over the walkable walkmesh, a walkable face within 2.5 m below some
part of the box: a rock or a crate in a roofless yard, a rim the box
overhangs at the floor's edge, a plain jump's ledge over ground that
falls away) or escapes
(outside, or a fall out of the world), which get clip brushes. Static and
transition doors stay shut. What the audit measures is what play does:
- a start is where KOTOR lands a creature: on the walkable face under the
  place, else under one of four places a metre away; a waypoint over a
  roof or beyond a railing never starts the flood up there (TSL 301nar's
  waypoints beside its landing pads had flooded 12 000 leaks);
- a walking step is refused as `sim::motion` refuses it, settling with the
  box's middle over a `NONWALK` surface; jumps may land there;
- each level's regions join only the spots it and the levels below reach
  (a walk's perch by a wall is not one region with the ground a force
  jump reaches beyond it), and count only their leaks;
- a minigame's area (a swoop track, a turret: every seat parked while it
  plays) is never walked; the sweep draws it and does not audit it
  (`kjk collision audit` still does, and says so).
Each region printed names where the flood left the walkmesh for it and
its first spot outside, with the spot that reached it: where to look for
the gap. Higher than 2.5 m over the floor is outside even over it: the
edge walls leave the sky open over doorways and seams, so the Endar
Spire's hull, reached over a door frame from a roofless airlock, lies
over the decks' floor and is still an escape (`data/clip/k1/end_m01ab.clip`
closes it; with `--no-clip --doors all` the audit finds it).

Measured on the whole-game sweep (`kjk sweep <game> --skip
scripts,render`, 2026-10-18), no module worse in any count: TSL from 53
escapable modules (251 regions on foot, 1386 by a plain jump, 495 000
leaks into the void) to 21 (0, 125, 7 800); K1 from 72 (392, 2205,
391 000 leaks) to 25 (0, 236, none). What is left is climbing, with no
leak: chains of jumps over drawn furniture inside the walls up onto roofs
and tall structures more than 2.5 m over the floor (TSL 901mal 29
regions, 902mal 20, 301nar 12; K1 manm26aa 54, tar_m04aa 34); the leaks
left come by force jumps (TSL 650dan, 302nar). It writes `facts.json`, `audit.png` and `escape-*.png` maps
(edge walls light blue). Two checks of what walking keeps: the column
centres on walkable walkmesh (the topmost face under the feet) reached on
foot, and those reached only by jumping though the walkmesh joins them to
a start (a navigation-mesh island a start is on: a KOTOR walker gets
there, so should we); and each door or trigger leading out, whether a
player reaches it on foot.

Cost: about a second for a room-sized module, half a minute at most (on
12 cores). Threads scan the columns and make the flood's moves ahead of
it; the flood takes their traces in its own order, so the report is the
same on any number of threads. A grid of more than 4 194 304 columns
(`Config::columns`) gets the smallest multiple of the cell that fits:
TSL 421dxn is audited at 3.5 m, K1 m12ab at 2 m, TSL 510ond, 231tel and
501ond and K1 manm26mg at 1 m (`facts.json` names the cell).

## Commands
```bash
kjk collision build k1 end_m01aa        # what went in; `all`: every module
kjk collision probes [k1 end_m01aa]     # known answers, differentials, cost (collision::probes, collision_odyssey::probes)
kjk collision trace k1 end_m01aa 15,20,0 15,30,0 [--shape capsule]
    [--doors openable|closed|all] [--repeat dx,dy,dz,N] [--no-clip|--clip F]
kjk collision audit k1 end_m01aa [--cell 0.25] [--seeds walkmesh] [--zoom x,y,r]
```

## Known issue: render-mesh slopes at wall feet (open, not acted on)
KOTOR's own movement walks the walkmesh only, so its rooms are full of
trims, skirts and console bases that nothing ever collided with. JA's
movement meets them as real surfaces:
- anything flatter than 45.6° (normal z above 0.7) is walkable to JA, so the
  player steps onto a 40° trim (render mesh, material 0) at a wall's foot;
- the steeper face above it then turns the push into an upward shove in
  the slide move: backing into a wall on the Endar Spire (`kjk move run
  --stage end_m01aa`, 18.56 s) pops the player 3 units up, then jitters in
  0.1-unit hops of 30–40 ms.

Considered and deferred by the owner (2026-09-30): colliding near the floor
with the walkmesh's boundary as vertical walls, and the room meshes only
higher up. It would remove the snags but restrict mobility (fewer perches;
consoles and furniture unreachable unless walkmeshed), so it is not done
lightly. Revisit if these snags keep showing up in play; smaller options
are clip brushes over the worst spots (`data/clip`) or treating
render-only surfaces near a walkmesh as unwalkable.
