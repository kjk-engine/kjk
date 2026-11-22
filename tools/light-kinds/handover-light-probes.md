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

## Where it stands

- Step 1 is the behaviour under the bake, on pull request 576.
- Models in the open are somewhat dark against their floor, and the owner
  prefers that to the brighter baked probes.
- The baked probes and the hybrid are both open options. Either one needs
  pictures for the owner before it goes further.
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

- The bakes and shots are in `target/light-kinds/game` of the worktree
  `lighting-step6` on the Windows PC. Set `KJK_ROOT` to that worktree and
  `KJK_EXE` to the build to test.
- Every launch is headless and muted with the scratch settings, as the
  scripts do. Run one game or one bake at a time.
- unk_m44ac's round r2 bake is
  `detail-bake unk_m44ac --game k1 --lights lights-r2.txt --gpu --sun "0.837 -0.224 0.500 1.183 1.188 1.123 0.385 0.458 0.535 0.01" --no-even --weigh --levels levels-b1.txt`,
  run from its folder.
- The approved tests run in the pull request, not locally.
