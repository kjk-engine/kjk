# Hand-over for the fitted lights and their bake

Written on the evening of 18 November 2026. The enclave's part was brought up to date on 19 November 2026. It replaces the hand-over of 17 November, which is in this file's git history (commit `1dcef9fd`). Read that older text only for the history of the survey, the review page and the live lights' rules.

In this file `<root>` stands for the worktree's root. The repository's commit check refuses a personal folder path in any file, so no file may hold the real one.

## Where the work is

- The branch is `feat/fitted-lights`, draft PR 576 on `kjk-engine/kjk`. Its head is `1dcef9fd`, "Switched lamps shine at a quarter of their kind's strength".
- The worktree is named `lighting-step6`. Everything in this file is committed there except the bakes and pictures, which lie in `target/` and are not in git.
- The bake scripts of this file and its first text are commit `0147b85b`. The work of 18 and 19 November 2026 after it is commit `f5cb1d01`, "Sun, sky and roof openings in the lightmap bake, for the Dantooine enclave". Neither is pushed. One thing in the worktree is not committed: another session's fix of the game's drawing of the council chamber's roof opening (`crates/game/src/scene.rs`, `material.rs`, `crates/asset_odyssey/src/model.rs`, `crates/approved_tests/tests/approved.rs` and five files that only fill in a new field).
- `docs/LIGHTING-PLAN.md` is the whole lighting plan. Its steps 7 and 10 are the background. It does not describe the fitted lights' rules. Those are in the code's comments and in this file.

## What the fitted lights are

The games' lightmaps were baked from lamps that were never shipped. The fitted lights put a light at every lamp the level art shows: a glowing tube, panel, screen or sign.

- A kind of fitting is one glowing texture. `data/lights/k1.toml` and `data/lights/tsl.toml` hold one entry for each kind. `data/lights/fittings.toml` holds the general rules' numbers, the kinds that differ, and the lights placed by hand.
- `crates/game/src/fitted_lights.rs` finds the lamps of each glowing mesh when an area loads and puts a light on each. Its top comment and the comments on its constants say why each rule exists.
- The rooms' hidden lamps shine too. They are the lamp list the original engine used for characters.
- `fitted_lights::level` holds each room to about the level it shows today. A room's strong lights are made weaker until the room fits.
- The lights lie in a grid over the floor plan (`fitted_lights::grid`, `fitted_lights` in `crates/game/src/odyssey.wgsl`). No light switches on or off with the camera.
- Characters and placeables cast shadows from the fitted lights (`fitted_visibility` in `odyssey.wgsl`, the boxes in `crates/game/src/shadows.rs`).

## What the baked trial is

The owner wants lightmaps made from the lamps alone, with no pixel of BioWare's lightmaps. He said: "we have to do B so we can actually ship it". The bake is also the path for weaker graphics cards. The live lights cost about 0.27 ms in the opaque pass, and the bake costs about 0.03 ms.

- `kjk detail-bake <module> --game <k1|tsl> --lights <dump> --gpu --levels <file> --out <folder>` makes the bake on the graphics card in about 13 seconds (`crates/launcher/src/detail_bake.rs`, `crates/launcher/src/detail_bake/gpu.rs`, `gpu.wgsl`).
- The bake writes two sets of lightmaps. Set `a` is the live look with shadows and bounced light. Set `b` is the lamps alone. Only `b` matters.
- Set `b` is scaled room by room to the levels file. The levels file has one line for each room: its name and the mean stored level to aim at.
- A room that would need more than `B_MOST` (6) times its light takes an even light instead (`B_FILL_SHARE` 0.4).

## The environment variables

| Variable | What it does |
|---|---|
| `KJK_FITTED_LIGHTS=on` | The live fitted lights, under Enhanced lighting. The rooms keep half of their stored lightmaps. |
| `KJK_FITTED_LIGHTS=baked` | The rooms show the bake that `KJK_DETAIL_BAKE` names and take no live fitted light. Doors, placeables and characters keep the live lights. |
| `KJK_FITTED_LIGHTS=off` or unset | The normal game. |
| `KJK_DETAIL_BAKE=<folder>` | The folder of one set of baked lightmaps, for example `<root>/target/light-kinds/game/bake-103per/bake/c5/103per/b`. |
| `KJK_FITTED_DUMP=<file>` | The game writes every textured mesh and every fitted light of the area to this file, a line each. The bake reads it with `--lights`. A line that starts with `fitted` is a light. A line that starts with `switched` is a room's switched lamps. |
| `KJK_SETTINGS`, `KJK_SAVES`, `KJK_MUTE=1`, `APPDATA` | Scratch settings, scratch saves, no sound and a scratch profile folder. Every launch that is not the owner's own sets all four. The scripts below do. |

