# Hand-over for the fitted lights and their bake

## Round r2 (22 November 2026, afternoon)

This section is the newest. Read it first, then round r1 below it, then the KOTOR 1 section, then "Start here".

The owner looked at round r1 and gave notes. Round r2 acts on them. The owner asked to bake only the modules they commented on, so r2 holds 44 modules and not all 171. The work is committed as `514100cc` on `feat/fitted-lights`. Another session's sky fix is still uncommitted in the worktree (`material.rs`, `scene.rs`, `approved.rs` and six small files).

### What changed

- **Blended room surfaces show their bake.** Under the baked trial a room's surface that blends showed its bake at a quarter of its light, because only an opaque room's surface took `BAKED_GAIN`. The new flag `ROOM_LIGHTMAP` (`material.rs`, `odyssey.wgsl`) marks every room surface with a lightmap. Dxun's boulders (401dxn) and the enclave's shrubs (605dan) were black, and they show their texture now.
- **Inside a wall.** A texel counts as inside a wall only if the backs it meets are within `WALL_NEAR` (1 m, `gpu.wgsl`). The rays from the sand of tat_m20aa met the tents' cloth overhead, and whole triangles of sand were left out.
- **See-through layers.** A mesh with a lightmap whose texture's mean alpha is under `LAYER_ALPHA` (0.9, `detail_bake.rs`) casts no shadow. The sand and foam layers of unk_m41ac walled in the shore's bottom.
- **A floor of stored light.** `floor` in `bake.toml` (`detail-bake --floor`) writes `floor.txt`, and the game holds each texel to that share of its stored linear light (`BAKED_FLOORS`, `viewer.rs`). Only stunt_18 has it (0.2). Its wall in view v180 went from 4 to 24, against 47 in the original game.
- **Stale notes.** A bake removes an old `fill.txt` or `floor.txt` from its folder before it writes.
- **Debug pictures.** `KJK_BAKE_DEBUG=1` writes `debug/<lightmap>.png` and `debug/texels.txt` into the bake's folder. Each picture is three wide: what each texel is, the stored light and the bake. Red is a texel inside a wall, blue one off its triangle, dark cyan one that no light faces, and grey the share of its light that its shadows leave. `--probe` prints whether each texel counts.
- **Terminals.** Strength 0.6, which the owner approved. A terminal leaves about half of the blue on the floor it left before each screen became one light.
- **Malachor.** Each level is half of round r1's, and `mal_floor3`, `mal_gtbeams` and the new door kind `mal_dor02` shed more light. The new key `door = true` lets a door take a named kind (`fitted_lights.rs`, `kinds.py`). 903mal has no fill, and the owner called it perfect. 902mal, 904mal, 905mal and 906mal keep the stored fill, because without it they went nearly black and the owner said 902 and 904 were too dark.
- **The even light of round r1.** kas_m22ab, 304nar, 352nar, 411dxn, 506ond, 610dan and 853nih bake without the fill again. 410dxn and tar_m02ab keep it, because the owner asked for more light there. tar_m08aa has `backdrop = "stored"`.

### Open

