# Running and scripting `kjk`

```bash
kjk                                   # opens the window, runs until closed
kjk --cmds 'wait 60f; screenshot first; dump; quit'
kjk --headless --cmds 'wait 60f; screenshot; quit'   # no window
kjk --load quick                      # opens on a save (`quick`, `0001`, …), as the main menu loads it
KJK_PERF=1 kjk --cmds '…'             # also writes a trace
kjk play --mute …                     # no sound (or KJK_MUTE=1); the settings' volumes untouched
kjk --reset-display                   # opens in a window: the settings' screen resolution put back
KJK_MOVIES=1 kjk play --headless …    # movies played headless too (SOUND.md)
KJK_AA=taa kjk play --headless …      # anti-aliasing for the run (1, 2, 4, taa, dlaa, dlss_quality, …; `game::temporal`)
KJK_AA=taa_performance kjk …          # kjk's own upscaler: the scene drawn at a half of the window's side (taa_quality, taa_balanced, taa_ultra_performance; `game::upscale`)
KJK_SHARPEN=0.5 kjk …                 # the upscaler's sharpening for the run, from 0 (off) to 1 (0.35 by default), for tuning
KJK_AA=taa KJK_MOTION_DEBUG=1 kjk …   # the motion vectors TAA/DLSS read, drawn over the scene
KJK_SHADOW_TILES=off kjk …            # Shadows at High without shadow tiles: capsules and boxes only
KJK_SKIN_ONCE=on kjk …                # a compute pass skins each character with shadow tiles once a frame (`game::skin_once`)
KJK_STEALTH_SHELL=1 kjk …             # Stealth Mode drawn with KOTOR's fallback shell instead of its distortion (COMBAT.md)
KJK_SHADOW_DEBUG=3 kjk …              # rooms show the shadow tiles' atlas: the characters' layer (4: the placeables')
KJK_RAYS=on KJK_AA=taa kjk …          # Shadows at Ultra for the run, whatever the settings say, on a card with ray queries (`game::ray_structures`)
KJK_RAYS=off kjk …                    # the render device is made without ray queries, as on a card that has none; Ultra draws as High
KJK_RAYS=on KJK_AA=taa KJK_SHADOW_DEBUG=5 kjk …  # characters, placeables and doors show in colour where a ray from the camera meets their structures
KJK_RAYS=on KJK_AA=taa KJK_SHADOW_DEBUG=6 kjk …  # the rooms' meshes show in colour where a ray from the camera meets their merged structures, one colour for each mesh
KJK_PROBES=off kjk …                  # Enhanced lighting without the light probes: characters take the area's flat ambient (`game::light_probes`)
KJK_PROBES=only kjk …                 # characters, placeables and doors show their probe light alone, with no lamps and no texture
KJK_LAMP_SHARE=1 kjk …                # all of each lamp's share is taken out of the probe light (0 to 1; half by default), for tuning
KJK_AO=on kjk …                       # Ambient Occlusion on for the run, whatever the settings say (`off` turns it off); it still needs Enhanced lighting and TAA, DLSS or the upscaler (`game::ambient_occlusion`)
KJK_AO=on KJK_AO_DEBUG=1 kjk …        # the surfaces that take ambient occlusion show it alone: white in the open, darker where something stands close
KJK_AO=on KJK_AO_DEBUG=2 kjk …        # those surfaces show the normals the occlusion is worked out from (green faces up)
KJK_AO_LIGHTMAP=0.3 kjk …             # the share of the occlusion a lightmapped surface takes (0 to 1; 1 by default), for tuning
KJK_AO_CONTACT=0 kjk …                # the share of the contact darkening under bodies kept while ambient occlusion is on (0 to 1; all of it by default), for tuning
KJK_ROOM_SHADOW=up kjk …              # the room's light that characters and placeables keep off the floor comes from straight above, as it did before step 7b, to compare pictures
KJK_ROOM_SHADOW=lamps_out kjk …       # that light's direction is found after the listed lamps are taken out of the probe light, to compare pictures (by default it is found before)
kjk movie import k1 01a               # a movie made ours by ffmpeg, in the store
kjk view k1 c_bantha --headless --cmds 'screenshot; quit'   # a model
```

## Commands

