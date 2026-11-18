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

The next phase makes the game place the lights itself, behind the Enhanced lighting option. Steps 1 and 2 below are done and not committed. Step 3 is next, and it starts with the owner's answer to the open decision above.

### Where steps 1 and 2 stand

- **The survey finds the lamps of a texture.** Each example now has `bright_mates` beside `mates`. `mates` are the welded pieces of the mesh, as before. `bright_mates` are the bright patches of the texture on the mesh, each with the colour of its bright texels (`room_lights.rs`, `parts`). A kind takes them with `lamps = "bright"` in its entry. Applying them to every kind changed about 30 approved KOTOR 1 kinds, so a kind opts in.
- **No kind has hand-made fittings any more.** `dan_light03`, `ond_lt01`, `dxn_lt03`, `tel_lt03` and `nar_lt01` take the bright patches. `plc_marker1` takes the survey's pieces as they are. The pictures of all six match the approved ones.
- **`nar_lt01` has one light on each real tile.** The hand-made list had two lights on some tiles, so the strength went from 0.7 to 0.9 to keep the same light.
- **The rules are the game's rule data.** `data/lights/fittings.toml` holds the general rules' numbers, the 43 kinds that differ with the reason for each, and the 24 lights placed by hand. `propose.py` gives all 361 kinds the same proposal as before the move. `kinds.py` writes one entry for each kind into `data/lights/k1.toml` and `data/lights/tsl.toml`.
- **Still by hand.** The colours of `tel_lt03`, `har_lt01` and `plc_bed1` are entries, because the texture's own colour is paler than what the owner approved. The red fill of the large `tel_lt03` ceiling panels and the daylight over `dan_leaf02` are placed lights in one area each. Other areas with large `tel_lt03` panels get no fill yet.
- **The owner chose lightmaps at half** (17 November 2026). The lights make up the rest.

### Where step 3 stands

