# Lighting and shadows: the plan

Status: proposed, 2026-11-11. Step 0 is built and measured on the 4070 Ti
SUPER. Its results are under "Measured in step 0". The 1050 Ti run and the
TAA baselines are still owed. Nothing else here is built or measured yet
unless a line says so.

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

Checked in the code on `origin/main` and `claude/dlss`
([#208](https://github.com/kjk-engine/kjk/pull/208)).

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

**What `claude/dlss` already has:**

- A depth and motion-vector prepass for every surface that writes depth
  (`odyssey_prepass.wgsl`, `temporal.rs`), on under TAA and DLSS only.
- TAA, DLAA and DLSS as anti-aliasing choices, with jitter, mip bias and cut
  resets.
- An HDR camera, on only under DLSS (Bevy's `Dlss` requires it), still with
  `Tonemapping::None`.

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
   same lighting.
5. **The upscaler is a slot.** It takes depth, motion vectors, jitter and HDR
   colour. DLSS is the first thing in it.
6. **No stencil shadow volumes.** wgpu has no geometry shaders, the games'
   meshes are not closed, the edges are hard only, and a third-person camera
   sits inside the volumes all the time.
7. **Measure before and after every step**, back to back, at 1920×1080.

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

One shader change plus a visibility flag. Needs nothing from `claude/dlss`.

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

### 3. HDR, bloom and glow

Built on `claude/dlss` (or on main once #208 has merged).

- The HDR target is on for every anti-aliasing mode.
- The authored look stays as authored: the base colour (lightmap, fixed
  function lights, texture) is worked out as today, then converted to linear
  at the end of the shader. Only the new light (fight lights, emitters) is
  added in linear HDR on top.
- A tonemapper whose curve leaves the base's range alone and rolls off
  above it.
- Bloom. Bolts, saber cores and muzzle flashes carry most of their
  brightness themselves, and the light they put on surfaces drops again.
- Shiny floors catch a highlight from a passing bolt.
- "Classic" turns all of step 3 off; the approved tests run in Classic and
  must not change.

The cost we know of: additive effects, alpha blends and fog were authored for
gamma-space blending. In a linear target they shift, and no tonemap curve
undoes that. Each effect family (sabers, particles, fog, distortions) needs a
look with captures side by side, and some will need retuning or blending in
the old space.

Done when: with fight lights off, new and Classic captures match within the
approved tests' tolerance for opaque surfaces; the effect families are signed
off one by one; bloom's cost is measured on the 1050 Ti.

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

Done when: High costs less than today's soft shadows in all three scenes and
close-up captures show hands and weapons in the shadow.

### 5. Characters lit by the room

- Light probes built from the lightmaps when an area loads, so a character
  takes the colour of where it stands.
- Part of the new look, off in Classic (the games light characters from the
  area's ambient and the light list).

### 6. Ray tracing and other upscalers

Not scheduled. What the steps above leave ready:

- Skinned-once vertex buffers (step 4) are what acceleration structures for
  characters are built and refitted from.
- The per-light visibility value (step 2) is what a shadow ray replaces.
- Depth, motion vectors and linear HDR colour (steps 3, `claude/dlss`) are
  what denoisers and upscalers need.
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
| Low | capsules, lamps and sun only | |
| Medium (default) | capsules, every light | |
| High | plus atlas tiles near the camera, contact shadows | contact shadows need TAA or DLSS |
| Ultra | ray-traced visibility | later; a ray-tracing card |

"Soft Shadows" goes: every tier is soft.

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
- **The Classic look.** The approved tests and the owner's screenshots pin
  it. Every step runs them.

## Branches

- Step 0 and step 1: from `origin/main`, small PRs.
- Step 2: from `origin/main`.
- Steps 3 and 4: on top of `claude/dlss`, or from main once #208 has merged.