- **tar_m03aa's square shadows are fixed** (after the commit, not yet committed). The cause was the bake's smoothing of bounced light and of shadow shares. It gathered a texel's neighbours in its lightmap, and the walkway's floor tiles each have their own small patch, so every tile took one level and the tiles met at hard straight edges. Finer texels and a softer fire changed nothing. The smoothing now gathers neighbours by their place in the area, across patches and lightmaps (`Places` in `gpu.rs`), within `PLACE_REACH` (1 m). The owner's "Telos tile one" is read as this module. The fires (`plc_burnjunk`, `plc_firepit2`) now reach 7 m at strength 0.15, which the owner kept. `KJK_BAKE_TEXEL` bakes coarse lightmaps finer, and nothing uses it. The enclave's 17 views moved by at most 2 in mean brightness with the new smoothing.
- **stunt_18** is lit by its floor and is still half as bright as the original.
- **Characters and placeables** do not follow their room's light under the bake. The owner moved it elsewhere: `handover-models.md`.
- **tat_m20aa** is brighter than today (39 against 23 at the report's camera), as the bake the owner walked was.

### Files

- `target/light-kinds/batch/r2.sh` bakes round r2. It reads `walk/walked.txt`, and it shoots each module at its `r1-poses.txt`.
- `target/light-kinds/batch/walked-shoot.sh` shot each walked bake at the same poses as `shots/walked`.
- `python -I target/light-kinds/batch/r2-sheets.py` writes `target/light-kinds/review-r2`. Its `points.html` is written by hand.
- A path given to `bake-shoot.sh` must be absolute or relative to the module's folder. A path relative to the root loads no bake, and the game shows the stored lightmaps at twice their light.

## The KOTOR 2 notes and round r1 (22 November 2026, morning)

The owner walked KOTOR 2 before the KOTOR 1 fixes, spoke notes for Telos and Nar Shaddaa, and made 20 F12 reports. The owner then said to fix all of it, bake every module again, and show a before and an after of each report and of each bake. Nothing is committed.

### The KOTOR 2 fixes

- **The Telos station (`202tel`)** is darker and lit by its ceiling lights. It has `share` 0.55 with `fill = "stored"`, `most` 2 and `least` 0.2.
- **The prison (`203tel`)** has 0.6 with the stored fill, its batch sun and `even = false`, so its lamps make its light.
- **`222tel`** has 0.75 and **`232tel`** has 0.8.
- **`304nar`** has 0.5 with the stored fill, `most` 1.5 and `least` 0.15.
- **The tunnels of `305nar`** have the new key `dark_unlit = true`. A room with no lightmap then shows little light (`BAKED_UNLIT_LIGHT` 0.12 in `odyssey.wgsl`), and only its bright texels keep their light. The circular strips stay bright and the walls go dark. The owner's liked part of the module did not change. `bake.sh` writes `unlit.txt` into the set, and the game reads it (`DARK_UNLIT_AREAS` in `viewer.rs`).
- **`351nar` and `352nar`** have 0.7, and their glow panels light them. `nar_d_lit01` is a panel kind with a reach of 5 m.
- **Lamps of the reports.** `nar_jtrm`, `nar_jtrm2` and `nar_d_wl_o` are `always`. `tel_hl3` and `dan_hlt1` are panels with `tint`. `dan_cgrn`, `dan_cblu` and `dan_cylw` are coloured points. `mal_gtbeams` and `mal_floor3` shed faint coloured light. `fx_ritual01` reaches 8 m. `tel_flwr` is no lamp. `ond_lt02` reaches 9 m.
- **The grid lines of `604dan`, `702kor` and `710kor`** came from the older mixed fill of round b3. A fresh bake with `bake.sh` has none.
- **Modules from the reports** have entries too. `410dxn`, `411dxn`, `506ond`, `610dan` and `853nih` take their stored fill at their full level. `401dxn` has its sun with a weaker sky (0.40). `605dan` has 0.75 of its batch sun. `510ond` and `901mal` have their batch suns without the even light. `903mal` has 0.85, and `904mal` and `905mal` have 0.75.

### Round r1

- `target/light-kinds/batch/r1.sh` baked every module of `walk/chosen.txt` again as `r1`. A module with a `bake.toml` entry baked with `bake.sh`. Any other module baked as the owner's walked bake was made, with its flags, its sun and its levels file, from a fresh dump of the lights. All 171 baked, none failed, in about six hours.
- Each module was shot at its entry views and at the cameras of the owner's reports.
- The first round wrote the rooms with no walkable face at their stored light in every module. `target/light-kinds/batch/backdrops.py` lists those lightmaps of a set. They were moved out of 33 modules' `r1` sets into `bake/r1-backdrop`, and `target/light-kinds/batch/r1-reshoot.sh` shot those modules again. Set `a` is the same either way.
- `python -I target/light-kinds/batch/r1-sheets.py` writes `target/light-kinds/review-r1`. `index.html` shows every report beside the same camera now, and every module's entry views from the walked bake beside `r1`.
- `walk-lists.py` puts `r1` first in each module's labels, so `play --walk` shows round r1.

### Open

- **The swoop track of `510ond`.** The report's camera is in the swoop race, and the shot shows the track's start. The track needs probes over it.
- **The red lights of `410dxn`** are on `dxn_ttrm2`, a mesh over the sky. They need placed lights.
- **The torches of `411dxn`** are emitters. Only `fx_ritual01`'s reach changed.
- **The crates of `411dxn` and `unk_m44ac`** are too bright under the probes. This is still parked.
- **The red door of `853nih`** still tints its frame.
- **The grass of `401dxn`** has no lightmap and glows.
- **The KOTOR 1 open items below** are still open.

## The KOTOR 1 notes are worked through (22 November 2026, night)

The owner's notes of his KOTOR 1 walk are in a file he uploaded, not in the repository. Two agents read them against his 20 F12 reports. The fixes below are in the worktree and are not committed. The owner has not asked for a commit.

### General rules, all seen by the owner in before and after pictures

- **Rooms with no walkable face can show their stored light at today's level.** With `backdrop = "stored"` in `bake.toml` (`detail-bake --stored-backdrop`), the bake writes their stored lightmap into set `b` (`write_sets` in `crates/launcher/src/detail_bake.rs`). Left out, the game shows them at twice their light. Taris's buildings and Kashyyyk's trees glared beside dark walkways, so `tar_m02ab` and `kas_m22ab` have the key. It was for every module at first, and round r1 showed it darkened modules the owner had walked and liked, such as Davik's estate (`tar_m08aa`) at 54 against 107.
- **A kind with `tint` sheds its texture's colour.** Before, only a kind with `lamps = "bright"` did, and 49 kinds shed white. The Leviathan's red lamps are red now (`fitted_lights.rs`, `ON_PIECE`). `plc_starmap15` lost its `tint`, because the Star Map then shed red.
- **`lsi_tech01` and `lts_lolit03` have `dark = true`.** The blue displays and the hanging ceiling panels shed light.
- **A terminal sheds one light for each screen.** The new key `lift` gathers a kind's pieces within 1 m into one light, aimed out of the screen and up by the lift in radians. The terminals have `lift = 0.785`, `cone = 0.6` and strength 0.24, which is four fifths of the old light. The owner accepted that a terminal now leaves little blue on the floor. `kinds.py` knows the key.
- **Fire and force cages shed light.** `plc_burnjunk` and `plc_firepit2` have one faint warm light each (strength 0.2). `plc_frcdist01` and `plc_frcdist02` are kinds with `tint`.
- **A box does not shadow a light inside it** (`cone_visibility` and `fitted_visibility` in `odyssey.wgsl`). The kolto tanks of manm26ad and the Star Map of manm28ad threw a dark ring round them from their own light.
- **Dark rooms are darker.** The modules of the owner's darken list have `share` 0.55 and `most` 2.0, and the cantinas `tar_m02ae` and `tar_m03ae` have 0.4 and 1.5. All of them have `least` 0.2, or 0.15 in the cantinas. Many rooms were held at the least scale 0.5 and could not go darker. The Endar Spire is held to its approved file times `scale = 0.65` with `most = 2.5`. `bake.sh` reads `scale` for a module with a levels file. Every Star Forge area has 0.85 of its level, and `sta_m45ab` 0.6.

### Fixes of single modules

- `tar_m02ab` and `kas_m22ab` take `fill = "stored"`, as `tar_m02ac` does. Their catwalks and the frames on the trees were far darker than what stands beside them.
- The Anchorhead door texture `lta_dor02` is no lamp. Its green passed as a force field.
- `plc_map01` reaches 2.4 m. `lts_gwall01` sheds a paler green, and `tar_m02ad` has 0.85 of its level.
- `plc_desk1` is a kind with `always`, so the swoop manager's desk lights the floor round it. The owner accepted the swoop lobby as it is now.
- `lma_light01` has `cone = 0.7`, `lift = -0.6`, `always = true` and a reach of 4 m. `manm28ad` has the new key `every_room = true`, which is `detail-bake --every-room`. It bakes every room that has a lightmapped mesh, as a sun does, so the pole lamps light the sea floor.
- `tat_m20aa`'s suns have `width = 0.025`. The pictures did not change.

### What is parked or open

- **The plasteel crates of unk_m44ac are too bright.** Under the bake with `KJK_PROBES=off` a crate is as bright as today. With the probes it is half as bright again, even where the whole view is at today's level. Ruled out are a light of its own, the probe floor, the probes' open rays, half of the probes' sides, the glow cap, and the module's levels. The owner parked it on 22 November 2026. The next step would be a debug view of each term of a model's light.
- **The sun of unk_m44ac** from the batch is strong (1.18). The view is 89 against 67 today.
- **Kashyyyk m23ab** is 40% darker in its lightmap and only a little darker in the picture.
- **Three notes need a report zip.** They are the red light by the benches in `tat_m17ae`, the droid bench in `korr_m33aa` and the three ceiling lights of the rancor room.
- **The square shadows of lower Taris (`tar_m03aa`)** may be the edges between rooms. Each room scales its lamps by its own amount, and the fire's light there stops at a straight line. Nobody has confirmed it.
- **Every walked module is baked again** in round r1, above.
- **The builds.** The owner's game ran from `target/release/kjk.exe` all night, so every build was copied to `target/release/kjk-bake.exe` and run from there.
- **The pictures** of each round are in `target/light-kinds/review-n1` to `review-n11`, `review-c` and `review-p1`.

## Start here (written 21 November 2026, evening)

This part is for the agent who takes this worktree over. Read it first.
The rest of the file is the history, oldest first, and its last sections
are the newest.

### The task now

The owner walked every KOTOR 1 module on 21 November 2026 with the baked
lightmaps shown, and made notes and F12 reports. The next agent fixes
KOTOR 1 from those notes. After that the owner walks KOTOR 2 the same way.

1. Ask the owner where his notes of the walk are. He may have typed them
   into the F12 boxes, or he may hold them elsewhere, such as voice notes
   or a `docs/MODULE-NOTES.md` on another branch. This worktree has no
   such file.
2. Read the reports. They are the 20 zips named `kjk-report-20261121-*`
   in the owner's `AppData\Roaming\kjk\reports`. Each holds `note.txt`
   (his words), `breadcrumbs.txt` (a line `module <name>`), `camera.txt`
   (a console `camera` line that puts the camera back) and
   `screenshot.png`.
3. Sort the notes into general rules and single-module faults. His rule
   is that feedback on one area applies to all areas. Show him the sorted
   list before changing anything.
4. Fix, bake again, and show before and after pictures at his cameras.
   "The working method" and "House rules" below still hold.

### What is not committed

The branch is `feat/fitted-lights`, draft PR 576. Its head is `951fab66`
and matches origin. These are in the worktree and not committed. The
owner has not asked for a commit of them.

- The stepper of walk mode: `crates/game/src/walk.rs`, `play_view.rs`,
  `sim_host.rs`, one line in `scene.rs` (`.add_systems(Update,
  crate::walk::step)`), the list of bake folders in `viewer.rs`,
  `tools/light-kinds/walk-lists.py` and this file.
- Another session's sky fix, from 19 November: `approved.rs`,
  `asset_ja/src/lib.rs`, `rig.rs`, `asset_odyssey/src/model.rs`,
  `course.rs`, `material.rs`, `model_fx/probes.rs`, `viewer_rig.rs` and
  the rest of `scene.rs`. Leave it alone. `scene.rs` holds both, so a
  commit of the stepper must stage only its one line there.
- `target/release/kjk.exe` is built from the worktree as it is, with both.

### Walking KOTOR 2

"Walking every module in one run" near the end of this file says how the
walk works. Before the owner walks KOTOR 2:

1. Bake the 33 filled KOTOR 2 and KOTOR 1 modules again with `bake.sh`,
   so they are the new kind with `fill.txt`. Their bakes `b3` under
   `target/` are the older kind with the stored light mixed in. `902mal`
   and `107per` are done (`bake/t2`). Add the new label to `rounds` in
   `walk-lists.py`, at the front.
2. Apply the general fixes from the KOTOR 1 notes first, so he does not
   report them twice.
3. Run `python tools/light-kinds/walk-lists.py <root>` again. It writes
   `target/light-kinds/walk/bakes.txt` and `modules-tsl.txt`.
4. Give him these lines for a new PowerShell window, with the real full
   path in place of `<root>`:
   - `$env:KJK_FITTED_LIGHTS = "baked"`
   - `$env:KJK_DETAIL_BAKES = "<root>\target\light-kinds\walk\bakes.txt"`
   - `$env:KJK_WALK_MODULES = "<root>\target\light-kinds\walk\modules-tsl.txt"`
   - `& "<root>\target\release\kjk.exe" play --stage 001ebo --walk`
5. Page Down is the next module and Page Up the last one. `201tel` and
   `505ond` have no bake and are not in the list.

### What else is open

- The default for a module without its own entry in
  `data/lights/bake.toml` is still the even light. The plan was to make
  `fill = "stored"` the default, and the owner has not said yes to it.
  The batch's pictures of those modules were made with no fill at all.
- A bake sometimes dies without a message (exit code 127). Try it once
  more.
- `tat_m17ac` looks flat. `305nar`, `manm26ad` and `903mal` stay darker
  than today. The Sand People Enclave has bright wall bases and shadows
  that begin in the wrong place.
- The owner said there are no tokens left on 21 November 2026. Work
  cheaply: scripts that run alone, one summary at the end, small pictures.

Written on the evening of 18 November 2026. The enclave's part was brought up to date on 19 November 2026. It replaces the hand-over of 17 November, which is in this file's git history (commit `1dcef9fd`). Read that older text only for the history of the survey, the review page and the live lights' rules.

In this file `<root>` stands for the worktree's root. The repository's commit check refuses a personal folder path in any file, so no file may hold the real one.

## Where the work is

- The branch is `feat/fitted-lights`, draft PR 576 on `kjk-engine/kjk`. Everything in this file is committed and pushed there, except the bakes and pictures, which lie in `target/` and are not in git.
- The worktree is named `lighting-step6`. One thing in it is not committed: another session's fix of the game's drawing of the council chamber's roof opening (`crates/game/src/scene.rs`, `material.rs`, `crates/asset_odyssey/src/model.rs`, `crates/approved_tests/tests/approved.rs` and five files that only fill in a new field).
- `docs/LIGHTING-PLAN.md` is the whole lighting plan. Its steps 7 and 10 are the background, and its part 10b is the summary of this work. It does not describe the fitted lights' rules. Those are in the code's comments and in this file.

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
| `rebake-held.sh <folder> <module> <game> <levels> <label> [<of>]` | One bake of the game's lights as they are now, held to a levels file made earlier. `<of>` is `lit` when the file holds `lit` levels, and the bake then runs with `--weigh`. The result is `bake/<label>`. |
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
- `[modules.danm13]` names `levels/danm13.txt` and has `of = "lit"`, because its file holds `lit` levels. They are the levels of `bake/s9`, which the owner approved. A bake held to the file (`bake/h1`) is the same as `bake/s9` to the last pixel.
- A module's entry has either `share` (and maybe `ceiling` and `of`) or `levels` (and maybe `of`). It may also have a `sun` and `even = false`.
- A `sun` may hold `toward2` and `colour2`, a second sun. Anchorhead has one. `detail-bake --sun` then takes fifteen numbers.
- A light that reaches farther than `FAR_LIGHT` (500 metres) is no lamp in the bake. The report prints the way to each. Anchorhead's files hold three, and two of them stand where the sky's two suns are. None of the five approved areas has such a light.
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
| The Dantooine enclave, `danm13`, k1 | `bake-danm13` | `bake/s9` | `bake/s9/m13aa/b` | held to `data/lights/levels/danm13.txt`, the `lit` levels of `bake/s9`, with the enclave's sun and no even light | `poses-r.txt` (the 14 poses of `poses.txt`, named by room, and the owner's three report cameras `r1-grass`, `r2-train`, `r3-council`), `probe-poses.txt` |