The settings file is `<root>/target/detail-bake/shots/enhanced.json`. It is a copy of the game's settings with `display.lighting` set to `enhanced`, `display.msaa` to `taa` and `display.shadows` to `high`. The saves folder is `<root>/target/detail-bake/saves`.

## Today's rules and the owner's words

The owner's words are in quotation marks. Where there are none, the sentence gives his meaning as the last session noted it. Feedback on one area is a rule for every area.

- **Switched lamps.** A room's lamp that the files do not set to glow looks lit only because the lightmap on it is bright. It now sheds light where its stored lightmap is bright (`ON_FROM` 0.3, `ON_FULL` 0.6, `ON_ABOVE` 0.75). The owner on Peragus: "these light panels only emit light at the top of them" and "its actual panels dont emit any light at all, we should make them glow".
- **`SWITCHED_SHARE` is 0.25.** A switched lamp shines at a quarter of its kind's strength at the most, and its bright texels glow by `SWITCHED_GLOW` 0.35. At full strength the owner said: "it just looks too bright and the frame of the lamp is equally bright like the panel". At 0.6 the Harbinger's ceiling lamps filled in the shadow under a terminal, and he said: "I'm missing the shadow from the terminal".
- **Unnamed glowing textures.** A glowing texture that the rule data does not name is a screen or a panel. Each bright patch sheds a faint light of its own colour (`UNNAMED_STRENGTH` 0.5, `UNNAMED_REACH` 3 m, `UNNAMED_CONE` 0.2). A piece wider than 8 m is a wall and sheds nothing. A piece up to 1 m sheds light even with no bright texel, as a beacon or a crystal does. The owner: "this can emit some light from its panels" and "give em a lil light from the panels".
- **Door force fields.** A door sheds light only where its bright texels are strongly coloured (`FIELD_CHROMA` 0.5). That is a force field. A grey metal door glows in the files in place of baked light and sheds nothing. The owner asked that force fields shed a little light.
- **Door glow held to the room.** Under the bake a door or a placeable glows at most `GLOW_BY_ROOM` (2) times the light the probes hold where it stands (`odyssey.wgsl`). The owner: "the doors are too bright compared to the surrounding door frame and wall".
- **Rounded placeable shadows.** A placeable that fills less than `LEAST_FILL` (0.5) of its box casts no shadow. One that fills less than `BOXY_FILL` (0.7) casts as the rounded shape that fits in its box (`BOX_ROUND`, `shadows.rs`). Under the trials the light's source for a box is at least `FITTED_BOX_SOURCE` (0.25) wide, so the shadow is soft away from its foot. The owner of a standing droid: "this shadow is square which is wrong". With `LEAST_FILL` at 0.7 the droid had no shadow, and he said: "this droid (and other placeables) are missing shadows". Do not raise `LEAST_FILL` again.
- **Scrolling speed 50.** `UV_ANIMATION_SPEED` in `crates/game/src/scene.rs` is 50, as in KotOR.js. It was 250. The owner of Peragus's force cage: "the force cage animation is moving way too fast". The games' own rate is not measured.
- **Peragus is darker and less even.** The owner called its rooms "too bright" and "too evenly illuminated". Light should sit at the panels. He said a darker Peragus "would be cool, and fitting". So Peragus is baked at 0.6 of its live level, and no room above 0.22.
- **Older rules that still hold.** Every glowing fitting sheds at least a little light. A glowing floor, wall or trim is no lamp. A light shines out of its fitting and must not wash out the fitting's texture. Doors and objects darken with the room. A lamp's light should read as a pool near the lamp.

## The scripts

They are in `tools/light-kinds/` and run from there with `sh`. They take the worktree's root from their own place. Set `KJK_ROOT` to use another root. Every launch they make is headless and muted, with scratch saves, settings and `APPDATA`.

