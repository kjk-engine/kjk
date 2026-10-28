# Navigation (`collision::nav`, `sim::walk`; I27)

**The mesh.** The session bakes one per planned module from the area's
walkable walkmesh faces and doorways' floors, cut along placeables'
footprints
(`collision_odyssey::AreaCollision::nav_faces`), leaving out faces steeper
than move_body walks (normal z below 0.7) that rise more than a step (18
units; a lower one is stepped over: dropped, it cut the Taris sewers'
rancor lair into three islands). Corners
within a millimetre weld; faces sharing an edge are neighbours; room seams
(boundary edges along each other) become portals over their overlap. A
face a door or placeable stands on (its closed or in-place brushes, with
a body's box), or three of four points of it over a placeable's footprint
(its flat non-walkable faces), carries that entity; one with the level's own solid over its
middle, from a step up to a body's height (a table's top, a prop drawn
without a walkmesh), is `covered`; but a sliver, narrower than the
10 cm column looked over each point with (its points lie on its edges, so
the column meets a wall along one), is open where two faces it joins are:
a body only crosses it (TSL's Peragus mining tunnels, where a 6 mm sliver
along the hallway's wall past the damaged droid was the one way into the
cave, and shut a third of the routes). It bakes in under 10 ms, and is kept
with the area's collision in the store (`area-v6`). `sim::Level::nav` carries it; its digest joins the level's and the
transition announcement's (`Mismatch::Nav`).

**A route.** A* over faces (whole-millimetre costs, ties to the lower
face), over faces whose entity the caller lets by and not covered (the
start's and the goal's always: one stands in a doorway, or walks up to a
guard standing in one), then the funnel. Every leg must keep to the mesh
as a walker would: walked from face to face, seen from above, it must
leave each face by a portal into the next (round a corner many faces
share, into the one it goes on over), with the floor there within a metre
under it and half a metre over it, and end over a face at its height. A
leg over a hole, or straight from a platform down a ramp and up again, is
no way; a rounded corner over its fan's faces is. It costs a face crossed,
no sampling. The corners are rounded less, by quarters, down to the bare
ends' line; where even that leaves the mesh (from the Star Forge's viewing
platform to the floor under it, across the walkmesh's edge, which move_body's
walk-clip walls stop), the corners before that leg are kept and the way
goes on to the middle of the farthest portal along the corridor, out of a
face that leg crossed, that a straight leg reaches, and is pulled on from
there. A corner where a standing
body (lifted a step) would be inside the level's solid, a pillar drawn
over walkable floor, moves to the nearest spot within 24 units where it
fits and the mesh goes on (`sim::walk::route`).
Corners on the mesh's edge are stepped off the walls there by
`sim::walk::CLEARANCE` (20 units: JA's half-width and a hand, since
KOTOR's walkmeshes run on under the walls' feet); a corner inside the mesh
nearer a wall than that (a floor's vertex beside a wall's corner) is
stepped straight away from the nearest wall until it clears it. A portal
whose stepped ends pass each other (a passage narrower than a body, or a
short edge both of whose ends step the same way off one wall) is crossed
between the stepped ends. At every apex (the start, and each corner
the funnel turns at), a portal next on from it that the apex stands past
already, judged on the stepped gate (on its far side as it is walked, and
between its stepped ends seen across it), while the apex stands within a
body's width of the bare portal, drops each stepped end at an inside
corner of the floor: one round which the floor lies within nominally
191.25° (a way bends round it by 11.25° at most; `NavMesh::inside_corner`
counts 64 looks round it at 2 cm, which quantize that to about 186° to
197°). Such an end can lie past a walker standing that near the portal,
and taken as the funnel's side it turned the way back round it (Malak, a
step from the Leviathan hangar's doorway, whose frame meets the wall
beyond it at about 190°, walked 2 s away from it first). So are both ends
of a gate whose corners the apex stands beside neither of (farther from
each than the end is stepped off its walls): between two blocks' corners
the stepped gate lies aslant the bare portal, and a walker on the portal
past it was sent back round one stepped end and then the other (TSL's
Peragus hangar bay, 106PER, the start room's way to the lower door: the
walk turned back and forth between two ways). Otherwise an end at an
outside corner still bounds its side, so a walker beside a block's corner,
clear of it, or in a doorway between two blocks' corners still turns round
them stepped off their walls; a portal whose stepped ends pass each other
(crossed between them) is never passed over, even with that middle behind
the apex.
`sim::walk::passable`: a door that is open or opens by use, one that
leads out too (its link is a player's click or use, reone's
`Module::onDoorClick`; a walker opens it as any other: K1's cantinas put
the NPCs' way out, `p_exit` and the trigger that takes them out of play,
past their door to the street), not a locked door or a placeable.