Separated by `;` or new lines. Each one finishes only when what it promises
is true, and the next starts after it. A command that cannot keep its promise
ends the run with exit code 1; a script that does not parse exits with 2.

| command | finishes when |
|---|---|
| `wait 2s`, `wait 500ms`, `wait 30f` | the time, or that many frames, has passed |
| `screenshot [name]` | no render pipeline has been compiling for 3 frames (60 s limit), then `<name>.png` exists (10 s) |
| `film 8s [name]` | as `screenshot`, then every frame for that long (wall time) went to `<name>.rgb` (raw RGB, one frame after another, written on its own thread; about 1.4 GB for 10 s at 1280×720), and `<name>.txt` gives the size (`1280x720`), then each frame's ms since the first |
| `dump [name]` | `<name>.json` is written: frame, elapsed time, entity count, facts such as the render adapter |
| `wait world`, `wait loading`, `wait loading_screen` | the state a plugin publishes is true (no module loading; one loading; its screen up) |
| `module end_m01ab [tag]` | the module is installed (`kjk play`), arriving at the tag's door or waypoint, and the world is up |
| `travel end_m01ab [tag]` | the same install asked for, done as soon as the game takes it: what follows runs while it loads (`wait loading_screen; screenshot`) |
| `camera 20 20 1.5 90 0 [60]` | at once: the camera at x y z (metres, game space) looking along yaw and pitch (degrees), with a vertical field of view if given |
| `ui open inventory`, `ui press BTN_OK`, … | at once: the request handed to the game's screens (`UI.md` lists them) |
| `vfx 2007 end_trask [9] [source]` | at once: a `visualeffects.2da` row played on the creature with that tag, as a script's would be, for that many seconds, a beam from the source's tag |
| `anim 2.5`, `anim run` | at once: animation and game time are held at that second (`kjk play` steps the sim up to it), or run with the clock again |
| `record name 1.5 5.5 [30]` | game time is held at each frame's moment from 1.5 s to 5.5 s in turn (30 a second), each screenshotted (`name-0000.png`, …), then `name.mp4` is made of them with the run's `audio.wav` by `ffmpeg` (none: the frames stay, with a warning) |
| `clip name 9 [12]` | the game, running on, screenshotted 12 times a second of its time for 9 s (later when capturing falls behind), each frame stamped with the game's time it shows, then `name.mp4` of them with the sound (`SOUND.md`) |
| `size 1920 1080` | at once: a headless run's frame is resized to that width and height in pixels, as a player resizing the game's window would resize it. A run with a window fails this command. |
| `quit` | always; the run ends with success |

Unnamed screenshots and dumps are numbered (`screenshot-1`, `dump-1`, …).

`kjk play` (and `host`, `join`) opens its window at once on KOTOR's loading screen and loads its
stage on a thread meanwhile (as the main menu's New Game does), drawing the
scene's remembered pipelines while it waits. Its script starts once the
stage is in, so `wait 30f` is 30 frames of the game, as before. A stage that
does not load ends the run with exit code 1 and why.

## The run folder: `kjk-artifacts/runs/<utc time>-<pid>/`

- `kjk.log`: the log, without colours.
- `manifest.json`: git commit (with `-dirty`), build profile, headless or
  not, the script, the render adapter, and each install's fingerprint when
  the fingerprint cache already covers it (`kjk fingerprint` fills it).
- screenshots and dumps the script asked for, and `record`'s videos.
- `audio.wav` from a headless `kjk play`: its sound on the game's clock
  (`SOUND.md`).
- `trace.json` with `KJK_PERF=1`: Chrome trace format, with a span per Bevy
  system per frame. Open it at <https://ui.perfetto.dev>. It is large (about
  15 MB per second of running), so it is opt-in.

## Where a launch's time goes

```bash
kjk launch-report                      # end_m01aa, 3 launches
kjk launch-report --stage course --runs 5
```

Each launch is a headless `kjk play` that dumps `launch` (`dump launch`
in any script does too): the loader's phases in ms (the JA rig, the KOTOR
stage's installer, archives, plan and its parts, scene; the app's engine
and game plugins; the scene's spawn; the pipeline warm-up), and moments
since the process began (loaded, app built, world up, scene spawned,
pipelines warmed). The report tables each one's median, fastest and
slowest. A phase is added with `launch_times::timed` or `record`.

