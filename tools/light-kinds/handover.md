# Hand-over: fitted lights, from approved pictures to the game

Written 17 November 2026 for a session on another machine. The first part of this work is finished: the owner has approved, as pictures, what every common kind of light fitting in KOTOR 1 and 2 should do. This note says what was approved, how to set the work up on a new machine, and what the next phase is. The next phase makes the game place these lights itself.

The earlier hand-over, with the owner's first KOTOR 2 marks word for word, is in this file's git history (commit `3f9b51e6`).

## Where the work is

- Branch `feat/detail-bake`, draft PR 499 on `kjk-engine/kjk`. The branch is an experiment. It is not to be merged as it is. Its head when this was written is the commit after `e166282f`.
- Use a worktree of that branch. The scripts are tracked in `tools/light-kinds/`. They run from `target/light-kinds/`, which git ignores and where the survey and the pictures are written.
- The review page is https://claude.ai/artifact/6nhmGKmtSVGMQ6b1fisAAV. It is private to the owner and is at version 10.
- Both games' installs must be on the machine. `kjk light-kinds`, `kjk room-lights` and `kjk play --stage` read them.

## What is finished

1. **The survey.** `kjk light-kinds all` finds every lamp and every glowing surface in both games and groups them into kinds. A kind is mostly one glowing texture. KOTOR 1 has 181 kinds over 8,902 lights. KOTOR 2 has 180 kinds over 12,425 lights.
2. **The proposals.** `propose.py` says, for each kind, what light it should shed: its form, colour, reach and strength.
3. **The review.** Each of the 70 commonest kinds of each game was shown to the owner as two pictures: the game today, and the proposal shining with the room's light turned down to half. He marked what looked wrong over several rounds.
4. **His verdict.** KOTOR 1: "i like all the lights pretty much now". KOTOR 2, after two rounds: "ok looks good".

**The proposals are still only pictures.** The game does not place these lights. A trial switch places them for the screenshots.

## What the owner decided

These rules hold for every kind in both games. Feedback on one area is a rule for all areas.

- Every glowing fitting sheds at least a little light.
- A floor, a wall, a door, trim or a track with a glowing texture is not a lamp. It lights nothing. Such a room is lit some other way. The answer he approved is the area's own hidden lamps.
- The room's general light level stays about as it is today. Hot spots read as wrong at once.
- A light shines out of its fitting, from where the fitting is. It must not wash out the fitting's texture, and the panel around it must not go black.
- A mesh of several separate lamps has one light on each lamp, never one between them. A rectangular lamp lights as a rectangle of its own width. A ceiling panel of tiles has one light on each tile.
- Screens, signs, crystals, fires and kolto tanks shed a faint light. A display lights the wall around it. On a bed, only the small screen sheds light.
- A lamp's light should be readable as a pool near the lamp. A light that fills a whole wall evenly looks baked to him.
- Doors and objects must darken with the room. In the first pictures they kept all of their light and seemed to glow.
- Swoop tracks must not be dark.

How he reviews: ready proposals with before and after pictures and one "wrong" button. No questionnaires. He copies his marks from the page and pastes them into the chat. The page then shows only the kinds he marked, each with what he said and what changed. If a picture's angle does not show the fitting, retake it before asking him.

## The open decision

The owner asked whether these lights replace the lightmaps. He has not decided. Ask him before step 3 below. The three ways:

- **Lightmaps at full, lights on top.** Rooms become brighter than today.
- **Lightmaps at half, lights make up the difference.** Every approved picture shows this. It is the recommendation.
- **No lightmaps.** This needs bounced light and shadows from the room's walls first. An earlier trial found that fitted lights alone make a room look flat. Another session works on bounced light in its own worktree ("Bounced light pass by ray"). Do not touch that worktree.

## The next phase

The goal is that the game places the approved lights itself, in every area, behind the Enhanced lighting option. Do the steps in this order.

### Step 1: fix the survey

The approved pictures rely on hand-made corrections in `overrides-*.json`, and those exist only for the pictured areas. The survey must get these right by itself.