- The levels files of the Harbinger and the Endar Spire are the levels the owner approved. He said "Looks good" of the Harbinger's bake and "looks good!" of the Endar Spire's. Their copies in the repository are `data/lights/levels/152har.txt` and `data/lights/levels/end_m01aa.txt`. Do not make them again from the live lights.
- The last pictures of each bake are in `shots/re-r2` (the Harbinger, the Endar Spire) and `shots/w-c5` (Peragus).
- All four areas were baked again with `bake.sh` on 18 November 2026, last as `bake/g6` in each folder, after every change of that day. The Harbinger's 108 lightmaps and the Endar Spire's 90 are the same as `bake/r2` to the last pixel. Peragus differs from `bake/c5` in two small rooms that are on the even light, `103pero` and `101peras`: 1 of 47 lightmaps in 103per (largest pixel difference 8 of 255) and 7 of 175 in 101per (largest 5). Their levels rose from 0.040 to 0.099 and from 0.086 to 0.110. Nobody has looked at those two rooms in the game. The means of set `b` are 0.1296, 0.1809, 0.3408 and 0.4840.
- All four were baked once more as `bake/g7` after the sun, the skylights and the `lit` level were built. Every lightmap is the same as `bake/g6` to the last pixel: 108, 90, 175 and 47.
- All four were baked again as `bake/g8` on 19 November 2026, after the grass texels and the stronger openings were built. Every lightmap is the same as `bake/g7` to the last pixel: 108, 90, 175 and 47.
| Anchorhead, `tat_m17aa`, k1 | `bake-tat_m17aa` | `bake/a8` | `bake/a8/m17aa/b` | `levels-a8.txt`, the `lit` level with two suns and no even light | `poses-r.txt` (the owner's four report cameras and the 14 poses of `poses.txt`), `sun-poses.txt` (the sky's suns) |