## Frame rate: `kjk bench`

```bash
kjk bench                                   # every scene, the default settings
kjk bench --preset low --label mine         # MSAA and shadows off
kjk bench --scenes cantina,harbinger --seconds 10
kjk bench --exe path/to/old/kjk.exe --label v0.10.0   # another build, run from here
kjk bench --scenes cantina --trace          # also keep each scene's trace.json
kjk bench --scenes ebon_hawk,peragus_hangar,dark_fight --preset shadows-hard --gpu
kjk bench --scenes ebon_hawk --preset enhanced --gpu               # the Lighting setting's cost
kjk bench --scenes dark_fight --preset shadows-soft --gpu --headless --size 3840x2160
kjk compare-pictures first.png second.png --out difference.png
```

Each scene (`bench::SCENES`: a stage or a chain save, copied to
`kjk-artifacts/bench/saves`, and a script) opens in a 1920x1080 window
with v-sync off, movies and sound off, its settings a preset over the
defaults. After the warm-up it dumps every few seconds; the dumps' vitals
give the frame times (mean, p50, p95, p99), the CPU of the main app's
schedules and of the render schedule per frame (the render graph's share
apart), the views and the main passes' draws, and the frames counted
between the first and last dump give the mean (so an older `kjk` that
dumps no CPU is measured alike). A table, a line of JSON per scene in
`kjk-artifacts/bench/LABEL/results.jsonl`, and each scene's last frame
beside it; the run folders are deleted. A frame takes about the longer of
the two threads plus the extract (which runs on the main thread while the
render thread waits).

Two more scenes, Citadel Station (`telos`) and Kashyyyk's Great Walkway
(`kashyyyk`), run only when named (`--scenes telos,kashyyyk`): a
profile-guided build is trained on the others (docs/RELEASE.md), and these
check that its gain holds beyond them. The lighting plan's two scenes
(`docs/LIGHTING-PLAN.md`) also run only when named. `ebon_hawk` is TSL's
Ebon Hawk (003ebo), loaded from the chain save `tsl-14`. `dark_fight` is a
fight with eight foes in Leviathan's dark hall (`arena@lev_m40ac`). The
plan measures them together with `peragus_hangar`.

The scene `main_menu` is K1's main menu, started with no arguments. Malak
stands in smoke and mist there, which the menu draws into a 3D view of its
own (`game::gui_scene`). Its line in the table is followed by a line for
each such view: the particle quads drawn, the pixels they cover, the
view's own pixels, and the samples each pixel takes. A pixel covered by
ten quads counts ten times. These numbers do not depend on the GPU, so
they compare a fast machine's run with a slow one's. On 2026-11-13 the
menu drew about 640 quads, which covered each pixel of its view about 75
times over. The results file has them as `view_particles`.