**Walking.** `sim::walk::Walker` (fixed-size): keeps to a waypoint until it
reaches it (or stands on the way on from it: within reach of the stretch
to the next corner, past it; a route planned from beside a corner may
turn back round it first), then plans again from where it stands; after a
second with no progress it plans again passing over the corner, stepping
aside every other time, right and left by turns (KOTOR's walkers never
jump; jumping put bodies on props and creatures). After a stall, a body
standing in its way (`Walker::around`, the others' origins) is gone
round: it makes for the side a body fits on (`Level::fits`) that is nearer
its corner. A walker off the mesh where the mesh lies under it (within
`REACHED` of its spot: a floor beside a wall the bake left out) plans from
that spot on the mesh as from where it stands; stepping back on to its own
spot planned it nowhere, and it turned on the spot step after step (Taris's
raid, the Twi'lek against the South Apartments' wall). The mesh knows only the level, not who stands on it. Planning every
step instead let two near-equal ways take turns (walkers dithered in
place). A corner it only stands beside, short of the way on, is still
gone round (skipping it walked a player into the Endar Spire bridge's
rail).

A script's move of a body no seat drives, out of a fight, that stalls on
move_body (`Walker::stalls`: a second with no headway, where move_body meets
bodies at once, so the level is in its way) goes on KOTOR's walker to
its next corner, then on move_body again (`world::move_actors`). KOTOR's
walker is a point on the walkmesh and meets nothing drawn; the mesh
planned the leg, so it carries it: a tent's guy rope at waist height
across the Outcast village's walkway (Taris, `tar_m04aa`) held each
outcast's box until its walkway's forced move jumped it on, 30 s a leg.
A step that went nowhere with the level's own solid on the way ahead
(`walk::way_clear`, no body in it) is that stall at once, not a second
later: a mesh run on under a wall's foot or between a barrel and a crate's
walkmesh edge held Peragus's Sith troopers boarding the Ebon Hawk (107PER)
a second at each such gap, twenty times in the turret game.

While a conversation holds the world (scene mode, S3) the walk is
KOTOR's (`walk::kotor_step`, `docs/MOVEMENT.md`): a walk KOTOR's walker
carried on last step never steps aside, and keeps its headway as it gets
nearer its corner (`Walker::keep_headway`), so it does not plan again
short of one; a walk going nowhere stalls and gives up as before, whatever
moves it (move_body's steps where the mesh has no floor ahead, or the walker
held still). A walk with no
route is given up, as KOTOR's walker gives up when path-finding fails. A
new walk (no corner yet) is not stalled: planned as one, it passed over a
goal within a skip of it, and a scripted step of a metre or so was given
up at once (`Walker::command_round`). A body whose box its step would enter (not one at the walk's
goal, nor the one it is going round) stops it: a bump. It goes round by
the side `around` would take (`beside`), and gives the walk up on its 6th
bump (`STUCK_STALLS`), with no side to take, or on bumping into the same
body where it met it last (`Walker::bumps`, `bumped`, counted afresh for
each move). A body walking a walk of its own away along the
walker's way is no bump: the walker waits behind it (`Walker::waits_behind`
[verify]). A walk given up is
`Walker::stuck`, which ends the scripts' move; a forced move
(`ActionForceMove*`) takes it up afresh instead and tries on until its
timeout jumps it there, as the NWN Lexicon has `ActionForceMoveToObject`. A move to a place arrives on
it (KOTOR's range 0, to the queue's 0.1 m floor); a jump goes where the
K1 and TSL executables' safe-location search puts it
(`install::jump_place`): each place tried stands on the walkmesh under its
x and y at the walkmesh's height (the mark's own height only chooses among
floors over one another), free when a column 2 cm wide and a body high
has room over a walkable floor, clear of the other living bodies by the
two bodies' personal spaces (`perspace`, 0.7 m for two humanoids: K1's own
marks stand 0.81 m apart); the mark itself when it is free, else the first
free place of square rings about it, 1 m out each way and each next ring
wider by the jumper's personal space, out to 20 m, each ring's edge tried
every personal space along it (its rows across by x, then its sides), a
ring's place needing a straight walk to the mark when the mark is on the
walkmesh; none found, the jump fails and the jumper stays. The same rule
whether or not a conversation holds the world. A module's creatures are
stood so as the area loads, one after another (`install::settle`: a GIT
place that finds none keeps its place as written), so two the GIT puts on
one spot stand apart. A creature a script makes (`CreateObject`,
`SpawnAvailableNPC`) is stood so too, clear of the living bodies and those
made before it in its step, keeping its place where none is found (stunt_07's
Bastila and Carth, both made at one waypoint, stand 1.41 m apart, the first
ring's corner). The party's companions arriving in a module stand by the
same search within 10 m of the arrival with no walk back to it
(`install::PARTY`), clear of the seats, the area's creatures and each
other [verify the 10 m and the missing walk-back in K1's executable]. A
shut door's doorway (its faces of the mesh, the door closed) stops
the step short of the panel, and the mover runs into the door as a move_body
walker does: a creature opens it and walks on; one that will not open
for it (locked, scenery, the held party's) gives the walk up. An open door is nobody's
concern (one asked the Elders' gate on the Unknown World to take the
player out mid-scene). A party member opens nothing while the world is
held (S4), so its walks route round a closed door, or have no way and end.

```bash
kjk collision nav k1 end_m01aa     # the bake twice, 500 routes checked leg by leg
kjk collision walk k1 end_m01aa    # 50 walks through move_body, doors opened by use
```

The walk test plans its module without scripts (`Level::without_scripts`):
a scene's trigger held the walker in its conversation where it stood (the
North Apartments' interrogation and Sarna's party took 147 of
`tar_m02ad`'s 159 failures). 2026-10-03, 300 walks, with the scenes and
without: end_m01aa 223 → 296, end_m01ab 297 → 300, tar_m02aa 287 → 300,
tar_m02ad 141 → 285, tar_m03aa 167 → 300, danm13 116 → 293. Of
`tar_m02ad`'s 15 left, all end at Sarna's party room's furniture: the
walker's box ends against a `NONWALK` face by the corner it makes for.

2026-10-03, 300 walks, main and after covered faces and on-mesh legs:
end_m01aa 225 → 223, end_m01ab 297, tar_m02aa 287, tar_m02ad 140 → 141,
tar_m03aa 167, danm13 116.

2026-10-02, 300 walks each, before and after the clearance, sidestep,
way-on and going-round rules (and the edge walls' short ends,
`COLLISION.md`): end_m01aa 190 → 225, end_m01ab 285 → 297, tar_m02aa 287
→ 287, danm13 101 → 116 arrive.

2026-10-01, 50 walks each: end_m01aa 50, end_m01ab 44, tar_m02aa 50,
danm13 45, ebo_m12aa 48 arrive, at 0.98–1.04× the route's length at a run
(median). The rest stall where the render geometry and the walkmesh
disagree: a lip or sill under a low overhang, terrain steeper than its
walkmesh face (Dantooine), a body pushed inside a prop.