- All five approved areas were baked again on 19 November 2026 with the build that has the far lights and the second sun, as `bake/g9` and the enclave as `bake/h2`, once more as `bake/g10` and `bake/h3` after the bake learned to read every room, and as `bake/g11` and `bake/h4` after the suns' share went into the alpha. Every lightmap is the same as `bake/g8` and `bake/s9` to the last pixel: 108, 90, 175, 47 and 110.
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
- **Outdoor areas.** The bake has a sun and a sky since 18 November 2026, for an area whose entry in `data/lights/bake.toml` has a `sun`. The Dantooine enclave and Anchorhead have one.
- **Anchorhead on Tatooine** (`tat_m17aa`, 19 November 2026). It is baked with `bake.sh tat_m17aa k1 <label>`. The current bakes are `bake/a5` and `bake/a7`. `bake/a7` is `bake/a6` with the suns' share in its alpha. The owner walked `bake/a4`. He has not approved a bake.
  - **The first bake never ended.** The area's files hold three hidden lights about a kilometre off that reach 5,000 and 18,000 metres. The bake's light grid filled 21 GB on them. Such lights are now left out of the lamps (`FAR_LIGHT`).
  - **One sun, then two.** `bake/a2` has one sun, in the way of the 5,000 metre light, which is also the best fit of `--sun-fit` (0.722). The owner saw it and said: "general lighting looks good! but it doesnt look like shadows are aligned with the actual suns". The sky's two suns stand in the ways of the two 18,000 metre lights (`shots/sun`, taken with `sun-poses.txt`). `bake/a4` has those two suns, each with half of the light on level ground.
  - **The owner's four reports on `bake/a4`** (19 November 2026). They are `kjk-report-20261119-084858.zip`, `-084944.zip`, `-085019.zip` and `-085049.zip` in his `AppData\Roaming\kjk\reports`. Their cameras are the first four poses of `bake-tat_m17aa/poses-r.txt`. He said of the walk: "it looks pretty okay except for the shadows casted inside the shade, and the overly bright areas, and the strange square ground tiles in one large area. bushes are probably fine".
    - "this area is very strange and seems to have large squares of ground in different light/colors we need to fix that". Anchorhead has 140 lightmaps in rooms with no walkable face: every house and much of the ground. The bake skipped those rooms. Their triangles cast no shadow, and the game showed their stored lightmaps at twice their light beside the baked ground. A bake with a sun now reads every room that has a lightmapped mesh, and of such a room only those meshes (`every_room` in `texels`). A bake without a sun is not changed.
    - "the ground is definitely a bit too bright in the sun". That ground was one of the unbaked squares at twice its light. Earlier he had said "no revert your change it was good when it was bright" of a trial that showed the unbaked rooms at half.
    - "sun should be hitting here I think? did u place them correctly?". The suns are placed right. The spot lies behind a wall from one sun and at the edge of a far building's shadow from the other, which stands 16 degrees up. The game's own lightmaps were made with one sun 30 degrees up, so the town has more shade under the sky's two suns.
    - "characters are casting shadows in the shade. not sure if that is correct, should they really do that? what light are they blocking?". Fixed, and the owner has not seen it in a walk. The game shares a room's lightmap out among its lights by a guess (`room_shadow` in `crates/game/src/odyssey.wgsl`), and a sun's shadow took the sun's guessed share where the sun does not reach. Under the bake, `fitted_lights` in the same file also took light for the three far lights. The bake now writes the share of the suns that reaches each texel into the alpha of set `b` (`sunlit` in `write_sets`), as the mean of the two suns' shares. Under `KJK_FITTED_LIGHTS=baked` a light 100 metres wide or more takes and gives that share alone, in both functions. A bake without a sun has an alpha of 1 and is not changed. `bake/a7` is `bake/a6` with that alpha, and its colours are the same to the last pixel. `bake-tat_m17aa/shade.png` shows the shade view and a sunlit street before and after. Where one of the two suns reaches, a character's shadow is at half strength for both suns, which is too weak for the sun that reaches and too strong for the other. The enclave's 17 views are the same with the new alpha (`shots/s9-new` and `shots/h4-new`, mean difference 0.0).
  - **Two rounds for the owner to choose from.** `bake/a5` has the suns at the stored level, and its 18 views have a mean brightness of 90 against 88 today. `bake/a6` has the two suns' and the sky's colours at 1.3 times that, and its mean is 104. `bake/a4` was at 138. `data/lights/bake.toml` holds the colours of `a6`, and `bake-tat_m17aa/bake-toml-a5.txt` is the file as `a5` was baked. The sheets are `cmp-1.png` to `cmp-5.png`: today, `a5`, `a6`. The pictures are `shots/off-r`, `shots/a4-r`, `shots/a5-r` and `shots/a6-r`.
  - **The bake hung on the far land.** A room with no walkable face also holds the sky and the far land, with triangles kilometres long. They filled the ray grid. Only such a room's lightmapped meshes are read.
  - **`KJK_BAKE_LAPS=1`** makes `detail-bake --gpu` print each step's time as it ends, to find where a bake hangs.
  - **The dry bushes are black under the bake** (`tatshrub01`, texture `lta_shrub`). Today they are pale. Nobody has looked for the cause. The owner has not named them.
