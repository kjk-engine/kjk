# Lighting and shadows: the plan

Status: proposed, 2026-11-11. Steps 0, 1 and 2 are built and measured on
the 4070 Ti SUPER. So is step 3, on 2026-11-12.
Step 4's shadow tiles were built on 2026-11-12 too. So was its compute
pass, for the tiles alone. It is off by default, because on the 4070 Ti
SUPER it costs more than it saves. Its contact shadows are not built. On
the 4070 Ti SUPER the tiles do not cost less than the capsules.
Step 0's results are under "Measured in step 0", and steps 1, 2, 3 and 4
have theirs under their own headings. The 1050 Ti runs and the TAA
baselines are still owed. Nothing else here is built or measured yet unless a line
says so.

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
  thread, shared among the machine's threads. A lightmap texel already is
  the light arriving on its surface. So for each direction a probe casts
  five rays up to 6 m the other way: one straight, and four leaning
  0.6 radians off it. A ray that meets a lightmapped room surface facing
  the probe's direction takes that surface's lightmap colour, read at a mip
  level no larger than 128 texels. Nearer hits weigh more. Other opaque room
  surfaces only block rays. A direction whose rays meet nothing takes the
  mean of the directions that did. A surface's facing comes from its
  triangle's corners, because some models store no normals.
- An area with no lightmapped room mesh, such as the Dune Sea, gets no
  probes. Everything there keeps the flat ambient.
- Each frame every placed model that is not a room gets the blend of the
  probes within 1.6 m of the point 1 m above its origin, in the room it
  stands in (`FightRooms::room_of`). Light never blends across rooms. A
  room with no probe near hands over to the rooms linked with it. A head or
  a weapon hung on a body is lit by the body's light. A weapon armed during
  play takes its body's number once it is hung.
- The lamps are not counted twice. Each lamp of the shader's list that is
  the scene's own (not a carried light, a body's light or a fight's light)
  is taken out of the six colours by what the shader adds for it to a
  surface facing each way, never below black. A lamp too far from the
  camera to be in the list stays in the probe light.
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

### 6. Ray tracing and other upscalers

Not scheduled. What the steps above leave ready:

- Skinned-once vertex buffers (step 4) are what acceleration structures for
  characters are built and refitted from. Step 4's compute pass makes
  them for the characters that have tiles (`skin_once.rs`), when it is
  turned on. Ray tracing needs them for every character in view, and
  with their triangles' indices beside them.
- The per-light visibility value (step 2) is what a shadow ray replaces.
- Depth, motion vectors and HDR colour (step 3, #323) are what denoisers
  and upscalers need. They want the colour linear, and step 3's frame is
  encoded (decision 6). A decode pass in front of them gives it, or the
  frame moves to linear then. The surface shader's light is worked out in
  linear either way.
- Lightmaps and probes (step 5) are what probe-based or ray-traced global
  illumination would replace, so a bolt can bounce red onto a wall.
- FSR, XeSS and MetalFX fit the upscaler slot. Bevy 0.19.1 ships none of
  them; until one is ported, TAA at a reduced render size plus the
  sharpening pass Bevy has is the answer for cards without DLSS.

Things to do only when needed: clustered light lists (a win past the 32
lights we carry today, overhead below that) and room meshes merged for
acceleration structures.

## Settings

| Setting | Shadows | Needs |
|---|---|---|
| Off | none | |
| Low | capsules and boxes, under the lamps, the sun and 2 of a fight's lights | |
| High (default) | every light casts, and the 12 characters nearest the camera and the placeables cast from tiles | |
| Ultra | ray-traced visibility | not built; a ray-tracing card |

These are the options' Shadows (`display.shadows`) since #344 and step 4.

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
  step 6.
- **The Classic look.** The owner's screenshots pin it. The approved tests
  compare no rendered pixels. Since step 3, `kjk compare-pictures` compares
  a Classic picture with another.

## Branches

- Step 0 and step 1: from `origin/main`, small PRs.
- Step 2: from `origin/main`. It was built on step 1's branch instead,
  because both change the same shader.
- Step 3: from `origin/main`, in one PR (`feat/hdr-bloom-glow`).
- Step 4: from `origin/main`.
