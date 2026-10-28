# Grass and weather (`game::grass`, `game::weather`)

Both are view layers: the sim knows nothing of them, so the sim hash does
not move and each co-op peer draws its own. Their values come from the games'
executables (K1's and TSL's), read for this work; reone's grass
stands only where the executables were silent, and those points are marked
`[verify]`: not settled, to be checked against a render or the original.

## Grass
- **Where it grows:** on the walkmesh faces of a room's model whose
  `surfacemat.2da` row has `grass` set (`collision_odyssey::Surfaces`; only
  K1's `Grass` row), in an area whose ARE names a grass texture and a density
  above 0 (`asset_odyssey::module::AreaGrass`: texture, density, quad size,
  the four variants' chances, ambient, diffuse, TSL's emissive).
- **How much, and where:** `trunc(face area × density)` clusters a face, so a
  face under one cluster's worth has none. Each takes a point of the face,
  a turn, one of four quarters of the texture and a size of 0.8 to 1.2, from
  the C library's `rand` (`msvc_rand`) reseeded for each face from its
  corners, so the grass is the same every time and on every peer
  (`asset_odyssey::grass`).
- **What is drawn:** one upright quad a cluster at its own turn (it does not
  face the camera), two-sided, `Grass_QuadSize` wide at the bottom and the top
  edge scaled by the cluster's size; the texel is alpha tested at 0.4 and
  blended, depth written. Its light is the texture times the room's lightmap
  under it, then the area's fog (`grass.wgsl`).
- **How far:** whole to 15 m, fading in the square of the distance to nothing
  at 30 m, nothing drawn past. Each cluster fades by its own point; the
  original fades a 2.6 m bin at a time.
- **Wind:** within 15 m the top edge leans along +X+Y with the area's
  `WindPower` (0: still), easing to still from 12 m out. A gust in the vertex
  shader, not the original's spring per quad, so the motion differs a little.
- **The option:** `display.grass` on the options page, on by default, saved
  with the other settings. Off hides the patches (nothing is rebuilt).
- **In the games:** K1's Dantooine (`danm13`, `danm14aa` to `ad`, `danm16`),
  Kashyyyk's Shadowlands (`kas_m24aa`, `kas_m25aa`), Manaan's seafloor
  (`manm28ab` to `ad`), the Taris Undercity (`tar_m04aa`) and the Unknown
  World (`unk_m41aa` to `ad`); TSL's Telos, Dxun and Dantooine.
- **`[verify]`:**
  - The top edge's height (`Grass_QuadSize` times the size), by eye.
  - The light. K1 gives `Grass_Ambient` and `Grass_Diffuse` 0 in every area
    read, so they cannot multiply the texture; the texture times the
    lightmap is what matches the ground. TSL's areas give them (and an
    emissive) to a vertex program whose arithmetic is unread: its grass may
    lack a tint. The area colours are carried and unused.
  - A room with no lightmap draws its grass white; the original takes the
    room mesh's vertex colours there.
  - The alpha test is on the texel alone; the original may thin the grass
    toward 25 m by testing the faded alpha.
  - The sway: its amplitude here rises and falls with the gust; the original
    holds a steady lean. It is strong on screen at `WindPower` 1.
  - Faded grass near 30 m still writes depth and can hide a transparent drawn
    after it (the original writes depth too).
  - K1's code was read less closely than TSL's (same constants, same density
    rule, the code not line by line).

## Rain and snow (TSL)
- **What an area has:** one weather at a time (`asset_odyssey::weather`): rain
  if the ARE's `ChanceRain` is above 0, else snow if `ChanceSnow` is, else
  lightning alone (`ChanceLightning`; Malachor). There is one roll for each
  300 s epoch of the sim's clock, counted from the clock's zero, not from
  entering the area: re-entering within an epoch gives the same weather.
  `rand % 100` against the chance, running while under it, so a chance of 100
  holds for good. The roll is seeded by FNV-1a of the area's resref and the
  whole epoch number hashed in, so every peer rolls alike and each epoch's
  roll is a fresh one. An interior area (ARE `Flags` bit 0) never has weather.
- **How it is drawn:** six copies of the game's `fx_rain` or `fx_snow` model
  stand on a hexagon of radius 4.5 m round the camera's ground point, turned
  with the camera, 8 m over the player's feet (the four side ones 5 m more),
  moved there every frame. Their particles are folded into a 10 m square round
  the camera so it never leaves the fall, and pushed along +X+Y by
  `WindPower` m/s (`model_fx` `WeatherSpray`).
- **Indoors:** by the player's room. In a room the area keeps dry
  (`DisableWeather`, ARE `Rooms`) the copies are gone at once, no drop
  finishing its fall, and made again outside. After a stop the fall goes on
  for 6 s.
