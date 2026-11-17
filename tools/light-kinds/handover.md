# Hand-over: the kinds of light in KOTOR 1 and 2

Written 16 November 2026 for a session on another machine. It covers what the work is, what the owner has decided, how to rerun everything, and what he asked for last (not yet acted on).

## What this is

Rooms in kjk are lit by baked lightmaps; characters are lit by lamps in the models. The lighting plan wants light fittings (glowing textures in rooms and on objects) to give real light. This work:

1. surveys every light in both games and groups them into **kinds** (a kind is mostly one glowing texture);
2. proposes, per kind, what light it should give;
3. shows the owner each proposal as two pictures (the game today, and the proposal shining in a darkened room) on an Artifact page, where he marks what looks wrong.

The page: https://claude.ai/artifact/6nhmGKmtSVGMQ6b1fisAAV (private; the owner means to share it with a graphics friend). It is version 6 and shows only the three KOTOR 1 kinds of his last KOTOR 1 round; a button reveals the rest.

**The proposals are only pictures.** The game does not place these lights. A trial switch does it for the screenshots.

## Where it lives, and what another machine does not have

- Worktree `.claude/worktrees/lighting-step6` under the repository on the Windows machine, branch `feat/detail-bake`, draft PR 499. The branch is an experiment and is not to be merged as it is.
- The code is committed on the branch. Changed: `crates/game/src/{material.rs, odyssey.wgsl, scene.rs, shadows.rs}`, `crates/launcher/src/{lib.rs, room_lights.rs, room_light_fit.rs, verbs.rs}`; new: `crates/launcher/src/light_kinds.rs`. Parts of `room_light_fit.rs`, `verbs.rs` and `scene.rs` are an earlier trial of the parent session (lights found from lightmaps) and were there before this work.
- **The scripts are tracked in `tools/light-kinds/`, beside this file, but they run from `target/light-kinds/`**, which git ignores and where the survey writes. On a new machine, copy them there first (see "How to rerun"). They find the repository two folders up from where they sit, so they work from either place.
- **The pictures are not in git** (about 15 MB of thumbnails and textures). The published page still holds them, and a publish keeps every file it is not sent. But `build_page.py` rebuilds `page/shots` from `kept/`, so on a new machine take all pictures again (`shoot.py`, then `shine.py`, roughly an hour for both games) before the first `build_page.py`. KOTOR 2 needs retaking anyway.
- `shots.json` (the place chosen for each picture) is tracked, but `shoot.py` rewrites it from the new survey.
- The game data (both games' installs) must be present on the other machine; `kjk light-kinds` and `kjk play --stage` read it.

### Files in `target/light-kinds/` (scripts tracked in `tools/light-kinds/`)

| File | What it does |
|---|---|
| `light-kinds.json`, `light-kinds.txt` | Output of `kjk light-kinds all --out target/light-kinds` (45 s). Each kind has counts, surface facts, lamp facts, and `examples`: places to look at it, each with a camera `view` and `mates`. |
| `propose.py` | **The rules.** `propose(kind)` gives key, sentence, colour, reach, strength, cone, `gather`, `line`. Holds the owner's per-texture overrides. This is the file that records what he approved. |
| `shoot.py [most]` | Today's picture of each game's commonest kinds (70), one game launch per module. Writes `shots.json` (the place chosen per kind). A slug in `retry.txt` uses the kind's next example. |
| `shine.py [slug ...]` | The darkened picture: one launch per kind with `KJK_SHADOW_DEBUG=7` and a `KJK_FOUND_LIGHTS` file holding a light at every mate. Two launches at a time. Skips a kind whose `shots/<slug>.shine.png` exists. |
| `round3.py` | Retakes today's picture of the KOTOR 1 kinds named in `round3.txt` from the newest survey and rewrites their `shots.json` entries. |
| `build_page.py` | Writes `page/kinds.json` and `page/shots/*.jpg` from `kept/` (640-wide thumbnails; raw PNGs are deleted after each run to save disk). `FIXES` holds what the owner said and what changed for the kinds of the current round; the page shows only kinds with a `fix`. A slug in `bad.txt` gets no picture. |
| `sheet.py` | Contact sheets of `shots/*.png`, for checking pictures before publishing. |
| `page/index.html` | The checklist page. Marks are kept in localStorage `light-kind-checks-4` and db collection `checks4`; bump both when a round should start clean. |

Slug of a kind: `<game>-` + group and name lowercased with runs of other characters turned to `_`, for example `k1-glowing_surface_of_a_room_lsi_lite02`.

## How to rerun

All from the worktree root. First, on a machine that has no `target/light-kinds/`:

```bash
mkdir -p target/light-kinds && cp -r tools/light-kinds/. target/light-kinds/
```

Copy changed scripts back into `tools/light-kinds/` before committing. The settings file `target/detail-bake/shots/enhanced.json` is not in git either: it is a copy of the game's settings with `display.lighting` set to `enhanced`, `display.msaa` to `taa` and `display.shadows` to `high`; make one and point `SETTINGS` in the scripts at it.

```bash
cargo build --release -p launcher
```

```bash
target/release/kjk.exe light-kinds all --out target/light-kinds
```

```bash
python target/light-kinds/shoot.py
```

```bash
python target/light-kinds/shine.py
```

```bash
python target/light-kinds/build_page.py
```

Then publish `target/light-kinds/page/index.html` with the Artifact tool to the URL above, `root` = the page folder, `files` = `kinds.json` plus the changed pictures (files left out are kept; one publish takes at most 255 files). The db capability is carried forward.

A picture is taken with `kjk play --stage <module> --headless --size 960x540 --cmds "wait world; wait 200f; anim 5; wait 20f; camera x y z yaw pitch 60; wait 14f; screenshot <name>; quit"`, with `KJK_SETTINGS=target/detail-bake/shots/enhanced.json` (Enhanced lighting, TAA, high shadows) and `KJK_MUTE=1`. Screenshots land in `<cwd>/kjk-artifacts/runs/<id>/<name>.png`; the scripts move them. It works for KOTOR 2 modules too.

## The engine pieces

- **Survey** (`room_lights.rs`): per area, lamps and glowing meshes. A wall grid (`Level`, 2 m cells, bounded to the lights' box plus 10 m; unbounded it took 26 GB) places a camera that looks straight at the light, so the light is at the picture's centre and the page draws a ring there. A glowing mesh is split into its separate pieces (`Glow.parts`, `fn parts`, union-find over corners welded to a millimetre): each piece is one lamp with its own centre, facing, width, long axis and half length.
- **Kinds** (`light_kinds.rs`): a glowing texture is a kind; a lamp at a glowing surface joins that kind; other lamps group by model or by a room lamp's flags; plus the sun. `Example.mates` lists every lamp of the kind in the example's area as 11 numbers: place xyz, facing xyz, width, long axis xyz, half length (up to 600).
- **Trial switches in the game** (not for shipping as they are):
  - `KJK_SHADOW_DEBUG=7`: rooms keep half of their baked light (`odyssey.wgsl`, `dimmed`).
  - `KJK_FOUND_LIGHTS=<json file>`: a list of `{at, colour, radius, rooms, way, half, aim, cone}`. `rooms: true` marks the light as a fight's light (`LIGHT_COMBAT`), the one path by which a light reaches baked room surfaces. `way` + `half` make it a strip along that axis. `aim` + `cone` make it shine out of its fitting (0.2 a half-space, 0.7 a spotlight); what is beside or behind keeps a third. Only the 32 nearest lights are used (`MAX_LIGHTS`).
- These lights cast no shadows and do not bounce.

## The proposal rules as they stand (`propose.py`)

Keys: `keep` (hidden lamp or sun, unchanged), `glows` (lights nothing), `faint`, `panel` (lights the way it faces), `down` (spotlight down), `strip`, `point` (all ways).

- Colour: the kind's lamp colour if it has lamps, else the texture's glow, scaled so the largest channel is 1.
- Reach: lamp radius or 2.5 m + the square root of the surface's area, clamped to 2.5 to 5 m.
- Faint (2 m, strength 0.4) if a piece is over 40 m², or it has no lamp and the bake is not brighter near it, or its name hints at a screen, sign, map, panel, crystal, fire, kolto and the like.
- Facing down and not faint: spotlight, cone 0.7. Long pieces (median half length 2 m or more): strip. Flat: panel, cone 0.2. Otherwise point. Strength 1.1.
- Overrides by texture name, all from the owner: `NOT_LIGHTS` (floor and track textures), `REMOVED` (`lma_light06`), `MORE`, `WINDOWS` (`lts_lowin01`, 8 m), `GATHERED` (`plc_btlclrs`: one soft light above the middle of many tiny pieces), `GLOW` (`lza_lit01`, 1.7), `LINES` (`lsi_lite02`: the lamp shines along its whole width), `SPOTS`, and `lts_loscre01` (no cone, 4 m, 1.6, so the display lights the wall around it).

## What the owner has decided

His rules, which hold for every kind in both games (feedback on one area is a rule for all):

- Every glowing fitting gives at least a little light. "Glows only" is for floor and track textures alone.
- Keep the room's general light level about as it is. Hot spots read as wrong at once.
- A light shines out of its fitting, from where the fitting is. It must not wash out the fitting's texture, and the panel around it must not go black.
- A mesh of several separate lamps gets one light on each lamp, never one between them. A rectangular lamp lights as a rectangle of its own width, not a round spot.
- Screens, signs, crystals, fires, kolto tanks: a faint light. A display lights the wall around it.
- A floor, a wall or a door with a glowing texture is not a lamp. If the room then lacks light, it must be lit some other way.
- Swoop tracks are too dark.
- A darkened comparison room at a fifth was too dark; half is what he sees now.

How he wants to review: ready proposals with before and after pictures and one "wrong" button. No questionnaires. He copies his marks from the page and pastes them into the chat. When he says "fix these and hide the rest", the page shows only the fixed kinds, each with what he said and what changed. If a picture's angle does not show the fitting, retake it before asking him.

**KOTOR 1:** after four rounds he said "i like all the lights pretty much now".

**On replacing the lightmaps** (he asked): not decided. These lights are added to the baked light. Three ends are possible: lights on top of full lightmaps (rooms get too bright); lightmaps turned down and lights making up the difference (what the pictures show, and my recommendation); lightmaps gone, which needs bounced light and shadows from the room's walls. Another session ("Bounced light pass by ray", its own worktree, do not touch) works on bounce. Step 10 found that fitted lights alone flatten a room and no rebake was worth shipping.

## Next: his KOTOR 2 marks (given 16 November 2026, he said do not fix yet)

**Read this first.** The KOTOR 2 darkened pictures he judged were taken in round 2: rooms darkened to a fifth, and one light at the middle of each whole mesh. Since then rooms are darkened to half, each separate lamp has its own light, and the panel behind a fitting keeps a third. The KOTOR 2 proposals in `kinds.json` were recomputed, but **their pictures were not retaken**. So before changing any rule, retake all KOTOR 2 pictures (`shoot.py` for new views from the split meshes, then `shine.py`); many "too dark" and "between the lamps" marks may already be answered. Then work through the list.

The marks sort into these groups:

- **Room too dark, door or other thing too bright** (`har_wl01`, `har_wl07`, `har_rd01`, `har_wl06`, `har_wl02`, `har_fl01`, `har_tr01`, `tel_hl1`, `tel_lt03`, `dan_wall02`, `nar_cr8s`): mostly the fifth-darkening. On the Harbinger the door stays bright because it is not lightmapped, so it does not darken with the room.
- **More light** (`tel_hl1`, `tel_sstn2`, `plc_koltank`, `leh_light02`, `leh_light03`, `per_lt02`, `dxn_lt03`, `har_wl02`, `plc_marker1`, `nar_lt02`); **less far** (`leh_light01`).
- **One light per lamp, placed on the lamp** (`ond_lt01`, `ond_lt03`, `dan_light03`, `dxn_lt03`, `nar_lt02`, `nar_lt01`, `nar_lt03`, `plc_marker1`): the split into pieces should fix most; check the placement offsets on `nar_lt01`, `nar_lt03`, `plc_marker1` in the new pictures.
- **Not a lamp: a floor, wall or door** (`har_wl09`, `har_fl01`, `har_tr01`, `dan_wall08`, `dan_wall07`, `dan_grate02`, `dan_unwal07`, `plc_bed1`): add to `NOT_LIGHTS`. He wants those rooms lit some other way, for `dan_unwal07` from the lamps at the sides. That is an open design question, not a rule change.
- **Wrong source** (`dan_leaf02`): the light should come from the ceiling windows, not the leaves.
- **Swoop tracks too dark** (`nar_lt04`, `tel_st01`): same remark as KOTOR 1; needs its own answer.
- **Too red or looks wrong** (`har_tr03`, `har_wl08`, `har_wl04`, `nar_jtrm`): check the colour source (lamp colour against texture glow) and whether these are lamps at all.
- **Bad angle, retake** (`plc_koltoglass`, `leh_scre02`, `dan_trim04`, `har_tr02`, `plc_kolpuz`, `dan_window01`, `plc_holoart1a`, also `har_wl01`, `har_tr03`).

`dxn_lt03` appears twice because it is two kinds (a room's and an object's).

His marks, word for word:

```
tsl | dan_leaf02 | proposed faint | artificial and weird light source. light should come from the ceiling windows instead
tsl | har_wl01 | proposed faint | hard to see from this angle, but room looks al il too dark, and the door in the background shines way more than the room
tsl | tel_hl1 | proposed faint | room looks too dark. lamp needs more light
tsl | tel_sstn2 | proposed faint | more light
tsl | plc_koltoglass | proposed faint | cant see from this angle
tsl | leh_scre02 | proposed faint | cant see from this angle
tsl | har_wl07 | proposed faint | more light needed also room look too dark but door is bright
tsl | leh_light01 | proposed panel | shouldnt light quite as far
tsl | plc_marker1 | proposed point | your light position seem a bit off to the left from the lamp, also should be a tad brighter
tsl | nar_lt03 | proposed point | light should be placed further down to the left on the actual lamp
tsl | ond_lt01 | proposed down | this source is placed in between two thin rectangles on the sides, split into two light sources along the lamp meshes
tsl | har_tr03 | proposed point | this looks very wrong, too bright and ver red, doesnt even look like an original lamp to me. maybe its the camera nagle
tsl | plc_bed1 | proposed panel | i dont think this should light up area
tsl | nar_lt01 | proposed down | lamp should be a bit further down ontop of the actual lamp
tsl | plc_koltank | proposed faint | give it a little more light i think
tsl | tel_lt03 | proposed faint | way too dark in here
tsl | har_tr01 | proposed faint | too dark in here, but this just seems to be a floor or something
tsl | har_wl04 | proposed down | no idea what this is, but it looks wrong
tsl | leh_light02 | proposed panel | needs a slight bit more glow
tsl | dan_unwal07 | proposed faint | not sure why light source is placed on the door here but it looks wrong. room is too dark, should prob be lit up from something else, like the lamps on the sides or something
tsl | har_rd01 | proposed point | too dark, door too bright
tsl | per_lt02 | proposed panel | needs a bit more light
tsl | har_wl09 | proposed panel | this is just the floor. should be lit up somehow else
tsl | nar_lt04 | proposed faint | swoop area way too dark!
tsl | dan_trim04 | proposed point | cant really make out what this is
tsl | ond_lt03 | proposed panel | light source doesnt seem placed on the actual lights, there are like a few thin light panels here on this desk and they all need their own light sources
tsl | har_tr02 | proposed point | weird angle, doesnt look like a lamp
tsl | dan_wall07 | proposed point | they put the door as a lamp here, which doesnt make sense
tsl | plc_kolpuz | proposed faint | not sure if an actual lamp. angle doesnt really show it
tsl | har_fl01 | proposed faint | this is just a floor tile. btw room too dark
tsl | leh_light03 | proposed panel | give it a tad bit light
tsl | dan_grate02 | proposed point | this is just a door
tsl | dan_light03 | proposed point | this looks weirdly placed inbetween all the actual lamps, but the lamps should have light sources of their own, seems to be like 10 or something in the pic
tsl | har_wl06 | proposed faint | not sure what har_wl06 is but this place looks too dark, needs more light
tsl | dxn_lt03 | proposed faint | each dxn_lt03 square should be its own light
tsl | nar_lt02 | proposed strip | light source seem to be placed inbetween the two lamp squares. should have one light each and be a bit brighter, in the image its very dark
tsl | dan_window01 | proposed point | not sure if done properly, would need a better image
tsl | dxn_lt03 | proposed point | increase lighting a tad
tsl | har_wl02 | proposed faint | increase lighting
tsl | nar_cr8s | proposed point | area wayy too dark
tsl | nar_jtrm | proposed down | i dont think this is a spotlight but i cant really make it out
tsl | plc_holoart1a | proposed faint | bad angle
tsl | dan_wall08 | proposed panel | this is just a wall. we gotta illuminate somehow else
tsl | dan_wall02 | proposed faint | too dark.
tsl | har_wl08 | proposed point | too... red?
tsl | tel_st01 | proposed faint | track is too dark
```

## Known weak spots

- Numbers: KOTOR 1 has 181 kinds over 8,902 lights, 68 pictured (93.8% of lights). KOTOR 2 has 180 kinds over 12,425 lights, 64 pictured (93.3%). The rarer kinds have no picture and follow the rules unseen.
- A module that opens on a cutscene gives letterboxed pictures with a subtitle, and outdoor and cutscene rooms barely darken. The Harbinger pictures look down through ceilings drawn from one side only.
- KOTOR 1 `lhr_lit01` may be too bright now (the ceiling comes out lighter and warmer than today). `lts_lite08`, `lts_lowin01` pictures show little difference. He has not objected.
- Strengths are trial values judged in pictures at half baked light. They are not tuned in the running game.
- The kinds listed in `bad.txt` never got a usable picture and were not retaken: both suns, k1 `plc_rakatcom`, tsl `dan_trim03`, `kor_web`, `tel_blst1`, two room-lamp kinds.
- KOTOR 1 kinds hidden on the page had proposals recomputed after the mesh split, with pictures from round 2.

## House rules for whoever continues

- Do not commit or push unless the owner asks. Branches are never named `claude/`. No attribution lines in commits or PRs.
- Never run the approved tests locally; they run in the PR. Ask the owner before any benchmark.
- Other sessions share the machine and its GPU: kill only your own processes, by PID. Disk was about 20 GB free here; delete raw PNG frames once thumbnails exist.
- Commands given to the owner use full absolute paths.
- Comments and anything the owner reads follow the writing rules in CLAUDE.md on `origin/main` (plain full sentences).
- Before publishing, look at every new picture yourself (`sheet.py`).

## KOTOR 2 round after his marks (16 November 2026, on the Mac)

This round ran on the Mac in the worktree `.claude/worktrees/lighting-step6` under the repository. All pictures of both games were taken again there (140 of today, 119 darkened). The page is version 7. It shows the 46 KOTOR 2 kinds he marked and 5 kinds he did not mark that the new rules changed. Marks are kept under `light-kind-checks-5` and `checks5`.

What is new in the scripts:

- `propose.py` reads every `overrides-*.json` beside it. Each maps a kind's slug or texture name to the proposal fields that differ from the rules, to `mates` (the fittings' places), `extra` (raw lights), `dim` (false keeps the room's full baked light), and to `example` and `view` (the place and the camera of the pictures). `by_rule(kind)` is the rule alone. The five files hold what four subagents and the main session settled for the marked kinds.
- `shine.py --retake <slug> ...` takes both pictures of the named kinds again. Python needs Pillow for `build_page.py` and `sheet.py`; on the Mac run them with `uv run --with pillow python`.
- New rule: a texture whose glow is under 0.1 is a wall, a floor, a door or trim, and lights nothing (`LOW_GLOW`). It covers 37 KOTOR 2 kinds and 8 KOTOR 1 kinds. Three are excepted: `lma_lite01` and `dan_scren2` are small displays, and `lts_light01` was never seen.
- New rule: a room whose kind lights nothing is lit by the area's own hidden lamps (`fill.py`, `room_fill`). Each lamp shines at its own place with its own colour and radius. One strength holds for the whole area, worked out from the area's mean baked light. A lamp of one strong colour counts at 0.35. `room-lamps.json` holds the lamps. Make it again with `kjk room-lights all --out <folder>` and `python fill.py <folder>/room-lights.json`.
- `round.py` holds what he said of each kind and what changed. `build_page.py` reads it.

Findings that need engine or survey work, not done:

- The survey welds lamps that share a frame into one piece (`room_lights.rs`, `fn parts`): the two tubes of `ond_lt01` and `per_lt02`, the squares of `dxn_lt03`. It cuts a tube lamp in two at its clamp (`dan_light03`). A piece's centre is the centre of the whole mesh, not of its bright part (`plc_marker1`). The overrides hold corrected places for the pictured areas only. The fix is to keep only the triangles whose texels are bright before welding.
- A kind's colour comes from the room's lamps or from the texture's glow value, which is white for strongly coloured textures (`tel_lt03` is red, `nar_jtrm` is pink and blue, `har_lt01` is amber). The colour should come from the texture's bright pixels.
- Swoop tracks have almost no lamps. Their glowing textures are paint, floors and crates. The answer tried here: a room that no fitting reaches keeps all of its baked light (`dim: false`). In the engine that belongs in the `dimmed` path of `odyssey.wgsl`.
- Lighting a room from its hidden lamps works only for the 32 lights nearest the camera (`MAX_LIGHTS`).
- A general rule for very large real lamps (over 40 m², such as `tel_hl1`, `tel_lt03`, `nar_lt04`) is written down by the subagent but only set as overrides, because it would also change KOTOR 1's approved `lsf_lit02`.
- Pictures that still do not show their kind well: `har_tr02`, `dan_window01`, `dan_grate02`, `har_tr03`, and the three swoop kinds, where the race ignores the camera.