- **The Dantooine enclave.** `danm13` is baked with `bake.sh danm13 k1 <label>`. The current bake is `bake/s9`. The owner walked `bake/s8`. He saw `bake/s9` on 19 November 2026 and said: "approved!". Its levels are held in `data/lights/levels/danm13.txt` since then. His words on the first bake (`bake/g1`), all of 18 November 2026: "the lights are not warm enough. also its too dark in the jedi masters area. i guess the sun isnt hitting it or something? its too dark overall everywhere", then "without the sun its all very dark. all npcs are very dark. sleeping quarters / smaller rooms and cooridors look cool, but many doors are dark. and outside areas are too dark. we should add the sun". What was done:
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

## Anchorhead's sharp suns

- The owner walked `bake/a7` on 19 November 2026 and said: "overall brightness is good now! but some weirdness with shadows being diffuse/scattered along its edges, or being oddly shaped". So the brighter colours of `bake/a6` stay. His four reports are the poses of `poses-s.txt` in `bake-tat_m17aa`, each with a close-up.
- The cause was in the bake. A sun was 0.04 radians wide (`SUN_WIDTH` in `gpu.wgsl`), about ten times the real sun, and each texel cast its 16 rays from its middle. Far from a wall the two suns' wide edges ran into one wide blur. Close to a wall a texel was all lit or all dark, which gave steps. A thin thing far away, such as the antenna towers, cast no shadow that could be seen.
- A sun in `data/lights/bake.toml` may now have a `width`, half its width in radians. `bake.sh` passes it as one last number of `--sun`. With it each texel casts 64 rays (`SHARP_SUN_RAYS`) from a grid of places over its own square (`Place::side`, `point.shading.w` in the shader). A place that a surface cuts off from the texel's middle is not used. Anchorhead has `width = 0.01`. A sun without a width bakes as before, so the enclave is not changed.
- `bake/a8` is that bake. `sharp-1.png`, `sharp-2.png` and `sharp-3.png` in `bake-tat_m17aa` show the four views under `bake/a7` and `bake/a8`. The ground's shadows are crisp, each wall casts two shadows, one for each sun, and the towers cast thin shadows. The owner has not walked it.
- The shadow on the wall of the fourth report (`s4-stair`) is smoother and still has a lumpy edge. That wall's lightmap is coarse, and the bake writes four new texels to each stored texel's side (`SCALE`). A finer bake of coarse lightmaps is not built.
- The owner also said: "its probably gonna look much cooler if the light is more directional and whatnot". The sharp suns are the first step. More of the light from the suns and less from the sky is not tried.
- The five approved areas were baked again as `bake/g12` and `bake/h5` with this build. Every lightmap is the same as `bake/g11` and `bake/h4` to the last pixel: 108, 90, 175, 47 and 110.
- `bake/a9` is a trial of more directional light: the sky gives half as much light (0.28, 0.24, 0.20) and the suns a little more (1.33, 1.23, 1.05 and 1.54, 1.41, 1.20), so that sunlit level ground keeps its level. `sky-1.png` and `sky-2.png` in `bake-tat_m17aa` show `bake/a8` and `bake/a9` side by side. The shade is a little darker and the difference is small: the mean of 26 views is 102 under `bake/a8` and 99 under `bake/a9`. `data/lights/bake.toml` still has the colours of `bake/a8`, and `bake-toml-a9.txt` in the folder is the file as `bake/a9` was baked. The owner has not chosen.
- The owner chose `bake/a8` on 19 November 2026: "i pref a8. its decent although some weird shadows remain. we can move on". Anchorhead is approved at `bake/a8`, with the colours that `data/lights/bake.toml` holds. The weird shadows that remain are not fixed: the lumpy edges on coarse lightmaps, and a character's shadow at half strength where one of the two suns reaches.

## The Dune Sea has no lightmaps

- The Dune Sea (`tat_m18aa`, k1) was tried on 19 November 2026 with Anchorhead's entry of `data/lights/bake.toml` unchanged, as `bash tools/light-kinds/bake.sh tat_m18aa k1 d1`.
- The bake stopped at once with "m18aa: no lightmap on a room". That is not a fault of the bake. The game's dump (`bake-tat_m18aa/lights-d1.txt`) lists 516 meshes, and every one has `lightmap None`. The five rooms `m18aa_01a` to `m18aa_05a` hold 78 of them. Anchorhead's dump names a lightmap on 757 of its 1,365 meshes.
- So the lightmap bake has nothing to write in the Dune Sea, and `KJK_FITTED_LIGHTS=baked` cannot change its rooms. The question whether one setting of the suns serves all of Tatooine is not answered by this area.
- The area has 33 fitted lights. All are the path markers (`plc_marker1`, reach 2.5 metres).
- `data/lights/bake.toml` holds a comment for the area and no entry. No code was changed, so the six approved areas were not baked again.
- Not done: the far lights and `--sun-fit` were not read, because `detail-bake` stops before it prints them. No picture was taken. The disk had 3 MB free at that hour, and a launch writes more than that.
- The survey (`target/room-lights/room-lights.json`) says which Tatooine areas have lightmaps. `tat_m18ab` has none either. `tat_m18ac` has 3. `tat_m20aa` has 64. The Anchorhead interiors `tat_m17ab` to `tat_m17af` have 1 to 16 each.
- Not known: how the game lights the Dune Sea's ground today. A bake for such an area would need lightmap coordinates made for its meshes, or the suns' shadows drawn live.

