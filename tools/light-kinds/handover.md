# Hand-over for the fitted lights and their bake

Written on the evening of 18 November 2026. It replaces the hand-over of 17 November, which is in this file's git history (commit `1dcef9fd`). Read that older text only for the history of the survey, the review page and the live lights' rules.

In this file `<root>` stands for the worktree's root. The repository's commit check refuses a personal folder path in any file, so no file may hold the real one.

## Where the work is

- The branch is `feat/fitted-lights`, draft PR 576 on `kjk-engine/kjk`. Its head is `1dcef9fd`, "Switched lamps shine at a quarter of their kind's strength".
- The worktree is named `lighting-step6`. Everything in this file is committed there except the bakes and pictures, which lie in `target/` and are not in git.
- The bake scripts of this file and this text are the commit right after `1dcef9fd`.
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
| `rebake.sh <folder> <module> <game> <label> <share> <cap>` | The two-pass bake. It dumps the game's lights, bakes once to measure each room's live level, writes `levels-<label>.txt` with min(cap, share × live) for each room, and bakes again with `--levels`. The result is `bake/<label>`. About 75 seconds. |
| `rebake-held.sh <folder> <module> <game> <levels> <label>` | One bake of the game's lights as they are now, held to a levels file made earlier. The result is `bake/<label>`. |
| `bake-shoot.sh <folder> <module> <fitted> <bake> <poses> <label>` | One launch that takes a picture at every pose of a poses file. `<fitted>` is `off`, `on` or `baked`. `<bake>` is the baked set's folder or `""`. The pictures are `shots/<label>/<pose>.png`. |
| `bake-play.sh <folder> <module> <fitted> <bake> <commands>` | One walk-mode launch with a command list. The other scripts use it. |
| `bake-env.sh` | Sourced by the others. It sets the root and the scratch variables. |
| `bake-sheet.py <area folder's path> <labels> <titles>` | Sheets of several rounds' pictures side by side, written to `cmp-<n>.png` in the area's folder. It needs Pillow. It was copied from the scratch folder and has not been run from its new place. |

A poses file has one pose a line: `name x y z yaw pitch fov`. An F12 report in walk mode holds `camera.txt` with the same numbers.

The module name differs between the tools for KOTOR 1. `detail-bake` takes `end_m01aa --game k1` and writes into a folder named `m01aa`. Peragus and the Harbinger take `--game tsl`, and their folder has the module's name. Each bake script prints the `KJK_DETAIL_BAKE` path at its end.

An example for 103per, from `<root>`:

