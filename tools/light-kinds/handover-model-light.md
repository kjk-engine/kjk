# Hand-over for the light on characters and placeables under the bake

## State on 22 November 2026, on the Windows PC

- The Metal change to the baker gives the same bake on Vulkan: unk_m44ac
  baked with both builds, 11 of 11 lightmaps the same to the last pixel.
- Step 1 is done and is the normal behaviour under the bake. A model that
  the probes light takes no fitted light and none of the game's
  ambient-only lamps. No lamp is taken out of its probes. Its least light is
  0.35 of the flat ambient after the doubling. The directed lamps stay at
  half. This is the trial set `nofitted`, `keepabove`, `ambout` and
  `lowfloor`.
- `floor` and `sides` changed the ratios by 0.1 at most with step 1 in
  place, so their code was taken out. `KJK_MODEL_FIX` is gone.
  `KJK_MODEL_LIGHT` stays.
- With the bake off, pictures before and after differ only as much as two
  launches of the same build do.

Object over floor, today, under the r2 bake before, and after:

| Module | Pose | Today | Before | After |
|---|---|---|---|---|
| unk_m44ac | Rakatan chair in the sun | 0.69 | 0.72 | 0.50 |
| unk_m44ac | cylinders by the corner lamps (200253) | 7.28 | 13.25 | 8.26 |
| unk_m44ac | cylinder in the shade (200333) | 1.90 | 2.58 | 1.39 |
| 411dxn | foot locker | 2.44 | 5.57 | 2.11 |
| 411dxn | glowing table | 1.98 | 2.05 | 1.59 |
| danm13 | Yoda | 0.70 | 0.77 | 0.85 |
| danm13 | terminal | 0.36 | 0.85 | 0.40 |
| danm13 | glowing table | 0.93 | 1.40 | 1.21 |
| 203tel | Czerka officer | 1.79 | 1.58 | 1.09 |
| 203tel | Rodian | 0.89 | 1.67 | 1.15 |
| 203tel | weapon rack | 2.31 | 3.21 | 2.20 |

The r2 bake leaves the room of 411dxn's Mandalorian and of report 222059
nearly black, so their ratios are not meaningful. Under the bake before,
the Mandalorian was lit as if the room were bright. After, he is dark with
it.

What is left: models in the open and characters of 203tel are darker
against their floor than today. That is step 3, probes written by the
baker. The owner has not seen pictures of step 1 yet.

Written on 22 November 2026 on the Mac, for a session on the Windows PC.
It follows `handover-models.md`, which states the problem and the owner's
three reports. Read that file first, then this one, then
`model-light-findings.md` for the full tables.

The work stopped on the Mac because the Mac crashed twice while games and
bakes ran on its graphics card. All further work is meant for the Windows
PC.

## Where the work is

- Branch `feat/model-light`, cut from `feat/fitted-lights` at `514100cc6`.
- Fetch it on the Windows PC and work on it, or merge it into the worktree
  `lighting-step6`.
- The bakes and pictures made on the Mac are in the Mac's `target/` folder
  and are not in the branch. The Windows PC has its own bakes of round r1
  and r2, so nothing needs to be copied.

The commits on the branch:

| Commit | What it is | State |
|---|---|---|
| `26ceae9f5` Lightmap bake on Metal | The baker runs on Metal as well as Vulkan. Each ray query calls `rayQueryProceed` once. Every wait for the graphics card ends after 30 seconds with a message. | Works on the Mac. Not run on Windows. |
| `1eb9184fc` Fitted lights' buffer for the fragment stage only on Metal | The game on this branch did not start on a Mac. The fitted lights' buffer is now visible to the fragment stage only. | Works on the Mac. Not run on Windows. |
| Debug view and trial switches for a model's light | `KJK_MODEL_LIGHT` and `KJK_MODEL_FIX`, described below. | Trial code. The switches are not the final behaviour. |
| This hand-over and the findings | Text only. | |

## Check first on Windows

The Metal fix changed the baker's shader for every graphics card. A ray
query used to loop on `rayQueryProceed` until it returned false. It now
calls it once. Every triangle of the bake is opaque, so on Vulkan the first
call should walk the whole scene and the result should be the same. Nobody
has tested that.