## The Sand People Enclave

- The owner chose "Another Tatooine area" on 19 November 2026 after the Dune Sea. The Sand People Enclave (`tat_m20aa`, k1) is baked with `sh tools/light-kinds/bake.sh tat_m20aa k1 <label>`. The current bake is `bake/e5` in `bake-tat_m20aa`, and `KJK_DETAIL_BAKE` ends in `bake/e5/m20aa/b`. The owner has not walked it.
- **One setting of the suns does not serve all of Tatooine.** `bake/e1` has Anchorhead's entry unchanged (`bake-toml-e1.txt` in the folder is the file as it was baked). Its 14 views have a mean brightness of 80 against 47 today, and its shadows fall away from the suns that the enclave's sky shows.
- **The enclave's sky.** It is a dark blue evening sky with the two suns low in the south-east. Measured from pictures (`sun.png`, `shots/sun`, `shots/sun2`, the poses in `sun-poses.txt` and `sun2-poses.txt`), the ways to them are 0.831, -0.418, 0.367 and 0.594, -0.749, 0.292. That is about 53 degrees to the right of Anchorhead's suns, at nearly the same heights. A pose's field of view is the picture's height in degrees, and a thing to the left of the middle has the larger yaw.
- **The area's far lights.** The files hold three. One reaches 18,000 metres toward 0.550, -0.772, 0.318, within 3 degrees of the lower sun, and its colour is black. One reaches 18,000 metres toward 0.490, -0.265, 0.830. It has the upper sun's bearing and stands 56 degrees up, where the sky shows nothing. One reaches 1,000 metres toward 0.636, -0.636, 0.436, between the suns.
- **`--sun-fit`** (`sunfit.log`). The best fit is 0.612, -0.612, 0.500 (0.662), between the two suns and above them, as in Anchorhead. The stored lightmaps under the open sky are 0.535, 0.491, 0.404 in the sun and 0.194, 0.176, 0.155 out of it. Anchorhead's are 0.91, 0.84, 0.71 and 0.24, 0.21, 0.17.
- **What is the same as Anchorhead.** The method, `share = 1.0`, `of = "lit"`, `even = false`, `width = 0.01` and the factor 1.3. The recipe for the colours, with the stored levels taken to the power 2.2 and back: each sun's colour is 1.3 times the level of half the difference between the sunlit and the shaded ground, divided by the sun's height (the third number of its way). The sky's colour is 1.3 times the level of 1.6 times the shaded ground. The recipe gives Anchorhead's own numbers from Anchorhead's stored levels.
- **What differs.** The two ways and the three colours: 0.76, 0.70, 0.57 and 0.84, 0.78, 0.63 for the suns and 0.31, 0.28, 0.25 for the sky.
- **The wooden gates shed orange light.** The area's 136 fitted lights were all on the huts' gates (`lta_dor03`). The files make the gate glow, and its orange wood passed as a force field (`FIELD_CHROMA`). `data/lights/fittings.toml` and `data/lights/k1.toml` now call the texture no lamp. `k1.toml` got the entry by hand, as `kinds.py` would write it. A door ignored such an entry, so `lights` in `crates/game/src/fitted_lights.rs` now skips a door whose texture the data calls no lamp. The area has 4 lights left, all hidden lamps. The game reads the rule data when it is built.
- **The views.** The mean brightness of the 14 views of `poses.txt` is 47 today, 80 in `bake/e1` and 58 in `bake/e5`. View by view, today then `bake/e5`: a-0 48 and 60, a-180 63 and 66, c-0 62 and 75, c-090 55 and 69, c-180 52 and 68, c-270 60 and 73, d-0 46 and 43, d-180 44 and 49, e-135 34 and 49, f-090 34 and 47, g-045 26 and 58, h-315 45 and 48, i-270 39 and 46, j-225 45 and 62. The sheets are `cmp-1.png` to `cmp-4.png`: today, `bake/e1`, `bake/e5`. The pictures are `shots/off5`, `shots/e1` and `shots/e5`. `bake/e2` to `bake/e4` are the steps between and may be deleted.
- **The room levels of `bake/e5`.** Six rooms are above their target with their lamps at the least scale, because the suns and the sky alone give more. The middle room `m20aa_01` is at 0.29 against 0.42. It lies under the tents and has no even light.
- **Known faults.** The ground's shadows are softer than today's, because each of the two suns gives half of the light. The tents' red cloth is lit from below under the bake and is nearly black today. The view g-045 is more than twice as bright as today. The smooth sand in the middle looks the same as today in every round. Nobody has looked at the huts' insides.
- **The six approved areas** were baked again with this build as `bake/g13`, `bake/h6` and `bake/a10`. Every lightmap is the same as `bake/g12`, `bake/h5` and `bake/a8` to the last pixel: 108, 90, 175, 47, 110 and 169. `cargo fmt --all -- --check` reports only `crates/game/src/shadows.rs` at line 1130.

## The batch of every remaining module