Each area has a scratch folder `<root>/target/light-kinds/game/<folder>`. The first argument of every script is that folder's name. A script makes the folder when it is missing. Paths in the other arguments are read from that folder.

| Script | What it does |
|---|---|
| `bake.sh <module> <game> [<label> [<folder>]]` | The general command. It reads the module's level from `data/lights/bake.toml` and runs one of the next two scripts. The folder is `bake-<module>` when left out, and the label is the date and time. A new area needs only its module and game. |
| `rebake.sh <folder> <module> <game> <label> <share> <cap> [<of>]` | The two-pass bake. It dumps the game's lights, bakes once to measure each room's levels, writes `levels-<label>.txt` with min(cap, share × level) for each room, and bakes again with `--levels`. `<of>` is `original` or `live` and says which level. The result is `bake/<label>`. About 75 seconds. |
| `rebake-held.sh <folder> <module> <game> <levels> <label>` | One bake of the game's lights as they are now, held to a levels file made earlier. The result is `bake/<label>`. |
| `bake-shoot.sh <folder> <module> <fitted> <bake> <poses> <label>` | One launch that takes a picture at every pose of a poses file. `<fitted>` is `off`, `on` or `baked`. `<bake>` is the baked set's folder or `""`. The pictures are `shots/<label>/<pose>.png`. |
| `bake-play.sh <folder> <module> <fitted> <bake> <commands>` | One walk-mode launch with a command list. The other scripts use it. |
| `bake-env.sh` | Sourced by the others. It sets the root and the scratch variables. |
| `bake-sheet.py <area folder's path> <labels> <titles>` | Sheets of several rounds' pictures side by side, four poses a sheet, written to `cmp-<n>.png` in the area's folder. It needs Pillow. |
| `bake-same.py <one set's folder> <the other's>` | Says how many lightmaps of two bakes are the same to the last pixel, and the largest difference of any pixel. It needs Pillow. |

A poses file has one pose a line: `name x y z yaw pitch fov`. An F12 report in walk mode holds `camera.txt` with the same numbers.

The module name differs between the tools for KOTOR 1. `detail-bake` takes `end_m01aa --game k1` and writes into a folder named `m01aa`. Peragus and the Harbinger take `--game tsl`, and their folder has the module's name. Each bake script prints the `KJK_DETAIL_BAKE` path at its end.

## The level of each module

`data/lights/bake.toml` says how bright each module's bake is. `bake.sh` reads it. The game does not.