1. Bake one module on the branch with the same settings as an existing
   bake of round r2, into a new label.
2. Compare the two with `tools/light-kinds/bake-same.py`.
3. If they differ, put the loop back for Vulkan only and keep the single
   call for Metal.

## What was measured

Everything was measured in unk_m44ac, with a sun bake made on the Mac. The
sun is the one the Windows batch used: `--sun "0.837 -0.224 0.500 1.183
1.188 1.123 0.385 0.458 0.535 0.01" --no-even --weigh`. No other module was
measured.

The measure is the mean brightness of the object over the mean brightness
of the floor just below it. The object's pixels come from a mask picture,
so the measure follows the object's outline.

| Object | Today | Baked | Baked, the game's lamps left out |
|---|---|---|---|
| Cylinder in the shade (report 200333) | 1.90 | 2.58 | 1.14 |
| Cylinders by the corner lamps (report 200253) | 7.26 | 13.26 | 4.43 |
| Rakatan chair in the sun | 0.69 | 0.72 | 0.27 |

Each part of the light on the object, as a multiple of the floor's light:

| Object | Probes | From above | Sides | The game's lamps | Fitted lights |
|---|---|---|---|---|---|
| Cylinder in the shade | 1.01 | 0.00 | 1.01 | 2.13 | 0.00 |
| Cylinders by the corner lamps | 0.51 | 0.00 | 0.51 | 6.51 | 2.14 |

## The causes

1. **The game's own lamps are doubled.** This is measured. The shader adds
   the area's ambient-only lamps to the same sum as the probes' light, and
   then doubles the whole sum (`ambient_sum * BAKED_GAIN_PROBES`). The
   doubling is meant for the probes alone, because the baked lightmaps are
   stored at half. unk_m44ac has an ambient-only lamp with a reach of 5000
   metres. It lights every model the same in sun and in shade, at twice
   its strength. It is the largest part of the light on both cylinders.
2. **The probes' light from above is zero on every model.** This is
   measured. `without_lamps` takes each of the game's lamps out of the
   light from above, because the original lightmaps hold those lamps. The
   lamp with the 5000 metre reach empties it everywhere. So the shade on
   the floor never reaches the model that stands in it. Under the bake the
   lightmap holds the fitted lights, the sun and the sky, and not the
   game's lamps, so nothing should be taken out.
3. **The fitted lights are counted twice.** This is measured at the corner
   cylinders, where they add 2.1 times the floor's light. The bake holds
   the fitted lights, the probes read the bake, and the shader adds the
   fitted lights to the model again. A room surface with a bake skips that
   call. A model does not.
4. **The probes' sides are too dark for a model in the open.** This is
   inferred from the chair, which drops to 0.27 with the lamps out. A side
   takes the colour of the wall it sees, which is the wall's texture times
   its lightmap. That is light leaving a wall. It is not the light arriving
   at the model, and it holds none of the sun's direct light on the model's
   own side. The game's lamps were covering for this.
5. **The least probe light is doubled.** This is read from the code. A
   model never goes below 0.7 of the area's flat ambient under the bake,
   where 0.35 was meant.
6. **A probe sees a room mesh with no lightmap at full light.** This is
   read from the code. The screen shows such a mesh at half, or at 0.12 in
   a dark area.

The 411dxn crates (report 222059) were not measured. Causes 1 and 3 are
the likely ones there. Look at the lamp list of 411dxn first.

## The tools in the branch

`KJK_MODEL_LIGHT=<part>` shows one part of each probed model's light alone,
with no texture. The rooms show their whole light with no texture. The
parts are `probes`, `above`, `sides`, `lamps`, `fitted`, `glow` and
`floor`. `whole` shows the whole light. `mask` paints every probed model
magenta. The parts are filled in only under `KJK_FITTED_LIGHTS=baked`.

`KJK_MODEL_FIX=<names with commas>` turns on trial fixes under the bake:

