# Lighting and shadows: the plan

Status: proposed, 2026-11-11. Steps 0, 1 and 2 are built and measured on
the 4070 Ti SUPER. So is step 3, on 2026-11-12.
Step 4's shadow tiles were built on 2026-11-12 too. So was its compute
pass, for the tiles alone. It is off by default, because on the 4070 Ti
SUPER it costs more than it saves. Its contact shadows are not built. On
the 4070 Ti SUPER the tiles do not cost less than the capsules.
Step 0's results are under "Measured in step 0", and steps 1, 2, 3 and 4
have theirs under their own headings. The 1050 Ti runs and the TAA
baselines are still owed. Step 5 was built on 2026-11-13. Step 6 was
planned on 2026-11-13 and is not built. Nothing else here is built or
measured yet unless a line says so.

## What we want

- Fight lights (bolts, muzzle flashes, sabers, Force powers) that are
  noticeable but belong in the room, not a spotlight on the floor.
- Shadows that look good and cost less than today's.
- A renderer that ray tracing, upscalers and global illumination can be
  added to later without redoing this work.

The target for speed: the new default shadow setting costs less, on CPU and
GPU, than today's shadows with Soft Shadows off. The machine that decides is
the GTX 1050 Ti class tester, not the 4070 Ti SUPER.

## Where we are