- `[default]` is `share = 1.0` and `of = "lit"`, with no ceiling. Each room aims at its level where the game lights it today. That is the mean of its stored lightmaps with each texel counted by its own stored level. `detail-bake --gpu` prints it for each room as `lit`, beside `original` (the plain mean) and the stored lightmaps' mean colour. The second bake runs with `--weigh`, which counts the bake's texels the same way. Each texel of the bake is still made of the lamps', the sun's and the even light alone. The stored lightmaps only set how much each room's lamps are scaled.
- Why `lit` and not `original`. A bake that reaches a room's plain mean is darker than today in the pictures. It spreads its light into the corners that the stored lightmaps leave black, so the floors and walls a player looks at take less. The proof is in `bake-danm13/shots/bake-stored`: the stored lightmaps shown through the baked path look like today (41 against 42), and the bake at the same plain mean was at 31. Under `lit` it is at 43.
- `[modules.101per]` and `[modules.103per]` have `share = 0.6`, `ceiling = 0.22` and `of = "live"`. The share is of the level under the live fitted lights, because the bakes the owner has seen were made that way. Peragus was not moved to the original level.
- `[modules.152har]` and `[modules.end_m01aa]` name a levels file: `levels = "levels/152har.txt"` and `levels = "levels/end_m01aa.txt"`. The two files are in `data/lights/levels/`. They are the levels the owner approved in his walks, room by room. A held module bakes once and is not moved by a later change to the live lights.
- A module's entry has either `share` (and maybe `ceiling` and `of`) or `levels`. It may also have a `sun` and `even = false`.
- `even = false` takes the even light away from every room of the module (`detail-bake --no-even`). A room that its lamps, the sun and the sky cannot bring to its level stays darker, and its light lies where its lamps and its openings put it.
- An opening in a roof shines down. The bake first sorts the rooms. A room is under a roof when less than `ROOFED_FROM` (0.1) of its upward texels see more than `OPENING_TO` (0.2) of the sky. A room is a courtyard when more than `ROOFED_TO` (0.3) of them do. `detail-bake --gpu` prints both numbers for each room. In the enclave the rooms under a roof have 0.05 at the most, and the two courtyards have 0.74 and 0.82.
- In a room under a roof an opening shines with `ROOF_GAIN` (8) times the light that falls on it from the sun and the sky. A texel takes that light times the share of the sky it sees, and at most the light that falls on the open ground.
- A courtyard keeps the rule of 18 November 2026. A texel that sees less than `OPENING_FROM` (0.06) of the sky takes `OPENING_GAIN` (4) times that light. A texel that sees more than `OPENING_TO` (0.2) takes the sky's own light. The constants are in `crates/launcher/src/detail_bake/gpu.rs`.
- Grass takes its light from the walkmesh's lightmap texels, which no drawn mesh covers. The bake fills them too (`grass_texels` in `crates/launcher/src/detail_bake.rs`). It prints how many they are. An area without grass is not changed by it.
- `sun` gives an area its sun and sky. The games' files hold neither. `toward` is the way to the sun in game space. `colour` is the stored level a surface that faces the sun squarely shows of it. `sky` is the stored level a surface that sees the whole sky shows of it. `skylights` names meshes by their node's name. A skylight sends the light that falls on it from above down into its room, evenly to every side, and its own texels show that light. The sun and the sky are not scaled to a room's level. Only the lamps are.
- `detail-bake <module> --game <game> --lights <dump> --gpu --sun-fit` helps to write a `sun`. It prints the eight ways to the sun that fit the stored lightmaps best, with the stored colour in the sun and out of it. It also prints what is over each room's floor with the floor's stored level. A skylight shows there as one mesh over a floor that is as bright as a floor under the open sky.
- A room whose lamps cannot reach its level takes an even light on every texel. Since 18 November 2026 the even light gives all that the lamps cannot, so every room reaches its level. Before, it gave at most `B_FILL_SHARE` (0.4) of the level and a room lit by the sky stayed dark.

An example for 103per, from `<root>`:

```bash
sh tools/light-kinds/bake.sh 103per tsl c6
```

```bash
sh tools/light-kinds/bake-shoot.sh bake-103per 103per baked bake/c6/103per/b rep-poses.txt w-c6
```

```bash
sh tools/light-kinds/bake-shoot.sh bake-103per 103per off "" rep-poses.txt off-c6
```

```bash
python tools/light-kinds/bake-sheet.py target/light-kinds/game/bake-103per off-c6,w-c5,w-c6 today,before,after
```

The Endar Spire and the Harbinger have scratch folders with older names, so they take the folder as the fourth argument:

```bash
sh tools/light-kinds/bake.sh end_m01aa k1 r3 bake-gpu
```

```bash
sh tools/light-kinds/bake.sh 152har tsl r3 bake-har
```

The game is built with `cargo build --release -p launcher`. The scripts use `<root>/target/release/kjk.exe`. A bake uses the graphics card, and other sessions and the owner's own game share it.

## The current bakes

All are under `<root>/target/light-kinds/game/`. They are not in git and not in the PR.

| Area | Folder | Current bake | `KJK_DETAIL_BAKE` ends in | Levels | Pose files |
|---|---|---|---|---|---|
| The Harbinger, `152har`, tsl | `bake-har` | `bake/r2` | `bake/r2/152har/b` | held to `152har-levels.txt` | `poses.txt` (a to f) |
| The Endar Spire, `end_m01aa`, k1 | `bake-gpu` | `bake/r2` | `bake/r2/m01aa/b` | held to `end_m01aa-levels.txt` | `poses.txt` (16 poses), `poses-r.txt` |
| Peragus, `101per`, tsl | `bake-101per` | `bake/c5` | `bake/c5/101per/b` | `levels-c5.txt`, share 0.6 and cap 0.22 | `rep-poses.txt` (r1 to r11), `poses.txt` |
| Peragus, `103per`, tsl | `bake-103per` | `bake/c5` | `bake/c5/103per/b` | `levels-c5.txt`, share 0.6 and cap 0.22 | `rep-poses.txt` (s1 to s7), `poses.txt` |
| The Dantooine enclave, `danm13`, k1 | `bake-danm13` | `bake/s9` | `bake/s9/m13aa/b` | `levels-s9.txt`, the `lit` level with the enclave's sun and no even light | `poses-r.txt` (the 14 poses of `poses.txt`, named by room, and the owner's three report cameras `r1-grass`, `r2-train`, `r3-council`), `probe-poses.txt` |