| Name | What it does |
|---|---|
| `nofitted` | A probed model takes no fitted light. |
| `ambout` | A probed model takes none of the ambient-only lamps, and those lamps are not taken out of the probes. |
| `nolamps` | A probed model takes none of the game's lamps, and no lamp is taken out of the probes. |
| `floor` | The light from above is the floor's lightmap within 0.5 metres of the model's origin. |
| `sides` | The probes see each room surface at the light the screen shows it with. |
| `lowfloor` | The least probe light is 0.35 of the flat ambient after the doubling. |

The measuring script and the shooting scripts were lost when the Mac
rebooted. `model-light-findings.md` section 6 holds them as the agent
remembered them. They are not tested in that form. The measuring script
needs Pillow.

## The plan

### Step 1. The plain errors

These four changes are small and each one removes a measured or plain
error. Make them the normal behaviour under the bake, one commit each.

1. Double only the probes' light. Keep the lamps' ambient light out of the
   doubling.
2. Take no lamp out of the probes' light from above under the bake.
3. Leave the fitted lights off a probed model (`nofitted`).
4. Halve the least probe light before the doubling (`lowfloor`).

Then decide what the game's lamps do under the bake. The bake's lamps are
the fitted lights, and many of the game's lamps stand for the same lamps,
the sun or the sky. The trial `ambout` leaves the ambient-only lamps out
and keeps the directed lamps at half. Its pictures were shot on the Mac
and never measured, so shoot and measure it again. Start from `ambout`
with changes 1 to 4.

### Step 2. The floor under the model

Turn on `floor` and `sides` and measure them with step 1 in place. Their
effect could not be seen on the Mac, because the doubled lamps held every
model at full brightness.

`floor` gives a model the shade of the floor it stands on. It gives the
whole model one value. A model that stands half in a shadow will not show
the shadow's edge.

### Step 3. Probes written by the baker

Do this if models in the open are still too dark after steps 1 and 2. The
chair suggests they will be.

Today the game makes the probes at load by casting five rays a side and
reading the colour of the walls. The baker knows the real light. It should
write the probes.

1. The baker places probes as the game does today (`probe_places`: a 1
   metre grid, 1 metre above each walkable floor). Move that function to
   where both can call it, or have the game write the places beside the
   lights dump.
2. At each probe, for each of the six sides, the baker gathers the light
   arriving on a small surface that faces that way: the fitted lights with
   their shadows, the sun and the sky with their shadows, and the bounced
   light from the baked lightmaps. It uses the same rays and the same
   levels as the lightmap pass.
3. The baker writes the probes into the bake's set, beside the lightmaps,
   at full range. The doubling then goes away for these probes.
4. Under `baked` the game loads that file in place of `light_probe`'s
   rays. The blend between probes, the easing and the shader's
   `probe_light` stay as they are.
5. A module baked before this change has no probe file. The game then
   falls back to today's rays.

This also answers the owner's note that the cylinders are "too evenly
illuminated". A probe that holds the corner lamp's light on the side that
faces the lamp lights a cylinder from that side.

### Step 4. Proof

1. With `KJK_FITTED_LIGHTS` off, one pose before and after must be the
   same picture. Classic lighting and today's Enhanced lighting do not
   change.
2. One sheet per module with today, baked before and baked after, and the
   table of ratios. Use unk_m44ac, 411dxn and danm13 and the poses of
   `amb-poses.txt`, plus the three report cameras in `handover-models.md`.
3. The aim is that the ratio under the bake is close to the ratio today,
   and that the three reported objects read as part of their room.
4. Look at the glowing tables (`plc_reptab2`) and the doors. Their glow is
   held to `GLOW_BY_ROOM` times the probes' light, so every change to the
   probes changes them.
5. Check one KOTOR 2 module.
6. Delete `trial_fix`, `ProbeList::fixes` and every switch that did not
   become the normal behaviour. Keep `KJK_MODEL_LIGHT`.

## Notes for running it

- On the Mac, run one game or one bake at a time and nothing else on the
  graphics card. Two crashes came while several ran together.
- `--sun-fit` is the longest job of the baker. It took 73 seconds on the
  Mac.
- Do not commit the file-mode changes of `tools/light-kinds/*.sh`.