- **In the games:** no K1 area has a chance. TSL: Dxun's landing and tombs
  (`401dxn`, `402dxn`, `410dxn`: rain 100, lightning 100, wind 1), Telos's
  polar area (`261tel`: snow 100), Malachor (`901mal`, `902mal`: lightning
  100, wind 2).
- **`[verify]`:**
  - The original's wind gusts (a noise-driven variation) are not copied; the
    wind here is steady.
  - The wind pushes only the weather's sprays: a room's or placeable's
    emitter with the wind flag does not drift.
  - Indoors goes by the room of the topmost floor at the player's x and y,
    so a bridge over a dry room may count as that room.
  - The weather the original saves in the GIT (`CurrentWeather`,
    `WeatherStarted`) is not mirrored: ours is not saved, and TSL's
    `EnableRain` script routine is not read.
  - The side copies' drops by the model's numbers fall from 9 m to 3.75 m over
    the player and never reach the camera; the rain is faint by its data
    (`fx_rain01`, alpha at most 30/255). To be checked against the game.
  - A paused sim freezes the rain mid-air.
  - Playing through `kjk play` with real rain is unexercised, since it cannot
    stage TSL.

## Lightning and thunder (TSL)
- **A check** comes every 5 to 10 s while the weather has lightning and the
  player is outdoors; it passes with `rand % 100` under `ChanceLightning`.
  A coin then picks distant thunder alone or a flash.
