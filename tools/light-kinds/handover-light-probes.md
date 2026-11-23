# Hand-over for improving the light probes

Written on 22 November 2026 for an agent who takes on the light of
characters and placeables under the baked lightmaps. It gathers everything
tried so far, what worked, what did not, and where the work stands. The
older notes are `handover-models.md` (the problem and the owner's reports),
`handover-model-light.md` (the Mac session's plan) and
`model-light-findings.md` (the Mac session's full tables).

## The problem

Under `KJK_FITTED_LIGHTS=baked` the rooms show new lightmaps that the
baker makes from the fitted lights, one sun, the sky and three bounces.
Each room is then scaled to a target level and put through a tone curve.
Characters and placeables are not in the lightmaps. They take their light
from light probes, and they should look as dark or as bright as the room
they stand in.

The owner reported three faults.

- In unk_m44ac (report 200253), the plasteel cylinders were "a bit too
  evenly illuminated by corner light".
- In unk_m44ac (report 200333), the "plasteel cylinder in the shade is too
  bright".
- In 411dxn (report 222059), the "crates in the corners are too bright,
  should have same brightness as env".

The reports, with their screenshots and cameras, are in
`target/light-kinds/handover-models/reports` of the worktree
`lighting-step6`. They are not in the repository.

## How a model is lit today

The code is `crates/game/src/light_probes.rs` and the model branch of
`fragment` in `crates/game/src/odyssey.wgsl`.

- The game makes the probes when an area loads. A probe stands on a grid
  1 metre apart, 1 metre above each room's walkable floor.
- Each probe holds six colours, one for a surface facing east, west, up,
  down, south and north.
- The light from above is the floor's lightmap under the probe. Each other
  side takes the drawn colour (texture times lightmap) of the walls its
  five rays meet.
- A model takes the blend of the probes within 1.6 metres of its origin.
  The whole model gets one set of six colours, which eases over a quarter
  of a second when it moves. The shader blends the six colours by the
  surface's normal.
- Under the bake the shader doubles the probes' light, because the baked
  lightmaps are stored at half. It adds the game's own directed lamps at
  half and the model's glow, and stops the total at full light.

## How to measure

The measure is the mean brightness of a model over the mean brightness of
the floor just below it, in the same picture. A ratio that stays the same
from today to the bake means the model follows its room.

- `model-shoot.sh` shoots the poses of `ml-poses.txt` in a module's folder,
  today or under a bake set.
- `KJK_MODEL_LIGHT=mask` paints every probed model magenta. The measure
  takes the model's pixels from that picture.
- `model-measure.py` prints the ratios. `model-sheet.py` makes a picture of
  the shots side by side with the ratios in the captions.
- `KJK_MODEL_LIGHT` also shows one part of a model's light alone, with no
  texture. The parts are `whole`, `probes`, `above`, `sides`, `lamps`,
  `fitted`, `glow` and `floor`.

The poses used are in `ml-poses.txt` of each module's folder under
`target/light-kinds/game` of the worktree `lighting-step6`. The modules
are unk_m44ac, 411dxn, danm13 and 203tel. unk_m44ac's poses include the
cameras of reports 200253 and 200333, and 411dxn's the camera of report
222059.

Two launches of the same build differ by about 1 in mean pixel value,
because characters idle and fires flicker. A difference that small is not
a change.

## What was tried

### On the Mac (before 22 November)

The Mac session found these causes in unk_m44ac with the debug view.

1. The game's ambient-only lamps were doubled with the probes' light.
   unk_m44ac has one with a reach of 5000 metres, which lit every model
   alike in sun and in shade.
2. The probes' light from above was zero on every model. The game took its
   lamps out of the light from above, because the original lightmaps hold
   them. The lamp with the 5000 metre reach emptied it.
3. The fitted lights were counted twice. The bake holds them, and the
   shader added them to each model again.
4. The least probe light was doubled, to 0.7 of the area's flat ambient
   where 0.35 was meant.

It added the debug view and six trial switches, then the Mac crashed
twice while running the game and the baker together. All later work was
on the Windows PC.

### Step 1, the plain errors (kept)

Each trial set was shot in unk_m44ac, 411dxn and danm13 under the round r2
bake. The table gives the model over its floor.

| Pose | Today | r2 bake | Step 1 set | Step 1 with the floor and sides trials |
|---|---|---|---|---|
| Rakatan chair in the sun (unk_m44ac) | 0.69 | 0.72 | 0.50 | 0.50 |
| Cylinders by the corner lamps (200253) | 7.28 | 13.25 | 8.27 | 8.34 |
| Cylinder in the shade (200333) | 1.90 | 2.58 | 1.39 | 1.39 |
| Terminal (danm13) | 0.36 | 0.85 | 0.40 | 0.46 |
| Glowing table (danm13) | 0.93 | 1.40 | 1.21 | 1.36 |
| Foot locker (411dxn) | 2.44 | 5.57 | 2.11 | 2.32 |

The step 1 set is what the game now does under the bake for a model that
the probes light.

- It takes no fitted lights.
- It takes none of the game's ambient-only lamps.
- No lamp is taken out of its probes.
- Its least light is 0.35 of the flat ambient after the doubling.
- The game's directed lamps stay at half.

The two Mac trials `floor` (the light from above read off the floor right
under the model) and `sides` (the probes seeing walls as the screen shows
them) changed the ratios by 0.1 at most. Their code was taken out.

With the bake off, the pictures before and after differ only as much as
two launches of the same build. Every change acts under the bake alone.

The owner looked at the pictures and kept step 1. It is commit `328a5de1`
on `feat/fitted-lights`, pull request 576.

Step 1 did not fix everything.

- Some models are darker against their floor than today. The Rakatan
  chair in the sun is 0.50 against 0.69. In 203tel the Czerka officer is
  1.09 against 1.79. Others are brighter. 203tel's Rodian is 1.15 against
  0.89, and danm13's Yoda 0.85 against 0.70.
- The round r2 bake leaves the room of 411dxn's Mandalorian and of report
  222059 nearly black, so their ratios mean nothing. Before step 1 the
  Mandalorian was lit as if the room were bright. After it he is dark with
  the room.
- The glowing tables lose some glow (1.40 to 1.21 in danm13), because a
  placeable's glow is held to `GLOW_BY_ROOM` times the probes' light.

### Step 3, probes written by the baker (tried and set aside)

The probes made from the lightmaps read the light leaving the walls. They
hold none of the sun's direct light on a model's own sides, which is why
models in the open came out dark. So the baker was made to write the
probes itself.

- The baker places probes as the game does, on a 1 metre grid 1 metre over
  each room's walkable faces.
- Each probe becomes six places in the baker's passes, one facing each of
  the six ways. Each place is lit exactly as a texel facing that way. It
  takes the fitted lights with their shadows, the sun with its shadow, the
  sky, and three bounces. It is scaled by its room's scale and goes through
  the same curve as the lightmaps.
- The baker writes `probes.txt` in the bake's set. The game loads it under
  the bake in place of making its own probes.
- The lightmaps come out the same to the last pixel. A probe's light from
  above matches the baked floor under it. Over 1410 probes in unk_m44ac the
  median ratio is 1.00, with 80% of them between 0.76 and 1.22.

| Pose | Today | Step 1 | Baked probes |
|---|---|---|---|
| Rakatan chair in the sun | 0.69 | 0.50 | 0.70 |
| Cylinders by the corner lamps (200253) | 7.28 | 8.26 | 11.34 |
| Cylinder in the shade (200333) | 1.90 | 1.39 | 2.49 |

The chair came right, but both cylinders came out bright again. The probes
around the cylinder in the shade hold, out of 255, 89 from above, 83 from
below, 128 from the east, 96 from the west, 156 from the south and 119 from
the north. So the cylinder does stand in shade, since its light from above
is dim. Its south side catches light bounced off a sunlit cliff beside it.
That may be physically right, but one set of colours for the whole model
spreads it over the model's whole side.

The owner compared the two and said "I actually think your darker light
prob thingy was better than the latest brighter one". Step 1 is the look
to keep. The baked probes are on the local branch `feat/baked-probes`,
commit `1fdceb60`, on the Windows PC. That branch is not pushed.

### The hybrid (proposed, not taken up)

Fable was asked for advice. It recommended the usual split for games with
baked rooms.

- The probes keep only the bounce and the sky, which change slowly.
- The sun lights a model live, as the sun's colour times how squarely each
  surface faces it, times a sun share stored with each probe. That needs
  no shadow pass.
- The fitted lights light a model live, as they do outside the bake, times
  the room's scale.
- The bake's tone curve is applied once in the shader, to the sum of the
  three.

It also suggested checking three things about the bright cylinder. One is
whether the bounce uses the cliff's colour. Another is whether the room's
scale is above 1 and so multiplies the sun's bounce. The last is whether a
wall facing south beside the cylinder is as bright as its south side.

The owner asked whether the hybrid would hurt older graphics cards. The
sun part costs one dot product a pixel. The fitted lights cost what they
cost outside the bake, which the game paid under the bake too before step
1. Nothing has been measured. The owner has not chosen to go ahead.

### Step 4, baked vertex light for placeables (22 November 2026)

This work is uncommitted on the branch `feat/placeable-light` of the
worktree `model-light`. The code is `crates/game/src/placeable_light.rs`,
the placeables' part of `crates/launcher/src/detail_bake/gpu.rs`, and the
`VERTEX_LIGHT` branch of `odyssey.wgsl`.

What was built:

- `kjk detail-bake --placeables` makes the placeables block the bake's
  rays where they rest, so they cast their shadows into the lightmaps. A
  placeable that holds a fitted light blocks no ray, because its housing
  would shut in its own light. Skinned and dangling meshes block no ray
  either.
- `kjk detail-bake --placeable-light` also lights each vertex of each
  placeable's mesh that has no skin and does not dangle, exactly as it
  lights a texel. A vertex takes the fitted lights with their shadows, the
  sun with its shadow, the sky, three bounces, its room's scale and the
  bake's curve. A mesh that a clip moves, such as a cylinder's lid, is lit
  where it rests. A placeable that holds a fitted light takes its own light
  in a special way, which the end of this step describes. The light goes
  into `placeables.txt` in the bake's set, at half level like the
  lightmaps.
- The game gives each such mesh its light as a vertex colour, and the
  shader takes it in place of the probes' light.
- Long triangles are split before the bake lights them, so that no edge is
  longer than half a metre (`placeable_light::subdivide`). The game splits
  the mesh the same way before it draws it. Without the split one vertex lit
  a whole fan of long triangles, and the light showed in streaks. The split
  raises the vertices the bake lights from 8,074 to 17,671 in unk_m44ac,
  from 13,240 to 16,836 in 411dxn, from 2,432 to 3,350 in danm13, and from
  54,671 to 78,840 in 203tel. A file baked before the split no longer
  matches a mesh's vertex count, and the game then keeps that mesh on the
  probes.
- A vertex inside the box of a smaller placeable that blocks rays is not
  seen. The food on 203tel's oval tables is such a placeable. Such a vertex
  takes the light of the vertices around it, as a vertex inside a wall
  does. Before this, the vertex under the food was nearly black, and its
  dark spread over the whole middle of the table. The vertices beside the
  food still take its shade, and the floor's texels are not changed. 1,455
  vertices of 203tel and 851 of 411dxn are under a smaller placeable.

The switches, all under `KJK_FITTED_LIGHTS=baked`:

- `KJK_PLACEABLE_LIGHT=probes` puts every placeable back on the probes.
  This is step 1.
- A mesh with baked light takes none of the game's own lamps by default.
  The owner walked Peragus this way on 23 November 2026 and settled on it
  for now. `KJK_MODEL_LAMPS=on` gives such a mesh the lamps back at half.
- `KJK_MODEL_LAMPS=off` gives every model that the bake lights none of the
  game's own lamps, the characters among them.
- `KJK_BAKED_PROBES=on` reads the bake's `probes.txt` in place of the
  game's own probes.

The game's lamps count twice under the bake. Nearly every one of a room's
lamps that lights from a direction is also a fitted light, a hidden lamp at
the same place. That is 14 of 17 lamps in unk_m44ac and 153 of 156 in
danm13. The bake holds that light already, and the game adds the lamp again
at half. Without the lamps the models match their room, but they are darker
against their floor than today.

The trial of the baked probes in step 3 had a fault. It wrote `probes.txt`
at the probes' whole level, while the game doubles the probes' light under
the bake because the lightmaps are stored at half. Its models showed at
twice their light. The baker now writes the probes at half. That trial's
pictures, and its table in step 3, are too bright for that reason.

The causes found on the way:

- 411dxn's rooms baked nearly black because its fitted lights give far too
  little light for the room levels in `levels-r2.txt`. Every room's lamps
  reached the most scale the bake allows, six times. In the room `411dxnr`
  the lamps at six times give a level of 0.02 against a target of 0.61.
  The round r2 command has `--no-even`, which keeps a room under its level
  dark. The set `pl3` of 411dxn takes `--fill-stored` in its place, as
  203tel's command does. The bake's report says that each room then takes
  its own stored light for the rest and reaches its level. The pictures do
  not bear that out yet. The Mandalorian and the floor around him are still
  nearly black, at 0.39 of the floor against 1.79 today. The foot locker
  and the table `plc_reptab2` became too bright, at 4.01 and 3.48 against
  2.45 and 1.98 today. The likely cause of the second fault is that a
  vertex takes its room's fill at the room's mean, while a texel takes it
  times the stored light at its own place, which is darker in a dark
  corner. The first is not explained. Neither is fixed.
- danm13's terminal has a dark wedge on the floor to the left of its post,
  in the step 1 shots and the new shots alike. It is not the model's
  shadow from the room's light (step 7b of the lighting plan), because the
  wedge stays the same with `KJK_ROOM_SHADOW=up`. The terminal blocks no
  ray in the bake, since it holds a fitted light. The cause is not found.
  The next thing to check is the lightmap texels under the wedge in
  `bake/pl3/m13aa/b`, against those of the round r2 set without
  placeables.

The sets are `bake/pl3` of each module's folder under
`target/light-kinds/game` of the worktree `lighting-step6`. The shots are
`shots/p3-*`, and the sheets are `sheets-pl3`. Each sheet has three columns.
They are the game today, step 1, and the new candidate. The new candidate is
the vertex light for placeables with the game's lamps dropped
(`KJK_MODEL_LAMPS=off`).

The table gives the model over its floor. The new candidate's set is
`pl3`. Step 1 is under `pl2`, except in 411dxn, where it is under `pl3`.

| Pose | Today | Step 1 | New candidate |
|---|---|---|---|
| Rakatan chair in the sun (unk_m44ac) | 0.69 | 0.59 | 0.33 |
| Cylinders by the corner lamps (200253) | 7.26 | 8.89 | 2.37 |
| Cylinder in the shade (200333) | 1.89 | 1.74 | 1.35 |
| Yoda (danm13) | 0.70 | 0.79 | 0.42 |
| Zhar (danm13) | 0.61 | 0.49 | 0.33 |
| Chair (danm13) | 2.16 | 2.59 | 1.23 |
| Terminal (danm13) | 0.35 | 0.38 | 0.27 |
| Glowing table (danm13) | 0.93 | 1.30 | 0.71 |
| Czerka officer (203tel) | 1.81 | 1.21 | 1.11 |
| Opo Chano (203tel) | 1.33 | 0.84 | 0.56 |
| Rodian (203tel) | 0.89 | 1.11 | 0.90 |
| Rifle stand (203tel) | 2.31 | 2.29 | 1.71 |
| Mandalorian (411dxn) | 1.79 | 0.39 | 0.39 |
| Foot locker (411dxn) | 2.45 | 1.72 | 4.01 |
| Table (411dxn) | 1.98 | 1.45 | 3.48 |
| Crates of report 222059 (411dxn) | 0.44 | 0.33 | 0.71 |

The split of long triangles and the rule for covered vertices changed the
ratios by 0.06 at most against the set `pl2`. Their effect is in the
pictures. The streaks are gone from 203tel's oval tables, and the middle of
each table is as bright as its ends.

A placeable's own light (23 November 2026). At first the bake left a
placeable's own fitted lights out of its vertices, because with them in,
danm13's terminal `plc_comppnl` came out bright blue. Without the game's
lamps, nothing then lit the glowing table `plc_reptab2` from its own lamp,
and the owner found it better on step 1. The bake log now names each
placeable that holds lights of its own. In danm13 the table holds one and
the terminal two. The bake now treats them in two ways.

- A vertex takes its own placeable's light as if the light stood no nearer
  than half a metre (`OWN_LIGHT_LEAST` in `detail_bake/gpu.wgsl`).
- The placeable's own meshes shade its own light (`own_seen` in
  `detail_bake/gpu.rs`). The test runs on the CPU against the meshes that
  do not glow, and a hidden vertex has the light left out of its list. This
  shade is needed. With the least distance alone, even at half a metre, the
  whole terminal was bright blue, at 1.32 of its floor. With the shade, the
  light reaches 430 of the table's 873 vertices and 733 of the terminal's
  910.

The set is `bake/pl5` of danm13, the shots `shots/p5-sun` (with the same
switches as `p4-sun`), and the sheet `sheets-pl5/danm13.png`. The terminal
is 0.37 of its floor (0.35 today, 0.38 on step 1, 0.27 in `pl4`) and is dark
with a faint blue edge, as on step 1. The table is 1.12 (0.93 today, 1.30 on
step 1, 0.71 in `pl4`). Its top is lit as on step 1, but its sides are
darker, because the top shades them. A least distance of 0.3 metres gave
1.13 and looked the same, and 1.0 metre dimmed the table to 1.01. The trial
sets are `bake/pl5t-*`. Every other placeable, character, lightmap and probe
of danm13 came out the same as in `pl4`. A bake of unk_m44ac, which has no
placeable with a light of its own, came out the same as its `pl4` to the
last byte.

The bright foot locker of 411dxn (23 November 2026). The bake was searched
for a fault, and none was found. `KJK_VERTEX_PARTS=<placeable>,...` makes
the baker print each part of the light on those placeables' vertices, by the
way they face, beside the texels within 1.5 metres that face the same way.

- The fill from the stored lightmaps gives nearly all of the locker's light.
  Its top takes a fill of 0.112 0.165 0.211 (linear), and the floor around it
  takes 0.106 0.157 0.201. The nearest texel of every vertex is the floor,
  0.04 to 0.5 metres away.
- The fitted lights give 0.002 on the top at the room's scale of 6. The
  cyan panels give 0.05 at most on the side that faces them. No light is
  close enough to blow up.
- The cold colour is the stored lightmaps' own colour in that room. The
  floor has the same colour, and its brown texture hides it.
- In the view without textures the locker is 1.15 of its floor (0.97
  today), and the workbench `plc_reptab2` 0.86 (0.73 today). The corner
  crates of report 222059 take 0.71 of their floor's light (0.39 today).
  The workbench's sides read 47 to 51 against 86 to 94 for the floor beside
  them.

So the vertices take what the room's surfaces take. The locker and the
crates look bright because their textures are about twice as bright as the
floor's, and today the game lit them with less light than the floor gets.
Three changes were tried and dropped. Taking the nearest texel that faces
the same way gave the workbench's sides the light of the red lamps on the
wall behind it. Scaling the fill by what each vertex gathers from the
surfaces around made sides brighter than tops, because the stored light
comes mostly from lamps the rooms do not show. Scaling it by the room's mean
stored light on walls against floors made the sides brighter too, because
411dxn's walls are brighter than its floors. If the owner wants placeables
darker than the room, that is a choice of look, such as one share for all
vertex light. The set `bake/pl6` is the same as `pl5`, and the sheets are
`sheets-pl6/411dxn.png` and `sheets-pl6/411dxn-light.png` (without textures).

### Step 5, the live sun for characters (22 November 2026, later)

This work is uncommitted on `feat/placeable-light` too. It is the sun part
of the hybrid. The fitted lights stay in the probes, because the game's
lamps are the ones `KJK_MODEL_LAMPS=off` drops.

- The baker writes a second probe file, `probes-sun.txt`, beside
  `probes.txt` (`write_probes_sun` in `detail_bake/gpu.rs`). It does so in
  every bake with a sun and probes.
- Its first line gives the sun's way in Bevy's space, the sun's linear
  colour, and the bake's curve. The curve is the gamma of 2.2, the knee of
  0.8 and the most of 0.45 over the knee (`eased_level`).
- Each probe line gives the probe's share of the sun, from 0 to 1. The share
  is the sun's light on the probe's sides that face the sun, over what those
  sides would take with nothing in the way. Each side's light is the share
  of `SUN_RAYS` rays that reach the sun, so the share is softened already.
- Each probe line then gives the six colours as linear light before the
  curve and before the halving. They hold the sky, the fitted lights at the
  room's scale, the room's fill and every bounce, the sun's bounce among
  them. Only the first sun's direct light is taken out. A second sun stays
  in the colours.
- The game reads that file under `KJK_BAKED_PROBES=on` with
  `KJK_PROBE_SUN=on` (`baked_probes` in `viewer.rs`). It blends the share as
  it blends the colours, and eases it over the same quarter second. A model's
  share goes to the shader in the w of its second colour.
- The shader blends the six linear colours by the normal, adds the sun's
  colour times how squarely the surface faces the sun times the share, and
  puts the sum through the bake's curve and the halving
  (`probe_sun_curve` in `odyssey.wgsl`). A room's scale is for its lamps,
  so the sun takes none, as in the baker.

The split is exact for a surface that faces one of the six ways. For any
other surface the shader puts the blend of the sides through the curve,
where the baker put each side through it. Below the knee the curve is a
power of 1/2.2, so the two differ by a few percent at most.

The live sun barely changes the ratios. The reason is in the measures
below. The models that are dark against their floor stand in shade, so
the share is near 0 for them, and the live sun gives nothing. A model in
the open is as bright with the bake's sun in the probes as with the live
sun, which is what the split should give.

| Pose | Today | Step 1 | Lamps off, baked probes | Lamps off, live sun |
|---|---|---|---|---|
| Rakatan chair on the probes (unk_m44ac) | 0.69 | 0.59 | 0.31 | 0.34 |
| Cylinders by the corner lamps (200253), on the probes | 7.26 | 8.89 | 3.94 | 4.15 |
| Cylinder in the shade (200333), on the probes | 1.89 | 1.74 | 1.44 | 1.48 |

In the view without textures (`KJK_MODEL_LIGHT=whole`) the Rakatan chair's
seat reads 89 against 106 to 112 for the shaded floor beside it, and the
sunlit floor beyond reads 222. So the camera of that pose shows the chair
in shade, not in sun.

In danm13 Yoda is 0.42 of his floor on the game's probes with the lamps
dropped, 0.51 on the bake's probes, and 0.54 with the live sun. Zhar is
0.33, 0.52 and 0.54. Today they are 0.70 and 0.61. In the view without
textures Yoda's head and shoulders read 97 to 99, and the floor beside him
117 to 129. Half of danm13's probes and half of unk_m44ac's have a share of
0, and a tenth have a share of 1.

The same bakes changed how a vertex takes a room's fill under
`--fill-stored` (`places_pattern` in `detail_bake/gpu.rs`). The fill of a
room from its stored light is the room's stored light times a number. A
texel takes it times the stored light at its own place. A vertex and a
probe took it at the room's mean before, which is brighter than a dark
corner. Now each takes the stored light of the nearest counted texel of its
room, within 4 metres. Without `--fill-stored` nothing changes, and the
lightmaps and `probes.txt` of unk_m44ac came out the same to the last byte.

| Pose (411dxn) | Today | Step 1 | `pl3` | `pl4` |
|---|---|---|---|---|
| Mandalorian | 1.79 | 0.39 | 0.39 | 0.39 |
| Foot locker | 2.45 | 1.72 | 4.01 | 2.88 |
| Table `plc_reptab2` | 1.98 | 1.45 | 3.48 | 1.52 |
| Crates of report 222059 | 0.44 | 0.33 | 0.71 | 0.68 |

The table is now close to today. The foot locker is still too bright and
shows a cold, white colour, and the crates are still too bright. Their
cause is not found.

The sets are `bake/pl4`, the shots `shots/p4-*` and the sheets
`sheets-pl4` of unk_m44ac, danm13 and 411dxn. 411dxn is a KOTOR 2 module,
so its bake takes `--game tsl`.

### The fingerprint of a bake (23 November 2026)

Players run the games unpatched, KOTOR 2 with TSLRCM, KOTOR 1 with the K1
Community Patch, or with other mods. TSLRCM changes the placeables of about
11 areas and the rooms of 2. The K1 Community Patch changes the rooms of 18
areas and the placeables of about 37 modules. A bake made over one set of
files must not be drawn over another. So each bake now carries a
fingerprint of the files it was made from. The code is
`crates/game/src/bake_fingerprint.rs`, which the baker and the game both
call.

The fingerprint has three parts.

- `rooms` is a digest of the area's layout (each room's model and its
  place), each room model's MDL and MDX, and each stored lightmap that the
  rooms name, with its TXI.
- `placeables` is a digest of the placeables the bake lights or lets block
  its rays. These are the placeables with a mesh that has no skin and does
  not dangle, in the area's order. Each one counts with its number, its
  model's name, its place, its bearing as stored, and its model's MDL and
  MDX.
- `format` is a number in the code (`FORMAT`). It is raised by hand when
  the baker's files change their meaning.

Every file is the one the game reads after the game folder, `override/`
and the Steam Workshop items are ranked. Scripts, dialogue, sounds and the
diffuse textures are left out. The digests do not depend on where the
files lie, on their times, or on the machine.

The baker writes `fingerprint.txt` into each set it writes, beside the
PNGs. The file looks like this.

```
# (five lines of comment)
area 105per
format 1
rooms <32 hexadecimal digits>
placeables <32 hexadecimal digits>
```

A set baked before this has no fingerprint. Two commands give it one or
show one without baking.

- `kjk detail-bake 105per --stamp <set>` writes `fingerprint.txt` into the
  set from the game files as they are now. This is right only when the
  files are still the ones the bake was made from.
- `kjk detail-bake 105per --fingerprint` prints each area's fingerprint.

When an area loads, the game computes its fingerprint once and compares
it with the fingerprint of each listed set of that area.

- When `rooms` or `format` differs, the game draws the area with its own
  lightmaps. It takes none of the set's fills, floors, probes, placeables'
  light or `unlit.txt`, as if no bake existed for the area.
- When `rooms` matches and `placeables` differs, the game shows the
  lightmaps, and the placeables take the light probes, as under
  `KJK_PLACEABLE_LIGHT=probes`.
- A set without `fingerprint.txt` is shown as before, with one warning for
  each such set.
- When two listed sets hold a bake of one area, the game takes the one that
  matches the most. Of two that match as much, it takes the later one in
  the list. An unpatched and a patched bake can then ship side by side.

The log says in one line for each set which of these it was. Computing the
fingerprint takes about 0.1 seconds for a Peragus area.

A build with the developer verbs (the `probes` feature) leaves the Steam
Workshop items out unless `KJK_WORKSHOP=on` is set. The Peragus sets `pv1`
were baked that way, without TSLRCM. On this PC, with `KJK_WORKSHOP=on`,
TSLRCM moves a terminal in 105per and changes the placeables of 101per too.
Both areas then keep the bake's lightmaps, and their placeables take the
probes. The sets `pv1` of 101per and 105per are stamped. The other 169
areas are not yet.

### No stored light in the probes and placeables (23 November 2026)

A bake with `--fill-stored` used to write the fill into `placeables.txt`,
`probes.txt` and `probes-sun.txt`. The fill there is the stored lightmap's
light at the room's texel nearest the vertex or probe, so those files held
light taken from the games' own lightmaps. They now hold none of it, so a
bake can be given to players as a download. This is uncommitted on
`feat/placeable-light`.

- The baker writes the light of a vertex or a probe without the fill where
  its room takes the fill from its stored light. After the light it writes
  a word such as `@0:244581`. The word names a lightmap from a list at the
  top of the file and a texel of the bake's grid on that lightmap. This is
  the texel the fill was read from. A lone `@` in `placeables.txt` names the
  same texel as the vertex before it on the line.
- A vertex that takes its light from its neighbours (`fill_vertices`) takes
  the texel of the neighbour that counts most.
- When the game reads the files, it reads that texel of the player's own
  stored lightmap. It adds the fill that `fill.txt` gives the lightmap, and
  it holds the light to the share that `floor.txt` gives, as it does for the
  room's texels. The code is `crates/game/src/stored_fill.rs`.
- Where the bake writes a floor, every vertex and probe with a texel near it
  gets the word, and the game holds it to the floor. Before this the
  vertices and probes took no floor. Only stunt_18 has a floor.
- The bake's format (`FORMAT` in `bake_fingerprint.rs`) is now 2. The three
  files carry it in a `format` line too. The game takes nothing from a
  `placeables.txt` or a probe file without that line, and logs one line
  saying so. The placeables then take the light probes, and the probes are
  made from the lightmaps. Every older set needs baking again, and the sets
  `pv1` of 101per and 105per are now turned away by their fingerprint.

The sets are `bake/nf2` of 411dxn and 101per. Their lightmaps and
`fill.txt` are the same to the last byte as those of `pl6` and `pv1`.
101per's rooms take no stored fill, so its files changed only by the
`format` line. `placeables.txt` of 411dxn grew from 356 KB to 456 KB, and
its `probes.txt` from 1.91 MB to 2.04 MB.

The test of 411dxn's room `411dxnr`, which no lamp reaches: the four chests
`plc_cht01` there hold a mean of 0.0007 to 0.0015 in the file, against 0.19
to 0.24 in `pl6`. The game draws them as before. The shots `nf-chest-*` use
the poses in `nf-chest-poses.txt`. The sheet `sheets-nf/chests.png` shows,
from left to right, the old game with `pl6`, the new game with `nf2`, and
the old game with `nf2`, which reads no fill and draws the chests black.

The shots `nf-old` and `nf-new` of both areas, and `nf-old-bp` and
`nf-new-bp` of 411dxn under `KJK_BAKED_PROBES=on`, differ by no more than
two runs of the same game differ. The probes for the live sun and the floor
were not run, because neither area has a sun or a floor.

Other files of a set, and what they take from the stored lightmaps:

- `fill.txt` gives one number for each lightmap of a room that takes the
  fill. It is the room's fill over the mean of its stored linear light.
  The fill is chosen so that the room's mean level reaches its target. So
  the number comes from the stored light, as one ratio for each room.
- `floor.txt` gives the share from `data/lights/bake.toml` and takes
  nothing from the stored light.
- The PNGs of `b` hold the lamps, the sun, the sky and their bounces. Each
  room's scale comes from its target level, and the targets in the levels
  files are a share of the room's mean stored level. With `--weigh` a texel
  counts by its stored level in that mean. So each room's brightness is set
  from the stored light, as one number for each room, but no texel holds
  stored light.
- `--stored-backdrop` (backdrop = "stored", in tar_m08aa and two other
  modules) no longer writes the stored lightmaps of the rooms with no
  walkable face into `b`. The bake names them in `stored.txt`, and the game
  makes each one's picture from the player's own stored lightmap, at half
  its stored level as the PNG held it (`stored_fill::backdrop`). The game
  doubles it back, so the room shows its stored light as before. A set
  baked before this has the copied PNGs and no `stored.txt`. The format
  stays 2.
- Set `a` mixes the stored lightmaps with the lamps. It must not be given
  to players.
- `fingerprint.txt` holds digests of the game files, the stored lightmaps
  among them. A digest cannot be turned back into a picture.
- `detail-bake.txt` is the bake's log. It gives each room's mean stored
  level and, with `--sun-fit`, the mean stored colour in and out of the
  sun. These are means of a room, not texels.
- `unlit.txt` holds the word "dark".

### The lighting as a download (23 November 2026)

Players get the bakes as a download. The launcher downloads one zip for a
game, with a manifest beside it, and kjk installs it into the store. kjk
itself downloads nothing. The code is `crates/game/src/lighting_bakes.rs`,
which builds without the `probes` feature, `crates/launcher/src/lighting_verbs.rs`
for the commands, and `crates/launcher/src/lighting_pack.rs` for the
developer's pack command. This is uncommitted on `feat/placeable-light`.

The bakes are made only for KOTOR 1 with the K1 Community Patch and for
KOTOR 2 with TSLRCM.

The manifest is plain text, like `fingerprint.txt`.

```
# (five lines of comment)
game tsl
format 2
needs TSLRCM
archive kjk-lighting-tsl-c1.zip blake3 <64 hexadecimal digits> bytes 3964098
area 101per module 101per rooms <32 hexadecimal digits> placeables <32 hexadecimal digits>
```

The zip holds `<area>/<file>` for each area. It holds only what the game
loads from a set `b`: the PNGs, `fill.txt`, `floor.txt`, `unlit.txt`,
`placeables.txt` and `fingerprint.txt`. The PNGs are stored as they are, and
the text files are deflated. The probe files, `detail-bake.txt` and set `a`
stay out.

The commands, with the store and the settings pointed at scratch folders by
`KJK_STORE` and `KJK_SETTINGS` for trials:

- `kjk lighting pack <k1|tsl> <folder> --out <dir> [--label c1] [--round c1]`
  packs every finished set `b` under `<folder>/<module>/`. It writes
  `kjk-lighting-<game>-<label>.zip` and `.txt`. A set without
  `fingerprint.txt` is left out and listed as not finished. A set of
  another format stops the pack. A module baked with `--stored-backdrop`
  is packed when its set holds `stored.txt`, and a PNG that file names is
  left out of the zip. Such a set baked before `stored.txt` existed is left
  out and listed, because its backdrop PNGs copy the games' lightmaps. The
  pack tells such a module by the second line of `detail-bake.txt` ("kept
  as stored") or by `backdrop = "stored"` in `data/lights/bake.toml`. Two sets of one area stop the pack unless
  `--round` names the round to take.
- `kjk lighting check <manifest> [--game tsl] [--json]` compares each area's
  fingerprint with the player's files. The install is allowed when at least
  90% of the areas match (`MATCH_SHARE`). An area whose rooms match and
  whose placeables differ counts as a match. Below that the player is told
  that the lighting needs TSLRCM or the K1 Community Patch. The answer is
  kept in the store (`lighting-check-v2`), keyed by the manifest and by a
  digest of the names, sizes and modification times of the files a bake
  reads (`files_key`: the game folder's `chitin.key`, `patch.erf`, `data`,
  `modules`, `override`, `rims` and `texturepacks`, and the Workshop items'
  `modules` and `override`, in their load order). No file is read, so the
  first check of the 35 finished K1 areas against the K1 Community Patch
  copy took 2.5 to 5.2 seconds, and a second one 0.45 seconds. The digest
  of the whole install took 94.5 seconds before.
- `kjk lighting install <manifest> <zip> [--json]` checks, then compares
  the zip's size and BLAKE3 with the manifest. It unpacks into
  `<store>/lighting/<game>.partial`, compares each area's
  `fingerprint.txt` with the manifest, and renames the folder to
  `<store>/lighting/<game>`, in place of an older install. The manifest is
  kept there as `manifest.txt`. `kjk store gc` never removes anything under
  `lighting`.
- `kjk lighting status [--json]` and `kjk lighting remove <game>`.

With `--json`, a check or an install prints one line
`{"progress": {"step", "done", "total", "area"}}` for each area, and then
the answer: `{"check": {...}}`, or `{"games": [...], "check": {...}}` for an
install. Status prints `{"games": [{"game", "status", ...}]}`, as
`kjk installs --json` does.

The options' Lighting has a third value, Enhanced Lightmaps
(`display.lighting = "baked"`, `LightingMode::Baked`). It is Enhanced with
the download's lightmaps. The options offer it only when a download is
installed for the game whose area loaded last, or for either game before
any area has loaded. When kjk starts with that setting and a download
installed, `fitted_lights::wanted()` and `baked()` are on, as under
`KJK_FITTED_LIGHTS=baked`, and the download's area folders start the list
of bake folders. `KJK_FITTED_LIGHTS` wins over the setting when it is set,
and the folders of `KJK_DETAIL_BAKE` and `KJK_DETAIL_BAKES` come after the
download's, so they win for an area both hold. A change in the options
takes effect the next time kjk starts. The check of each area's
fingerprint when it loads stays as it was.

Under the setting, the trial is decided for each area as it loads
(`fitted_lights::set_area_baked`). Only an area with a bake the game
accepted takes it. Any other area is drawn as under plain Enhanced: an area
of the game with no download, an area the download lacks, and an area whose
fingerprint was turned away. Plain Enhanced has no fitted lights unless
`KJK_FITTED_LIGHTS=on` is set. So such an area has no fitted lights, no
lamps switched on, an empty grid of fitted lights, shader trial 0, the
probes with the lamps taken out, and no dark unlit surfaces. Before this,
the whole run took the trial, and such an area showed its stored
lightmaps at twice their light, under live fitted lights. In 101per (the
TSLRCM copy, three poses) the shots under Enhanced differed from the
setting with only a KOTOR 1 download by a mean of 0.65 to 1.9 of 255, and
from the setting with a TSL download whose fingerprint was edited by 0.07
to 3.4. Two runs of one setup differed by 0.8 to 3.4. The old game with
only a KOTOR 1 download differed by 41 to 71, and the accepted bake by 23
to 52. `KJK_FITTED_LIGHTS` still decides for every area when it is set.

`unlit.txt` is now matched by the area's resref, from the set's
fingerprint, or from the folder's name for a set without one. KOTOR 1
areas such as `m13aa` of module `danm13` were missed before. Only 305nar
has `dark_unlit = true` in `bake.toml`, so no bake was affected.

The trials of 23 November, with the canon bakes finished at the time and
the sets `nf2` of 411dxn and 101per:

- The pack of `nf2` (made from the Steam KOTOR 2 without TSLRCM) took 234
  files, 8.8 MB, into a zip of 7.8 MB. The six finished K1 areas of
  `target/canon-bakes/k1` took 365 files into 23.4 MB. TSL's 101per needed
  `--round c1`, because `c1-first` holds it too.
- `nf2` against the Steam KOTOR 2 (a developer's build, so without the
  Workshop items): 2 of 2 areas match. The first check took 94.5 seconds,
  most of it hashing the install for its digest. With the digest cached it
  took 1.1 seconds, and the answer from the store came at once.
- `nf2` against the TSLRCM copy: both areas keep their lightmaps and their
  placeables differ, so the check allows the install. The canon 101per
  (made with TSLRCM) against the Steam KOTOR 2 does the same. The rooms of
  101per and 411dxn are the same with and without TSLRCM, so the check
  cannot tell an unpatched KOTOR 2 apart from these areas alone.
- The six K1 areas against the K1 Community Patch copy: 6 of 6 match (63
  seconds, with the install's first hashing). Against the Steam KOTOR 1
  without the patch: 1 matches, 1 matches with other placeables, and 4
  fall back. The install is refused with: "The new lighting needs KOTOR 1
  with the K1 Community Patch installed. Only 2 of the 6 areas in this
  game's files match the files the lighting was made from, and at least 90%
  must match. Nothing was installed."
- A zip of the wrong size is refused before anything is written.
- 411dxn, headless at 960x540, two poses. With the setting at Enhanced
  Lightmaps and no variables, the log shows the bake in
  `<store>/lighting/tsl/411dxn` accepted whole. Against the same area run
  the old way (`KJK_FITTED_LIGHTS=baked`, `KJK_DETAIL_BAKE` on the set),
  the mean difference of a channel was 0.38 and 0.24 of 255. With the
  setting at Enhanced and the download installed, against a run with no
  download at all, it was 0.25 and 0.02. Those two are the noise between
  runs, since the pose v000 sees things that move. The setting against the
  plain run differs by 2.6 and 4.4, which is the bake.

## Where it stands

- Step 1 is the behaviour under the bake, on pull request 576.
- Models in the open are somewhat dark against their floor, and the owner
  prefers that to the brighter baked probes.
- Step 4, the placeables' baked vertex light, is uncommitted on
  `feat/placeable-light`. The owner has not seen its sheets.
- Without the game's lamps, characters are darker against their floor than
  today. The live sun of step 5 is built behind `KJK_PROBE_SUN=on`, and it
  barely helps, because the dark models stand in shade. The gap is in the
  light without the sun.
- danm13's terminal wedge also shows in the shots `ml-base`, from before
  the bake had placeables, so it does not come from the placeables blocking
  rays.
- The baked probes stay shelved on `feat/baked-probes`.
- The bakes can be packed into a download, checked against a player's
  files, installed into the store and turned on by the options' Lighting
  (Enhanced Lightmaps). This is uncommitted on `feat/placeable-light`. The
  launcher's side comes later.
- Each bake now carries a fingerprint of the game files it was made from,
  and the game shows a bake only over those files. This is uncommitted on
  `feat/placeable-light`. Only 101per and 105per of round `pv1` are
  stamped.
- A bake's probes and placeables hold no light from the games' own
  lightmaps, and the bake's format is 2. This is uncommitted on
  `feat/placeable-light`. Every set baked before must be baked again.
  `--stored-backdrop` now names the backdrop's lightmaps in `stored.txt`
  and copies none of them. In tar_m02ab the shots of the old and the new
  set differed by a mean of 0.87 and 0.43 of 255, and two runs of the new
  set by 0.78 and 0.38. The canon kas_m22ab of round c1 still holds copies
  and is left out of a pack until it is baked again.
- 411dxn's Mandalorian room is still nearly black under `pl3`, and its
  vertex-lit placeables are too bright there. danm13's terminal wedge is
  not explained.
- Two Metal fixes from the Mac session are in the branch. One makes the
  baker run on Metal. It gives the same bake as before on Vulkan, checked
  to the last pixel in unk_m44ac. The other makes the fitted lights' buffer
  visible to the fragment stage only, since a Mac has too few buffer slots
  for the vertex stage. The fitted lights' buffer is now binding 20, since
  main took binding 19 for swaying meshes.

## Ideas not yet tried

- Pull each probe's six sides part of the way toward their mean, which
  would soften the cylinder's bright south side. The owner would judge it.
- Raise the least probe light for models in the open only, for example by
  the share of the sky the probe sees.
- Read the probes' blend at two heights, so a tall model under a ledge is
  right.
- Look at 411dxn with a bake whose rooms are not black, before judging its
  crates.

## Running it

The work is on this same Windows PC, in two worktrees of the main checkout.

- The worktree `.claude/worktrees/model-light` is on the branch
  `feat/model-light`, which holds the same commits as pull request 576.
  This hand-over and the scripts are in it, and its game is built in its
  `target/release`. Work on the probes here. The local branch
  `feat/baked-probes` is in the same repository, so `git switch` reaches
  it from this worktree.
- The worktree `.claude/worktrees/lighting-step6` is on `feat/fitted-lights`.
  Its `target/light-kinds/game` holds every module's bakes, poses and shots.
  Another session's uncommitted sky fix is in it. Do not commit, reset or
  switch branches there.

- Set `KJK_ROOT` to the worktree `lighting-step6`, for its bakes, and
  `KJK_EXE` to the game built in `model-light`.
- Every launch is headless and muted with the scratch settings, as the
  scripts do. Run one game or one bake at a time.
- unk_m44ac's round r2 bake is
  `detail-bake unk_m44ac --game k1 --lights lights-r2.txt --gpu --sun "0.837 -0.224 0.500 1.183 1.188 1.123 0.385 0.458 0.535 0.01" --no-even --weigh --levels levels-b1.txt`,
  run from its folder.
- The approved tests run in the pull request, not locally.