- The levels files of the Harbinger and the Endar Spire are the levels the owner approved. He said "Looks good" of the Harbinger's bake and "looks good!" of the Endar Spire's. Their copies in the repository are `data/lights/levels/152har.txt` and `data/lights/levels/end_m01aa.txt`. Do not make them again from the live lights.
- The last pictures of each bake are in `shots/re-r2` (the Harbinger, the Endar Spire) and `shots/w-c5` (Peragus).
- All four areas were baked again with `bake.sh` on 18 November 2026, last as `bake/g6` in each folder, after every change of that day. The Harbinger's 108 lightmaps and the Endar Spire's 90 are the same as `bake/r2` to the last pixel. Peragus differs from `bake/c5` in two small rooms that are on the even light, `103pero` and `101peras`: 1 of 47 lightmaps in 103per (largest pixel difference 8 of 255) and 7 of 175 in 101per (largest 5). Their levels rose from 0.040 to 0.099 and from 0.086 to 0.110. Nobody has looked at those two rooms in the game. The means of set `b` are 0.1296, 0.1809, 0.3408 and 0.4840.
- All four were baked once more as `bake/g7` after the sun, the skylights and the `lit` level were built. Every lightmap is the same as `bake/g6` to the last pixel: 108, 90, 175 and 47.
- All four were baked again as `bake/g8` on 19 November 2026, after the grass texels and the stronger openings were built. Every lightmap is the same as `bake/g7` to the last pixel: 108, 90, 175 and 47.
- The old per-area `play.sh` and `shoot.sh` still lie in each scratch folder. Use the scripts in `tools/light-kinds/` instead.

## The working method

1. Change one rule.
2. Build the game.
3. Rebake all five areas with `bake.sh`. Give the round a new label. The Harbinger and the Endar Spire need their folder named (`bake-har`, `bake-gpu`).
4. Take the same poses of every area with the new bake. The pictures of the round before are the "before".
5. Make sheets and look at every picture yourself. A fix for one area often breaks another: the switched lamps of Peragus washed out a shadow on the Harbinger.
6. Only then show the owner. He reviews before and after pictures of ready proposals, and he walks the area himself. He does not fill in forms.

The owner walks with `kjk play --stage <module> --walk` in a real window. Give him the whole command with full absolute paths, the worktree's included, and with `KJK_FITTED_LIGHTS=baked` and `KJK_DETAIL_BAKE` set. When he reports an odd shadow, first check whether it is a fitted light's edge, with a picture taken at `off`.

A module that opens on an intro (`end_m01aa`, `101per`) fades the screen in a normal launch. The scripts launch in walk mode, which runs none of the module's scripts.

## The open items