- The owner walked the Sand People Enclave's `bake/e5` on 19 November 2026 and said: "some things are not casting shadows quite properly, its like it doesnt begin and end in the correct places always like some walls have certain part around their base that is completely bright, not sure what that is all about. but its fairly minor detail". Nobody has looked for the cause.
- He then asked for a bake of every remaining module with settings chosen by a quick rule, to review later. The batch ran from 14:08 on 19 November to 01:09 on 20 November 2026, with a second run for the failed modules until 02:05.
- The batch is `target/light-kinds/batch/all.sh` with its helper `auto.py`. They are not in git. Run it from the worktree's root with `sh target/light-kinds/batch/all.sh`. It skips a module whose folder holds `bake/b1.done`, and a file named `stop` in the batch folder ends it after the module it is on. It uses `target/release/kjk-batch.exe`, a copy of the build of 19 November.
- It bakes each of the 167 modules that have lightmaps and had no bake, as `bake/b1` in `bake-<module>`, and shoots four views of today (`shots/off-b1`) and of the bake (`shots/b1`) from the module's entry point (`auto-poses.txt`).
- **The rule for the settings.** Every module takes `share = 1.0` and `of = "lit"`. The script runs `--sun-fit` first. A module takes a sun when at least 20,000 texels lie under the open sky, the best way fits at 0.4 or better, and the stored sunlit level is at least 0.08 over the shaded one. The sun is one sun at the best fit with `width = 0.01` and no even light. Its colours come from the recipe of the Sand People Enclave's section, with the factor 1.0, and 1.3 for a module whose name starts with `tat_`. 63 modules took a sun. `auto-suns.txt` in the batch folder holds each one's numbers as `--sun` takes them, and `auto-suns.log` says why each module did or did not take one. None of these settings is in `data/lights/bake.toml`.
- **The results.** `summary.tsv` in the batch folder has a line for each module: its fitted lights, whether it took a sun, the mean of set `b`, the mean brightness of the four views today and baked, and how many rooms have their lamps at the most scale. The five modules of the second run have two lines each, and the later one counts. `sorted.txt` sorts the first run's lines.
- 129 modules are within 0.7 to 1.4 times today's brightness. 27 are brighter than 1.4 times: `ebo_m12aa`, `kas_m23aa`, `kas_m25aa`, `korr_m38ab`, `manm28aa`, `manm28ab`, `manm28ad`, `sta_m45ab`, `stunt_03a`, `stunt_07`, `stunt_16`, `stunt_19`, `stunt_35`, `stunt_42`, `tar_m02af`, `tar_m03af`, `tar_m05aa`, `tar_m08aa`, `tar_m10aa`, `tat_m17ac`, `tat_m17af`, `001ebo`, `262tel`, `303nar`, `604dan`, `853nih` and `904mal`. 8 are darker than 0.7 times: `danm14ae`, `danm15`, `manm26ad`, `stunt_18`, `tat_m18ac`, `unk_m44ac`, `305nar` and `710kor`. The numbers are of four views at the entry point and are a rough guide. Nobody has looked at any picture of the batch.
- **Three modules have no bake.** `stunt_57` has no fitted light, and `detail-bake` refuses a dump without one. `201tel` hangs after its first line, "64999 triangles", whose bounds are 3,400 metres wide: the sun fit ran into the script's limit of 10 minutes and the first bake into its limit of 15. `505ond` printed every step's time up to "pictures" in its first bake with a sun and then did not end within 15 minutes. Its sun fit alone ended in 71 seconds. Nobody has found either cause.
- `202tel` and `209tel` failed in the first run with an empty log and baked in the second run without a change. The cause is not known.
- The Tatooine modules of the batch have one sun between the sky's two. Each needs its two suns measured from pictures of its sky, as the Sand People Enclave's were.

## The batch's outliers

- The 36 modules outside 0.7 to 1.4 times today's brightness were looked at on 20 November 2026 on four sheets, `target/light-kinds/batch/out-1.png` to `out-4.png`. Each row is one module, with today's four views on the left and the bake's on the right, sorted from the brightest to the darkest. The owner was sent the sheets. He has not answered.
- **The sun rule gave an indoor module a sun** (about 11 modules). A ship or a station with windows to space, a cave and a temple pass the rule. With a sun the module is washed grey-white (`ebo_m12aa`, `stunt_03a`, `stunt_07`, `stunt_16`, `stunt_35`), or it loses the even light and goes dark (`danm14ae`, `danm15`, `710kor`, `unk_m44ac`). `202tel` and `tat_m17ac` are of this group too. A rule of at least 100,000 open texels and a stored sunlit level of 0.5 or more would drop all of them but `stunt_03a` and `unk_m44ac`. The game's `indoors` fact is false for nearly every module and does not help.
- **The even light fills dark rooms flat and grey** (about 19 modules). Rooms whose lamps cannot reach their level take the even light, and their black corners and coloured gloom turn an even grey: `manm28aa`, `manm28ab`, `manm28ad`, `sta_m45ab`, `604dan`, `kas_m23aa`, `kas_m25aa`, `korr_m38ab`, `262tel`, `303nar`, `stunt_19`, `stunt_42`, `tar_m02af`, `tar_m03af`, `tar_m05aa`, `tar_m10aa`. The red rooms of `853nih`, `904mal` and `001ebo` lose their red. Two fixes are open and neither is tried: no even light anywhere, as the owner chose for the enclave, or an even light in the room's stored mean colour.
- **Too white.** `tar_m08aa` is washed white (101 against 59). The lamp strips of `tat_m17ac` and `tat_m17af` glow much hotter than today.
- **Rooms without a bake show black.** `stunt_18` plays in an area of which only two rooms have a walkable face. Without a sun the bake leaves the other rooms out, and under `KJK_FITTED_LIGHTS=baked` they are black.
- **Ground without a lightmap is darker under the bake.** `tat_m18ac` has three lightmaps. Its sand has none, and it shows at 80 against 126 today.
- **A little darker.** `manm26ad` and `305nar` are at 0.7 times today. Nobody has looked for the cause.

## The batch's second round, without the even light

- The owner saw the outliers on 20 November 2026 and chose "No even light" for a room whose lamps cannot reach today's level.
- The second round is `target/light-kinds/batch/all2.sh`. It bakes `bake/b2` in each module's folder with `--no-even` and a stricter sun rule, and shoots the bake as `shots/b2`. It uses the first round's dump, sun fit and pictures of today. A module that would bake the same has no `bake/b2`, and its line in `summary2.tsv` says "same as b1": 58 modules. It ran from 07:15 to 09:20.
- **The sun rule now** needs 100,000 texels under the open sky, and five modules are marked indoors by hand in `auto.py`: `stunt_03a`, `unk_m44ac`, `710kor`, `danm15` and `202tel`. No number tells a dusk area outdoors from a cave. 42 modules take a sun, and `auto-suns2.txt` holds their numbers.
- **The result.** 113 modules are within 0.7 to 1.4 times today's brightness, 12 are brighter and 38 are darker. `sorted2.txt` lists them, and `out2-1.png` to `out2-6.png` show the outliers, the brightest first. `unk_m44ac` failed in its second bake, and nobody has looked why.
- **Without the even light the modules with few lamps are nearly black.** Malachor (`902mal` to `906mal`), the Korriban tombs (`korr_m37aa` to `korr_m39aa`, `702kor`, `711kor`), the Peragus fuel depot `107per`, `unk_m42aa`, the Dantooine cave and ruins, and the Kashyyyk landing `kas_m22aa` are at a fifth to a half of today. Their rooms have no lamp the level art shows, so nothing lights them.
- **The brightest are bright by their lamps.** The Ebon Hawk `ebo_m12aa` is at 70 against 19 without a sun and without the even light, `sta_m45ab` at 49 against 24, `tar_m08aa` at 101 against 59. Their lamps are scaled up to the room's `lit` level, which is the level of the room's bright parts. Nobody has tried `of = "original"` or a ceiling for them.
- So neither round is right for the dark modules: the even light makes them flat and grey, and no even light makes them black. The owner has not decided what such a room takes.