- **A flash** plays one of `fx_lightning01`'s clips `flash01` to `flash03`
  once, as a point light of radius 20 whose colour the clip keys, 10 m over
  the player and up to 9.9 m aside in each of x and y. The next check is
  armed when it ends. Every Odyssey material and the grass add the light's
  falloff (`Flash`, held in the area's shared light list), rooms included,
  and the sum stops at full.
- **Thunder** is TSL's four sounds: `as_wt_thundercl1`/`2` with a flash,
  `as_wt_thunderds1`/`2` without, at once, only while the area's ambient
  sound plays. K1 has none.
- **`[verify]`:**
  - Whether the original lights the rooms. Our rooms ignore other lights; the
    flash is added to all of them, so they may show brighter than in the
    original (the texture in full while it lasts).
  - The cut of the shadows to 40 % for a flash is not copied: there are no
    dynamic shadows to cut.
  - The strikes cannot match the original's: it draws from the C library's
    shared `rand`, ours from the area's own stream.
  - K1 has no `fx_lightning01`: a K1 area with a lightning chance would count
    flashes with no light and no thunder (none exists).
  - A flash running when the player steps indoors freezes until they leave.
- **Sound in the viewers:** `kjk module` and `kjk view` open a sound mixer
  only when their scene has thunder, so a TSL run can be heard.

## Rooms' emitters (`model_fx`, `own_fx`; the rest is in `MODEL-FX.md`)
- **A script's room clip** (`PlayRoomAnimation`) plays in every module,
  emitters and events included, not only in minigames: Peragus's sparks and
  ship smoke, the Ravager's flames and smoke. It loops from when it began.
- **The particle limit:** an emitter keeps as many particles as its stream
  holds (birth rate plus random birth rate, times life), not 200: Manaan's
  bubbles, Dxun's tomb, the swoop tracks' sand. 4096 is a frame guard.
- **Two orientations:** `Aligned_to_World_Z` stands the quad up (the star
  map's glyphs, holograms); `Aligned_to_Particle_Dir` lays it along the
  particle's heading (the surf's waves). Both are read from **TSL's**
  executable; K1's was not read for them `[verify]`.
- **A detonation at a loop's last frame** fires once as the clip wraps.
- **`[verify]`:**
  - Script room clips loop; whether the original loops or plays once is
    unread (Peragus's 76 s flight suggests a one-shot for some).
  - A room's emitter values freeze where they were when the area's clip moves
    to another room; the snapshot holds one room clip an area.
  - K1's particle cap was not read (stripped); TSL has none. The original's
    birth count a step depends on the frame rate and drops the fraction of a
    step's births; not copied, so ours may be denser at high rates.
  - The roll about a particle's heading for `Aligned_to_Particle_Dir`
    (invisible on round foam); a still particle takes the emitter's y as its
    heading, where the original keeps its birth heading.
  - `Aligned_to_World_Z`'s up sign when the camera is upside down.
  - A detonation is missed when a clip is no longer than the step, and one
    exactly at the previous step's moment can fire twice.
  - The holo placeables (`plc_holoart`, `plc_rakathol`) were not rendered.
  - The frame cost of the raised limits (up to 4096 particles an emitter,
    e.g. in `411dxn`) was not measured.

## Rooms shown, the fog and the clear colour (`game::scene`)
- **The fog:** the ARE's `SunFogOn`, `SunFogNear`, `SunFogFar` and
  `SunFogColor`, linear in the eye distance (K1's vertex programs write the
  radial distance as the fog coordinate), on the models whose header sets
  the fog flag (every K1 room does).
- **The clear colour** is the area's fog colour, black without fog, as K1
  sets its clear colour with the fog's (`clear_to_fog`): whatever nothing
  draws looks like what the fog has swallowed.
- **Rooms shown** (`Culling::room_visible`, from the room under the camera;
  every room when it stands over no floor): the room's own VIS links, either
  way round (K1 makes every link mutual), and besides them any room some
  part of which is nearer the camera than `SunFogFar`; in an area without
  fog, every room (each mesh's frustum test then draws what is in view). K1
  draws the VIS's alone: the rest is Q-0305, approved so that the Taris
  cantinas' archways across the bar show their rooms rather than fog colour.
  Creatures, doors and placeables are drawn whatever their room.
- **What it costs** (render app's CPU time a frame, headless 1280x720,
  before and after the near rooms): Taris' upper cantina 13.4-14.1 to
  14.2-14.7 ms, Javyar's 9.5 to 10.5-11.3, the Lower City (no fog, 8 to all
  15 rooms) 9.5-9.6 to 11.9-12.0, Dantooine's courtyard (`danm13`, 5 to 21)
  6.9-7.0 to 7.5-7.6, TSL's Ebon Hawk (`001EBO`, 6 to 18) 7.6-7.7 to 8.1,
  Peragus' mining facility (`101PER`, 13 to 66) 7.2-7.3 to 8.0. The GPU's
  share was not measured.

## Seeing them
`kjk play --stage` stages K1 only, so TSL's layers are looked at with
`kjk module tsl <module> --headless --cmds …` (no sim: the scene's clock, the
player taken as the floor under the camera). `record <name> <from> <to>
<fps>` films a range of seconds (with `audio.wav`); `anim N` freezes the
clock, so use `wait` for anything that is born.

```bash
kjk play --stage danm14aa --headless --cmds 'wait 3s; screenshot grass; dump; quit'
kjk module k1 danm14aa --headless --cmds 'wait 2s; camera 163.2 139.3 2.4 90 -5 40; wait 1s; screenshot closeup; quit'
kjk play --stage kas_m25aa --headless --cmds 'wait 3s; screenshot shadowlands; quit'
echo '{"display.grass":"false"}' > off.json
KJK_SETTINGS=off.json kjk play --stage danm14aa --headless --cmds 'wait 3s; screenshot grass_off; quit'
kjk module tsl 401dxn --headless --cmds 'camera -117.2 -143.8 44.6 213 5; wait 2s; record dxun_rain 3 9 24; dump; quit'
kjk module tsl 261tel --headless --cmds 'wait 2s; record telos_snow 3 9 24; dump; quit'
kjk module tsl 901mal --headless --cmds 'wait 2s; record malachor 3 23 24; dump; quit'
kjk module k1 manm26aa --headless --cmds 'camera -10 -30 70 180 -25 70; wait 8s; screenshot surf; quit'
kjk module k1 kas_m25aa --headless --cmds 'camera 83.4 20 12.3 90 -5 45; wait 2s; screenshot starmap; quit'
```
- `dump` has a `grass` fact (`texture`, `missing_texture`, `shown`, the
  `patches` by room with their tufts) and a `weather` fact
  (`fall`, `lightning`, `wind`, `running`, `indoors`, `player_room`,
  `emitters`, `particles`, `flashes`). A flash comes within 5 to 10 s of a
  check; film 20 s to see one (`record malachor 3 23 24`).
- A flash and the Dxun rain are faint by their data; look at stills of the
  recording's frames, not its first seconds.
- Peragus's and the Ravager's script clips need a running sim and TSL cannot
  be staged, so they are seen only through the check `script_room_clips`
  (`kjk::probes::script_room_clips`); no viewer command starts a room clip.
- Manaan's bubbles (the particle limit): `kjk play --stage manm28ab
  --headless --cmds 'camera 235.4 39.3 8 127 3 22; wait 5s; screenshot bubbles; dump; quit'`.

The checks are `surface_probes`, `area_grass_and_weather` (and `…_in_the_games`),
`grass_placement`, `grass_in_scenes`, `weather_rolls`, `weather_in_scenes`,
`script_room_clips` and `particle_limits` (`docs/TESTS.md`).