- **A level and a ceiling for each module.** It is built as `data/lights/bake.toml` and `bake.sh` (see above). The owner asked for it: "we should be able to have different levels of light/darkness in different modules" and "we want a script that is general and versatile and can bake new areas well". He has not seen the file. Only scripts read it. The game does not, and nothing bakes on a player's machine yet.
- **How the lightmaps ship.** The bakes are not in the PR. Nothing is decided. The last session's reading was that the game bakes on the player's machine by default, and that a pack of ready lightmaps is the owner's call.
- **Outdoor areas.** The bake has a sun and a sky since 18 November 2026, for an area whose entry in `data/lights/bake.toml` has a `sun`. Only the Dantooine enclave has one. Tatooine has not been tried.
- **The Dantooine enclave.** `danm13` is baked with `bake.sh danm13 k1 <label>`. The current bake is `bake/s9`. The owner walked `bake/s8`. He saw `bake/s9` on 19 November 2026 and said: "approved!". The enclave has no levels file in `data/lights/levels/` yet, so a later change to the live lights can still move its bake. His words on the first bake (`bake/g1`), all of 18 November 2026: "the lights are not warm enough. also its too dark in the jedi masters area. i guess the sun isnt hitting it or something? its too dark overall everywhere", then "without the sun its all very dark. all npcs are very dark. sleeping quarters / smaller rooms and cooridors look cool, but many doors are dark. and outside areas are too dark. we should add the sun". What was done:
  - **The sun and the sky.** `[modules.danm13]` has a `sun`. Its way is the best fit of `--sun-fit` (0.746). The first courtyard's shadow falls where today's does.
  - **Skylights were tried and taken out.** `bake/s5` made the training room's ceiling (`plane06`) and the council chamber's (`plane01`) skylights. The owner walked it and said what the next point quotes. The `skylights` key still works and no module uses it.
  - **Openings shine and there is no even light** (`bake/s8`). The owner walked `bake/s5` and said: "the openings should shine light down through them, and we shouldnt have so much artificial light generated for the rooms. we want them naturally dark in the sides and then have the skylight come down in the middle and scatter". His three reports are `kjk-report-20261118-202333.zip`, `-202425.zip` and `-202455.zip` in his `AppData\Roaming\kjk\reports`. They show the council chamber's round opening ("there is no skybox here but there should be. also there should be light from here, not just artificial room light"), the training room's slit ("have light shine down from here across the whole opening. and make the room darker") and a bedroom's slit ("light from here"). The enclave's entry has `even = false`. The mean brightness of the 14 views is 32 in `bake/s8`. View by view, today then `bake/s8`: 01a 53 and 54, 02a 34 and 51, 02b 22 and 13, 02c 23 and 14, 02e 53 and 27, 02g 36 and 19, 02h 36 and 24, 02j 37 and 26, 03a 67 and 33, 04a 66 and 53, 06a 33 and 37, 09a 39 and 31, 11a 43 and 29, 13a 52 and 43. The sheets are in `bake-danm13/sheets-s8`: today, `bake/s5`, `bake/s8`.
  - **The owner's three reports on `bake/s8`** (19 November 2026). They are `kjk-report-20261119-065329.zip`, `-065358.zip` and `-065443.zip` in his `AppData\Roaming\kjk\reports`. Their cameras are the last three poses of `bake-danm13/poses-r.txt`.
    - "grass is black here", in the first courtyard. The cause and the fix are in "The level of each module" above (`grass_texels`). The enclave has 26,278 such texels. The black tufts are gone in both courtyards.
    - "shine down some more light here", by the training room. "shine down even more light here from the skylight (opening in the roof)", in the council chamber. The fix is `ROOF_GAIN` (8) for the rooms under a roof. The courtyards keep the old rule, so the second courtyard is as bright as in `bake/s8`. The rooms' sides are still dark and `even = false` stays.
  - **The views of `bake/s9`.** The mean brightness of the 14 views is 42 today, 33 in `bake/s8` and 36 in `bake/s9`. View by view, today, `bake/s8`, then `bake/s9`: 01a 53, 53 and 54. 02a 34, 52 and 52. 02b 22, 12 and 14. 02c 22, 14 and 15. 02e 52, 27 and 28. 02g 35, 19 and 20. 02h 35, 24 and 24. 02j 37, 26 and 26. 03a 69, 34 and 47. 04a 66, 53 and 71. 06a 33, 37 and 37. 09a 39, 31 and 37. 11a 43, 30 and 39. 13a 52, 44 and 47. The report cameras: the grass 55, 51 and 53. The training room 69, 33 and 44. The council chamber 81, 58 and 84. The sheets are `bake-danm13/sheets-s9/cmp-1.png` to `cmp-5.png`. The pictures are `shots/off-r`, `shots/s8-r` and `shots/s9-r`, all taken with one build on 19 November 2026.
  - **Trial files of 19 November 2026.** `bake/t1` and `bake/t2` in `bake-danm13`, with their logs and `shots/t2-r`, are trials on the way to `bake/s9`. `bake/t2` is the same as `bake/s9` to the last pixel. They may be deleted. The build that made `bake/s9` is `target/release/kjk-open.exe`, a copy of `deps/kjk.exe`.
  - **The room levels of `bake/s9`.** The training room's `b` level is 0.31 (0.25 in `bake/s8`, its target is 0.59). The council chamber's is 0.51 (0.42, target 0.63). The bedrooms 09a and 11a rose too, because their slits are openings under a roof.
  - **The council chamber's round opening showed black, not the sky.** Another session fixed the game's drawing on 19 November 2026. A room's untextured mesh that casts shadows is no longer drawn (`mesh_drawn` in `crates/game/src/material.rs`). The fix is not committed. Nobody has looked at the Ebon Hawk with it.
  - **The owner's game may run from `target/release/kjk.exe`.** A build then cannot replace it. Copy `target/release/deps/kjk.exe` to `target/release/kjk-bake.exe` and set `KJK_EXE` to its full path. `bake-env.sh` reads it.
  - **The level is `lit`.** See "The level of each module".
  - **Characters take the probes' light whole under the bake** (`BAKED_GAIN_PROBES` in `crates/game/src/odyssey.wgsl`). The probes read the baked lightmaps, which are stored at half. A character then kept half of that, a quarter in all. Now it takes the full level from the probes and half of the lamps' own light. This changes characters, doors and placeables under every bake, the four approved areas' too. Nobody has looked at those four with it.
  - The wall lamps' kind `lda_lite01` has the colour 1.0, 0.93, 0.76 (`fittings.toml`). The rules had taken 0.749, 0.996, 1.0 from the lamp's blue-white glass.
  - The texture `lda_trim03` is no lamp (`fittings.toml`). It is the trim of the ceiling grilles and the wall arches. As an unnamed glowing texture it took 186 lights, and they lit the grilles' dark bars away.
  - The mean brightness of the 14 views is 42 today, 22 in the first bake, 30 before the sun (`bake/g5`) and 43 in `bake/s5`. View by view, today then `bake/s5`: 01a 53 and 56, 02a 34 and 48, 02b 22 and 23, 02c 23 and 28, 02e 53 and 43, 02g 36 and 32, 02h 36 and 33, 02j 37 and 38, 03a 67 and 56, 04a 66 and 72, 06a 33 and 37, 09a 39 and 50, 11a 43 and 41, 13a 52 and 43.
  - The sheets are in `bake-danm13/sheets-s5n/cmp-1.png` to `cmp-4.png`: today, `bake/s5` with dark characters, and `bake/s5` with the characters' fix. `sheets-s5` has today, the bake before the sun, and `bake/s5`. `sheets-s1` to `sheets-s4` hold the steps between.
  - **Still wrong.** The halls without an opening are about half as bright as today (02b, 02c, 02e, 02g), with every lamp at its most scale of 6. The owner has not said whether that is what he wants. The second courtyard (02a) is brighter than today (52 against 34), in `bake/s8` and `bake/s9` alike. The light from the openings is yellow-green beside today's grey, because it has the sun's colour. The owner has not said anything of the colour. The training room is still under today's level (47 against 69). The courtyard's trees glow yellow: `lda_leaf02` is an unnamed glowing texture with 203 lights, as `lda_trim03` was. The doors the owner called dark have not been looked at one by one.