```bash
sh tools/light-kinds/rebake.sh bake-103per 103per tsl c6 0.6 0.22
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

The Endar Spire and the Harbinger are held to their approved levels:

```bash
sh tools/light-kinds/rebake-held.sh bake-gpu end_m01aa k1 end_m01aa-levels.txt r3
```

```bash
sh tools/light-kinds/rebake-held.sh bake-har 152har tsl 152har-levels.txt r3
```

The game is built with `cargo build --release -p launcher`. The scripts use `<root>/target/release/kjk.exe`. A bake uses the graphics card, and other sessions and the owner's own game share it.

## The four current bakes

All four are under `<root>/target/light-kinds/game/`. They are not in git and not in the PR.

| Area | Folder | Current bake | `KJK_DETAIL_BAKE` ends in | Levels | Pose files |
|---|---|---|---|---|---|
| The Harbinger, `152har`, tsl | `bake-har` | `bake/r2` | `bake/r2/152har/b` | held to `152har-levels.txt` | `poses.txt` (a to f) |
| The Endar Spire, `end_m01aa`, k1 | `bake-gpu` | `bake/r2` | `bake/r2/m01aa/b` | held to `end_m01aa-levels.txt` | `poses.txt` (16 poses), `poses-r.txt` |
| Peragus, `101per`, tsl | `bake-101per` | `bake/c5` | `bake/c5/101per/b` | `levels-c5.txt`, share 0.6 and cap 0.22 | `rep-poses.txt` (r1 to r11), `poses.txt` |
| Peragus, `103per`, tsl | `bake-103per` | `bake/c5` | `bake/c5/103per/b` | `levels-c5.txt`, share 0.6 and cap 0.22 | `rep-poses.txt` (s1 to s7), `poses.txt` |

- The two levels files of the Harbinger and the Endar Spire are the levels the owner approved. He said "Looks good" of the Harbinger's bake and "looks good!" of the Endar Spire's. Do not make them again with `rebake.sh`.
- The last pictures of each bake are in `shots/re-r2` (the Harbinger, the Endar Spire) and `shots/w-c5` (Peragus).
- `rebake.sh` was run again on 103per from its new place on 18 November 2026. It gave the same levels and the same means as `bake/c5` (live 0.2394, b 0.1295).
- The old per-area `play.sh` and `shoot.sh` still lie in each scratch folder. Use the scripts in `tools/light-kinds/` instead.

## The working method

1. Change one rule.
2. Build the game.
3. Rebake all four areas. Use `rebake-held.sh` for the Harbinger and the Endar Spire, and `rebake.sh` with 0.6 and 0.22 for both Peragus modules. Give the round a new label.
4. Take the same poses of every area with the new bake. The pictures of the round before are the "before".
5. Make sheets and look at every picture yourself. A fix for one area often breaks another: the switched lamps of Peragus washed out a shadow on the Harbinger.
6. Only then show the owner. He reviews before and after pictures of ready proposals, and he walks the area himself. He does not fill in forms.

The owner walks with `kjk play --stage <module> --walk` in a real window. Give him the whole command with full absolute paths, the worktree's included, and with `KJK_FITTED_LIGHTS=baked` and `KJK_DETAIL_BAKE` set. When he reports an odd shadow, first check whether it is a fitted light's edge, with a picture taken at `off`.

A module that opens on an intro (`end_m01aa`, `101per`) fades the screen in a normal launch. The scripts launch in walk mode, which runs none of the module's scripts.

## The open items

- **A level and a ceiling for each module.** The owner wants the light level to be able to differ per module. The proposal is one file in `data/lights/` with a share and a cap for each module, and the original level as the default. It is not built and the owner has not agreed to it. Today the share and the cap are arguments of `rebake.sh`.
- **How the lightmaps ship.** The bakes are not in the PR. Nothing is decided. The last session's reading was that the game bakes on the player's machine by default, and that a pack of ready lightmaps is the owner's call.
- **Outdoor areas.** The bake has no sun. It has only been tried indoors. The owner asked whether it can replace the Tatooine shadows, and the answer was that it cannot yet.
- **The Dantooine enclave is next.** It has hundreds of fittings and 179 hidden lamps.
- **The 57 kinds found by colour.** The survey finds 25 more kinds in KOTOR 1 and 32 in KOTOR 2 by their colour (`by_colour`). Their proposals are the last block of `fittings.toml`. The owner has seen none of them.
- **A cart's hard shadow.** A cart on Peragus casts a hard black rectangle. It is a kept shadow tile from a lamp of the game's own light list, not from a fitted light. The owner said to leave it.
- **A force field's baked light stays.** The bake holds the field's light, so the light stays after the field opens.
- **The force cage at the new speed.** Nobody has watched the force cage since the scrolling speed went from 250 to 50.
- **The Endar Spire's door frame.** The owner called it "still a bit bright". The door is at `camera 43.60 64.80 0.53`.
- **Faults the owner accepted in the Endar Spire's bake.** Room 07a is flatter and brighter than live, and the bridge is about 14% darker. He said "we can move on".
- **`bake-sheet.py` and the two held bakes from the new scripts.** Only `rebake.sh` and `bake-shoot.sh` were run from `tools/light-kinds/`. `rebake-held.sh` shares their parts and was not run.

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