- **The game places the lights itself** with `KJK_FITTED_LIGHTS=on` under Enhanced lighting (`crates/game/src/fitted_lights.rs`). It reads `data/lights/`, finds the lamps of each glowing mesh as an area loads, and puts a light on each. It also keeps half of the baked light, as `KJK_SHADOW_DEBUG=7` does.
- **The lamp finding moved into the game crate.** The survey (`room_lights.rs`) calls the same code, and its output is unchanged.
- **The rooms' hidden lamps shine in every room**, at one strength for the area (`fill.py`'s rule). Without them a room with weak fittings is far too dark.
- **The fitted lights are in a grid of their own, not in the light list** (`fitted_lights::grid`, `odyssey.wgsl`'s `fitted_lights`). The grid is built once when the area loads. It lies over the floor plan in cells of 1 m, and each cell lists the lights that reach it. A surface reads only its cell's lights. On the Harbinger that is 11.2 lights on average and 29 at most, out of 1186. Every light is always lit, and nothing depends on the camera. The light list is 32 long again, as in the normal game. The owner's walks showed why. With the lights nearest the camera chosen each frame, a light switched on only when he stood near it, and a hall seen through a door was dark. With 128 lights read by every pixel his graphics card's fans ran loud.
- **Fitted lights leave no highlights on shiny surfaces.** That went with the move to the grid.
- **A room's shadows fall on the fitted light too.** On a room's surface the fitted light is cut by the share that the characters' and placeables' shadows take from the baked light (`unshadowed` in `odyssey.wgsl`). The owner's fourth walk: "no shadow from this computer terminal". A fitted light still casts no shadow of its own.
- **Each room's hidden lamps have their own strength** (`fitted_lights::fill`). It puts back the half of that room's baked light. One strength for the area made the Harbinger's medical bay twice as bright as today (mean 134 against 69). The owner: "this area is too bright, i think we need to scale down the general light and have it come more from the light sources only". It is now 51. A placeable's fitting counts for the room it stands in. `STRONG_SHINE` is gone.
- **The survey misses lamps whose texture is red or orange.** It calls a texture bright by its brightness. The game now calls it bright by its strongest channel. `kinds.py` writes a `[[fitting]]` for a texture the survey does not list as it stands. `har_lt02` (the red wall lamps, `always = true`: most of its meshes are not set to glow) and `har_pn02` and `har_pn03` (orange screens) are such entries. The survey itself was not changed that day. It was changed later on 17 November 2026, as the next point says.
- **The survey finds a lamp by its colour too** (`Glow::by_colour`, `strong_share` in `room_lights.rs`). A glowing mesh that is no light by brightness counts when a tenth of its texture is bright by its strongest channel. Such a mesh makes a kind only where its texture is no kind yet, and it takes no part when the files' lamps are matched to fittings. So the 361 kinds from before are the same to the last number (checked field by field), and 57 kinds are new: 25 in KOTOR 1 and 32 in KOTOR 2. A kind found this way has `by_colour` set. `propose.py` makes it faint with `tint`, so each fitting has its own colour. `fittings.toml` ends with the entries that differ: four red lamps shine as `har_lt02` does (`lsi_lite04`, `lsi_lite10`, `lts_heater`, `nih_lt01`), and eight textures are no lamps (glasses, swords, rock, a floor, a blast mark). **The owner has seen none of the 57.** They have no pictures yet. `har_pn02` and `har_pn03` are still not found, and their hand-written entries stay.
- **A kind's `unglowing`** (in its `surface`) counts the meshes that carry its texture and are not set to glow. Where they are many and the kind is a lamp, it may need `always = true`, as `har_lt02` does. Most kinds with a high count are walls, so nothing acts on the number by itself.
- **`grow = true`** makes a lamp wider than 0.7 m shine up to 1.5 times stronger (`GROW_WIDTH`, `GROW_MOST`). `har_lt01` has it: the owner said the large tube panels seemed to emit no light.
- **`KJK_FITTED_DUMP=<file>`** writes every textured mesh and every fitted light of the area, a line each. It shows why a fitting gives no light.
- **A long lamp that bends is cut into lengths of 3 m** (`fitted_lights::lengths`, `parts` with `follow`). A light stands on the straight line through its lamp. The strips of the Harbinger's curved hall were lights 17 to 23 m long that left the hall and lit odd wedges of its wall. The owner took them for baked shadows (fifth walk, 17 November 2026). Each cut length shines at 0.6 of its kind's strength (`CUT_SHARE`), because lengths end to end overlap. Only the game cuts. The survey does not, so its numbers stay as approved. Pictures: `target/light-kinds/game/walk5-1.png` and `walk5-2.png`.
- When the owner reports an odd shadow, check first whether it is a fitted light's edge. Compare with a picture without the switch.
- The pictures of the fourth walk are `target/light-kinds/game/walk4-1.png` to `walk4-6.png`, today on the left.
- **A room's lights are held to the room's own level** (`fitted_lights::level`, 17 November 2026). The Endar Spire was the second area tried, and with every kind at its approved strength its halls were washed out white. The rule splits a room's baked surfaces into pieces of half a square metre. For each piece it works out the stored level of light the piece shows with the lights on (`shown`): half of the piece's own baked light, and the lights' sum read as a stored level and added as linear light. A room may show `LEVEL_OVER` (1.1) times what it shows today over all its surfaces, or `LEVEL_LIFT` (0.56) where that is more. Its brightest twentieth may show `LEVEL_HOT` (1.1), and its brightest hundredth `LEVEL_PEAK` (1.24) times that. A room that is bright today may show less on its brightest surfaces: the cap falls from 1.1 at a level of 0.38 today to `LEVEL_HOT_BRIGHT` (0.7) at 0.54 (`LEVEL_BRIGHT`). Where a room shows more, its strong lights are made weaker by one share, found by halving. No room of the Harbinger shows more, except that one room's faint lights keep 0.92. The Endar Spire's rooms keep 0.08 to 0.50 of their strong lights.
- **The faint lights have a share of their own.** A light no stronger than `LEVEL_FAINT` (0.65) is faint. The faint lights alone may bring a room's brightest surfaces to `LEVEL_FAINT_PART` (0.8) of the caps. Where they do, they stay whole. Where they do not (the Endar Spire's bridge), they are made weaker by one share, and the lamps have the rest.
- **The owner walked the Endar Spire on 17 November 2026** with a first, looser form of the rule (the caps were in the wrong unit). His nine reports said: the halls, a hall's one side and the door openings were too bright; the orange displays, the screens and the light panels gave too little light; a terminal's blue displays gave none; the leftmost of three chairs on the bridge glowed; his graphics card ran hard twice. What was done: the rule above; the door frames' trim `lhr_trim03` is faint; a faint kind whose name marks a screen (`screen_names` in `fittings.toml`) has strength 0.5 and reach 3 m (`screen`, `screen_reach`), where a faint light has 0.4 and 2 m; the terminal `plc_comppnl` (both games) and `plc_comppnl_b` have a blue light of strength 0.3 and reach 2.5 m, with the new field `dark = true` (it shines although little of its texture is bright). The Harbinger's six views were [51, 54, 41, 71, 51, 67] and are [53, 54, 45, 70, 51, 67]: its terminal now sheds blue light, and its screens are a little stronger. The Endar Spire's twelve views are [33, 42, 51, 44, 43, 21] and [48, 59, 55, 65, 33, 42], against [37, 41, 60, 50, 31, 22] and [58, 61, 27, 54, 36, 33] today (`target/light-kinds/game/spire-walk2a.png`, `spire-walk2b.png`). **The owner has seen none of this.**
- **Still open from that walk.** The chair is white in the normal game too, so it is not from the fitted lights; its cause is not found. The graphics card's load on the Endar Spire is not measured: its grid lists 15.9 lights a lit cell (62 at most), against 11.2 (29 at most) on the Harbinger. Ask the owner before any benchmark. The Harbinger's own orange screens (`har_pn02`, `har_pn03`) keep their approved 0.4 and 2 m. A door in a hall looks lighter than today, about as its walls.
- **Walk mode: `kjk play --stage <MODULE> --walk`** (18 November 2026, `crates/game/src/walk.rs`). The owner asked for walks without scripts and without collision ("so i dont have to play through everything"). The module loads with none of its scripts, every creature is neutral, the player stands at the arrival, and the camera flies through walls (WASD, E and Q, Shift, the mouse). The rooms draw as in `play`: the same picture to the pixel on the Endar Spire. Every F12 report now holds `camera.txt`, a `camera` line for `--cmds`. The keys, the mouse and F12 in a real window are not tested: headless runs have neither. Ask for every walk with this flag.
- The Endar Spire lists 17 lights in a usual cell and 70 at most, against 11 and 29 on the Harbinger. Its cost on the graphics card is not measured.
- **The cost on the graphics card is measured** (17 November 2026, RTX 4070 Ti, 1920x1080, the owner's settings). The owner asked for the benchmark and for half the cost. `kjk bench` has two scenes for it, `harbinger_hall` and `harbinger_medbay`. Run `kjk bench --preset ao-on --scenes harbinger_hall,harbinger_medbay --gpu --seconds 8` with `KJK_FITTED_LIGHTS` off and on in turn, and compare the opaque pass. A headless bench is paced at 60 frames a second, and its pass times are not to be trusted. Ask the owner before any further benchmark.
  - The opaque pass in the hall takes 1.23 ms without the switch. The fitted lights added 0.46 ms at first and add 0.21 to 0.25 ms now (the runs differ by 0.02 ms). In the medical bay they added 0.14 ms and add 0.11 ms now. The pictures are the same to the last digit of `walk.sh`'s means.
  - What cut the cost, in the hall: a light's numbers lie in the cell's list itself, packed into two `vec4` (0.07 ms); the light's sums are written out in `fitted_lights` with one square root (0.12 ms); a strip is listed only in the cells around its line (0.02 ms).
  - What did not help: cells of 0.5 m (the lights are large, and nearly all of a cell's lights reach every surface in it).
  - What is left in the hall: about 0.03 ms to find the cell, 0.07 ms to read each light's place and turn away the far ones, and 0.15 ms for the lights that reach the surface. `har_lt01`'s strips are 688 of the 1186 lights and 70% of the lists. Fewer or shorter-reaching strips is the next saving, and it changes the picture.
  - Three traps in WGSL: `from` is a reserved word; a `var` declared in a loop without a value keeps the last round's value; a test that leaves out part of a loop also lets the compiler drop what fed it, so its time is a lower bound.
- **The owner's second walk (17 November 2026): the glowing panels must clearly be what lights the room.** The floor looked lit by itself. So `har_lt01` is now a panel that shines along its whole length at strength 0.5 (it was a spotlight down at 0.08). In a room with fittings the hidden lamps keep 0.3 of their strength (`FILL_BESIDE`). He also asked for more glow from the red lamps. Those are hidden lamps of one strong colour at fittings the wall texture shows, and they now shine 2.5 times stronger than fill (`STRONG_SHINE`). Both numbers are first guesses from pictures in `target/light-kinds/game/t3/`.
- **First pictures of the Harbinger (`152har`) are in `target/light-kinds/game/`**. With the switch on, the corridors are about 30% darker than today and warmer, because the hidden lamps are warm white. The owner has not walked through it yet.
- **Not done.** The scene does not know which game it is from, so the code takes the game whose data knows more of the scene's glowing meshes. `shine.py`'s and `fill.py`'s numbers are constants in `fitted_lights.rs`, not rule data. No shadows from the fitted lights. The picture pair of the normal game without the switch is still owed before anything leaves the switch.

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

The pair was taken on 17 November 2026. The game built from the commit before this branch (`a49d963f`) and this branch's game took the same pictures with no trial switch, under Classic and under Enhanced, in five areas. Each was taken twice with this branch's game too, because two runs of one game differ a little (smoke, sparks, people's stance).

- The Harbinger (`152har`), the Dantooine courtyard (`danm14aa`) and Ahto City (`manm26aa`) differ between the two games by no more than two runs of one game do. One Ahto City picture under Classic is the same to the last pixel.
- The Endar Spire (`end_m01aa`) differs only by its intro's screen fade. Taken after the fade (`wait 1800f`), one picture is the same to 0.001 of 255 and the other differs by its smoke.
- Peragus (`101per`) is not proven. Its intro fades the screen in and out, and the two games are caught at different moments of it: one game is evenly 4 to 10% darker over the whole frame, its glowing lamps included. No picture of it without the fade was found.

The pictures are in `target/light-kinds/game/proof/`. To take them again, build the older commit in a scratch checkout with `CARGO_INCREMENTAL=0` and `CARGO_TARGET_DIR` set to this worktree's `target` (with the shared incremental folder the compiler crashed), keep that game as a copy, and build this branch again.

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
| `propose.py` | Applies the rule data (`data/lights/fittings.toml`) to the survey. `by_rule(kind)` is the general rules alone. `propose(kind)` adds the kind's own entry. |
| `pictures.json` | Where a kind's pictures are taken when the survey's first example does not show it: `example`, `view` and `dim`, by slug. |
| `kinds.py` | Writes `data/lights/k1.toml` and `data/lights/tsl.toml`, one entry for each kind, for the game to read. |
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

## The bake trial on the Endar Spire (18 November 2026)

The owner wants a bake made from the lamps alone, with no pixel of BioWare's lightmaps. He calls it bake B. Only that bake can ship.

- `kjk detail-bake end_m01aa --game k1 --lights <dump> --gpu --levels <file> --out <folder>` makes it in about 12 seconds on the GPU. `KJK_FITTED_DUMP=<file>` writes the dump of the levelled lights. The `--levels` file holds each room's mean level under the live lights.
- `KJK_FITTED_LIGHTS=baked` with `KJK_DETAIL_BAKE=<folder>/m01aa/b` shows it. The rooms then take no live fitted light. Doors, placeables and characters keep the live lights.
- The opaque pass costs 0.03 ms more with the bake than with no fitted lights. The live lights cost 0.27 ms more (windowed 1080p, 4070 Ti SUPER).
- The owner walked bake B and said: "I think it looks pretty good actually! some minor notes on shadows and door post lamps being a bit too bright".

What his three reports led to:

- No room's own light list blocks anything in the Spire's halls, so neither the shipped game nor the trial drew a character's shadow there. Under the trial the characters and placeables now cast shadows from the fitted lights (`fitted_visibility` in `odyssey.wgsl`). The four lights that give a fragment the most are traced. Under the bake a shadow takes the blocked lights' share and leaves `FITTED_BOUNCE` of the light.
- A placeable's shadow is its box's shadow. The arch-shaped console casts a solid block.
- The door ring and the door-post lamp looked too bright because the live lights were levelled against the baked lightmaps. `fitted_lights::level` now reads the stored lightmap (`viewer::stored_lightmap`) whichever lightmap the rooms show.
- The bake's stored level is eased above 0.8 and stops at 1.25 (`eased_level` in `detail_bake/gpu.rs`). A glowing surface's own glow no longer counts toward the bake's light above 1.

Still wrong in bake B: room 07a is flatter and brighter than live, the bridge is about 14% darker, and some door frames and one floor are patchy.