On macOS, `--gpu` can lose the device at the start of a run (Metal cannot
make the timing queries' buffer). Run without it there.

There are twenty presets. `default` keeps the settings' defaults. `low`
turns MSAA and shadows off. `high` sets MSAA 4 and Shadows to High. The
other three set the shadows alone, over MSAA 4. `shadows-off` sets
Shadows to Off, `shadows-hard` sets it to Low, and `shadows-soft` sets it
to High. Shadows are cast by the characters' capsules. At Low and at
High the area's lamps and its sun cast them. At High every one of a
fight's lights casts them too. At Low only the 2 fight lights that light
a character most cast from it, and each traces 3 merged capsules.
At High the characters nearest the camera and the placeables also
cast from shadow tiles, depth pictures drawn from their own meshes
(`game::shadow_tiles`). `KJK_SHADOW_TILES=off` turns the tiles off, so
one build measures High with and without them. When tiles are drawn, a
line under each scene gives their count, the meshes drawn into them, the
draw calls a frame, and the microseconds each step of the tile pass took:
`gather` on the main thread, `extract`, `prepare` and `draw` on the render
thread. The results file has them as `shadow_tiles`.
With `KJK_SKIN_ONCE=on` a compute pass places each character's skinned
vertices in the world once a frame, and the character's tiles draw them
(`game::skin_once`). Without it, the tile pass skins each mesh once for
each tile. The switch lets one build measure both. The compute pass's
work is inside `prepare` and `draw`. The results file's
`shadow_tiles.skin_once` gives the meshes it places and their vertices.
`shadows-ultra` sets Shadows to Ultra over TAA, and `shadows-high-taa`
sets Shadows to High over TAA, so the two can be compared. At Ultra each
light's shadow is one ray toward the light. The ray is traced against
acceleration structures of the casting characters, the placeables and the
doors (`game::ray_structures`). A fight's light is also traced against the
rooms' structures, which are built once as an area loads, so a wall or a
closed door stops it. No tile is drawn, and the skin-once pass
places every casting character for the structures. Ultra needs a card
with ray queries, and TAA or DLSS. TAA at the window's size and kjk's own
upscaler both count as TAA. Under MSAA, or on a card without ray
queries, Ultra draws as High does. The GPU spans
`ray_structures/characters` and `ray_structures/top_level` time the
builds. A span `ray_structures/placeables` is there only on a frame that
builds a placeable's, a door's or a weapon's structure, or the merged
structures of the placeables or the doors. A span `ray_structures/rooms`
is there only on a frame that builds the rooms' merged structures, which
happens once as an area loads.
A dump's `ray_structures` fact gives the counts, and whether the structures
are traced. `instances` counts the top-level structure's instances. wgpu
walks every one of them at the frame's submit, once for each time the
frame sets an Odyssey material. This count is what makes Ultra cost time
on the CPU.
`dependencies` counts the bottom-level structures those instances use,
each once. The rooms' meshes are geometries of a few merged structures
(`room_structures`, `room_geometries`, `room_triangles`). The placeables'
meshes that stand still are geometries of merged structures too
(`prop_structures`, `prop_geometries`), and so are the doors'
(`door_structures`, `door_geometries`). `room_builds`, `prop_builds` and
`door_builds` count how many times each kind of merged structure was built
since the run started. In a scene where nothing moves they stay the same.
`room_moving` counts the room meshes that an area's animation keeps
moving. They are left out of the rooms' structures. `room_lists` counts
how many times the list of room meshes was made since the run started.
`props_apart` and `doors_apart` count the placeables' and doors' meshes
that moved or appeared in the last 30 frames, which have instances of
their own. `rigid_meshes` counts the characters' weapons and helmets, which
have instances of their own too. `KJK_RAYS=on`
makes a run use Ultra whatever the settings say. `KJK_RAYS=off` makes the
render device without ray queries, so a run acts as a card that has none.
`classic` and `enhanced` set the options' Lighting alone, over MSAA 4 and
Shadows at High. `KJK_AA=taa` in front of the command runs either under
TAA. `effects-high` and `effects-low` set the options' Effects Quality
alone, over MSAA 4 and Shadows at High. Each view's particle line then
also names the Effects Quality and the pixels the particles are drawn
over at it. At Low that is a quarter of the pixels they cover, since the
particles are drawn at half the width and half the height.
`ao-off` and `ao-on` set the options' Ambient Occlusion alone, over TAA,
Enhanced lighting and Shadows at High, since the option needs TAA and
Enhanced lighting. With `--gpu`, a line under each scene gives the GPU time
of the pass (`ambient_occlusion`, its three compute passes together) and
of the depth prepass (`early prepass`), which also writes the normals while
the option is on. A dump's `ambient_occlusion` fact says whether the scene
camera has the pass (`on`), why not (`why_not`: `off`, `classic`, `msaa`
or `unsupported`), how many frames it ran in since the run began
(`frames_run`), the size of the frame it last ran over (`size`, the
smaller frame under an upscaler), its samples a pixel (`samples`), and the
two shares set by `KJK_AO_LIGHTMAP` and `KJK_AO_CONTACT`.
Six presets set the anti-aliasing alone. `taa` sets TAA at the
window's size. `taa-quality`, `taa-balanced`, `taa-performance` and
`taa-ultra-performance` set kjk's own upscaler at two thirds, 58%, a half
and a third of the window's side. `dlss-performance` sets DLSS at a half
of the side, and falls back to `taa-performance` on a card or a build
without DLSS. Under the upscaler, a line under each scene gives the GPU
time of its resolve (`taa_upscale`) and of its sharpening
(`taa_upscale_sharpen`).

`kjk compare-pictures` tells how far two pictures of one size differ. It
prints the largest and the mean difference of a colour channel, in steps
of 255, and how many pixels differ by more than 2 steps. `--out` writes a
grey picture of the differences, 16 times as bright.

Two runs of one build never draw quite the same picture. Characters
breathe, console lights blink and dust blows. `--still third.png` takes a
second picture made the same way as the first. Only the pixels that are
exactly the same in those two are then compared. `--up-to 229` compares only the
colour channels that are at most 229 of 255 in the first picture. That is
the roll-off's knee (`game::rolloff`), below which Enhanced lighting
leaves a value alone.

The pictures for such a check come from headless runs, which draw the
same frames each time. A bench's picture does not, because its window
runs as fast as it can. This takes one picture of the cantina at frame
240, under the settings file named in `KJK_SETTINGS`:

```bash
kjk play --stage tar_m03ae --headless --cmds 'wait world; wait 240f; screenshot still; quit'
```

The Dune Sea's dust moves over the whole frame, so almost every pixel
changes between two runs there.

`--size 3840x2160` sets the frame's size in place of 1920x1080.
`--headless` runs each scene with no window. A headless run is paced at
60 frames a second, so its frame times say little, but `--gpu`'s pass
times hold. A card too fast to show a GPU cost at 1920x1080 shows it this
way.

`--gpu` times the render passes on the GPU. It sets `KJK_GPU_TIMES=1`,
which any run can set. Bevy's render diagnostics then time its main
passes, and count their fragment shader invocations where the adapter
counts them. kjk's own passes over the frame are timed too, as `roll_off`,
`video_effect` and `ui_composite`. A `shadows` span also times Bevy's
shadow-map systems. No
light draws a shadow map since the capsule shadows, so the span is about
0 ms on builds that have them. Each
pass's milliseconds are summed over every camera and light view of a
frame. A dump's `vitals.gpu` has them per pass (`ms` and `fragments`),
and their total (`spans_ms`). The table shows them on a line under each
scene. The results file has them as `gpu_ms`, `gpu_fragments` and
`gpu_spans_ms`. The timestamp queries cost a little. So a comparison of
two builds runs both with `--gpu` or both without it. An older build
has no GPU timings at all.

Two switches for measuring against: `KJK_EXEC` names the schedules run
on one thread (`main,render,graph` by default; `none`: Bevy's
multithreaded executor everywhere; on macOS a run with a window keeps
`render` multithreaded, since its surface is made on the main thread),
and `KJK_PERF=1` keeps Bevy's spans
(which cost about a quarter of the frame rate).

## The rooms' own light: `kjk room-lights`

```bash
kjk room-lights                          # K1 and TSL, into target/room-lights
kjk room-lights tsl --out somewhere/else
```

This survey opens no window. It reads every area's rooms, doors,
placeables and creatures, and the rooms' lightmaps. It writes
`room-lights.json` with every lamp, every glowing mesh, every room and
every lightmapped room mesh. It writes `room-lights.txt` with the tables
of step 7a and 7d of `LIGHTING-PLAN.md`. A run over both games takes
about half a minute.

## Lights fitted to the lightmap: `kjk room-light-fit`

```bash
kjk room-light-fit tar_m03ae               # into target/room-lights/fit
kjk room-light-fit 003ebo --game tsl
kjk room-light-fit lev_m40ac --every-glow  # every glowing mesh is a panel
```

This trial tool opens no window and works on one module. It searches
K1 and then TSL for the module unless `--game` names one. It takes the
area's lamps and glowing panels as lights at fixed places. It then solves
for the strengths that best match BioWare's lightmaps, as step 7c of
`LIGHTING-PLAN.md` describes. For each area it writes four files. The
`.txt` file holds the tables. The `.json` file holds every candidate light
with its fitted strength. The `-lightmaps.png` picture shows every
lightmap beside the fitted light and their difference. The `-top.png`
picture shows the floors from above in the same three panels. A run takes
from 10 seconds to about a minute.

## The whole-game sweep

```bash
kjk sweep                       # K1 and TSL, every module
kjk sweep tsl --only 1          # one game, the modules starting so
kjk sweep k1 --skip render,audit --seconds 20 --jobs 6
```

Every module is built (`ModuleIr`), its scripts stepped headless for
`--seconds` (`kjk script sweep`: gaps, faults, and the scripts the game
names but does not have, which run nothing as in the original), a
640×360 frame rendered from its entry as a `kjk tour` `entry` stop (a child `kjk module` run that
also dumps the scene's facts: missing models, textures and clips, refused
creatures, room walkmeshes that do not read; rooms the game gives no
walkable face, a sky or a backdrop, are listed as seen only and are not a
fault), and its containment audit run as `kjk
collision audit` does by default. `--jobs` modules are built, rendered
and audited at once while the scripts step. The run folder holds:

- `index.html`: tallies per game, then every module worst first with its
  frame, its audit map and what is wrong, filterable by game and area,
  then the bug lists as tables. The score orders it: a module that does
  not build 1000, whose scripts do not install 500, a failed render or
  audit 300 and 200, script faults 100 and up, a near-black frame 80,
  escapes on foot 50 and up and by jump 20 and up, missing assets, then
  one per script gap reached and per script named but missing. A game whose install is missing is left
  out with a warning; the others are still swept.
- `bugs.md`: the same lists in Markdown, for handing out: refusals and
  failed stages, script faults by message, script gaps by modules reaching
  them per game, scripts named but missing, containment escapes, the
  slowest audits, missing assets; each missing texture with what is drawn
  in its place (no lightmap, the lightmap alone, nothing, no env map, or
  the body model's own textures: `viewer::stand_in`). On the Steam installs
  every texture listed is one the game does not ship in any archive: TSL
  models naming K1 textures, lightmap names from before the lightmaps were
  packed, body textures `appearance.2da` names.
- `sheet-k1.png`, `sheet-tsl.png`: every frame of a game, worst first.
- `shots/`, `audits/`, `logs/` (each render's output), and `facts.json`.

## Playing a module through: `kjk playthrough`

The working method (walkthrough, nav checks, routes, reading blockers): `PLAYTESTS.md`.

```bash
kjk playthrough k1 end_m01aa 'talk; use end_locker01; to end_door01'
kjk playthrough k1 end_m01aa route.txt --arrive end_wp_flame01 --follow end_trask
kjk playthrough k1 end_m01aa data/routes/k1/endar_spire.route \
  --god --skill security=2     # the Endar Spire, the bunks to Taris
kjk playthrough k1 @taris_start data/routes/k1/taris_upper_city.route --god
```

`--god` keeps the player above 0 HP (the sheet's immortal flag, which a
script's `SetMinOneHP` leaves alone): a walk that tests what the scripts and the world do, not
the fights' balance. `--mighty` makes the player's blows count ten times
over, in KOTOR 2 each with the greatest chance to hit (fights end quickly; a test aid as `--god` is). The player clicks
past every conversation line that plays out on its own, as a player
skips dialogue (`Pick::Skip`); `--listen` hears every line out.
`--stale` starts from a checkpoint made with other level data than this
build's: its module takes this build's data and the modules it left are
met anew — a dev aid to go on at the chain's top while the chain is
remade, not the run the chain would make. `--skill NAME=RANK`,
`--class`, `--global NAME=N`, `--run SCRIPT` and `--follow TAG` as
`kjk play`'s.

No window: the authority stepped as fast as it goes, a player driven goal
by goal (`launcher::playthrough` lists the goals: `talk`, `pick`, `say`, `party`, `travel`, `to`,
`at`, `use`, `exit`, `fight`, `race` and `turrets` (a minigame played to
its end, `MINIGAMES.md`; `turrets hold` without firing), `pazaak`, `equip` (`equip TEMPLATE`: that item; `equip saber`: the strongest lightsaber the party has),
`put`, `save NAME` (a checkpoint; `@NAME` for the module starts from it), `unlock` (Security, with a spike
the party carries if named), `bash` (struck with what is in hand until its lock breaks), `disarm X,Y`
(the mine the party sees nearest there, Demolitions, from where the player stands; `recover X,Y`
the same, keeping its kit), `cast ROW`
(that power cast on the player from its power list until its effect lasts), `heal ROW`
(that healing power cast on the player until its vitality is full), `item TEMPLATE`
(one of that item the party carries used on the player, once, as the inventory's Use Item button does:
Manaan's sonic emitter), `use item:TEMPLATE` (what
holds that item: remains searched, their creature fallen in a route before, or a foe fought), `leader`, `solo`, `return` (the map's return to the Ebon Hawk or back, blocked where the page refuses it), `levelup`, `sell`, `credits`, `wait`, `until`,
`module`, `if` and `while` blocks closed by `end`; and, for what a route stands in for, `global`,
`gold`, `skill NAME=RANK` and `goto MODULE [TAG]`, the module's entry
without a tag). Walks go over the
navigation mesh (`sim::walk`), pressing use on the way; offers are
answered once their line has played (`pick`'s choices, else the first); a
container used is emptied; a party selection screen a script shows is
closed keeping whom it forces in; a door that leads out installs its module and
the route goes on there (an exit the party may not take in a fight, or whose way a door
stayed shut on with foes near, has its foes fought first, as `fight 25`,
as a player must). A level the party's XP allows is taken as a goal
starts, as a player would, in the class taken last (`levelup` names
another); a `use` of a foe still standing attacks it, and a target is
found by its tag or its template. A goal not met within `--goal-seconds` of play is
**BLOCKED**: the line says where the player stood, the conversation that
holds it and every script queue at work (a move's target, its route,
the mind walking it), how far the target is, whether a way leads there
and the doors and placeables on the straight way (locks, DCs); the run
ends there (with `--keep-going` it goes on with the next goal), and exits
1. `save` goals write to `kjk-artifacts/checkpoints/GAME` (`--save-to
DIR`: there). The run folder has `facts.json`
(each goal with its play time and its wall time, `wall_ms`; the
scripts' faults, gaps, missing scripts, the globals),
`transcript.txt`, `trace.txt` (the scripts' first lines) and
`inputs.kjkr` (a recording; `kjk play --replay` draws one made off a
module, not yet one made in a module).

## Playing the routes through: `kjk chain`

```bash
kjk chain k1                                 # every route of data/routes/k1
kjk chain k1 manaan                          # Manaan and every route after it
kjk chain k1 manaan tatooine --alone --jobs 2  # just these two, from their kept checkpoints
kjk chain k1 --quick                         # every route alone, at once
kjk chain k1 --dry-run                       # each route's command and cached outcome
```

A game's routes as the chain they make (`PLAYTESTS.md`): each route a
`kjk playthrough` with the game's chain flags (`data/routes/GAME/
chain.flags`, k1's `--mighty --skill security=2`; `--bare` leaves them
out), its header line's start and flags, and `--mighty`, each `--skill`,
`--keep-going` and `--goal-seconds` when given here, each flag once. A
route starts as soon as the route before it (the one whose `save` makes
its checkpoint) has saved it in this run (one saved in a `while`, or
twice, once that route has ended); one not saved skips it and the routes
after it. The chain stops at its first block: no route starts after one
(`--keep-going` plays on). Routes named start from the checkpoints kept
(`kjk-artifacts/checkpoints/GAME`) as they stood when the run began;
`--alone` (`--quick`) starts every route chosen so, none waiting on
another. Up to `--jobs` play at once (half the cores by default).

The run folder holds a copy of the binary every route runs (let go at
the end), the routes as they were, `starts/` (the kept checkpoints it
started from) and `checkpoints/` (what its routes saved, `--save-to`).
After the run, the checkpoints `data/routes/GAME/saves.list` names are
written as saves `k1-01`, `k1-02`, … for the window's Load screen and
`kjk --load k1-03` (the main checkout always; a worktree with `--saves`;
`PLAYTESTS.md`).
The checkpoints of a route met replace the kept ones; a blocked route's
stay in the run folder. A route met or blocked is cached
(`kjk-artifacts/chain-cache/GAME`, the last three binaries') under a
hash of the binary, the game data's fingerprint, its start checkpoint's
bytes, its route and its arguments; an identical rerun is taken from
there, its checkpoints and log with it (`--no-cache`: every route
played). Each route's line gives how it ended (`met`, `BLOCKED`,
`FAILED`, `skipped`), its wall time, `(cached)` when taken from the
cache, and its summary; the run folder has each route's log and
`facts.json` (each route's command). Exit 0 when every route met all its
goals.

## Headless

`--headless` runs with no window and no windowing system: the camera renders
into a 1280×720 image (`--size 1600x1200` for another), and `screenshot`
reads that image back. Use it for
CI, agents and, later, the dedicated host.