Checked in the code on `origin/main` and `feat/dlss`
([#323](https://github.com/kjk-engine/kjk/pull/323)).

**Why a bolt looks like a spotlight** (`combat_lights` in
`crates/game/src/odyssey.wgsl`):

- The falloff is `(1 − (d/r)²)²` times `COMBAT_ROOM_GAIN`, and the sum is
  clamped at 1. In a dark room the middle goes flat at full brightness: an
  evenly lit disc with a soft rim.
- The sum is done on gamma-encoded values with `Tonemapping::None`, so
  nothing rolls off.
- Nothing checks whether a wall or floor is in the way.
- The bolt itself has no glow, so all of its presence has to come from the
  floor it lights.

**Why shadows are slow** (measured 2026-11-08, v0.11.0):

- About 36 texture taps per shadow slot per fragment, 3 slots (2 lamps and
  the sun). Soft Shadows off keeps every tap.
- No depth prepass on the MSAA path, so overdrawn fragments pay it all.
- Each casting light is its own Bevy view, running a whole Core3d schedule,
  and skinned characters are skinned again for each. Shadows cost about 35%
  of the frame on the CPU in 106per.

**Why shadows look bad** (from the code; not yet checked against captures):

- Only 2 lamps and the sun cast, so a shadow jumps from lamp to lamp.
- A shadow takes a fixed share of the lightmap (`LAMP_SHADOW_DARKNESS` 0.95,
  `SUN_SHADOW_DARKNESS` 0.6, `CHARACTER_SHADOW_DARKNESS` 0.85), not the light
  the lamp gives there.
- Long shadows are faded out (`LAMP_FADE_*`, `SUN_FADE_*`).
- Feet get stand-in discs (`feet_shade`).
- A wide cone's 2048 map spends most of its texels on empty floor.

**Measured in step 0.** The bench ran on 2026-11-11 with `kjk bench --gpu`
at 1920x1080 and MSAA 4. The machine was an RTX 4070 Ti SUPER with a Ryzen 7
3700X. The full results are in the step 0 PR.

- Drawing the shadow maps costs the GPU almost nothing. The lamps' room maps
  are kept between frames (`still_maps`). The characters' maps took 0.00 to
  0.03 ms a frame in all three scenes.
- The cost is the texture lookups in the main opaque pass. In the Peragus
  hangar, the pass shades the same 2.32 M fragments in every setting. It
  takes 0.48 to 0.53 ms with shadows off, 1.09 to 1.12 ms with hard
  shadows, and 1.26 to 1.60 ms with soft shadows. In the dark fight it takes
  0.91 to 0.93 ms, 1.05 to 1.10 ms and 1.17 to 1.23 ms. So hard shadows are
  already cheaper than soft ones on main.
- On the CPU, each casting light still has views of its own. The Peragus
  hangar draws 2 views with shadows off and 9 with them on. The dark fight
  draws 4 and 8.
- Frame rates on this machine are bimodal. One build with one preset runs
  at one of two rates about 2x apart, and the rate changes between runs.
  The Ebon Hawk ran at 131 fps in one run and 262 fps in another, with the
  render thread at 7.1 ms and 3.2 ms. A frame-rate comparison here needs
  several alternating runs of each side. Draw counts and fragment counts
  are exact.
- Areas without fog went back to their VIS (below). Opaque draws fell from
  73 to 37 in Peragus and from 329 to 225 in the dark fight. Opaque
  fragments fell only 2 to 6%. At the bench's poses, most of the rooms
  behind walls were already outside the view. The 2026-11-08 measurement
  that halved 003ebo's fragments was taken at another pose and at 800x600.
  It does not hold in general.

**What main has had since #323, #342 and #344:**

- A depth and motion-vector prepass for every surface that writes depth
  (`odyssey_prepass.wgsl`, `temporal.rs`). The depth pass is drawn under
  every anti-aliasing choice, so the line above about the MSAA path is
  no longer true. The motion pass is drawn under TAA and DLSS only.
- TAA, DLAA and DLSS as anti-aliasing choices, with jitter, mip bias and cut
  resets.
- A float frame under DLSS (Bevy's `Dlss` requires `Hdr`), still with
  `Tonemapping::None`, and its values are still gamma-encoded.
- One Shadows setting with Off, Low and High. Soft Shadows is gone.

## Decisions

1. **Rooms are baked; only moving things get dynamic shadows.** Everything
   below follows from this.
2. **Capsule shadows are the main path, not the Low setting.** They need no
   shadow views, no re-skinning, no prepass and no temporal filter, and they
   work under every anti-aliasing mode.
3. **A shadow removes the light that is blocked.** One visibility value per
   light and pixel, applied to that light's own contribution. Capsules, shadow
   maps and later ray queries all produce that same value.
4. **HDR belongs to the renderer, not to the anti-aliasing choice.** One
   "Classic" switch turns the new lighting off; MSAA, TAA and DLSS show the
   same lighting. The switch is the options' Lighting (`display.lighting`),
   with the values Classic and Enhanced.
5. **The upscaler is a slot.** It takes depth, motion vectors, jitter and HDR
   colour. DLSS is the first thing in it.
6. **The HDR frame stays gamma-encoded.** Decided with the owner on
   2026-11-12, before step 3. The frame's values are encoded as before
   and may now go above 1. The frame does not become linear. Every blend,
   the fog, the fades, the HUD, the screens and the brightness setting were
   made for encoded values, and they keep their look with no retuning. The
   surface shader works out new light in linear and encodes the result, so
   an opaque surface looks the same as it would in a linear frame. What
   this gives up is under step 3 and step 6.
7. **No stencil shadow volumes.** wgpu has no geometry shaders, the games'
   meshes are not closed, the edges are hard only, and a third-person camera
   sits inside the volumes all the time.
8. **Measure before and after every step**, back to back, at 1920×1080.

## The steps

Each step is its own branch and PR, and each leaves the game shippable.

### 0. Timings and baselines

- Add GPU timings for the shadow passes (Bevy 0.19's diagnostics time the
  main passes only).
- Record `kjk bench` baselines in 003ebo, 106per and `arena@lev_m40ac`:
  shadows off, hard, soft; MSAA 4 and TAA.
- Take reference captures of the cases we want to fix: a bolt down a dark
  corridor, a firefight, a character under two lamps, a character in sun.
- Check whether fogless areas still draw every room (`Culling::room_visible`
  in `scene.rs`). If they do, fix it first: on 2026-11-08 it halved 003ebo's
  fragment work. Done. They did draw every room. Now the VIS decides which
  rooms are drawn there. The rooms that are only seen are drawn too. Skies
  and backdrops are such rooms, like the Ebon Hawk's space, which no VIS
  lists.

Step 0 built these tools (`docs/RUN.md`). `KJK_GPU_TIMES=1`, which any run
can set, and `kjk bench --gpu` time Bevy's main passes on the GPU. They also
time a `shadows` span around Bevy's shadow-map systems. The bench has three
new presets, `shadows-off`, `shadows-hard` and `shadows-soft`. It has two new
scenes, `ebon_hawk` and `dark_fight`, which run only when named.

Done when: the numbers and captures are in the PR, and the 1050 Ti tester has
run the same bench.

### 1. The spotlight, in today's pipeline

One shader change plus a visibility flag. Needs nothing from `feat/dlss`.

- Replace the clamp on the fight lights' sum with a soft shoulder, so the
  middle never goes flat.
- A falloff that is strong close by with a long soft tail, windowed to zero
  at the radius, and a lower gain.
- A bolt lights as a short line along its streak, so the pool on the floor
  stretches the way it flies.
- A fight light reaches only rooms that can see its own room (the VIS
  lists), so it stops lighting the far side of a wall.
- Characters take the same curve, so a bolt lights a trooper and the floor
  alike.

Done when: the corridor capture shows no disc, the bolt still reads in a lit
room, and frame time is unchanged.

Built on 2026-11-11 (`odyssey.wgsl` `fight_light` and `add_fight_light`).

- A fight's light falls off as an inverse square around a core whose size
  is 0.45 of its radius. It eases to nothing at the radius.
- Its strength is multiplied by 1.5. The old curve used 2.
- A flying bolt or tracer lights from a line up to 1.5 m long. The line
  stops at the gun and at the point where the bolt stops.
- Rooms and characters add a fight's light through the same soft shoulder.
  The shoulder replaces the clamp.
- A fight's light is kept out of the rooms not linked with its own. In
  Leviathan's hall, a bolt is kept out of 18 of the area's 31 rooms. Each
  room mesh carries its room as a `MeshTag`, so the rooms still share
  their materials.
- A bolt 1.2 m above a dark floor leaves about 0.55 red under it. One metre
  to the side it leaves 0.39, and two metres to the side 0.06. The old
  curve gave a flat 1.0 out to 1.45 m.
- The cost was measured in two alternating rounds on the 4070 Ti SUPER,
  with soft shadows. The dark fight ran at 131 to 134 fps both before and
  after. Its opaque pass took 1.22 ms before and 1.24 to 1.25 ms after. The
  lit fight ran at 189 to 197 fps before and 187 to 193 fps after. Its
  opaque pass took 1.25 to 1.28 ms before and 1.30 to 1.31 ms after. The
  main thread's time per frame did not change, so finding each light's
  room costs nothing measurable. Frame time is unchanged within the
  machine's run-to-run spread.
- Red blades used to tint a whole lit hall pink. Now the hall keeps its own
  colour, and the red stays around the blades. The owner played it and
  approved the look.

### 2. Capsule shadows

- Each character sends its capsules (`crates/ragdoll/src/auto.rs` already
  fits them) to the GPU each frame, behind one bounding sphere per character.
- The material traces a cone from the fragment to each light against the
  capsules near it: a soft shadow whose edge widens with distance from the
  caster, and contact darkening where a foot meets the floor.
- Every light in the list can cast, fight lights included: a bolt passing a
  trooper throws a moving shadow on the wall.
- The shadow removes that light's estimated share of the lightmap (its
  colour, falloff and facing from the light list), never more than the
  lightmap holds and never below the room's fill light. This replaces the
  three fixed darkness constants and the distance fades.
- The feet discs go.
- Placeables and doors that cast today need a stand-in (a box) or keep a map
  until step 4. To decide once we see how many matter.

Done when: this path, with every shadow view off, beats today's hard shadows
on CPU and GPU in all three scenes, and the two-lamp capture shows no jump.

The known weakness: a capsule has no fingers, weapon or robe. Close up, the
silhouette is too round. Step 4 fixes that for the characters near the camera.

Built on 2026-11-11 (`shadows.rs`, `play_view::cast_shadows`, and
`odyssey.wgsl`'s `room_shadow` and `cone_visibility`).

- Each character's ragdoll capsules are placed on its bones every frame.
  A humanoid has 15 capsules. A droid's boxes become capsules along their
  longest side. A body without a ragdoll casts as one capsule along its
  model's longest side, as wide as the mean of the other two sides.
- The capsules are cast 30% wider than they are fitted, plus 2 cm, so a
  shadow covers clothes and armour.
- The 32 characters nearest the camera cast, with at most 512 capsules in
  all. A character whose capsules do not fit in what is left is skipped,
  and the next one may still fit. Each character has one bounding sphere.
- A point traces the 6 characters nearest it among those within 6 m of
  it. The last 2 m of that reach ease the shadow away. A point outside one
  sphere holding every character's reach traces none. A fight's light too
  faint to show at a point is not traced there.
- A character's shadow falls only on the rooms linked with the room it
  stands in, as a fight's light does. It does not fall on a surface more
  than 2 m below its lowest point, which is another floor. It does not fall
  on a surface it stands behind.
- A light's source is 0.35 m wide across its half, and never narrower than
  0.08 radians. The sun's is 0.03 radians. A fight's light's is 0.15 m,
  and a bolt's is wider by half its line.
- Each character casts only from the lights that see it past the level's
  walls and floors. The line is tested from the lamp, or from a point at
  most 2 m above the chest under it, toward the chest. When that line
  starts inside something solid, such as the lamp's wall fixture, it is
  tested from the chest toward the lamp instead. A fight's light is not
  tested, as it moves every frame.
- Each test is kept until the character or the light moves half a metre,
  or until a door opens or closes. At most 48 lines are traced in a frame.
  A pair not yet traced casts no shadow until its turn comes, which
  spreads the tests on entering an area over a few frames.
- A room's lightmap is shared out among the lights that light the point,
  by their fixed-function weight. Bounced light weighs 0.05. A shadow takes
  the shares of the lights it blocks. It leaves at least 20% of the baked
  light. Outdoors the sky weighs 1.25 times the sun, so a sun shadow takes
  at most 44%.
- Many rooms are lit by panels the light list does not have. A wide source
  straight overhead, weighing 0.05, throws a soft shade under each
  character. Contact darkening of up to 70% shows where a body nearly
  meets a surface. These two replace the feet discs.
- A light within 0.4 m of a body's skin casts no shadow of that body. A
  welding torch's sparks, a lit blade or a muzzle flash would otherwise
  throw the body's own shadow wide over the floor about it. T3-M4's
  welding on the Ebon Hawk flashed that way every couple of seconds.
- A character casts no shadow on itself, and a placeable none on itself.
  Each character's and placeable's meshes carry their model in their
  `MeshTag`, as a room's meshes carry their room. A head or a weapon hung
  on a body carries the body's model.
- The three fixed darkness constants, the distance fades, the lamp slots,
  the ceiling lights, Bevy's shadow lights and the kept rooms maps are
  gone. No light draws a shadow map.
- The options' Soft Shadows now decides whether a fight's lights cast.
- A placeable casts as one box, from its model's bounds. Placeables are
  not in the lightmaps, so without a box a crate or a console would sit on
  the floor with no shadow at all. Only a placeable with a solid surface
  gets one, as only those cast before. A placeable no longer than 0.3 m, or
  longer than 8 m, gets none. A placeable lower than 0.5 m and lower than
  35% of its longest side gets none either. That is a body lying on the
  floor, a mat or debris, whose box would leave a dark rectangle.
- A placeable whose model covers less than half of its box, seen along any
  one of the box's three axes, gets no box either (#328). The model's
  triangles are drawn on a 16 by 16 grid over each face of the box, and the
  share of cells they cover is how much of that face the model fills. A
  pile of junk, a swoop bike, a speeder, a wreck, a plant, a table on legs
  or a figure standing up fills too little. Its box threw a solid block of
  shadow, and shaded the floor around it, where the model shows only a few
  thin pieces. Crates, lockers, barrels, tanks and statues fill enough. Of
  the placeable models that are neither too small, too large nor too flat,
  49 of 136 keep a box in KOTOR 1 and 94 of 229 in KOTOR 2.
- A placeable left without a box keeps the look of the original. The
  games' models mark each mesh as casting a shadow or not, and 191 of
  KOTOR 1's 215 placeable models mark none of their meshes. The Dune Sea's
  junk piles, swoop bikes and speeders are among them.
- The boxes are written to the GPU when a placeable appears, moves or
  goes, which in most areas is once. Each frame the 64 boxes nearest the
  camera are traced, at most 4 at one point and within 3 m of a box.
- A box's soft edge is estimated from its distance to the ray toward the
  light. The ray is clipped against the box's sides, and tested where it
  enters the box, at the middle and where it leaves, so a thin box is not
  missed. A point inside a box is not shaded by it. A box shades only the
  rooms linked with its own, like a character.
- Peragus has 22 placeables with a solid surface. 13 get a box and 9 are
  flat. The Endar Spire arena has 14, of which 11 are bodies lying down.
- The boxes were measured in two alternating rounds against step 2
  without them, with Soft Shadows on. In Peragus they added 0.19 ms to the
  opaque pass in both rounds. In the dark fight they added 0.08 to 0.17 ms.
  In the Ebon Hawk, with 2 boxes, the difference was within the runs'
  spread. Every scene stays well under step 1's hard shadows.
- After the review of #319 the cost was measured again, in two alternating
  rounds against step 2 without boxes. The machine ran in its fast state.
  Peragus's opaque pass took 0.81 to 0.83 ms instead of 0.72 to 0.74 ms,
  so the boxes and the room checks now add 0.09 ms there. The Ebon Hawk's
  took 0.81 ms instead of 0.67 to 0.70 ms. The dark fight's took 1.92 to
  1.98 ms instead of 1.94 to 2.05 ms. The main thread's time per frame did
  not change.
- A door gets no box. Closed, it stands in its wall, whose baked shadow is
  there already. Open, it is out of the way. A box popping on and off as
  it moves would look worse than none.
- The cost was measured in two alternating rounds on the 4070 Ti SUPER,
  against step 1 with hard shadows. Step 2 ran with Soft Shadows off, so
  the lamps and the sun cast. Peragus drew 4 views instead of 9, and the
  dark fight 6 instead of 8. The opaque pass took 1.30 to 1.35 ms in
  Peragus instead of 1.72 to 1.77 ms. It took 1.04 to 1.05 ms in the Ebon
  Hawk instead of 1.35 to 1.48 ms, and 2.34 to 2.40 ms in the dark fight
  instead of 3.00 to 3.06 ms. The render thread's graph took 1.55 ms in
  Peragus instead of 2.36 to 2.38 ms. The dark fight ran at 101 to 102 fps
  instead of 71 to 72 fps.
- With Soft Shadows on, a fight's lights cast too. The first build traced
  every fight's light at every point, and the dark fight's opaque pass took
  3.76 to 3.94 ms. Since faint lights are left out, it takes 1.87 to 2.00
  ms. In the same runs, Soft Shadows off took 2.28 to 2.31 ms. The runs with
  Soft Shadows on also ran faster, at 127 fps instead of 101. Nothing
  explains that yet beyond the machine's two speeds.
- The shadow pass's prepass shader (`odyssey_shadow.wgsl`) is gone, and
  the Odyssey material no longer asks Bevy for shadow passes.
- The two-lamp jump is gone by construction, as every light in the list
  casts at once. No capture checks it yet.
- Captures from above the player in Leviathan's hall, Peragus, the Ebon
  Hawk and the Endar Spire put step 2's shadow where step 1's was. It is
  softer and lighter. In Leviathan's hall the list's lamps hang outside
  the hall's walls, so the shade under the player comes from the overhead
  source alone.

### 3. HDR, bloom and glow

Built from main in one PR (`feat/hdr-bloom-glow`, 2026-11-12). Lighting is
a row on the advanced graphics page, in the place of KOTOR 2's Frame
Buffer Effects box. Classic is the default until the owner has looked at
the captures.

**The frame.** Under Enhanced lighting every camera on the world's target
carries `Hdr`, under every anti-aliasing choice. The main texture is then
`Rgba16Float`. Its values are gamma-encoded (decision 6), and a value may
be above 1. Under Classic lighting nothing changes. The frame is 8-bit and
stops at 1, except under DLSS, which needs the float texture.

**The roll-off.** A screen cannot show a value above 1. A pass of kjk's own
(`rolloff.rs`) brings the scene camera's frame back into 0 to 1. A channel
below 0.9 is left exactly as it is. Above 0.9 the channel bends toward 1
and never reaches it, so a brighter value always shows brighter. A stored
white of 1.0 comes out at 0.963. The pass runs after anti-aliasing and
before the video effect, the fade, the HUD and the screens, which all
still receive values from 0 to 1. Bevy's own tonemapper is not used. It
writes linear values and has no curve of this shape.

**Bloom.** Bevy's bloom, on the scene camera only, before the roll-off. Its
threshold is 1, so only what kjk makes brighter than 1 blooms. A white wall
does not. Bloom blurs encoded values, as the games' own frame buffer glow
did. Its strength is 0.25 (`rolloff::BLOOM_STRENGTH`).

**The new light.** The surface shader works out the base colour (lightmap,
fixed function lights, texture, env map) exactly as before. Under Enhanced
it then decodes the base, adds the fight lights in linear, and encodes the
sum. The fight lights' sum is read as a stored level of light before it is
decoded. So a pool on a dark floor looks as it does under Classic, and on
a lit floor it adds as one more lamp would. The fight lights no longer go
through the soft shoulder of step 1, which stood in for a tonemapper. Their
strength is 0.8 under Enhanced (`FIGHT_GAIN_ENHANCED`) and 1.5 under
Classic. A fragment that no fight light reaches keeps its stored value bit
for bit.

**Glow.** A lightsaber's core is drawn 2.5 times as bright
(`scene::BLADE_CORE_GLOW`, through `OdysseyMaterial::glowing`). A blaster
bolt or tracer is drawn 3 times as bright and a muzzle flash 2 times
(`blaster_view::BOLT_GLOW`, `FLASH_GLOW`, through their particles'
colour). The glow sprites around a blade's core are drawn as they are.
They overlap, so their sum already passes white in the float frame. Every
other effect is drawn as under Classic. An additive effect whose layers add
up past white also blooms, such as a deflection's flare.

**Highlights.** A surface with an env map mirrors a fight's lights. A bolt
lights from a line, so its highlight is a streak along the way it flies.
The mask is the one the env map already uses, one minus the texture's
alpha. Its strength is 6 and its tightness 60 (`HIGHLIGHT_GAIN`,
`HIGHLIGHT_POWER`). Both are first guesses.

What the encoded frame gives up:

- Bloom's halo falls off a little differently from real light. It is tuned
  by eye.
- DLSS and Bevy's TAA are made for linear colour. DLSS already received
  encoded colour before step 3, but never a value above 1. Bright bolts
  may ghost or sparkle under DLSS. It has not been run.
- Under MSAA, a resolve averages a pixel's samples before the roll-off. The
  edge of a very bright bolt can then stair-step.

No approved test compares rendered pixels, so nothing checked that Classic
stays the same. `kjk compare-pictures` now compares two pictures, and
`kjk bench` has the presets `classic` and `enhanced` to take them
(`docs/RUN.md`).

**The frame alone**, before bloom and glow were added.

The same picture. Headless runs took one picture at frame 240, at
1280x720, twice under Classic and once under Enhanced. Only the pixels
that are exactly the same in both Classic pictures were compared, because
nothing moved there. On those pixels, the channels at or below 229 of 255
(the knee) were compared.

| Scene | Anti-aliasing | Pixels compared | Mean difference | Pixels differing by more than 2 |
|---|---|---|---|---|
| Cantina (`tar_m03ae`) | 4x | 98.2% | 0.064 of 255 | 0.011% |
| Cantina | TAA | 88.0% | 0.199 of 255 | 0.526% |
| Endar Spire (`end_m01aa`) | 4x | 42.9% | 0.012 of 255 | 0.083% |

At 4x, 99.99% of those channels in the cantina differ by 0 or 1. The 1 is
rounding. Classic rounds each sample to 8 bits before the samples are
averaged, and Enhanced rounds once at the end. The few larger differences
sit on the breathing characters. Under TAA more channels differ by 2 or 3,
because Bevy's TAA blends a float frame through a curve of its own. Above
the knee a Classic value of 250 came out 5 lower on average. The Dune Sea
cannot be compared this way, because its dust blows over the whole frame.

The cost on the 4070 Ti SUPER, with `kjk bench --gpu` at 1920x1080, two
runs of each preset, Classic and Enhanced taking turns:

| Scene | Anti-aliasing | Classic fps | Enhanced fps | Classic GPU ms | Enhanced GPU ms |
|---|---|---|---|---|---|
| Peragus hangar | 4x | 231, 249 | 247, 249 | 0.90, 0.99 | 0.98, 0.95 |
| Ebon Hawk | 4x | 218, 218 | 216, 216 | 1.20, 1.20 | 1.16, 1.16 |
| Dark fight | 4x | 170, 171 | 170, 172 | 1.49, 1.48 | 1.48, 1.52 |
| Peragus hangar | TAA | 245, 243 | 240, 241 | 1.14, 1.13 | 1.16, 1.15 |
| Ebon Hawk | TAA | 209, 210 | 208, 207 | 1.14, 1.18 | 1.37, 1.37 |
| Dark fight | TAA | 173, 171 | 168, 170 | 1.53, 1.58 | 1.65, 1.61 |

The GPU milliseconds are all timed passes of a frame together. The
roll-off itself takes 0.02 to 0.04 ms. The float frame costs this card at
most 0.2 ms of GPU time a frame, in the Ebon Hawk under TAA, and 1 to 2%
of the frame rate.

**With bloom and glow.** The cantina was compared again the same way at
4x. Classic from this build against Classic from before the glow differs
by a mean of 0.005 of 255, and 0.028% of the pixels differ by more than 2,
all on the breathing characters. Enhanced against Classic differs by a
mean of 0.066 of 255 below the knee, and 0.024% of the pixels differ by
more than 2. So Classic is unchanged, and a scene with no fight in it
looks the same under Enhanced.

The cost on the 4070 Ti SUPER, measured the same way:

| Scene | Anti-aliasing | Classic fps | Enhanced fps | Classic render thread ms | Enhanced render thread ms |
|---|---|---|---|---|---|
| Peragus hangar | 4x | 245, 241 | 224, 222 | 3.46, 3.53 | 3.93, 3.98 |
| Ebon Hawk | 4x | 212, 211 | 191, 191 | 4.21, 4.24 | 4.72, 4.73 |
| Dark fight | 4x | 169, 167 | 159, 156 | 4.92, 5.00 | 5.49, 5.59 |
| Peragus hangar | TAA | 234, 234 | 214, 215 | 3.64, 3.65 | 4.13, 4.12 |
| Ebon Hawk | TAA | 205, 203 | 184, 184 | 4.36, 4.41 | 4.92, 4.91 |
| Dark fight | TAA | 163, 164 | 149, 156 | 5.08, 5.08 | 5.74, 5.65 |

Enhanced costs 6 to 10% of the frame rate on this card. Almost all of it
is bloom. Bloom draws 16 small passes, and setting them up takes the
render thread about 0.5 ms a frame. On the GPU it adds 0.1 to 0.4 ms.
Fewer passes (`Bloom::max_mip_dimension`) would cost less and give a
tighter glow. That has not been tried.

The bench also found a crash. Under Enhanced in a window, starting a
stage removed the menu's camera in the same frame that `temporal::apply`
gave it the float frame. The change is now skipped for a camera that is
gone.

A playtest found a second crash, which main already had since #342. The
game quit when anti-aliasing was switched from TAA to 4x, 2x or Off, under
Classic as well. Bevy gives a camera that draws motion vectors a
background motion pass and never takes it back. The scene camera keeps its
depth pass when it leaves TAA, so that pass still ran with no motion
texture to write to. `temporal::forget_background_motion` now takes the
pass off such a camera.

Not done: the 1050 Ti has not run it. DLSS was not run, since this build
has no DLSS. The glow, bloom, light and highlight strengths are first
values chosen from a few captures. Force lightning, shields, fog and
distortions have not been looked at under Enhanced.

Done when: Classic and Enhanced pictures of a still scene differ only above
the roll-off's knee and where particles moved; the effect families are
signed off one by one; the float frame's and bloom's costs are measured on
the 1050 Ti.

### 4. Sharp silhouettes: skin once, a fitted atlas

The High setting. This is the largest piece of engineering in the plan: it is
a render node of our own beside Bevy's view machinery.

- A compute pass skins each character once per frame into a vertex buffer.
  Every later use reads that buffer.
- A shadow atlas with one tile per (light, caster) pair, each fitted to the
  caster's bounds, so a character keeps the same resolution wherever it
  stands. All tiles draw in one pass of ours, not one Bevy view per light.
- Tiles for casters that have not moved are kept between frames.
- Only characters near the camera, under the lights that matter most on
  screen, get a tile. Everyone else keeps capsules, and the two blend.
- 4 to 8 taps per tile, and none outside a tile's bounds.
- Under TAA or DLSS, where the prepass exists: a short screen-space trace
  for contact shadows, and optionally the shadow term worked out once per
  pixel in a screen-space mask and filtered over frames. Under MSAA these are
  off.

Done when: High costs less than the soft shadows main had when this plan was
written, in all three scenes, and close-up captures show hands and weapons
in the shadow. Step 2 took those soft shadows out of the game. Since then
the tiles are measured against the capsules, which are what High drew
before the tiles.

Where it stands (2026-11-12, `feat/shadow-tiles`). Built differently from the
list above in these ways:

- The tile pass skins each mesh in its own vertex shader, once for each
  tile.
- A compute pass that skins once is built, and is off by default
  (`skin_once.rs`, `feat/skin-once`). `KJK_SKIN_ONCE=on` turns it on. It
  skins the characters that have tiles, and no others. It places each of
  their skinned meshes and shadow meshes in the world once a frame, in
  one buffer of four floats a vertex. The tile pass then draws those
  vertices into each of the character's tiles.
- The compute pass costs more than it saves on the 4070 Ti SUPER. It was
  measured headless at 1920x1080 with the pass on and off taking turns.
  The tile pass's own steps took 52 microseconds a frame on the CPU in
  the Ebon Hawk, against 47 without it. They took 56 against 52 in the
  dark fight, 88 against 81 in the Dune Sea and 36 against 30 in the
  Peragus hangar. On the GPU the tile pass took 0.043 ms against 0.028
  in the Ebon Hawk, 0.032 against 0.027 in the dark fight, 0.082 against
  0.054 in the Dune Sea and 0.030 against 0.021 in Peragus. A character
  has a few thousand vertices and at most 3 tiles, so skinning it again
  for each tile costs less than one more pass. The GTX 1050 Ti and the
  M4 Pro have not run it.
- The tile pass is the buffer's only reader. The main pass and the depth
  pass still skin a character with Bevy's own shader. Reading the buffer
  there needs a vertex shader of our own for every skinned surface.
- The joint matrices are made from the bones on the CPU. Bevy makes its
  joint matrices only for meshes a camera sees, and a character behind the
  camera still casts into view.
- The compute pass reads the meshes from Bevy's vertex buffers, which are
  made with storage usage when it is on. A mesh it cannot read is skinned
  in the tile pass's vertex shader.
- The 12 characters nearest the camera get a tile under each of the 3 lamps
  or suns that light them most. Those tiles are drawn every frame, in the
  atlas's layer 0. A mesh is drawn once for all its tiles.
- Each model of a character gets shadow meshes when it is spawned. A shadow
  mesh is the model's meshes that cast, joined into one mesh for each
  texture that cuts them, so a model takes one draw call or two and not one
  for each mesh. It is drawn while its meshes are all shown and as they were
  spawned. When one of them is hidden, cut or swapped, the model's meshes
  are drawn one by one again. A danglymesh is always drawn by itself.
- The characters' draw calls are kept from frame to frame. They are made
  anew only when the list of meshes and tiles changes. Every frame, only
  the tiles' views and the matrices are written.
- The placeables' tiles are the ones kept between frames, in layer 1. Each
  placeable gets one under each sun and under the 2 lamps that light it
  most, chosen from all the area's lights. Layer 1 is drawn again only when
  a placeable appears, moves or goes. A placeable too sparse for a box
  (a junk pile) now casts from its tiles. A surface whose texture's alpha
  is its coverage is cut by it.
- Each tile lookup first reads the depths around the point, and softens
  the edge by how far the shadow falls behind what casts it. A shadow is
  sharp where it meets what casts it.
- No screen-space contact trace.

What it costs, on the 4070 Ti SUPER with Shadows at High and MSAA 4. The
full tables are in #351 and #353.

- The tiles do not cost less than the capsules on this card. That is the
  step's goal, so the step is not done.
- On the GPU at 3840x2160, the opaque pass with tiles and with capsules
  differs by less than 0.2 ms. Two runs of one setup differ by that much.
  Drawing the tiles takes 0.01 to 0.04 ms.
- On the CPU at 1920x1080, the tile pass's own steps take 34 microseconds
  a frame in the Peragus hangar, 46 in the dark fight, 52 in the Ebon Hawk
  and 87 in the Dune Sea. Before the shadow meshes and the kept draw calls
  (#353) they took 58, 96, 125 and 260.
- Before #353 the render thread was 0.17 to 0.28 ms a frame slower than
  the commit before the tiles. The whole frame has not been measured
  since.
- The GTX 1050 Ti and the M4 Pro have not run it. The 1050 Ti is the
  machine that decides.
- No approved test covers the tiles. No picture checks a double-bladed
  lightsaber's shadow. The game was not played in a window.

### 5. Characters lit by the room

- Light probes built from the lightmaps when an area loads, so a character
  takes the colour of where it stands.
- Part of the new look, off in Classic (the games light characters from the
  area's ambient and the light list).

As built (`light_probes.rs`, on `feat/light-probes` from step 4's
`feat/skin-once`):

- A probe is a point 1 m above a room's walkable floor. Probes stand on a
  grid 1 m apart, room by room. A probe holds six colours. Each is the
  light arriving on a surface that faces east, west, up, down, south or
  north. The shader weighs the six by a surface's normal.
- The probes are baked as the area loads, on the loading screen's worker
  thread, shared among the machine's threads. Each direction casts five
  rays: one straight, and four leaning 0.6 radians off it.
- The light from above is read off the floor. Its rays go down, up to 3 m,
  and take the lightmap of the floor they meet. A lightmap texel is the
  light arriving on its surface, and the floor lies right under the probe.
- Each of the other five sides looks the way it faces, up to 20 m. Its rays
  take the colour of the room surfaces whose fronts they meet, as the
  screen shows them: the texture times its lightmap and its own glow. A
  room mesh without a lightmap is drawn at full light, and is seen so. So
  a side facing a dark corridor stays dark, and a side facing a lit
  doorway takes the doorway's light.
- Textures and lightmaps are read at a mip level no larger than 128 texels.
  A side that sees nothing takes the mean of the sides that did. A
  surface's facing comes from its triangle's corners, because some models
  store no normals.
- The first build read the sides the other way. It cast each side's rays
  away from the way it faced and took the lightmap of the surfaces facing
  back. That is the light arriving at those surfaces, not at the probe.
  Through a lit doorway it put the hall's light on the side facing away
  from the door (the owner's report from the cantina's entry, 2026-11-13).
  At that doorway the sides held 104 to 149 of 255, more than the 50 from
  above. They now hold 21 to 83, and the side facing the door no longer
  takes the corridor's.
- An area with no lightmapped room mesh, such as the Dune Sea, gets no
  probes. Everything there keeps the flat ambient.
- Each frame every placed model that is not a room gets the blend of the
  probes within 1.6 m of the point 1 m above its origin, in the room it
  stands in (`FightRooms::room_of`). Light never blends across rooms. A
  room with no probe near hands over to the rooms linked with it. A head or
  a weapon hung on a body is lit by the body's light. A weapon armed during
  play takes its body's number once it is hung.
- The lamps are not counted twice. The floor's lightmap holds the lamps'
  light, so each lamp of the shader's list that is the scene's own (not a
  carried light, a body's light or a fight's light) is taken out of the
  light from above by half of what the shader adds for it to a surface
  facing up, never below black (`LAMP_SHARE`). A lamp is a point, which no
  side sees, so the sides keep their light. A lamp too far from the camera
  to be in the list stays in the probe light. `KJK_LAMP_SHARE` sets another
  share, from 0 to 1, to compare pictures.
- The light eases over 0.25 s, so a doorway does not pop. A model is worked
  out again only when it moves 5 cm or the list's lamps change.
- The shader's list (`AREA_PROBES`, material binding 16) holds 1024 entries,
  one per model number. A model numbered 1024 or above keeps the flat
  ambient. Only `odyssey.wgsl` declares the binding.
- Materials made for an area's models carry `PROBED`. Under Enhanced
  lighting such a mesh without a lightmap, and not static, takes the probe
  light in place of the area's flat ambient. The probe light never goes
  below 35% of the flat ambient (`PROBE_FLOOR`, to be tuned with
  captures). A static placeable takes no ambient at all, as before.
- `KJK_PROBES=off` and `KJK_PROBES=only` are the switches (`docs/RUN.md`).
  A dump's `light_probes` fact gives the area's probes and how many have
  light, the bake's time and triangles, the models lit by probes, the ones
  left on the flat ambient and why, and the meshes' own ambient colours.
  The launch report shows the bake as `kotor stage/scene/light probes`.
- The triangle grid the rays walk covers only the box the probes' rays can
  reach. A backdrop's triangles span hundreds of metres. Filled whole, the
  grid took 23 seconds to make in the Ebon Hawk.
- A lamp that darkens (a negative colour) takes nothing out of the probe
  light. Taking out its negative share made a Peragus door five times too
  bright.

Measured on 2026-11-13 with the first build's reading of the sides, on the
4070 Ti SUPER machine, from headless runs at
1280x720 with MSAA 4 and Shadows at High:

| Area | Probes | Bake | Models lit by probes | Kept the flat ambient | Sides the lamps took to black |
|---|---|---|---|---|---|
| Ebon Hawk (`tsl-14`) | 1060 | 31 to 37 ms | 20 | 4 (no probe near) | 15 of 120 |
| Peragus hangar (`tsl-13`) | 6929 | 50 ms | 88 | 0 | 0 of 528 |
| Cantina (`tar_m03ae`) | 1263 | 19 ms | 65 | 1 (no probe near) | 39 of 390 |
| Dune Sea (`tat_m18aa`) | 0 | 4 ms | 0 | all | |

- Every character and placeable mesh in those areas carries its own
  ambient colour at 1.0, but for a few at 0.2 in the hangar and the cantina.
  So the probe light is not weakened there.
- Classic is unchanged. Against `feat/skin-once`, with `--still`, 9 pixels
  of the cantina and 129 of the Ebon Hawk differ by more than 2 (0.001% and
  0.015%). Enhanced with `KJK_PROBES=off` is unchanged the same way: 728
  pixels of the cantina and 141 of the Ebon Hawk (0.08% and 0.017%). Two
  runs of `feat/skin-once` itself differ in 0.14% of the cantina's pixels.
- The Dune Sea cannot be compared by pictures, because its dust moves. Its
  dump shows no probes and no model lit by them.
- A walk from the cantina's entry corridor through the lit doorway into
  its hall, recorded 30 frames a second with `KJK_PROBES=only`, changes the
  player's light by at most 16 of 255 from one frame to the next.
- The lamp estimate is the open question. The shader adds a lamp's colour
  times its multiplier times at least a quarter anywhere inside its radius,
  and that is often more than the lightmaps hold (0.1 to 0.2 in the Ebon
  Hawk and the cantina). So the side of a model facing a lamp often loses
  all its probe light, and the cantina's entry corridor leaves the player
  no probe light at all. Under Enhanced the floor (35% of the flat
  ambient) and the lamps themselves still light that side. The table above
  and these numbers were taken with all of each lamp's share taken out.
- The owner compared shares of 0, 0.25, 0.5 and 1 in the cantina, the Ebon
  Hawk and four framings of the Peragus hangar, and chose 0.5 on
  2026-11-13. At 1 the lamp-heavy places looked almost as they do with
  probes off. At 0 the lamps count twice. The Ebon Hawk barely changes at
  any share.
The cost on the 4070 Ti SUPER, with `kjk bench --preset enhanced --gpu`
at 1920x1080, `KJK_PROBES=off` and on taking turns, two runs of each:

| Scene | Off fps | On fps | Off main ms | On main ms | Off opaque GPU ms | On opaque GPU ms |
|---|---|---|---|---|---|---|
| Peragus hangar | 210.9, 211.3 | 209.2, 208.5 | 3.47, 3.46 | 3.53, 3.54 | 0.71, 0.71 | 0.72, 0.72 |
| Cantina | 245.1, 245.1 | 244.2, 244.6 | 3.28, 3.29 | 3.31, 3.29 | 0.78, 0.78 | 0.78, 0.78 |
| Ebon Hawk | 184.5, 184.6 | 183.5, 182.9 | 3.09, 3.09 | 3.15, 3.14 | 0.93, 0.93 | 0.93, 0.94 |
| Dark fight | 159.0, 158.8 | 159.7, 158.3 | 5.36, 5.39 | 5.35, 5.38 | 1.24, 1.27 | 1.27, 1.23 |

- The probe light costs the main thread about 0.07 ms a frame in the
  Peragus hangar, which has 88 probed models, and 0.05 ms in the Ebon Hawk.
  That is up to 1% of the frame rate.
- The shader's opaque pass costs the same within 0.01 ms.
- The 1050 Ti has not run it.

With the sides read the way they face (all of the above was the first
build's reading), the bake takes 24 ms in the cantina, 46 ms in the Ebon
Hawk, 62 ms in the Peragus hangar and 124 ms on Dantooine (`danm14aa`,
24,976 probes), because the sides look up to 20 m. The lamps now take 1 of
120 sides to black in the Ebon Hawk, 5 of 390 in the cantina, 0 of 528 in
the hangar and 6 of 342 on Dantooine. A side sees only the light the room
throws back at it, so characters and props in dim places are darker than
with the first reading. That makes the floor (35% of the flat ambient)
matter more. The frame's cost was not measured again; the shader and the
per-model work did not change.

### 6. Ray tracing and other upscalers

Planned on 2026-11-13. Nothing in this step is built or measured.

The step has two halves that do not depend on each other. The first half
is an upscaler for every card. The second half is ray-traced shadows for
the cards that can trace rays. Each part below is its own branch and PR.

**What the steps above leave ready.**

- Step 4's compute pass (`skin_once.rs`) places a character's skinned
  vertices in the world once a frame. A vertex is four floats, and the
  first three are its position. Acceleration structures for characters
  are built from that buffer. Today the pass covers only the 12
  characters that have tiles, and it is off by default.
- Step 2's `light_visibility` in `odyssey.wgsl` returns one value for
  each light and pixel. A shadow ray gives that same value.
- The depth and motion prepass, the jitter and the float frame (#323 and
  step 3) are what an upscaler takes in.
- The lightmaps and step 5's probes are what ray-traced bounce light
  would replace later.

**What Bevy 0.19.1 and wgpu 29.0.4 give us.** Checked in their sources on
2026-11-13.

- wgpu has ray queries behind the feature `EXPERIMENTAL_RAY_QUERY`. Its
  Vulkan backend offers it on a card with `VK_KHR_ray_query`. Its DX12
  backend offers it on a card with ray tracing tier 1.1 and shader model
  6.5. Its Metal backend offers it on macOS 15 or later. wgpu's own
  notes still name Vulkan alone and call the feature experimental.
- Bevy turns wgpu's experimental features on for every app. kjk only has
  to ask for the feature when the card offers it.
- wgpu cannot refit an acceleration structure. Its `PreferUpdate` mode
  logs "only rebuild is implemented", in 29.0.4 and in 30.0.1 alike. So
  each animated character's structure is built anew every frame. What
  that costs is the largest unknown in this step.
- A shader can walk a ray's candidate hits and accept or reject each
  one (`rayQueryGetCandidateIntersection`,
  `rayQueryConfirmIntersection`). A texture that cuts a mesh, such as
  hair or a grate, can then cut its shadow too.
- Bevy's ray-traced lighting (`bevy_solari`) cannot light kjk. It takes
  only Bevy's `StandardMaterial`. It takes only meshes that carry
  positions, normals, texture coordinates and tangents with 32-bit
  indices. It builds each structure once from a mesh asset, so it has no
  skinned characters. Its lighting replaces all of a scene's direct and
  bounced light, where kjk keeps the games' lightmaps. kjk builds its
  own structures and traces its own rays.
- Bevy's `AsBindGroup` derive has no attribute for an acceleration
  structure. The Odyssey material's bind group needs a hand-written
  entry, or the structure lives in a bind group of kjk's own.
- Bevy ships FXAA, SMAA, TAA, a sharpening pass and DLSS. It has no FSR,
  no XeSS and no MetalFX. Bevy's TAA always resolves at the size the
  frame was drawn at, so it cannot upscale. NVIDIA's DLSS crate
  (`dlss_wgpu`) works on Vulkan alone.

#### 6a. An upscaler for every card

This part comes first, because it helps the GTX 1050 Ti, the machine
that decides. DLSS is the only way to draw a smaller frame today, and it
needs an RTX card.

- A new anti-aliasing choice, TAA with upscaling. It has the same four
  sizes as DLSS: two thirds, 58%, a half and a third of the window's
  side. The scene camera draws at that size through
  `MainPassResolutionOverride`, as it does under DLSS. The HUD and the
  screens still draw at the window's size after the resolve.
- A resolve pass of kjk's own, in the place of Bevy's TAA node for this
  choice. It keeps its history at the window's size. Each frame it
  places the small jittered frame into that history, guided by the
  motion vectors and the depth. It clamps the history to the colours
  around the pixel, which is what removes ghosting. It drops the history
  on a cut, through the same `TemporalCut` as today.
- The resolve works on the encoded frame, as Bevy's TAA does today under
  decision 6. Bloom and the roll-off run after it, at the window's size.
- Bevy's sharpening pass (`ContrastAdaptiveSharpening`) runs after the
  resolve. Its strength is one constant, tuned with captures.
- The mip bias is log2 of the size's share, as under DLSS.
- `Antialias` gets one variant for an upscaler and its size, with DLSS
  and kjk's own resolve as the first two upscalers. A later FSR, XeSS or
  MetalFX is then one more upscaler and no new plumbing.

Done when: at a half of the window's side, a 1920x1080 picture of a
still scene is closer to the full-size TAA picture than a plain
stretched one is, by `kjk compare-pictures`. A recorded fight shows no
trails behind bolts and blades. The opaque pass's GPU time falls roughly
with the pixel count in the Peragus hangar, the Ebon Hawk and the dark
fight. The 1050 Ti tester has run it.

The risks of 6a:

- The depth and motion prepass costs the CPU 5 to 9% of the frame rate
  on the 3700X (#323). On a machine held back by its CPU, a smaller
  frame gains nothing. The 1050 Ti run shows which side holds that
  machine back.
- Particles, glows and blended sheets write no motion. They smear more
  when the frame is upscaled than they do under TAA at full size.
- Thin geometry, such as railings and antennae, flickers at a third of
  the window's side. The two smallest sizes may not be worth offering.

Where it stands (2026-11-14, `feat/taa-upscaler`, `upscale.rs`). Built,
measured on the RTX 4070 Ti SUPER and committed (30c52863 and 89f93983).
It is not pushed. The worktree `.claude/worktrees/taa-upscaler` is cut
from main and does not hold 6c to 6e.

- **What is built.** The options offer kjk's own upscaler as "TAA:
  Quality", "TAA: Balanced", "TAA: Performance" and "TAA: Ultra
  Performance" (`taa_quality` and so on in the settings file). They are
  the same four sizes as DLSS. `Antialias` is now `Msaa`, `Taa` or
  `Upscale(Upscaler, Scale)`. TAA at the window's size is still Bevy's
  TAA. A DLSS setting on a card without DLSS falls back. DLAA becomes
  TAA, and a smaller DLSS size becomes kjk's upscaler at the same size.
- **The resolve.** It keeps a float history at the window's size. It
  follows the nearest surface's motion vector. It clamps the history to
  the colours of the smaller frame around the pixel. The clamp is 1
  standard deviation wide for a moving pixel, and widens to 2 once the
  pixel has stood still for 4 frames. A sample counts for more the
  nearer it fell to the pixel's centre. The jitter has more positions
  for a smaller frame, 8 at full size, 32 at a half and 72 at a third.
- **The dither at a third of the side is fixed.** The 3x3 drawn pixels
  the clamp reads move with the jitter, and at a third of the side they
  span 9 shown pixels. A clamp of 1 standard deviation pulled a still
  pixel to a different colour each frame. That showed as a fine dither
  on blades and armour. The wider clamp for still pixels removed it and
  brought every size closer to full-size TAA.
- **The still picture.** The scene is the Taris cantina at 1920x1080.
  The HUD is masked out, and only the pixels that two full-size TAA runs
  agree on are counted. The mean difference to full-size TAA is 0.17 at
  Quality, 0.21 at Balanced, 0.25 at Performance and 0.44 at Ultra
  Performance. A picture drawn at a half of the side and stretched
  differs by 0.40. One drawn at a third and stretched differs by 0.64.
- **The cost.** The bench ran in a window at 1920x1080 with the presets
  `taa`, `taa-quality`, `taa-balanced`, `taa-performance`, `taa-ultra-
  performance` and `dlss-performance`. Each preset ran twice in the
  Peragus hangar, the Ebon Hawk and the dark fight. The opaque pass's
  fragments fall exactly with the pixel count, from about 2.2 million to
  0.27 million at a third of the side. The opaque pass's time falls by
  30 to 85%, but it varies by up to two times between rounds. This
  machine is held back by its CPU at every size, so its card is mostly
  idle and its clock wanders. The frame rate does not change with the
  size. The resolve costs 0.17 to 0.46 ms and the sharpening 0.04 to
  0.10 ms. Bevy's TAA costs 0.13 to 0.15 ms. The output is in the
  worktree's `target/upscale-checks/`.
- **Sharpening.** The upscaler keeps its own pass
  (`upscale_sharpen.wgsl`) at a strength of 0.35. Bevy's
  `ContrastAdaptiveSharpening` hid no HUD in the frames tried the second
  time. It is still not used, for three reasons. It runs with no order
  against the grade and the screens' composite, which is the race
  `video_effect.rs` describes. It expects values from 0 to 1, and
  Enhanced lighting's frame goes above 1. It may also sharpen bloom.
  `KJK_SHARPEN` sets the strength for a run.
- **Motion.** In about 1.5 s of consecutive frames of the dark fight,
  swinging blades show no trail. At Performance and below, the edges of
  moving characters show stair steps the size of a drawn pixel. Force
  lightning leaves faint copies of its last bolts for a few frames at
  Ultra Performance, because blended effects write no motion.
- **A crash at exit.** Every run of the DLSS build ended with exit code
  0xC0000005 after `quit`, once its output was written. Nobody has
  looked into it.
- **Owed.** The 1050 Ti run, which shows whether a card held back by its
  GPU gains what the pixel count promises. A way for particles and
  lightning to tell the resolve to trust its history less.

FSR, XeSS and MetalFX are not planned. FSR and XeSS are C++ libraries
that need glue to wgpu's raw Vulkan and DX12 handles, the way
`dlss_wgpu` is glued. MetalFX needs the same on Metal. One of them is
worth porting only if kjk's own resolve does not look good enough on the
cards without DLSS.

#### 6b. A trial of ray queries

A short branch that is not merged. It answers the questions the rest of
the step depends on, on the 4070 Ti SUPER.

- Which backend kjk runs on in Windows, and whether the card offers
  `EXPERIMENTAL_RAY_QUERY` there. DLSS needs Vulkan, so Vulkan is the
  one that matters first.
- Whether the Odyssey material can bind an acceleration structure, and
  whether the shader still compiles for a card without ray queries. The
  ray code has to sit behind a shader definition.
- What it costs to build the structures of 32 characters anew each
  frame. The trial builds them from the skin-once buffer with
  `PREFER_FAST_BUILD` and times the build on the GPU and the CPU.
- What one shadow ray for each light and pixel costs in the opaque pass
  at 1920x1080 and at 3840x2160, against the capsules and the tiles.
- Whether rejecting a candidate hit works on this card, for the cut
  textures and for a character's own body.

Done when: those five answers are written here. The owner then decides
whether 6c to 6e are built. If building the characters' structures costs
more than about 1 ms a frame, the step stops here until wgpu can refit.

Where it stands (2026-11-13, `feat/ray-query-trial`, `ray_trial.rs`).
`KJK_RAY_TRIAL` turns the trial on, with the values `build`, `rays` and
`reject`. All five questions have an answer on this card. The answers
about cost come from headless runs, which have a weakness that is told
below.

- The backend and the card. On Windows kjk runs on Vulkan on the RTX
  4070 Ti SUPER, with driver 621.14. The card offers ray queries there.
  Bevy asks the card for every feature it offers, so kjk asks for
  nothing itself.
- The material. The Odyssey material can bind an acceleration
  structure. Bevy's own material bind groups cannot hold one, but Bevy
  lets a material make its bind group itself (`as_bind_group`). The
  trial keeps Bevy's derive on a twin of the material's bound fields and
  adds the structure at binding 17. The cantina and the Endar Spire ran
  that way with no error from Bevy or wgpu.
- The shader on other cards. This is not settled. The line
  `enable wgpu_ray_query;` has to be the shader's first line. Bevy's
  shader composer reads such lines before it reads `#ifdef`, so the line
  cannot be left out for a card without ray queries. No such card has
  run the trial. If the line fails there, the ray code moves to a second
  shader file that only cards with ray queries load.
- Rejecting a candidate hit. It works on this card. With `reject` the
  shader walks every candidate hit and accepts each one itself, and the
  rooms show the same shadows as with `rays`. The trial rejects a hit
  nearer than 0.35 m. Rejecting a hit by whose body it is has not been
  tried, because the trial's instances do not carry their owner's number
  yet. A cut texture has not been tried either.
- The picture. In the cantina, with the debug view that draws the
  rooms' shadows alone, a character under a low lamp throws long thin
  shadows of its legs with `rays`. The tiles show only the shade at its
  feet there. The ray's edge is hard, as one ray with no jitter gives.
- `KJK_TILE_CASTERS` sets how many characters get tiles, up to 21. The
  skin-once pass places only those, so the trial cannot build more than
  21 characters' structures yet.

What it costs, on the 4070 Ti SUPER on 2026-11-13. `kjk bench --gpu
--headless` ran each scene for 10 seconds with Shadows at High and MSAA
4. Today's tiles, the build alone, the rays and the rays with rejection
took turns, twice at each size. Each cell holds the two rounds.

The opaque pass on the GPU, in milliseconds:

| Scene | Size | Tiles | Rays | Rays with rejection |
|---|---|---|---|---|
| Peragus hangar | 1920x1080 | 2.79, 3.03 | 2.85, 2.86 | 2.97, 2.79 |
| Ebon Hawk | 1920x1080 | 3.04, 3.01 | 2.80, 2.74 | 2.90, 2.92 |
| Dark fight | 1920x1080 | 2.63, 2.97 | 2.54, 2.63 | 2.77, 2.76 |
| Peragus hangar | 3840x2160 | 3.89, 3.84 | 2.82, 3.27 | 3.32, 2.91 |
| Ebon Hawk | 3840x2160 | 3.79, 3.76 | 2.84, 3.18 | 3.26, 2.84 |
| Dark fight | 3840x2160 | 4.07, 4.14 | 2.96, 2.52 | 2.93, 2.98 |

The build of the structures on the GPU, in milliseconds, with rays on:

| Scene | Structures | Triangles | 1920x1080 | 3840x2160 |
|---|---|---|---|---|
| Peragus hangar | 1 | 1,555 | 0.77, 0.81 | 0.24, 0.27 |
| Ebon Hawk | 3 to 4 | 5,577 to 7,109 | 0.79, 0.80 | 0.26, 0.28 |
| Dark fight | 5 to 7 | 7,627 to 13,488 | 0.82, 0.79 | 0.29, 0.24 |
| Dark fight, 21 characters with tiles | 14 | 24,900 | 0.87, 0.88 | not run |

- One ray for each light and pixel costs no more than the tiles'
  lookups. At 1920x1080 the opaque pass takes the same time within the
  rounds' spread. At 3840x2160 it takes 0.6 to 1.6 ms less with rays.
- That comparison flatters the rays. The trial's structure holds only
  the characters, so a ray skips the placeables' boxes and kept tiles
  that the tiles' side still pays for. The ray is also one hard ray
  with no jitter.
- Walking the candidate hits costs little. The opaque pass with
  rejection is within 0.25 ms of the pass without it, on either side.
- The build costs 0.8 to 0.9 ms at 1920x1080 and 0.2 to 0.3 ms at
  3840x2160. The count of characters barely moves it. One character
  with 1,555 triangles costs about 0.8 ms, and 14 characters with
  24,900 triangles cost about 0.88 ms. So nearly all of it is a fixed
  cost for each frame, and a character adds about 0.01 ms. What the
  fixed cost is was found afterwards, and is told below.
- The same build reads four times cheaper at 3840x2160, which cannot be
  right. A headless run is paced at 60 frames a second. At 1920x1080
  the card is idle most of each frame and seems to slow its clock. The
  opaque pass shows the same thing, as it grows only 1.4 times for four
  times the pixels. A run in a window with v-sync off settled it, and
  is told below. Both columns here read too high.
- All the timed passes together took 0.7 to 1.0 ms more with rays than
  with tiles at 1920x1080, which is the build. At 3840x2160 they took
  0.1 to 0.5 ms less.
- On the CPU, handing the builds to the GPU takes 17 microseconds a
  frame for one structure and about 31 for 14. The render thread as a
  whole took 0.1 to 0.5 ms a frame more with rays than with tiles. That
  includes the skin-once pass, which the tiles' side had turned off.
  The main thread did not change.
- 32 characters were not measured. At 0.01 ms for each character they
  would cost about 1.1 ms at 1920x1080 by the slow clock's reading, and
  about 0.4 ms by the 3840x2160 reading. This is a guess from 14, and
  the windowed runs below give a better one.

The fixed cost, found later on 2026-11-13. The build was split into
its parts, each timed by itself, and run in a 1920x1080 window with
v-sync off as well as headless. In a window the card runs at full
speed, so these are the true costs. Two rounds agreed to within
0.02 ms.

| Part of the build, GPU ms in a window | Peragus hangar, 1 structure | Dark fight, 5 to 7 structures |
|---|---|---|
| The characters' structures | 0.21 | 0.26 to 0.27 |
| The top-level structure | 0.03 | 0.05 |
| An empty timed span | 0.001 | 0.001 |
| The whole build | 0.25 | 0.31 to 0.33 |

- The fixed cost is the first character's structure. Building one
  structure of 1,555 triangles takes 0.21 ms. Building 5 to 7 takes
  0.26 to 0.27 ms. So the card charges about 0.2 ms to start building
  bottom-level structures in a frame, and about 0.01 ms for each
  character after that.
- It is not the top-level structure. That takes 0.03 to 0.05 ms. Room
  for 16 instances in place of 256 changed nothing.
- It is not the timing itself. An empty span reads 0.001 ms.
- When the characters' structures are built once and only the
  top-level structure is built each frame, the whole build takes 0.04
  to 0.07 ms. A frame in which no character moved could skip the rest.
  The tile pass already knows such frames.
- The headless runs read the same parts about three times too high.
  The characters' structures read 0.69 to 0.70 ms there, and the whole
  build 0.77 to 0.84 ms. That confirms the idle card's slow clock. A
  GPU cost on this machine should be read from a run in a window.
- In a window the opaque pass took 0.78 ms with rays and 0.87 to
  0.94 ms with tiles in the Peragus hangar. In the dark fight it took
  0.98 to 1.01 ms with rays and 1.30 to 1.45 ms with tiles. All timed
  passes together came to the same with rays as with tiles: 1.08 to
  1.10 ms against 1.01 to 1.09 ms in the hangar, and 1.53 to 1.55 ms
  against 1.51 to 1.67 ms in the dark fight.
- The render thread took more with rays. It took 3.87 ms a frame
  against 3.68 to 3.82 ms in the hangar, and 5.51 to 5.54 ms against
  5.14 ms in the dark fight. About half of that is there with no build
  at all, from the skin-once pass and the material's own bind groups.
  The other half is wgpu checking and encoding the builds.
- The frame rate was 221 fps with rays and 221 to 230 fps with tiles in
  the hangar. In the dark fight it was 151 to 152 fps with rays and 153
  to 160 fps with tiles.
- 32 characters would cost about 0.2 ms to start, 0.3 ms for the
  characters and 0.05 ms for the top-level structure, so about 0.55 ms.
  That is still a guess, because the trial built 14 at most.

Against the limit of about 1 ms a frame: the build is well under it.
It costs 0.25 to 0.33 ms on the GPU for up to 7 characters, and the
rays win that much back in the opaque pass. What the trial leaves to
watch is the render thread's 0.1 to 0.4 ms. On 2026-11-13 the owner
asked for 6c to 6e to be built.

#### 6c. The scene's acceleration structures

Planned again on 2026-11-13, after the trial. This part turns the
trial's code (`ray_trial.rs`) into code that can be merged. It draws no
new shadow yet. `KJK_RAYS=on` turns it on until 6d adds the setting.

How it is built:

- **Where ray queries are known.** A resource says whether the card
  offers ray queries. It is set when the plugins finish, which is after
  Bevy has made the render device. `KJK_RAYS=off` makes it say no on any
  card. Bevy's vertex and index buffers get the usage that lets a
  structure be built from them only when it says yes.
- **The material's bind group.** The Odyssey material keeps the trial's
  hand-written bind group. Bevy's derive stays on a twin of the bound
  fields, and the structure is added at binding 17 when ray queries are
  on. The trial's names and comments lose the word "trial".
- **Two shaders from one file.** `enable wgpu_ray_query;` must be a
  shader's first line and cannot sit behind `#ifdef`. So the game loads
  `odyssey.wgsl` twice. The second copy has the line put in front of it
  as it is loaded, and the material's pipelines use that copy only when
  ray queries are on. A card without ray queries never compiles it.
- **A run that acts as a card without ray queries.** `KJK_RAYS=off` also
  takes the feature away from the device as it is made. A run with it
  must draw the same picture as main and log no shader error. This is
  the only check of such a card this machine can make.
- **Every casting character.** The skin-once pass places each of the 32
  characters that cast (`shadows.rs`'s casters), not only those with
  tiles. With ray queries on it always runs. A character's job is its
  shadow mesh where it has one, and its meshes one by one otherwise, as
  the tile pass chooses today.
- **A character's structure.** Each job gets one bottom-level
  structure, built each frame from the placed vertices and Bevy's index
  buffer, as in the trial. A frame in which no character moved builds
  none of them. The tile pass already knows such frames.
- **Placeables and doors.** Each casting mesh of a placeable or a door
  gets one structure, built once from Bevy's own vertex buffer when the
  mesh is first shown. Its instance carries the mesh's transform, which
  is written again when the mesh moves. The meshes are the ones the
  kept tiles draw today, plus the doors'. A mesh that a skin moves is
  left out for now.
- **The top-level structure.** One structure holds all of them and is
  built each frame. An instance's 24-bit number holds three things. Its
  low 10 bits are the owner's model number, as the mesh's `MeshTag`
  carries it. Its next 6 bits are the character's place among the
  casters, so the shader can read that caster's lights and rooms. Bit
  16 is set for a placeable or a door. The instance mask is 1 for all
  of them. 6e uses mask 2 for the rooms.
- **Meshes cut by a texture.** They are marked as not opaque, so a
  later part can reject a hit on them. In this part they cast whole.
- **A debug view.** `KJK_SHADOW_DEBUG=5` traces a ray from the camera
  to each pixel of a room. Where the ray meets a structure first, the
  pixel takes a colour from the instance's number. The picture then
  shows the structure's triangles standing where the characters,
  placeables and doors are drawn.
- **Timing.** The build keeps its GPU spans, named `ray_structures`
  with the characters' and the top-level builds inside it. A dump's
  `ray_structures` fact gives the counts of structures and triangles.

Done when: with `KJK_RAYS=on` the debug view shows the triangles on the
characters, placeables and doors in the cantina, the Peragus hangar and
the dark fight. With `KJK_RAYS=off`, and with the variable unset, the
picture matches main by `kjk compare-pictures` and the log has no
error. The build's GPU and CPU times, read from runs in a window, are
in the PR for the three bench scenes and the Dune Sea.

Where it stands (2026-11-13, `feat/ray-structures`, `ray_structures.rs`).
Every bullet above is built. The trial's code is gone, with its switches
`KJK_RAY_TRIAL`, `KJK_RAY_TRIAL_INSTANCES`, `KJK_RAY_TRIAL_PARTS`,
`KJK_RAY_TRIAL_LOG` and `KJK_TILE_CASTERS`. No shader traces a ray for a
shadow. The build's GPU and CPU times are not measured.

- **Where ray queries are known.** `RayQueries` says whether the card
  offers ray queries and whether the structures are on. It is in the
  main world and the render world. `RayStructuresPlugin::finish` sets
  it. `KJK_RAYS=on` turns the structures on. Unset, they are off.
  `KJK_RAYS=off` takes ray queries and the three other ray features away
  from the render device as it is made (`runtime`,
  `WgpuSettings::disabled_features`). Bevy's vertex and index buffers get
  `BLAS_INPUT` only with the structures on.
- **The material.** `OdysseyBound` keeps Bevy's derive, and
  `OdysseyMaterial` makes its bind group by hand. With the structures
  off, the bind group and its layout are the derive's alone, as on main.
  With them on, the layout has the structure at binding 17.
- **Two shaders.** The material loads `odyssey.wgsl` a second time with
  `enable wgpu_ray_query;` in front (`RAYS_SHADER`). `specialize` gives
  the main pass that copy, with the definition `RAY_QUERIES`, only when
  the structures are on.
- **Every casting character.** `upload` lists the casters' models in
  their order (`shadows::CasterSlots`). With the structures on, the tile
  pass lists the meshes of each of them (`shadow_tiles::RayDraw`), as its
  tiles would draw them. The skin-once pass then always runs and places
  them all. A character with no capsules is no caster today, so it gets
  no structure. The patrons seated in the cantina are such characters.
- **A character's structure.** Each placed mesh gets one structure, built
  from the placed vertices. A frame on which the skin-once pass placed
  nothing builds none. In the cantina, with the animation held (`anim`),
  no frame of 122 built one. Moving, 120 frames of 122 built them.
- **A character's rigid meshes.** A weapon or helmet that no shadow mesh
  holds has no skin. The plan did not say what it gets. It gets a
  structure from Bevy's own vertex buffer, as a placeable's mesh does,
  and its instance carries the matrix the tile pass places it by.
- **Placeables and doors.** Each shown placeable with a shadow box, and
  each door (`DoorModel`), has its casting meshes listed each frame in
  the main world. That is every placeable that could have kept tiles,
  not only those a lamp lights. A mesh's structure is built the first
  time it is shown, and kept for 600 frames after it was last shown. An
  instance's transform is written every frame, not only when it moves.
  A mesh whose position is not the first three floats of its vertex
  would get no structure, and the dump counts it (`unfit`). None did in
  the four scenes run. Since the change told under "Why Ultra is slow on
  the CPU", a mesh with its own instance is only one that moved or
  appeared in the last 30 frames. A mesh that stands still is a geometry
  of the placeables' or the doors' merged structure instead.
- **The top-level structure.** It is made once with room for 4,096
  instances, because the material's bind groups hold it. An instance past
  that is left out and counted (`dropped`). The number holds the owner's
  model in bits 0 to 9, the caster's place in bits 10 to 15, and bit 16
  for a placeable or a door. A model number of 1,024 or more loses its
  high bits there. The mask is 1. A merged structure of placeables or
  doors has bit 19 in its number. Its low 16 bits then say where its
  geometries' numbers start in a buffer the material binds at binding 18.
  Each geometry's number there is the one the mesh's own instance would
  carry.
- **Meshes cut by a texture.** Their geometry is not marked opaque.
- **The debug view.** `KJK_SHADOW_DEBUG=5` traces the ray at every
  fragment the Odyssey material draws, not only a room's. A character is
  drawn over the room behind it, so a room's fragment never shows where
  a character stands. A fragment whose ray meets a structure takes a
  colour from the instance's number. Every other fragment is grey. With
  the structures off, 5 shows the rooms' shadows alone, as 1 does.
- **Timing.** The GPU spans are `ray_structures`, and inside it
  `characters` and `top_level`. A third span, `placeables`, is there
  only on a frame that builds a mesh's structure for the first time, or
  builds the placeables' or the doors' merged structure. The dump's `ray_structures` fact gives the counts. The build runs in its
  own render system after the tile pass (`ray_structures::build`).

What the runs showed on the RTX 4070 Ti SUPER, headless at 1280x720:

| Scene | Character structures | Other structures | Instances | Triangles |
|---|---|---|---|---|
| Cantina | 41 | 60 | 101 | 60,251 |
| Peragus hangar | 17 | 177 | 194 | 39,452 |
| Dark fight | 32 | 218 | 250 | 87,933 |

- The debug view showed every caster's triangles over its body in the
  three scenes. It showed a cantina door's triangles over the door, and
  two barrels' triangles over the barrels in the Peragus hangar. A door
  or a placeable in the dark fight was not looked at.
- With `KJK_RAYS=off` and with the variable unset, the cantina and the
  Endar Spire matched within the noise between two runs of one build.
  Main was not built to compare against. With the variable unset, the
  code that runs is main's. Each picture was compared with an unset run
  over the pixels that two unset runs drew the same (`--still`). The
  cantina differed in 585 pixels with `off`, and in 321 with a third
  unset run. The Endar Spire differed in 4,010 pixels with `off`, and in
  26,395 with a third unset run. No log had an error.
- With `KJK_RAYS=on` the shadows are today's. The cantina differed in
  424 pixels and the Endar Spire in 4,421, the same as two unset runs
  differ. A picture of the differences showed scattered single pixels.
  With the structures on, the tiles are drawn from the skin-once pass's
  vertices, and they are drawn again whenever any caster moved.
- In the Peragus save the player's head shows in neither its tile nor
  its structure. The tile pass lists four rigid meshes of the player,
  and no hit was seen at them. The tile drawn with `KJK_RAYS` unset,
  which is main's code, has no head either. This was not followed
  further. The cantina's characters show their heads.
- No card without ray queries has run this part. `KJK_RAYS=off` is the
  only check of one.

Checked again on 2026-11-14, headless at 1280x720 on the same machine,
with 6d and 6e built. The pictures are in the worktree's
`target/ultra-captures/compare/`.

- No build of main was at hand. The worktree `tiles-base` holds a build of
  97fd7804, which is older than this branch's base, 06c91634. So
  `KJK_RAYS=off` was compared with `KJK_RAYS` unset, as before.
- The cantina was run under six settings. They were Shadows at Off, Low
  and High over MSAA 4, Classic and Enhanced lighting over High and MSAA 4,
  and High over TAA. The Shadows box has no Medium. Each setting ran twice
  with the variable unset and once with `off`. The `off` picture was
  compared with an unset one over the pixels that the two unset runs drew
  the same (`--still`).
- Off, High, Classic and Enhanced matched. Their `off` pictures differed
  in 11, 50, 86 and 94 pixels of about 900,000. The first `off` runs of
  Off and Enhanced had differed in about 75,000 pixels each. Those two runs
  were slow, as other work loaded the machine, and their head-up display
  was drawn lighter. A second run of each matched.
- High over TAA differed in 2,164 pixels, scattered over the picture. Two
  unset runs differed in 2,493 pixels.
- Low differed in 2,050 pixels, and a second `off` run in 703. Two unset
  runs differed in 92 pixels. The differences lie on the outlines of the
  characters, by up to 97 of 255 (`compare/tar_m03ae/low-off2-diff16.png`).
  A later check found that this is noise between runs. The 6d block
  below tells how.
- The Endar Spire's opening conversation moves too much between runs for
  this test. Over the pixels two unset runs drew the same, the `off`
  picture differed in 0.1 to 0.3% of them under Low, High, Classic and
  Enhanced. It differed in 3% under Shadows Off and in 4 to 9% under High
  over TAA. All of those pixels lie on the moving characters.
- No log of these runs had an error, a panic or a validation message.

#### 6d. Ultra shadows

Planned again on 2026-11-13, after the trial.

- **The setting.** Shadows gets a fourth value, Ultra
  (`display.shadows`). The options offer it only where ray queries are
  on, as they offer DLSS only where it runs. Ultra with an MSAA choice
  draws as High does, and the options say that Ultra needs TAA or DLSS.
  The text follows `strings.rs` and stays short and plain. `KJK_RAYS`
  stays as a switch for a run.
- **The ray.** Under Ultra, `light_visibility` traces one ray from the
  pixel toward the light in the place of the capsules, the boxes and
  the tiles. Everything after it stays. The shadow still takes the
  blocked light's share of the lightmap and still leaves 20% of the
  baked light (`room_shadow`).
- **The soft edge.** Each frame the ray goes to another point on the
  light's source, which is as wide as today's (`LAMP_SOURCE`,
  `SUN_SOURCE`, `FIGHT_SOURCE`). The point comes from a noise of the
  pixel and the frame's number. The temporal resolve blends the frames
  into an edge that widens with the distance from the caster. Under
  MSAA nothing blends the frames, which is why Ultra needs TAA or DLSS.
- **Step 2's rules, by rejecting hits.** The ray walks its candidate
  hits. It reads the instance's number and rejects a hit when any of
  these holds. The hit is on the pixel's own owner. The caster's bit
  for this light is not set, which covers the sight lines and the
  light within 0.4 m of a body. The caster's shadow does not fall on
  the pixel's room. The trial showed that walking the candidates costs
  under 0.25 ms.
- **What a ray is not traced for.** A fight's light too faint to show.
  A pixel outside every caster's and box's reach, as today.
- **Meshes cut by a texture cast whole.** Reading a texture where a ray
  hits needs every texture bound in one array, which kjk does not
  have. If hair, capes or grates look wrong in the captures, they keep
  their capsules, or the owner decides on the texture array.
- **What keeps its capsules.** The wide overhead source and the contact
  darkening. They stand in for ceiling panels the light list does not
  have, and one ray cannot do that.
- **What is off.** Under Ultra the tile pass draws no tile and the
  shader looks none up.

Done when: close-up captures show fingers, weapons, robes and a
double-bladed lightsaber in the shadow, under lamps, the sun and a
passing bolt. Ultra's frame time is measured against High in runs in a
window, in the three bench scenes and the Dune Sea, at 1920x1080 and
3840x2160. Classic and the other three Shadows values are unchanged by
`kjk compare-pictures`. DLSS has been run with Ultra on an RTX card.

Where it stands (2026-11-13, `feat/ray-structures`, on top of 6c). Every
bullet above is built, with the changes told below. The frame time is
measured under "What 6c to 6e cost together". DLSS ran with Ultra on
2026-11-14, as the close-ups below tell. The narrowed sun ray, the meshes
cut by a texture and the punch-through head were checked again later that
day, and the frame time was measured at 3840x2160 and 1920x1080. Two
calls are still the owner's, and each has a recommendation below.

- **The setting.** `ShadowSettings` has a fourth value, `Ultra`
  (`shadows.rs`, `display.shadows` = `ultra`). The options offer it only
  where ray queries are offered (`ViewModel::ultra_shadows`, set in
  `display.rs`'s `offer`). The Shadows box now steps only to the values
  offered, as the anti-aliasing box does. The help text says that Ultra
  needs a graphics card with ray tracing, and TAA or DLSS, and looks the
  same as High with other anti-aliasing.
- **When the structures exist.** The structures can be built whenever the
  card offers ray queries and `KJK_RAYS` is not `off`
  (`ray_structures::available`). The material's layout, Bevy's buffer
  usages and the skin-once pipeline are then made for them when the
  plugins finish, whatever Shadows says. They are built and traced only
  while Shadows is at Ultra and the anti-aliasing is TAA or DLSS
  (`RayQueries::traced`, chosen each frame in `choose` and copied to the
  render world). So Shadows and the anti-aliasing can change while the
  game runs. Going from Ultra to another setting drops every structure.
  The way back builds them anew. A run that changed between Ultra, Off,
  Low and High, and between TAA and MSAA 4, logged no error.
- **`KJK_RAYS`.** Unset, the setting decides. `on` makes a run use Ultra
  whatever Shadows says. `off` takes ray queries away from the device as
  before, so Ultra draws as High does. In 6c, unset meant off.
- **Ultra under MSAA.** The frame draws as High does
  (`ShadowSettings::drawn`). The material gives a view the shader with ray
  queries only when the view is jittered, which TAA and DLSS do
  (`MeshPipelineKey::TEMPORAL_JITTER`). So every view under MSAA draws with
  main's shader, at any Shadows. The debug view 5 shows the structures
  only while they are traced, and is as 1 otherwise.
- **The ray.** `light_visibility` calls `ray_visibility` when
  `ShadowList::rays` is 1, in the place of `cone_visibility`.
  `room_shadow` and everything after it are as they were.
- **The soft edge.** The ray goes to a point of a disc across the light,
  as wide as the source the capsules use (`LAMP_SOURCE`, `FIGHT_SOURCE`
  and a bolt's half line). The sun's ray has a narrower source of its
  own, `SUN_RAY_SOURCE`. The point comes from two
  gradient noises of the pixel, moved each frame by Bevy's frame count
  (`globals.frame_count`, `ray_noise`). The first try used one noise
  shifted for both numbers. Its points then lay along one curve across
  the disc, and the edge was not spread over the whole disc. A white noise
  for the distance from the middle left a grain the resolve did not
  smooth.
- **Step 2's rules.** The ray walks every candidate hit
  (`RAY_FLAG_FORCE_NO_OPAQUE`), and `ray_blocks` keeps the first it
  allows. A character's hit counts only when the character is among the
  casters near the pixel (`near_casters`), found by its place among the
  casters in bits 10 to 15. That one test keeps the rules of its own
  owner, its rooms (`unseen`), a storey above, standing behind the
  surface, and its reach. The light's bit (`GpuCaster::lights.x`) must
  be set. A placeable's hit counts only when its box is among the boxes
  near the pixel, by the box's model, and the light casts from that box.
  The last 2 m of a character's reach, and the last 1 m of a box's, ease
  the shadow away by a third noise.
- **What could not be kept.** A door has no box, so a door never stops
  the ray. Bit 17 of the instance's number marks a door. A door's
  structure is there for 6e. A placeable's box rule "a point inside the
  box is not shaded by it" is not kept, as the mesh itself now decides.
  A placeable too sparse for its box casts under Ultra too, also under a
  fight's light.
- **Model numbers of 1,024 or more.** A character is found by its place
  among the casters, so its model number is never read. A placeable is
  found by the low 10 bits of its model. A placeable hit whose low bits
  are the pixel's own is rejected. So a placeable never shades itself.
  When two models 1,024 apart stand within a few metres, one may lose its
  shadow on the other.
- **What is not traced.** A fight's light too faint to show is not traced,
  as before. A pixel with no caster and no box near it traces nothing. A
  pixel whose near casters and boxes the light does not cast from traces
  nothing either.
- **Meshes cut by a texture** cast whole. This is the owner's call. The
  captures of 2026-11-14 are told under "Checked again on 2026-11-14"
  below.
- **The capsules that stay.** The wide overhead source and the contact
  darkening keep their capsules and boxes.
- **No tile.** Under Ultra the plan has no character's tile and no kept
  tile, and the shader looks none up. The skin-once pass runs for the
  structures, and only then or with `KJK_SKIN_ONCE=on`.
- **The player's head in the Peragus hangar.** The head's face and hair
  meshes are punch-through surfaces (`Surface::Cutout`). The tile pass's
  `casts` leaves such surfaces out, so the head had no tile and no
  structure, also on main. The meshes listed for the structures now take
  punch-through surfaces too, whole (`shadow_tiles::casts_for_rays`). The
  debug view 5 shows the head's triangles. The tiles still leave them
  out, so High is unchanged. The head's shadow under the hangar's lamps is
  still faint, as the lamps' wide source spreads it over most of a metre.
- **Bench presets.** `shadows-ultra` and `shadows-high-taa` set Ultra and
  High over TAA for a later timed comparison. Nothing was timed.

What the runs showed on the RTX 4070 Ti SUPER, headless at 1280x720:

- In the cantina under TAA, the player's shadows under its four lamps
  show both legs and the body, with edges that are sharp at the feet and
  widen away from them. The penumbra shows a faint diagonal pattern from
  the noise. The character's own surface shows no stripes and no acne.
  In the Peragus hangar the player's shadow shows both legs and the body.
  The dark fight showed no fault either, though no capture there singles
  out a fight's light's shadow.
- Ultra with MSAA 4 matched High with MSAA 4. They differed in 10,349
  pixels, where two runs of High differed in 9,563.
- With `KJK_RAYS` unset, Off, Low and High matched `KJK_RAYS=off` within
  the noise between two runs. With MSAA 4, Off differed in 3,762 pixels,
  Low in 5,281 and High in 7,297. Under TAA, High differed in 5,786,
  where two runs differed in 4,022.
- With `KJK_RAYS=off` and Ultra in the settings, the cantina, the Peragus
  hangar and the dark fight ran with no error. In the cantina Ultra then
  differed from High in 6,303 pixels.
- No log had an error, a panic or a validation message.

The close-ups were taken on 2026-11-14, headless at 1600x900 with game
time held (`anim`). Each run drew one moment under Ultra over TAA. The
script then stepped the Shadows box to High and drew the same moment
again. `KJK_SHADOW_DEBUG=1` shows the rooms' shadows alone, which makes
their shapes easy to compare. The pictures are in the worktree's
`target/ultra-captures/`.

- **The sun.** A fight was staged in the Dune Sea (`arena@tat_m18aa`,
  arriving at `wp_tat18aa_rustbot_01`). The player held a double-bladed
  lightsaber (`--class duelist@staff`). A dark Jedi, a Sith trooper and a
  Mandalorian fought the player (`dune/`, `dune2/`, `dune-dbg1/`). The
  characters' shadows on the sand have the same shapes under Ultra and
  under High. Ultra's edges are softer. No acne and no band showed.
- **Thin things under the sun.** The double-bladed lightsaber's hilt casts
  no shadow that can be seen under the sun at Ultra. Under High the hilt's
  tile draws it as a clear band across the shadow of the arms
  (`dune-dbg1/staffcrop.png`). The debug view 5 shows the hilt's and the
  fingers' triangles in the structure (`dune-dbg5b/staff2.png`), so the
  hilt is traced. The sun's source is 0.03 radians across its half
  (`SUN_SOURCE`), about six times the real sun's. A hilt 3 cm thick, held
  1.2 m over the sand, then spreads its shadow over about 7 cm and loses
  most of its darkness. Fingers fade the same way. The tiles draw thin
  things sharper than their source would. A narrower source for the sun's
  ray alone would bring them back. That is for the owner to decide.
- **Three widths of the sun's ray.** One run drew the same held moment of
  that fight under Ultra with the sun's ray going to a source 0.03, 0.015
  and 0.005 radians across its half, then under High. A temporary switch
  changed the ray's width alone, each time the script stepped Shadows
  back to Ultra. The capsules kept their own width. The switch was then
  taken out, and `SUN_SOURCE` is still 0.03. The sheet is
  `sun-width/sheet.png`. Its top row shows the whole view. Its other two
  rows are close-ups of the shadow of the raised hand and the hilt.
  - At 0.03 the hand's and the hilt's shadow is a pale smear. Its penumbra
    shows the noise's faint diagonal pattern.
  - At 0.015 the hand's shape shows, faintly, with soft sides.
  - At 0.005 the hand and the hilt cast a clear, solid shape, darker than
    High's tile draws it. Its edge is sharp but shows no grain, because a
    narrow source leaves the noise little to spread. The body's shadow
    keeps the same shape at every width and gets crisper.
  - 0.005 radians is about the real sun's half width (0.0047). It keeps
    the thin shadows best and showed no noisy edge here. Only the Dune
    Sea was looked at, and only under TAA.
- **The sun's ray is narrowed.** The owner chose 0.005 radians on
  2026-11-14. The sun's ray now goes to a source `SUN_RAY_SOURCE` wide
  (`odyssey.wgsl`). The capsules and the tiles keep `SUN_SOURCE`, so no
  other setting changes. A capture of the same fight under the new
  build shows the hand's and the hilt's shadow as a clear shape
  (`sun-narrow/`, `zoom-ultra.png` against `zoom-high.png`). DLSS and
  another sunlit area were looked at with the new width later on
  2026-11-14, as told under "Checked again on 2026-11-14" below.
- **Low with and without ray queries.** In the cantina under Low and MSAA
  4, four runs were made with `KJK_RAYS` unset and four with `off`,
  taking turns (`low-repeat/`). The first run of each kind differed from
  the later runs of its own kind in about 22,000 to 28,000 pixels. The
  later runs differed from each other by 467 to 2,171 pixels with the
  variable the same. Over the pixels two runs drew the same (`--still`),
  an `off` run differed from an unset run in 93 to 222 pixels. Two `off`
  runs differed in 349 pixels that way, and two unset runs in 2,171. So
  an `off` run is no further from an unset run than two runs of one kind
  are from each other. The differences on the characters' outlines come
  from the characters' animation landing a little differently in each
  run. The code agrees. Under Low, ray queries being offered changes only
  the material's layout, Bevy's buffer usages and the skin-once pipeline,
  which is made but not run. None of these changes a pixel. Nothing was
  changed.
- **The lamps.** In the cantina the player held a blaster pistol, and a
  bounty hunter a rifle (`cantina/`, `cantina-dbg1/`). The lamps' source is
  0.35 m wide, so their shadows are wide and soft under both settings. The
  shapes match. The fingers and the pistol do not show as shapes of their
  own under either setting. No acne showed on the characters or the floor.
- **Passing bolts and blades.** The dark fight was drawn with the player's
  double-bladed lightsaber, two dark Jedi and four troopers (`fight-ultra/`,
  and `hallfight/` under Enhanced lighting). No acne, speckle or leak
  showed on the characters, their weapons or the floor. Under Classic
  lighting the lit corridor leaves a fight's light little room, so its
  shadows are hard to see. Under Enhanced lighting the pools are plain.
  Under Ultra the floor is a little brighter by the characters' feet than
  under High. The ray follows the body, where High traces fat coarse
  capsules.
- **DLSS.** A release build with the `dlss` feature was made in
  `target/dlss`, with `DLSS_SDK`, `VULKAN_SDK` and `LIBCLANG_PATH` set as
  `docs/BUILD.md` says. It took 29 minutes. Ultra ran in the dark fight
  under DLSS Quality and under DLAA (`fight-ultra-dlss/`,
  `fight-ultra-dlaa/`). The log said `anti-aliasing used=Dlss(Quality)`
  and `ray-traced shadows traced=true`, and it had no error. Under DLSS
  Quality the sun's shadow edge in the Dune Sea resolves into a soft
  gradient (`dune-dlss-dbg1/playertop.png`). Its penumbra shows a faint
  diagonal streak, coarser than under TAA.
- **Still owed.** Ultra's frame time against High's at 1920x1080 and
  3840x2160 in a window was not measured again here. No bench was run,
  as other agents shared the machine. It was measured later on
  2026-11-14, as told under "What 6c to 6e cost together".

Checked again on 2026-11-14, headless at 1600x900 with game time held.
Each run drew one moment under Ultra, then stepped the Shadows box to
High and drew the same moment again. The pictures are in the worktree's
`target/ultra-captures/final/`.

- **The narrowed sun under TAA.** A fight was staged on the Unknown
  World's beach (`arena@unk_m41aa`), where the sun casts plain shadows on
  white sand. The player held a double-bladed lightsaber (`sun-taa-unk/`).
  Under Ultra the player's shadow is a clear shape with the hilt and the
  raised arm in it. It starts at the heel, so it does not come away from
  the feet. Enlarged 4 times, its edge is smooth and shows no grain and no
  steps. High's shadow has the same shape with softer, blotchier edges.
- **The narrowed sun under DLSS Quality.** The Dune Sea fight
  (`arena@tat_m18aa`, arriving at `wp_tat18aa_rustbot_01`) was drawn by a
  new build with DLSS (`sun-dlss-dune/`). The log said
  `anti-aliasing used=Dlss(Quality)` and `ray-traced shadows traced=true`.
  The shadow starts at the feet here too. Its edge is sharp. Enlarged 5
  times, the edge shows a faint pattern of steps one or two pixels wide,
  which TAA does not show. At the game's own size it cannot be seen. The
  sheet is `sun-sheet.png`.
- **Meshes cut by a texture.** Under High, a tile cut by a texture was
  drawn only for placeables, and only in the Dune Sea (144 draws) and the
  Ebon Hawk (3 draws). No character's tile was cut by a texture in the
  twelve areas looked at. The dump's `shadow_tiles.cut_draws` was 0 in
  each. In the Dune Sea the refuse piles (`plc_rfpilelg`) carry cards of
  loose junk. Under the sun at Ultra each card casts a whole,
  straight-edged rectangle on the sand. High casts the junk's ragged
  outline (`decision-cut-meshes.png`). The pile is small and lies flat, so
  the fault shows only from close by, as a box of shade beside the pile.
- **What reading the texture at a hit would take.** The shader would need
  the hit's texture coordinates and the texture's alpha there. A candidate
  hit gives its geometry, its triangle and its barycentrics. Three more
  things would have to be bound. The first is the mesh's indices and
  texture coordinates, read from Bevy's mesh buffers as storage, with an
  offset for each geometry. The second is a table that gives each cut
  geometry its offsets and its texture's layer. The third is the cut
  textures' alpha. kjk binds no texture array, so the alpha would be copied
  into one array of a fixed size, for example 256 by 256 pixels at one byte
  a pixel, which is 64 KB a texture. Only the cut meshes would be marked
  not opaque, and only their hits would read the texture. The ray already
  walks every candidate hit, so the extra GPU work is a few reads at those
  hits. The work is in the bindings, the table and the copying, and would
  take a few days.
- **Recommendation for cut meshes.** Keep the whole meshes for now. In the
  areas looked at, the only plain case is a junk pile's flat cards, and no
  character's hair or cape is cut by a texture. If the owner wants the
  piles fixed cheaply, a placeable's cut meshes can be left out of its
  structure, as a cut room mesh is. Its box then still casts. The texture
  array is worth building only if a later area shows the fault on
  characters.
- **The punch-through head.** The player's head in the Peragus hangar
  (the save `tsl-13`) was drawn from 4 m above with the shadows alone
  (`head-per-dbg1/`, `decision-head-tiles.png`). Under Ultra the far end of
  the shadow has the round head. Under High the shadow ends thinner and
  ragged, as the tile has no head. Drawn normally, the two can hardly be
  told apart, because the hangar's lamps are 0.35 m wide and spread the
  head's shadow over most of a metre.
- **What letting the tiles draw punch-through meshes would cost.** The
  tile pass's `casts` would accept `Surface::Cutout` as `casts_for_rays`
  does. Such a mesh would then be drawn into the tile with its texture's
  alpha, as the tile pass already does for a mesh cut by a texture. For a
  TSL character that adds the head's face and hair meshes, so two more
  draws and a few hundred triangles for each of its tiles. The risk is
  that the default setting changes. Each TSL character's tile then gets a
  head, which can also darken the character's own face where the tile's
  bias is too small. The tiles' pictures would need checking again in the
  bench scenes and in the captures the owner approved.
- **Recommendation for the head.** Leave High as it is. The missing head
  shows only in the shadows-alone view, and changing it changes the
  default setting's look for every TSL character.

#### 6e. Walls that stop a fight's light

Planned again on 2026-11-13, after the trial.

- **The rooms' structures.** Each room mesh that writes depth gets one
  structure, built once as the area loads from Bevy's own vertex
  buffer. Skies, backdrops and see-through meshes get none. Their
  instances carry mask 2 and the room's number.
- **Who traces them.** A fight's light traces with mask 3, so walls,
  pillars, characters and placeables all stop it. The lamps and the sun
  keep mask 1. Their shadows of walls are in the lightmaps
  (decision 1), and tracing the rooms would darken those places twice.
- **The start of the ray.** A room's own surface must not stop its own
  ray. The ray starts 3 cm off the surface along its normal, and ends
  3 cm short of the light. A bolt that lies against a wall is the case
  to look at.
- **The room mask.** Under Ultra the fight lights no longer use step
  1's room mask (`fight_rooms.rs`). The other settings keep it.
- **Doors.** A closed door stops a fight's light through 6c's
  structure. An open one lets it through.

Done when: a capture of a bolt passing a pillar in Leviathan's hall
shows the pillar's shadow. A bolt in the next room lights nothing
through the wall, and lights through an open door. The time to build
the rooms' structures at load is measured in the largest area
(Dantooine, `danm14aa`), and the opaque pass is measured in the dark
fight against 6d, in a window.

Where it stands (2026-11-13, `feat/ray-structures`, on top of 6d). Every
bullet above is built, with the changes told below. The build at load was
measured on 2026-11-14, as told below. The opaque pass in the dark fight
is measured against High under "Why Ultra is slow on the CPU". It is not
measured against 6d. On 2026-11-14 the rooms' structures were found not to
be built while game time ran in three areas, and this was fixed, as told
below.

- **The rooms' structures.** The spawner marks each room mesh that writes
  depth with `ray_structures::RayRoom` (`scene`'s `Spawner::walled`). A sky
  or a backdrop is a room with no floor, and its meshes get no mark.
  Blended and additive meshes get none. A mesh cut by a texture (a grate, a
  fence, leaves) gets none either, though it writes depth. A ray cannot
  read the texture, so it would be stopped by the whole mesh. A mesh that a
  skin or its dangles move gets none, as its structure would not follow it.
  The plan did not say what these two kinds get.
- **Built once.** The main world lists the marked meshes (`gather_rooms`)
  only on a frame on which one appears, goes, moves or changes its mesh. The
  render world copies the list only when it was made again. As first built,
  each listed mesh got one structure from Bevy's own vertex buffer. Since
  the change told under "Why Ultra is slow on the CPU", all the listed
  meshes are geometries of one merged structure, or of a few in a large
  area. It is built once the list and the count of its meshes on the GPU
  have stayed the same for 30 frames. Rooms are listed whether they are
  shown or not, as a wall of a room the camera cannot see still stops a
  fight's light.
- **The instances.** A merged structure of rooms has the mask 2 and bit 18
  in its number. As first built, each room mesh had an instance, and the
  top-level structure had room for 16,384 of them. It now has room for
  4,096 again, and the rooms come last.
- **Who traces them.** Only a fight's light (`LIGHT_COMBAT`) traces with
  the mask 3 (`ray_visibility`'s `fight`). Every lamp's and the sun's ray
  keeps the mask 1. `light_visibility` is the only caller of
  `ray_visibility`, and it passes `fight` from the light's own kind. A room
  hit is kept only for a fight's light (`ray_blocks`).
- **Traced wherever the light shows.** Under Ultra a fight's light's ray
  runs with no caster or box near the fragment. It still runs only where
  the light lights the surface by more than `FAINT`, and so only inside the
  light's radius. That holds for the rooms (`combat_lights`), the shiny
  rooms' highlights (`fight_highlights`) and the characters and placeables
  (the fragment shader's own loop over the lights). So a bolt behind a wall
  lights neither the wall's far side nor a trooper there.
- **The start of the ray.** The ray starts 3 cm off the surface along its
  normal and ends 3 cm short of the point of the light's source it goes to
  (`RAY_LIFT`). A mesh without normals starts its ray 3 cm along the ray.
  The 3 cm go to the side of the surface that faces the light. A surface
  turned away from the light then starts its ray behind itself. This keeps
  a room mesh whose normals point the wrong way from shading itself.
- **Back faces stop the ray.** A room mesh is often one-sided and not
  closed. A wall's back faces the next room. If back faces let the ray
  through, a bolt in the next room would light through the back of the
  wall.
- **A bolt along a wall.** For a point on the wall, the light's nearest
  point (`fight_point`) lies straight off the wall, so the ray leaves the
  wall at a steep angle. A bolt nearer than 6 cm to a surface traces no ray
  there. For the floor under a bolt flying close to a wall, part of the
  source's disc lies inside the wall. Those rays are stopped, so the pool
  is a little darker on the wall's side, softly and without a band.
- **The room mask.** Under Ultra `fight_reaches` lets every fight's light
  reach every room, and the rays find the walls. It is true only in the
  shader with ray queries and only while `ShadowList::rays` is 1. Every
  other setting reads `reaches_room` as before.
- **Doors.** A door's hit stops a fight's light, unless the door is the
  fragment's own model. A door's structure follows its meshes each frame.
  An open door's meshes stand out of the way, so the light passes.
- **The soft edge.** The ray goes to a point of the source's disc by 6d's
  noise and 6d's source width.
- **The debug view.** `KJK_SHADOW_DEBUG=6` traces the camera's ray against
  the rooms alone, and each room mesh takes its own colour. The colour now
  comes from the merged structure's place and the geometry's.
  `KJK_SHADOW_DEBUG=5` now traces the characters, placeables and doors
  alone, as in 6c.
- **The dump.** The `ray_structures` fact has `room_structures`,
  `room_geometries`, `room_triangles`, `room_meshes`, `room_unfit` and
  `room_builds`. `room_structures` counts the merged structures, and
  `room_builds` how many times they were built since the run started.
  `triangles` leaves out the rooms'. The table below counts each room
  mesh's own structure, as first built.

What the runs showed on the RTX 4070 Ti SUPER, headless at 1280x720:

| Scene | Room structures | Room triangles | Frames that built them |
|---|---|---|---|
| Cantina | 815 | 31,035 | 1 |
| Peragus hangar | 265 | 75,398 | 1 |
| Dark fight | 455 | 83,754 | 1 |
| Dantooine, `danm14aa` | 194 | 19,177 | 1 |

- No room mesh was unfit, and no instance was dropped.
- The same moment was drawn under Ultra and High in one run of the dark
  fight. The script holds game time (`anim`), and steps Shadows from Ultra
  to High in the options. A wall face beside the hall's walkway shows the
  walkway's edge as a shadow under Ultra, and is lit evenly under High.
  Some wall faces that step 1's room mask kept dark are lit under Ultra,
  where the fight's lights see them. No black band or bright leak showed
  near the blades by the floor.
- A bolt behind a wall in another room, and a bolt through an open or a
  closed door, were not captured. No scripted fight puts a light by a door.
- Under Ultra the cantina, the Peragus hangar and Dantooine matched the
  build before this part within the noise between two runs. The cantina
  differed in 396 pixels, where two runs differed in 550. The Peragus
  hangar differed in 595 pixels, where two runs differed in 1,970.
  Dantooine differed in 3,258 pixels, where two runs differed in 3,226. So
  the lamps' and the sun's shadows did not change.
- Going from the cantina to the Upper City and back dropped the cantina's
  815 structures, built the Upper City's 72, and built the cantina's again.
  A placeable's structure still stays for 600 frames after the area was
  left, as in 6c.
- Off, Low and High under MSAA, `KJK_RAYS=off` with Ultra, and Classic
  lighting with Ultra ran the dark fight with no error.

Checked on 2026-11-14, headless at 1600x900 with game time held. Each run
drew one moment under Ultra over TAA, then under High. The pictures are in
the worktree's `target/ultra-captures/`. A signed picture shows in red
where Ultra is brighter and in green where High is.

- **A closed door and a wall.** In the dark fight's corridor the player
  backed up to the locked door of the hangar control room
  (`lev40_hngcntrlb`) before the foes came. The blades then fought 1 to 2 m
  from the door (`door-closed/`). Seen from inside the dark control room,
  Ultra lit nothing on the room's side of the door or of the wall around
  it. High lit the door's frame on the room's side by up to 106 of 255, as
  step 1's room mask lets the light into that room
  (`door-closed/side-diff.png`). So under Ultra a fight's light behind a
  wall or a closed door lights nothing through it.
- **A door that opens.** The Leviathan's doors stay locked in an arena
  run. So the player opened the Endar Spire's bunk room door (`end_door01`,
  `arena@end_m01aa`) with `use` in `--moves`. A fight was then staged in
  the bunk room by the door, under Enhanced lighting (`edoor-open/`). Under
  Ultra the blades and bolts lit the corridor's floor past the open door,
  as under High. In a run that left the door closed (`edoor-closed/`), High
  lit the corridor's floor by the door and Ultra left it dark
  (`edoor-closed/signed.png`).
- **A blade along a wall.** The player stepped against the corridor's wall
  of the dark fight, and the staff's blade lay against it (`wall2/`, under
  Enhanced lighting). The wall beside the blade is lit evenly. No speckle,
  no black band and no self shadow showed. The vent ledge at the foot of
  the wall is a little darker under Ultra than under High. Part of the
  blade's light is then behind the sloped wall, and the ray finds it
  there. No capture singles out a flying bolt along a wall. One was taken
  later on 2026-11-14, as told under "A bolt along a wall, and a column"
  below.
- **A pillar.** No free-standing pillar was found near the dark fight in
  `lev_m40ac`. Under Ultra the walkway's edge shades the sloped lower walls
  from the fight's lights, where High lights them evenly
  (`hallfight/signed-sheet.png`). A pillar's shadow from a passing bolt is
  still owed.
- **The build at load.** Dantooine (`danm14aa`) was loaded once under
  Ultra with `KJK_GPU_TIMES=1` (`danm14aa/d.json`). Its rooms are 194
  geometries of one merged structure, with 19,177 triangles. They were
  built on one frame, and the GPU span `ray_structures/rooms` took 1.54 ms
  on it. A headless run's GPU times read high (6b). By triangles
  Dantooine is not the largest area run. The dark fight's rooms hold
  83,754 triangles. The build's CPU time was not measured apart.
- **Still owed.** The opaque pass in the dark fight against 6d, in a window.
  No bench was run, as other agents shared the machine.

Checked again on 2026-11-14. The pictures are in the worktree's
`target/ultra-captures/final/`.

- **The rooms were not built while game time ran.** In the dark fight
  (`lev_m40ac`), the Sith base on Taris (`tar_m10aa`) and the Peragus
  hangar (the save `tsl-13`), the rooms' merged structures were never
  built while game time ran. `room_builds` stayed 0 after 600 frames. Every
  earlier capture held game time, and then they were built. Two things
  made the room list again almost every frame, so it never stayed the same
  for 30 frames. In the dark fight and the Sith base, the areas' own
  animations move a few room meshes on every frame. In the Peragus hangar
  the list was made again about every third frame with the same meshes in
  the same places. So under Ultra a fight's lights in those areas went
  through walls and closed doors while the game ran. The benches of
  2026-11-13 timed the dark fight and the Peragus hangar under Ultra
  without the rooms' structures.
- **The fix.** `gather_rooms` now leaves out a room mesh that still moves
  30 frames after it was first seen (`RayRooms::moving`). Such a mesh, a
  turning fan for example, stops no ray. A list with the same meshes in the
  same places, in the order of their entities, is not made again. The dump
  gives `room_moving`, the meshes left out, and `room_lists`, how many
  times the list was made. With game time running, the rooms were then
  built once in all six areas looked at. These are the dark fight, with 3
  meshes left out, the Sith base, with 8 left out, and the Peragus hangar,
  the Ebon Hawk, the Dune Sea and the cantina, with none left out. Each
  list was made at most 33 times in 300 frames, all of them in the first
  frames after the area loaded.
- **A pillar.** Fourteen areas were looked at for a free-standing pillar
  near where a fight can be staged. Leviathan's hangar, the Star Forge,
  Korriban's academy, the Sith base on Taris, Manaan's Ahto West and the
  Rakatan temple have columns, but they stand against walls or on ledges.
  The Star Forge hall of the `sandbox` stage has free-standing struts
  beside its walkway. A fight there was drawn at 25 moments under Ultra
  and High (`pillar-ultra/`, `pillar-high/`). The bolts flew along the
  walkway and none came near a strut, so no strut's shadow was caught.
- **A bolt along a wall, and a column.** In the Sith base's corridor the
  player stood in a recess of the wall, and three troopers fired along the
  wall at the player (`arena@tar_m10aa`, `wall-pair2/`). Thirty moments
  0.1 s apart were each drawn under Ultra, then under High, in one run. Six
  of them caught a bolt in flight beside the wall, at chest height. At 4.5
  s the bolt flies past a column that juts out of the wall. Under
  Ultra the column shades the wall panel and the floor beyond it from the
  bolt, with a soft edge. Under High the bolt lights them evenly
  (`bolt-wall-sheet.png`). The wall beside the bolt is lit evenly under
  both settings. No speckle, no black band and no bright leak showed. A
  free-standing pillar's shadow is still not captured.

#### What 6c to 6e cost together

Measured on 2026-11-13 on the 4070 Ti SUPER, with `kjk bench --gpu` in a
1920x1080 window with v-sync off, 10 seconds a scene. Three setups took
turns, twice. Ultra is the preset `shadows-ultra`. High is
`shadows-high-taa`, which is High with TAA. High without ray queries is
the same preset with `KJK_RAYS=off`, which runs main's shader. Each cell
holds the two rounds.

| Scene | Ultra fps | High fps | High without ray queries fps | Ultra render thread ms | High render thread ms |
|---|---|---|---|---|---|
| Peragus hangar | 136, 93 | 133, 132 | 117, 126 | 6.8, 10.2 | 7.0, 7.0 |
| Ebon Hawk | 81, 81 | 122, 120 | 115, 192 | 11.8, 11.8 | 7.7, 7.8 |
| Dark fight | 56, 57 | 113, 111 | 163, 112 | 17.0, 16.8 | 8.3, 8.4 |
| Dune Sea | 82, 84 | 102, 152 | 106, 106 | 11.6, 11.3 | 9.2, 6.0 |
| Cantina | 98, 101 | 243, 151 | 151, 153 | 9.7, 9.4 | 3.0, 6.1 |

| Scene | Ultra opaque pass GPU ms | High opaque pass GPU ms | Ultra structures GPU ms | Ultra all timed passes ms | High all timed passes ms |
|---|---|---|---|---|---|
| Peragus hangar | 1.31, 1.40 | 1.36, 1.38 | 0.49, 0.50 | 2.06, 2.26 | 1.63, 1.64 |
| Ebon Hawk | 1.29, 1.27 | 1.50, 1.51 | 0.49, 0.49 | 2.41, 2.40 | 1.80, 1.83 |
| Dark fight | 2.55, 2.57 | 1.23, 1.31 | 0.52, 0.54 | 3.85, 3.94 | 1.58, 1.62 |
| Dune Sea | 0.56, 0.54 | 0.47, 0.40 | 0.41, 0.39 | 2.43, 2.60 | 1.89, 1.94 |
| Cantina | 1.21, 1.28 | 0.96, 1.08 | 0.48, 0.48 | 2.20, 2.09 | 1.13, 1.29 |

- Ultra is too slow on the CPU as it stands. The render thread takes 3
  to 9 ms a frame more than under High in four of the five scenes. The
  dark fight runs at 56 to 57 fps under Ultra and 111 to 113 fps under
  High. The main thread also takes more, 7.7 to 8.1 ms against 5.9 ms
  in the dark fight.
- What takes that time on the CPU was found afterwards with traces,
  and is told under "Why Ultra is slow on the CPU" below. The trial,
  with at most 14 instances, added only 0.1 to 0.4 ms to the render
  thread.
- The frame rates of this machine are bimodal, as step 0 found. High
  ran the cantina at 243 fps in one round and 151 fps in the other.
  The Ultra numbers are steady between rounds in four scenes, so the
  slowdown is real, but its size against High is known only roughly.
- On the GPU the structures take 0.39 to 0.54 ms a frame. The
  characters' builds are 0.26 to 0.34 ms of that, and the top-level
  build 0.13 to 0.20 ms. The top-level build took 0.03 to 0.05 ms in
  the trial, before the rooms were in it.
- The opaque pass costs about the same under Ultra as under High in
  the Peragus hangar and the Ebon Hawk. It costs 0.1 to 0.3 ms more in
  the Dune Sea and the cantina. In the dark fight it doubles, from 1.2
  to 1.3 ms to 2.55 ms. There every fight's light now traces a ray at
  every surface it lights, for the walls.
- High with TAA on this card reads 0.4 to 0.6 ms more in the opaque
  pass than High without ray queries in the Peragus hangar and the Ebon
  Hawk, and the same or less in the other three scenes. Whether the ray
  shader's unused branch costs High anything is not settled.
- 3840x2160 was not run. The bench takes another size only in a
  headless run, and a headless run's GPU times read too high (6b).

#### Why Ultra is slow on the CPU

Found on 2026-11-13 with CPU traces of the dark fight (`kjk bench
--trace`, 6 seconds each, in a window). A trace slows the game, so
these numbers compare with each other and not with the tables above.

| Setup | Instances in the top-level structure | wgpu's submit, ms a frame | fps under the trace |
|---|---|---|---|
| High with TAA | none | 0.35 | 81 |
| Ultra | about 705 | 5.35 | 68 |
| Ultra with the rooms left out | about 250 | 1.76 | 63 |
| Ultra with the rooms and placeables left out | about 32 | 0.56 | 71 |

- The time goes to wgpu's submit of the frame's commands. It takes
  5.35 ms a frame under Ultra and 0.35 ms under High.
- The cause is in wgpu 29.0.4's source (`command/pass.rs`,
  `command/ray_tracing.rs`). Each time a pass sets a bind group that
  holds a top-level structure, wgpu notes one use of that structure.
  At submit it walks every instance of the structure for every such
  use, twice. The Odyssey material's bind group holds the structure,
  and a frame sets a few hundred materials. So the cost is the count of
  materials set times the count of instances.
- Leaving instances out confirmed it. Without the rooms the submit
  takes 1.76 ms. Without the rooms and the placeables it takes 0.56 ms.
- The instances are the lever, because Bevy sets the material's bind
  group for each material and kjk cannot change that. The rooms can be
  one structure of many geometries, built once as the area loads. The
  placeables can be one structure, built again only when one of them
  moves. The dark fight would then hold about 35 instances and not 705.
  A structure's geometry may carry its own transform, and a hit names
  its geometry, so nothing the shader reads today is lost. The rooms'
  hits need only the bit that marks a room.
- Two smaller costs remain after that. Building the 32 characters'
  structures takes kjk's `ray_structures::build` about 1 ms a frame on
  the render thread, with or without the rooms. Bevy's writing of the
  passes takes 1.8 ms under Ultra against 1.4 ms under High.
- The frame rate under the trace does not rise in step with the
  submit's time. This machine's two speeds and the trace's own cost
  hide it. The fix has to be judged by a bench without a trace.

Built on 2026-11-13, on `feat/ray-structures` (`ray_structures.rs`). The
submit's time and the frame rate are not measured.

- **The rooms.** An area's room meshes are the geometries of one merged
  structure. A structure holds at most 1,024 geometries, so a larger area
  gets a few. Each geometry is placed in the world by its own matrix,
  which wgpu 29 reads from a transform buffer (`USE_TRANSFORM`). The
  structure is built again only once the list of room meshes, and the
  count of them on the GPU, have stayed the same for 30 frames. The old
  one is traced meanwhile. When none of its meshes is listed any more, the
  area was left, and it is dropped at once.
- **The placeables and the doors.** A placeable's mesh that has stood
  still for 30 frames becomes a geometry of the placeables' merged
  structure. A door's mesh does the same in the doors' merged structure,
  so a door that opens does not make the placeables' structure be built
  again. A merged structure is built again when a mesh in it goes, moves
  or changes its mesh. It is also built again when meshes that stood still
  for 30 frames are not in it yet, at most once in 30 frames. A mesh that
  moves, or that appeared within the last 30 frames, keeps an instance of
  its own, as before.
- **What the shader reads.** A hit on a merged structure of placeables or
  doors names its geometry. The shader reads that geometry's number from
  a buffer the material binds at binding 18 (`ray_number`). The rules of
  `ray_blocks` then run on it as before. A room's hit needs only the
  room's bit.
- **What is left as it was.** The characters' skinned structures are built
  each frame, one for each job. A character's weapon or helmet keeps an
  instance of its own, as it moves with the character.

| Scene | Instances before | Instances after | Characters' structures | Weapons and helmets | Merged structures |
|---|---|---|---|---|---|
| Cantina, game time held | 927 | 66 | 46 | 17 | 3 |
| Cantina, game time running | 927 | 80 | 46 | 17 | 3 |
| Peragus hangar | 477 | 46 | 21 | 22 | 3 |
| Dark fight | 705 | 56 | 32 | 21 | 3 |

- The three merged structures in each scene are the rooms', the
  placeables' and the doors'. The cantina's rooms are 815 geometries, the
  Peragus hangar's 265 and the dark fight's 455. The doors' meshes are 14,
  105 and 138, and the placeables' meshes 35, 64 and 59.
- wgpu counts one dependency of the top-level structure for each
  instance, and the dump's `dependencies` equals `instances` in every run.
- In the cantina, 14 placeables' meshes move while game time runs. They
  keep instances of their own. In 900 frames of the cantina with game time
  running, no merged structure was built again. Each kind was built once
  as the area loaded. Holding game time then built the placeables'
  structure once more, with the 14 meshes in it.
- Before and after were drawn in one run, with game time held, by a
  temporary switch that has been taken out again. The switch traced each
  mesh as an instance of its own until a set frame, and the merged
  structures after it. In the cantina, the Peragus hangar and the dark
  fight, the pictures before and after the switch differed as little as
  two pictures before it. The cantina and the Peragus hangar also matched
  pictures of the build before the change within the noise between two
  runs, plain and with `KJK_SHADOW_DEBUG=1`. The dark fight differs too
  much between two runs for that test.
- Going from the cantina to the Upper City and back dropped the cantina's
  merged structures, built the Upper City's, and built the cantina's
  again. Going from Ultra to High and back dropped every structure and
  built them again. No log had an error.
- A door that opens with a fight's light behind it was not staged.

Measured after the merge, on 2026-11-13, the same way as the tables
under "What 6c to 6e cost together": in a 1920x1080 window with v-sync
off, the three setups taking turns, two rounds in each cell. The machine
ran in its fast state in every run this time, so the rounds agree. The
earlier tables mixed its two states, so compare Ultra with High inside
one table and not across the two.

| Scene | Ultra fps | High fps | Ultra render thread ms | High render thread ms | Ultra opaque pass GPU ms | High opaque pass GPU ms | Ultra structures GPU ms |
|---|---|---|---|---|---|---|---|
| Peragus hangar | 189, 199 | 228, 229 | 4.71, 4.49 | 3.73, 3.72 | 0.94, 0.98 | 1.06, 1.07 | 0.31, 0.32 |
| Ebon Hawk | 179, 179 | 191, 193 | 5.06, 5.06 | 4.71, 4.66 | 1.09, 1.11 | 1.25, 1.24 | 0.32, 0.32 |
| Dark fight | 150, 147 | 154, 155 | 6.07, 6.17 | 5.25, 5.20 | 1.35, 1.51 | 1.56, 1.44 | 0.31, 0.29 |
| Dune Sea | 135, 136 | 155, 155 | 6.86, 6.79 | 5.89, 5.90 | 0.50, 0.50 | 0.41, 0.41 | 0.31, 0.31 |
| Cantina | 231, 232 | 197, 250 | 3.74, 3.74 | 2.96, 2.96 | 0.94, 0.94 | 1.10, 1.06 | 0.31, 0.31 |

- Ultra now runs within 3 to 17% of High's frame rate. The dark fight
  runs at 147 to 150 fps under Ultra and 154 to 155 fps under High.
  Before the merge it ran at 56 to 57 fps under Ultra.
- The render thread takes 0.35 to 1.0 ms a frame more under Ultra than
  under High. Before the merge it took 3 to 9 ms more.
- A trace of the dark fight under Ultra gives wgpu's submit 1.21 ms a
  frame. It was 5.35 ms before the merge, and it is 0.35 ms under High.
  kjk's `ray_structures::build` takes 0.54 ms, where it took 1.1 ms.
- The opaque pass no longer doubles in the dark fight. It takes 1.35 to
  1.51 ms under Ultra and 1.44 to 1.56 ms under High. One structure of
  455 geometries is quicker to trace than 455 instances.
- The structures take 0.29 to 0.32 ms a frame on the GPU in every
  scene. The top-level build is back to 0.06 to 0.10 ms.
- All timed GPU passes together take 0.1 to 0.3 ms more under Ultra
  than under High.
- High with TAA on this card reads up to 0.2 ms more in the opaque pass
  than High without ray queries, in the Ebon Hawk and the dark fight.
  In the other three scenes the two are equal. So the ray shader's
  unused branch may cost High up to 0.2 ms on a card with ray queries.
- What is left of the submit's cost is the 53 to 80 instances that
  remain, most of them the characters' own structures and their
  weapons and helmets. Putting all characters in one structure of many
  geometries would cut that again. It is not built.
- 3840x2160 and the 1050 Ti class tester have not run it. The tester's
  card has no ray queries, so only the High and Low paths matter there.

Measured on 2026-11-14 on the 4070 Ti SUPER, with `kjk bench --gpu`, 10
seconds a scene. The build has the sun's narrowed ray and the fix that
builds the rooms' structures while game time runs (6e). Ultra is the
preset `shadows-ultra`, and High is `shadows-high-taa`.

3840x2160 could not be run in a window. This machine's screen is
2560x1440. The bench sets the window's size through the settings'
`display.size`, and `display::fits_window` refuses a window larger than
the screen, so the window keeps its old size. So the scenes ran headless
at 3840x2160 (`--headless --size 3840x2160`). A headless run is paced at
60 frames a second, so its frame rate says nothing, and its GPU times read
higher than a window's (6b). The two settings took turns in two rounds,
Ultra first in the first round and High first in the second. Each cell
holds the two rounds.

| Scene, 3840x2160 headless | Ultra opaque pass GPU ms | High opaque pass GPU ms | Ultra structures GPU ms | Ultra all timed passes ms | High all timed passes ms | Ultra render thread ms | High render thread ms |
|---|---|---|---|---|---|---|---|
| Peragus hangar | 4.35, 4.33 | 4.76, 4.69 | 0.40, 0.40 | 5.86, 5.81 | 5.90, 5.86 | 4.30, 4.24 | 3.72, 3.65 |
| Ebon Hawk | 4.46, 4.28 | 4.76, 4.82 | 0.44, 0.42 | 5.93, 5.71 | 5.87, 5.91 | 4.97, 4.80 | 4.55, 4.39 |
| Dark fight | 5.16, 5.06 | 4.95, 4.36 | 0.29, 0.29 | 6.20, 6.06 | 5.73, 5.31 | 5.96, 5.72 | 5.02, 5.28 |
| Dune Sea | 1.95, 1.83 | 1.71, 1.96 | 0.44, 0.41 | 5.95, 5.94 | 6.05, 6.03 | 5.68, 5.89 | 4.83, 5.13 |

- At 3840x2160 the opaque pass costs Ultra 0.4 ms less than High in the
  Peragus hangar and the Ebon Hawk. One ray toward each light is cheaper
  there than High's capsules and tile lookups. In the dark fight Ultra
  costs 0.2 to 0.7 ms more, as every fight's light traces against the
  walls. In the Dune Sea the two are equal within the rounds' spread.
- The structures take 0.29 to 0.44 ms a frame. Their cost does not grow
  with the frame's size, as they hold triangles and not pixels.
- All timed GPU passes together are within 0.2 ms of each other in three
  scenes. In the dark fight Ultra takes 0.5 to 0.8 ms more.
- So at 3840x2160 on this card, Ultra costs about the same on the GPU as
  High, and up to 0.8 ms more in a fight in a dark hall.

The same build ran once each at 1920x1080 in a window, Ultra first.

| Scene, 1920x1080 window | Ultra fps | High fps | Ultra render thread ms | High render thread ms | Ultra opaque pass GPU ms | High opaque pass GPU ms | Ultra structures GPU ms |
|---|---|---|---|---|---|---|---|
| Peragus hangar | 213 | 242 | 4.17 | 3.54 | 0.93 | 1.10 | 0.33 |
| Ebon Hawk | 186 | 200 | 4.88 | 4.52 | 0.96 | 1.11 | 0.29 |
| Dark fight | 150 | 167 | 5.93 | 4.99 | 1.63 | 1.32 | 0.29 |
| Dune Sea | 140 | 160 | 6.62 | 5.73 | 0.51 | 0.46 | 0.31 |

- The narrowed sun ray costs nothing that can be measured. In the Dune Sea
  the opaque pass takes 0.51 ms under Ultra, where it took 0.50 ms on
  2026-11-13.
- The machine ran faster than on 2026-11-13, under both settings. Ultra
  runs at 88 to 93% of High's frame rate, as it did then.
- In the dark fight the opaque pass now takes 1.63 ms under Ultra, where
  it took 1.35 to 1.51 ms. The rooms' structures were not built in that
  bench, as told under 6e, so its fight lights went through the walls.
  The render thread takes 5.93 ms, where it took 6.07 to 6.17 ms. In the
  Peragus hangar, where the rooms were not built either, the opaque pass
  is unchanged at 0.93 ms.
- The bench's output is in the worktree's
  `target/ultra-captures/final/bench/`.

#### Later, not scheduled

- Bounced light, so a bolt puts red on the wall beside the one it
  lights. A bounce needs the colour of what a ray hits, so every texture
  must be reachable from one shader, and the result needs a denoiser.
  It would replace step 5's probes for the fight lights only.
- DLSS Ray Reconstruction, NVIDIA's denoiser. Bevy has it for its own
  ray-traced lighting. It matters only once kjk has bounced light.
- A linear frame. An upscaler or a denoiser made for linear colour may
  ghost on the encoded frame (decision 6). A decode in front of it and
  an encode behind it is the first thing to try.
- Clustered light lists. They win past the 32 lights kjk carries today,
  and cost more below that.

## Settings

| Setting | Shadows | Needs |
|---|---|---|
| Off | none | |
| Low | capsules and boxes, under the lamps, the sun and 2 of a fight's lights | |
| High (default) | every light casts, and the 12 characters nearest the camera and the placeables cast from tiles | |
| Ultra | every light casts by one ray against the characters' and placeables' own meshes, and no tile is drawn | a card with ray queries, and TAA or DLSS; otherwise it draws as High (step 6d) |

These are the options' Shadows (`display.shadows`) since #344, step 4 and
step 6d.

- Soft Shadows is gone. Every setting's shadows are soft.
- At Low, the 2 of a fight's lights that light a character most cast from
  it, and each traces 3 merged capsules for that character.
- The plan's first table had a Medium, with capsules under every light.
  That became High in #344. At High a character without a tile still casts
  from its capsules.
- The first table also gave High contact shadows. They are not built.

## Risks and open questions

- **Capsule cost in crowds.** 106per has many casters around the player. The
  per-character sphere test has to reject most of them per fragment; if it
  does not, capsules need a coarse grid. Measured in step 2.
- **The blocked-light estimate.** The lightmap holds every lamp's light
  together; our estimate of one lamp's share can be off where the bake had
  bounces or coloured fill. Captures decide the floor.
- **The prepass's CPU cost** on a weak machine under TAA is unmeasured. It
  decides whether TAA can become the default anti-aliasing.
- **MSAA.** If we keep it, every screen-space feature needs an MSAA answer
  or is off there. If TAA holds up on the 1050 Ti, retiring MSAA makes steps
  3 and 4 simpler. To decide after step 0.
- **Bevy.** Step 4 goes around Bevy's shadow views; a Bevy upgrade may give
  us a cheaper way or break ours. wgpu's ray queries and Bevy's ray-traced
  lighting were experimental when this was written: check again before
  step 6. Checked on 2026-11-13, and both still are. wgpu cannot refit an
  acceleration structure, which step 6 pays for with a build each frame.
  A wgpu that can refit makes step 6c cheaper, so check each upgrade.
- **The Classic look.** The owner's screenshots pin it. The approved tests
  compare no rendered pixels. Since step 3, `kjk compare-pictures` compares
  a Classic picture with another.

## Branches

- Step 0 and step 1: from `origin/main`, small PRs.
- Step 2: from `origin/main`. It was built on step 1's branch instead,
  because both change the same shader.
- Step 3: from `origin/main`, in one PR (`feat/hdr-bloom-glow`).
- Step 4: from `origin/main`.
- Step 6: each part from `origin/main`. 6a needs nothing from the others.
  6b is a trial and is not merged. 6c, 6d and 6e follow one another.