## The batch's third round, after the owner's notes of 20 November 2026

The owner looked at the second round's outliers and gave notes on each.
This round acts on them.

Three new flags of `detail-bake --gpu` carry it.

- `--fill-stored`. A room whose lamps cannot reach its level takes its own
  stored light for the rest, scaled, and not an even light. The room keeps
  its colour and the places its light came from. A room whose lamps reach
  its level takes none of it.
- `--most <x>`. The most a room's lamps are scaled by. It is 6 when left
  out.
- `--least <x>`. The least a room's lamps are scaled by. It is 0.5 when
  left out.

The Ebon Hawk was bright because of the most scale. Its hold's level counts
a few bright texels that no lamp reaches, so its lamps went to 6 times
their strength and lit every wall.

The table of the round is `target/light-kinds/batch/b3.txt`, with
`b4.txt` and `b5.txt` for the bright modules. A line holds a module, the
share of its rooms' lit level and the flags. `all3.sh <table> <label>`
bakes and shoots it, and `sheet3.py <table> <label> <prefix>` makes the
sheets.

| Modules | Setting | Pictures, today and the round |
| --- | --- | --- |
| `ebo_m12aa` | share 0.3, `--no-even --least 0.1 --most 0.4` | 19 and 32 |
| `stunt_07` | share 0.35, `--no-even --least 0.1 --most 0.7` | 20 and 23 |
| `sta_m45ab` | share 0.7, `--no-even --least 0.3 --most 2.0` | 25 and 38 |
| `tar_m05aa` | share 0.7, `--no-even` | 21 and 28 |
| `tat_m17ac` | share 0.75, `--no-even --most 2.0` | 37 and 62 |
| The Korriban tombs of both games | share 0.85 or 0.9, `--fill-stored` | within 0.75 to 0.95 of today |
| `kas_m22aa` | share 0.75, `--fill-stored` | 29 and 22 |
| The other 26 dark modules | share 1.0, `--fill-stored` | most within 0.85 to 1.1 of today |

`summary-b3.tsv`, `summary-b4.tsv` and `summary-b5.tsv` in the batch's
folder hold every module's numbers.

Open after this round.

- `tat_m17ac` did not change. The owner called it "a bit flat". Its
  brightness comes from its large striped lamps, and neither the share nor
  the most scale moved it.
- `305nar` (25 against 37), `manm26ad` (42 against 61) and `903mal` (20
  against 27) stay darker than today with the fill.
- The swoop bike in the racing areas (`371nar`, `211tel`) is very dark
  under the baked mode. The owner said on 20 November 2026 that it "should
  be illuminated by some lights along the track or something". The bake
  does not light the bike. A model takes the room's light from the light
  probes, which stand over walkable floor, and a race track has none. The
  baked mode then gave the bike half of the area's ambient and lamps. Now
  a model that no probe is near keeps all of it, as it does today
  (`odyssey.wgsl`). The bike is as bright as today on both tracks
  (`target/light-kinds/batch/swoop.png`). It is not brighter than today,
  and the track's baked light does not reach it. That would need probes
  over a track's floor. The owner has not seen the result.

The owner said "Yes, write them" to the settings on 20 November 2026. They
are in `data/lights/bake.toml` as `fill`, `most` and `least`, and `bake.sh`
passes them on. `bake.sh` gave `902mal` and `ebo_m12aa` the same lightmaps
as the batch, to the last pixel. `tat_m17ac` has no entry.

## The fill comes from the player's files

The owner asked on 21 November 2026: "do we need to hold bioware lightmap
pixels? or could we load them from the game files instead in a smart way".
He said "yes" to building it.

- `detail-bake --fill-stored` now writes a filled room's lightmaps with
  its lamps alone. It writes `fill.txt` into set `b`, with one line for
  each lightmap of such a room: its name and a number.
- The game reads `fill.txt` from the `KJK_DETAIL_BAKE` folder. When it
  loads a baked lightmap that the file names, it adds the stored
  lightmap's linear light times the number (`add_stored_fill` in
  `crates/game/src/viewer.rs`). It undoes the bake's easing first and
  eases the sum again, so the result is the mixed bake's apart from
  rounding.
- `902mal` and `107per` were baked this way as `bake/t2` and shot. Against
  the mixed bake `bake/b3` the pictures differ by 0.6 to 1.9 of 255 on
  average. The rooms' surfaces are the same. Thin lines differ at the
  seams between a lightmap's patches, where the mixed bake had black
  beyond its filled rings and the new one has the stored light.
- The batch's bakes `b3` of the other 33 filled modules are still the
  mixed kind. `bake.sh` makes the new kind.
- The six approved areas baked the same as before with this build, to the
  last pixel (`g15`, `h8` and `a12` against `g12`, `h5` and `a8`).
- A bake sometimes dies without a message. Anchorhead's second bake ended
  with exit code 127 four times on 21 November 2026 and passed on the
  fifth, with the same command. `101per` failed once the same way. The
  first batch had this too, on `202tel` and `209tel`. The cause is not
  found. A bake that fails so is worth one more try.

## Walking every module in one run

The owner asked on 21 November 2026 for "a stepper to the walker so we can
step through the modules without closing it down", to "just walk all
modules and f12 everywhere".

- In walk mode Page Down goes to the next module of a list and Page Up to
  the last one (`walk::step` in `crates/game/src/walk.rs`). The list
  wraps around. `KJK_WALK_MODULES` names the list's file, with one module
  a line.
- The window's title names the module and its place in the list. An F12
  report names the module in its `breadcrumbs.txt`.
- A module the run goes to has no scripts and no fights, as the first one
  has, and the camera starts behind the player at its entry.
- `KJK_DETAIL_BAKES` names a file with one folder of baked lightmaps a
  line. The game then shows every listed module's bake. `KJK_DETAIL_BAKE`
  still names one folder.
- `python tools/light-kinds/walk-lists.py <worktree root>` writes the two
  files into `target/light-kinds/walk`: `bakes.txt`, `modules-k1.txt` and
  `modules-tsl.txt`. `chosen.txt` there says which bake each module shows.
  A module's approved bake comes first, then the newest round.
- One run holds one game. A KOTOR 1 run that is asked for a KOTOR 2 module
  says so in the log and stays where it is. So there is one walk for each
  game.
- Tried without a window on 21 November 2026: the Endar Spire, then
  `end_m01ab`, then `korr_m38ab` in one run, by the console's `module`
  command, which takes the same way as the keys. Each showed its bake with
  the camera at its entry. The keys and the window's title have not been
  tried, because a run without a window has neither.
