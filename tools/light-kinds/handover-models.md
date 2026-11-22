# Hand-over for characters and placeables under the baked lightmaps

This is a hand-over of one open problem of the fitted-lights bake. It is
written for an agent who takes the problem on in another session. The work
is on branch `feat/fitted-lights` (draft PR 576). The fixes of round r2
are committed with this file. The bakes and the pictures are not in git.

## The problem

Under `KJK_FITTED_LIGHTS=baked` the rooms show the baked lightmaps. Characters
and placeables are not in the lightmaps. They should take on the light of the
room they stand in: darker where the bake made the room darker, brighter
where it made it brighter. Some do not. The owner reported three of them.

- unk_m44ac, report `200253`: "plasteel cylinders a bit too evenly
  illuminated by corner light".
- unk_m44ac, report `200333`: "plasteel cylinder in the shade is too bright".
- 411dxn, report `222059`: "crates in the corners are too bright, should
  have same brightness as env". The fires in that note are a separate fault.

The three reports and the pictures below are in a zip the owner holds (`handover-models.zip`). They are not in the repository. Each has the owner's `note.txt`, the
`screenshot.png`, `breadcrumbs.txt` with the module and `camera.txt` with
the camera. A `camera.txt` line is a console `camera` command that puts the
camera back.

## What was measured

Five modules were shot from three metres away at a character or a
placeable, today (`amb-off`) and under the round r1 bake (`amb-r1`). The
poses are `amb-poses.txt` in each module's folder under `shots/`. The
number is the mean brightness of a box round the object over the mean of a
band of floor below it. A ratio that stays the same means the object
follows its room.

| Module | Pose | Ratio today | Ratio baked | Floor today | Floor baked |
|---|---|---|---|---|---|
| 411dxn | chr-n_mandalorian | 1.18 | 2.04 | 23 | 23 |
| 411dxn | plc-plc_footlker | 2.73 | 3.58 | 20 | 21 |
| 411dxn | plc-plc_gencrps2 | 0.98 | 0.99 | 19 | 19 |
| 411dxn | plc-plc_reptab2 | 5.04 | 3.22 | 10 | 21 |
| unk_m44ac | plc-plc_rakatcom | 0.62 | 0.63 | 74 | 81 |
| danm13 | chr-n_yoda | 0.72 | 1.00 | 78 | 58 |
| danm13 | chr-n_zharh | 0.80 | 0.79 | 62 | 68 |
| danm13 | plc-plc_chair2 | 0.35 | 0.32 | 136 | 131 |
| danm13 | plc-plc_comppnl | 0.49 | 1.23 | 93 | 22 |
| danm13 | plc-plc_reptab2 | 1.22 | 1.98 | 47 | 28 |
| 203tel | chr-n_czerkaoff | 0.90 | 0.99 | 29 | 42 |
| 203tel | chr-n_opochano | 0.55 | 0.62 | 65 | 86 |
| 203tel | chr-n_rodian | 0.91 | 1.08 | 63 | 45 |
| 203tel | plc-plc_desk2 | 1.21 | 1.62 | 64 | 65 |
| 203tel | plc-plc_foodpile | 2.11 | 1.95 | 50 | 54 |
| 203tel | plc-plc_riflepts | 2.73 | 3.47 | 36 | 31 |
| tar_m08aa | chr-n_calonord | 0.69 | 0.66 | 74 | 143 |
| tar_m08aa | chr-n_trandoshan | 0.47 | 0.65 | 75 | 60 |
| tar_m08aa | chr-n_twilekm | 0.93 | 1.27 | 65 | 65 |

`shots/all-pairs.png` shows every pair, today on the left of each pair.

- Where the bake darkens the floor, the model does not darken with it. The
  terminal of danm13 (`plc_comppnl`) stands on a floor that went from 93 to
  22, and its ratio went from 0.49 to 1.23. Yoda's floor went from 78 to
  58, and his ratio went from 0.72 to 1.00. The Rodian of 203tel and the
  Trandoshan of tar_m08aa are the same.
- Several models are brighter against their floor under the bake even
  where the floor's level did not change: the Mandalorian of 411dxn (1.18
  to 2.04), the foot locker of 411dxn (2.73 to 3.58), the desk and the
  weapon rack of 203tel and the Twi'lek of tar_m08aa. That fits the owner's
  notes of crates and cylinders that are too bright.
- Where the bake brightens the floor, the model mostly follows (Calo Nord
  of tar_m08aa, the Rakatan console of unk_m44ac).
- The glowing tables (`plc_reptab2`) lose part of their glow in 411dxn and
  keep it in danm13.
- The measure is rough. The box is the middle of the picture and does not
  follow the object's outline.

## What is known of the cause

- `odyssey.wgsl` lights a model under the bake as `kept + ambient_sum *
  BAKED_GAIN_PROBES + (diffuse_sum - own) * dimmed`, where the ambient is
  the probes' light (`probe_light`) and the lamps are halved.
- The probes read the floor's baked lightmap as the screen shows it, the
  texture times the lightmap, from rays cast round the model's origin
  (`crates/game/src/light_probes.rs`). A probe in a corner takes the walls'
  light on its sides, which may be why the corner crates of 411dxn are
  bright.
- `shots/unk_m44ac/rakatcom-probes-only.png` is the Rakatan console under
  `KJK_PROBES=only`, which shows each probed model in its probe light alone.
- A placeable with the file's `Static` flag takes no probe light at all.
  It is lit by the area's lamps alone (`is_static` in `odyssey.wgsl`). A
  trial gave static placeables the probes' light too. Its pictures were
  taken with a wrong bake path, so the trial says nothing. It was taken
  out.
- A placeable's glow is held to `GLOW_BY_ROOM` times the probes' light
  (`dull` in `odyssey.wgsl`). The owner asked for that of the doors. It
  takes glow off the tables and the racks.
- Ruled out earlier: a light of the crate's own, the probe floor, the
  probes' open rays, half of the probes' sides, the glow cap and the
  module's levels (see `tools/light-kinds/handover.md`, the parked
  plasteel crates).

## How to start

The bakes are not in git. Bake the five modules on the new machine first,
with `sh tools/light-kinds/bake.sh <module> <game> <label>` from the
repository's root, as `tools/light-kinds/handover.md` says. The settings
file must be the scratch one the scripts use, with Enhanced lighting.


1. Shoot a pose of `amb-poses.txt` today and baked, with `bake-shoot.sh`
   from `tools/light-kinds/`. Every launch is headless and muted with the
   scratch settings, as the scripts do.
2. Add a debug view that shows each term of a model's light on its own: the
   probes' light, the lamps, the glow and the room's lightmap under it.
3. Compare the probe light with the floor's lightmap at the model's feet.
4. Check the result across the whole game, as it changes every module.

## Paths

`<root>` is the worktree's root. The bakes are in
`<root>/target/light-kinds/game/bake-<module>/bake/<label>/<area>/b`. Round
r1 is the bake these pictures were taken with.