- **Lamps that share a frame.** The survey splits a glowing mesh into pieces by welding corners (`room_lights.rs`, `Glow.parts`). Many lamp meshes use one texture for the glowing part and its dark frame, and the whole mesh is self-lit. Tubes or squares that share a frame stay one piece: `ond_lt01`, `per_lt02`, `dxn_lt03`, `nar_lt01`, `nar_lt02`. The fix is to keep only the triangles whose texels are bright before welding (the threshold of `bright_share`). Where one quad repeats a texture with several bright spots, as on the `dxn_lt03` band, find the bright spots in the texture and map each repeat to a place in the world.
- **Lamps cut in two.** A tube lamp with a clamp at its middle becomes two pieces (`dan_light03`). Join pieces that lie end to end along their long axis.
- **The place of a lamp.** A piece's centre is the centre of the whole mesh, not of its bright part (`plc_marker1`). Take the centre of the bright triangles.
- **The facing of a lamp.** Some floor tubes have a facing tilted upward into the wall's lip (`ond_lt01`). Rings have no facing at all (the `dxn_lt03` pillar lamp).
- **The colour.** A kind's colour comes from the room's lamps or from the mesh's glow value. The glow value is white for strongly coloured textures. `tel_lt03` is red, `nar_jtrm` is pink and blue, `har_lt01` is amber. Take the colour from the texture's bright pixels.
- **Very large pieces.** One kind can hold small tubes, large ceiling panels and giant pieces of 20 m and more (`tel_lt03`). Only the tubes are lamps. A rule for very large real lamps (`tel_hl1`, `nar_lt04`) exists only as overrides, because as a general rule it would change KOTOR 1's approved `lsf_lit02`.

Done looks like this: with every `mates`, `colour` and `view` entry removed from the overrides, `shine.py` gives the same pictures as today for the kinds named above.

### Step 2: move the rules into the game's data

The rules live in `propose.py` and the overrides. The game should read them as rule data, in the way `data/rules/*.toml` holds every other rule number. One entry per kind of fitting: form, reach, strength, cone, and whether it is not a lamp. Keep the general rules as rules (the glow threshold of 0.1, faint names, facing down, long pieces) and the per-texture entries as exceptions with the owner's reason beside each.

### Step 3: one area in the running game

Every strength was judged in still pictures with the room at half its baked light. None was tuned while walking around. Pick one area, for example the Harbinger (`152har`) or the Endar Spire, put the fitted lights behind the Enhanced lighting option, and let the owner walk through it. Ask him the open decision above first.

### Step 4: the limits the stills hide

- The game uses only the 32 lights nearest the camera (`MAX_LIGHTS` in `material.rs`, chosen by `pick_lights` in `scene.rs`). The Dantooine enclave has hundreds of fittings and 179 hidden lamps. Far parts of a large room would fall back to the darkened baked light.
- These lights cast no shadows and do not bounce.
- The hidden lamps that light a room without fittings are paler than the baked light near coloured fittings. The Harbinger floor `har_wl09` loses its amber colour.

### Before any of it leaves the trial switch

Nobody has rendered a before-and-after pair of the normal game to prove that the shader changes leave it untouched. Take the same picture with and without the branch's shader, with no trial switch set, and compare them.

## How to set up on a new machine

All commands run from the worktree's root.

```bash
mkdir -p target/light-kinds target/detail-bake/shots target/detail-bake/saves && cp -r tools/light-kinds/. target/light-kinds/
```

Make the settings file `target/detail-bake/shots/enhanced.json`. It is a copy of the game's own settings file with `display.lighting` set to `enhanced`, `display.msaa` to `taa` and `display.shadows` to `high`. The scripts point `KJK_SETTINGS` at it, so the owner's real settings are never written.

```bash
cargo build --release -p launcher
```

```bash
target/release/kjk light-kinds all --out target/light-kinds
```

```bash
KJK_SAVES=$PWD/target/detail-bake/saves python3 target/light-kinds/shoot.py
```

```bash
KJK_SAVES=$PWD/target/detail-bake/saves python3 target/light-kinds/shine.py
```

Things that went wrong on the second machine:

- **Always set `KJK_SAVES`**, also in any script of your own and in every subagent's brief. A subagent's own script left it out and overwrote the owner's last-played note.
- The scripts pick `kjk.exe` on Windows and `kjk` elsewhere.
- `build_page.py` and `sheet.py` need Pillow. Where the system Python has none, run them with `uv run --with pillow python`.
- The branch is older than the build-folder seeding, so the first build is a full one (about 6 minutes on the Mac).
- Taking every picture of both games takes about an hour. The pictures are not in git. The published page holds them, and a publish keeps every file it is not sent.
- `room-lamps.json` is tracked. Make it again with `kjk room-lights all --out <folder>` and `python3 target/light-kinds/fill.py <folder>/room-lights.json`.
- Copy changed scripts back into `tools/light-kinds/` before committing. The repository's commit check refuses personal folder paths in any file.

To publish the page: build it (`build_page.py`), copy `target/light-kinds/page` into the session's scratch folder (the Artifact tool refuses a root outside the session's folders), call the Artifact `list` action with scope `files` on the page first (a publish that replaces unseen files is refused), then publish `index.html` to the URL above with `root` set to the copy and `files` naming `kinds.json` and the changed pictures. One publish takes at most 255 files. The db capability is carried forward.

## The files in `target/light-kinds/`