- **`per_lt01`'s reach is a hand edit in a generated file.** `data/lights/tsl.toml` has `reach = 7.0` for `per_lt01` with a comment, and `fittings.toml` has no entry for it. `kinds.py` writes 5.0 back. After running `kinds.py`, restore `tsl.toml` with git until `fittings.toml` holds the entry.
- **What was run from `tools/light-kinds/`.** Every script there has been run, `bake-sheet.py` on the enclave's pictures.

## House rules

- Do not commit or push unless the owner asks. Branches are never named `claude/`. Commits and pull requests carry no attribution lines.
- Do not run the tests or clippy locally. They run in the pull request.
- Ask the owner before any benchmark.
- Every launch is headless and muted with the scratch settings and saves, also in your own scripts and in every subagent's brief. A subagent's script once left `KJK_SAVES` out and overwrote the owner's last-played note.
- Subagents run on Opus or a smaller model.
- Other sessions and the owner's own game use the machine. Kill only your own processes, by PID, and never the owner's `kjk.exe`.
- Do not touch the worktree `bounce-light-trial-cc741e`.
- Everything the owner reads follows the writing rules in `CLAUDE.md`: plain full sentences, one fact each, short titles without colons.
- No file in the repository may hold a personal folder path. Scripts take the root from their own place.
