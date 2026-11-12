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
- Floor clutter (`floor_clutter`) is left out: a drawn triangle on no
  walkmesh face with some part over walkable floor and no part more than
  a step (18 units) from the walkmesh right under it or, over a hole in
  the walkmesh (under a console), more than a step over that floor.
  KOTOR's walker walks the walkmesh through it; solid, the player's box
  rode up onto the Ebon Hawk's floor pipes, was shoved up the sloped
  skirts of its consoles and stuck on the slanted tops of their sides
  (TSL 003ebo, playtest of 2026-11-07). The walkmesh is what bodies meet
  there instead, and the edge walls keep them off the floor's edge.
  Floor drawn flat at the floor's height stays, as does anything higher
  than a step (consoles' tops, crates, walls), so jumps still reach the
  perches they did. `kjk collision build` counts them (`skipped N: floor
  clutter`: 1771 in TSL 003ebo, 1768 in K1's Ebon Hawk). Shots and
  marks meet the floor under them, a few centimetres down.
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
  These walls now stand only round a placeable with no drawn shape (the
  next entry).
- Placeables (`placeable_shape`; playtest of 2026-11-11 in the Sith base
  on Taris, K1 tar_m09aa: a cylinder, a footlocker and environment suits
  no jump could get onto, a cage whose base could not be walked onto,
  and two piles of parts the player stuck on). Three rules.
  - A placeable is solid where its model is drawn, at rest, owned by the
    placeable and solid in state 0 as its walkmesh is. The drawn
    triangles are `SOLID | NONWALK`. A mesh drawn translucent by its
    alpha, or whose texture blends additively, is left out as it is for
    rooms. A transparency hint leaves a placeable's mesh out only from 2
    up, as the renderer blends a mesh only from 2 up. The cages' base and
    posts carry hint 1, their force fields 2 or alpha 0.35.
  - A placeable with a drawn shape keeps only the upper part of the
    walls round its footprint: from 2.44 m (a standing player and a plain
    jump's rise, `edges::SLOT`) over its highest drawn point up. Its
    footprint still cuts the navigation mesh, so routes go round it as
    before. A placeable whose model has nothing drawn (`plc_invis`) keeps
    its walls whole.
  - Triangles that touch are a part. A triangle is low when it rises no
    more than a step over the walkable floor under its lowest corner. Low
    triangles that touch, and whose up-facing ones span the player's box
    (0.76 m) along both of the placeable's axes, are a base. A base is
    plain solid, so a walker steps onto it and off it: the ring at a
    cage's foot, 0.25 m high, whose posts rise from it. A part no higher
    than two steps (0.91 m) that covers less than half of the player's
    box from above (0.29 m²) is left out, as floor clutter is. Each piece
    of a pile of parts is such a part: the tallest stands 0.63 m and the
    largest covers 0.15 m². Every other part is solid and `NONWALK`.
  - A walking step that would set the body down on a non-walkable top of
    an entity, higher than where the step started, is refused wherever
    under the box the top is (`sim::motion`'s `set_down`, and the audit's
    walk). The footlocker is 0.40 m high, lower than a step, and a box
    stepped up onto it from its edge while the box's middle was still over
    the floor. A jump still lands on top. The room's own furniture keeps
    the old rule (the box's middle decides).
  - `KJK_PLACEABLE_SHAPES=off` builds the `kjk collision` verbs' levels
    with placeables as their walkmeshes and footprint walls alone, to
    compare. `kjk collision build` counts each placeable's drawn
    triangles and those left out.
  - In the Sith base: the cylinder's top is 0.82 m up, the footlocker's
    0.40 m and the suits' 0.81 to 1.09 m. A point dropped on each rests
    there. A box walking at each is stopped. A box 1.2 m up passes over
    each. A box lifted a step crosses a cage's base. A box walks through
    both piles of parts.
  - Measured (2026-11-12, `kjk sweep <game> --skip scripts,render` with
    `KJK_PLACEABLE_SHAPES=off` and without). No module of either game
    has more leaks at any reach. Escapes grow, though: with the upper
    walls, 47 module and reach pairs of K1's 117 modules (unk_m43aa's
    plain jump from 0 to 5, tar_m04aa's from 34 to 56, danm15's first
    Force jump from 0 to 25, the Sith base's own Force jumps by one
    each). Without the upper walls it was 57 pairs (unk_m43aa's plain
    jump from 0 to 60) in K1, and 31 pairs of TSL's 82 modules. The new
    escapes are spots more than 2.5 m over the floor, reached by jumping
    from a placeable's top onto roofs and tall structures. This is not
    settled: the owner decides whether these count, or what else holds
    them.
  - What it does not cover:
    - placeables made during play (`MadeWalls` in `sim::walk`), which
      are not in the built level and keep their walls;
    - an animated placeable's other looks. The shape is the model at
      rest, so a footlocker's open lid is not solid;
    - a broken droid (`plc_brokndrd`) is all small, low parts and is
      walked through, as the piles of parts are.
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
  Reaching the sky, they end jumping off the walkmesh too (ledges, and
  everything out of doors).
- Furniture indoors (`edges::furniture_top`, playtest of 2026-11-11: the
  Sith base's table and crates on Taris, K1 tar_m09aa, which no jump
  reached). A table or a stack of crates stands in a hole of the walkmesh,
  so an edge wall stood in front of it to the sky. KOTOR keeps the
  walkmesh's edge about 0.4 m clear of furniture, which is where the wall
  stands, so the wall is inside the furniture or against its side. The
  wall now ends 1 cm under the furniture's top, is open above it for
  2.44 m (a standing player's height and a plain jump's rise,
  `edges::SLOT`), and goes on from there to its full height. A walker is
  stopped as before. A jump meets the furniture as it is drawn. A Force
  jump from the floor still meets the wall higher up.
  - The room must be indoors: 90% or more of its walkable floor is under
    a roof (a point over a face's middle cannot rise 10 m, the audit's
    measure). The enclave's courtyard on Dantooine has eaves over a
    plinth in one corner, with open ground beside it, and a jump from
    the plinth left the level.
  - The furniture is the rooms' solid (drawn triangles and walkmesh
    faces, no doors or placeables) at the wall's middle line, or a
    wall's thickness behind it. The probe is a column 0.1 m square, as
    tall as a standing player. The solid must be all of:
    - more than a step and 5 cm over the edge, with a rise of more than
      a step at once somewhere between the edge and the wall. The wall
      is then still higher than a step, and a walker meets it from the
      floor. The sloped foot of a room's wall has no such rise: a walker
      walks up it, and its wall stays whole;
    - level on top (its normal's Z 0.9 or more). The feet of the Sith
      base's corridor walls slope 43 degrees, which JA still walks on. A
      body let onto one is shoved against the wall above it (the known
      issue below). With only JA's limit, 469 m of that module's 1204 m
      of walls ended at one;
    - no more than 4 m over the edge, under room for a standing player.
      The top is the lowest height with that room, so a duct close over
      a crate is part of what the wall stands in;
    - under a roof (the probe cannot rise 10 m);
    - without a drop behind it: half a body's width on from the top
      there is more solid, or solid under it no lower than a metre under
      the edge. A parapet over a pit keeps its wall;
    - closed behind, straight back and 45 degrees to either side. Within
      4 m the probe comes over walkable floor again, or is stopped both
      at the top's height and just under the roof (a room's wall). Floor
      to both sides is enough: a table 4 m long is met from its end. A
      rock under a tree on Dantooine has a roof and open ground behind
      it.
  - A stretch of wall (half a metre at most) is lowered when both its
    ends find such a top, to the higher of the two. It is also lowered
    to a top at one end when solid stands as high at the other (a taller
    stack, a tilted crate), and across a gap of one sample between
    furniture and furniture or such solid. The player's box, 0.76 m
    wide, rests on what stands either side of the gap.
  - `walls whole` in a module's clip file keeps every wall whole there:
    the module's rooms are open somewhere above their furniture. Two
    modules have it. In Tatooine's tat_m17ac a plain jump from one piece
    left the level and Force jumps of rank 2 and 3 fell out of the world
    (729 leaks). In the unknown world's unk_m42aa Force jumps of rank 1
    to 3 did (16 182 leaks).
  - Measured (2026-11-12, `kjk sweep <game> --skip scripts,render` with
    `KJK_FURNITURE_TOPS=off` and without; the switch builds the verbs'
    levels with every wall whole). No module of K1's 117 or TSL's 82 has
    more escapes or more leaks at any of the five reaches, once those
    two keep their walls whole. Walls change in 59 K1 modules and 62 TSL
    modules. The pieces of wall go from 36 167 to 45 819 in K1 and from
    39 708 to 52 338 in TSL. A level takes about a quarter longer to
    build (the rooms' solid is built once more for the probes): K1's
    modules 292 ms on average where they took 232 ms, the slowest 791 ms
    where it took 523 ms; TSL's 356 ms where they took 284 ms, the
    slowest 767 ms where it took 653 ms (`kjk collision build <game>
    all`, a Ryzen 7 3700X). Without the wall's upper part the Sith base
    went from no leak to 25 610: Force jumps of rank 2 and 3 crossed the
    furniture and the room's own wall behind it. In that module a plain
    jump now reaches 35 perches where it reached 32 (cells of 0.25 m).
    A jump onto furniture 1.05 m high lands as it does with no wall
    there (`kjk trace m1`, a crate with the lowered wall in it and
    without). Probe `edge_walls_furniture_tops`.
  - What it does not cover:
    - furniture standing further than a wall's thickness behind the wall
      (the east side of the Sith base's table, 0.9 m behind the edge
      past a strip of floor lights). A lowered wall there would be a
      ledge in the air: a box stands on any top, a ridge too
      (`kjk trace m1`);
    - furniture in a room open to the sky (a courtyard's crates);
    - placeables, which have their own rules ("Placeables" below);
    - a walker already off the walkmesh. In the Sith base's two barracks
      a ramp at the table's foot leads a walker up to 1.05 m, as before.
      He now walks on over the table (494 spots off the walkmesh on foot
      where there were 177, cells of 0.25 m).
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
  Built at load, about 0.3 s in release (0.8 s at most, every K1 and TSL
  module, with the furniture's probes); a session keeps each area's level with its navigation mesh in
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

## Render-mesh slopes at wall feet (the low ones left out)
The floor clutter rule (above) now leaves out the trims, skirts and
console bases no higher than a step over the floor; what follows still
holds for those that rise higher. KOTOR's own movement walks the
walkmesh only, so its rooms are full of trims, skirts and console bases
that nothing ever collided with. JA's movement meets them as real
surfaces:
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