| File | What it does |
|---|---|
| `light-kinds.json`, `light-kinds.txt` | The survey. Each kind has counts, surface facts, lamp facts, and `examples`: places to look at it, each with a camera `view` and `mates` (every fitting of the kind in that area as 11 numbers: place xyz, facing xyz, width, long axis xyz, half length). |
| `propose.py` | The rules. `by_rule(kind)` is the rule alone. `propose(kind)` adds the overrides. |
| `overrides-*.json` | Changes to single kinds, by slug or texture name: proposal fields (`key`, `text`, `colour`, `reach`, `strength`, `cone`, `gather`, `line`, `off`), `mates` (the fittings' places), `extra` (raw lights), `dim` (false keeps the room's full baked light), `example` and `view` (the place and the camera of the pictures). Files load in alphabetical order and a later file wins. |
| `fill.py`, `room-lamps.json` | A room whose kind lights nothing is lit by the area's hidden lamps. Each lamp shines at its own place with its own colour and radius. One strength holds for the area, worked out from the area's mean baked light. A lamp of one strong colour counts at 0.35. |
| `shoot.py [most]` | Today's picture of each game's commonest kinds (70), one game launch per module. Writes `shots.json`. |
| `shine.py [--retake] [slug ...]` | The darkened picture of each kind with its proposed lights. With `--retake` it takes both pictures of the named kinds again. Two launches at a time. |
| `round3.py`, `round3.txt`, `retry.txt`, `bad.txt` | Leftovers of the KOTOR 1 rounds: kinds retaken from another place, and kinds that never got a usable picture. |
| `round.py`, `marks.txt` | What the owner said of each kind of the last round and what changed. `build_page.py` reads it. The page shows only these kinds until asked for the rest. |
| `build_page.py` | Writes `page/kinds.json` and `page/shots/*.jpg`. |
| `sheet.py` | Contact sheets of `shots/*.png`, for looking at pictures before publishing. |
| `page/index.html` | The review page. Marks are kept in localStorage `light-kind-checks-6` and db collection `checks6`. Give both a new number when a round should start clean. |

Slug of a kind: the game, a dash, then group and name lowercased with runs of other characters turned to `_`. For example `tsl-glowing_surface_of_a_room_nar_lt01`.

## The engine pieces

- **Survey** (`crates/launcher/src/room_lights.rs`, `light_kinds.rs`): lamps and glowing meshes per area, grouped into kinds. It places a camera that looks straight at the light.
- **Trial switches** (not for shipping as they are):
  - `KJK_SHADOW_DEBUG=7` halves the light of everything that is not a trial light: the baked light of rooms, and the light of doors, unbaked rooms, objects and characters (`crates/game/src/odyssey.wgsl`, `dimmed` and `dull_glow`). A surface without a lightmap keeps its own glow only where its texel is bright, so lamp tubes and screens still glow.
  - `KJK_FOUND_LIGHTS=<json file>` is a list of `{at, colour, radius, rooms, way, half, aim, cone}`. `rooms: true` makes the light reach baked room surfaces and fixed objects. `way` and `half` make it a strip along that axis. `aim` and `cone` make it shine out of its fitting (0.2 a half-space, 0.7 a spotlight). What is beside or behind keeps a third.
- A picture is taken with `kjk play --stage <module> --headless --size 960x540 --cmds "wait world; wait 200f; anim 5; wait 20f; camera x y z yaw pitch 60; wait 14f; screenshot <name>; quit"`. Screenshots are written to `kjk-artifacts/runs/<id>/<name>.png` under the folder the game was started in.

## Known weak spots

- The rarer kinds have no picture and follow the rules unseen. The pictured kinds cover 93.8% of KOTOR 1's lights and 93.3% of KOTOR 2's.
- The KOTOR 1 pictures on the page were taken on the Windows machine before doors and objects darkened. They are the pairs the owner approved.
- A module that opens on a cutscene gives letterboxed pictures with a subtitle. `305nar` gives black pictures. A swoop race ignores the camera command, so swoop kinds can only be pictured at the start line. The Taris swoop pictures show the tutorial box.
- Onderon's pictures differ from launch to launch by up to 15 of 255 in parts of the floor. The cause is not found.
- Pictures that still do not show their kind well: `har_tr02`, `har_tr03`, `dan_window01`, `dan_grate02`.
- The Dxun pillar under `dxn_lt03` stays black under any light, because its texture is almost black.
- The swoop tracks have almost no lamps. Their rooms keep all of their baked light (`dim: false`). In the engine that means a room that no fitting reaches is not turned down.

## House rules for whoever continues

- Do not commit or push unless the owner asks. Branches are never named `claude/`. No attribution lines in commits or pull requests.
- Do not run the tests or clippy locally. They run in the pull request. Ask the owner before any benchmark.
- Every game launch is headless and muted, with `KJK_SETTINGS` and `KJK_SAVES` pointing at scratch copies.
- Subagents run on Opus or a smaller model.
- Other sessions may use the machine and its GPU. Kill only your own processes, by PID.
- Everything the owner reads follows the writing rules in `CLAUDE.md`: plain full sentences, one fact each.
- Before publishing, look at every new picture yourself.
