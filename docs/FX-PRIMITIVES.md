# JA's effect primitives, in our words (`asset_ja::fx`)

How each primitive kind of an `.efx` file is spawned and drawn. Units are
Quake's, times ms; client frames are 60 a second (Q-0005). Saber ignition
and lightning: `FX-SPEC.md`.

- **Spawning.** The origin is the effect's plus the template's offset
  along its axis (forward, left, up), or in the world's axes with
  `cheapOrgCalc`. `orgOnSphere` adds a point on the ellipsoid `radius`
  wide and `height` tall (in the world's axes, two random angles, 0–360°
  and 0–180°); `orgOnCylinder` one on the cylinder round the forward axis,
  `radius` out, within half the `height` either way, turned at random.
  With `axisFromSphere` the primitive's axis then runs out along that
  point. Particles, oriented particles and tails take a velocity and an
  acceleration (gravity on Z on top).
- **Particle**: a sprite square to the view, `size` its half width.
- **Oriented particle**: a quad square to the axis it was played along,
  turned by its rotation.
- **Line**: a quad from its origin to its end, square to the line and
  the eye's view of it (`fx::line`). **Electricity**: `FX-SPEC.md`,
  `fx::bolt`.
- **Tail**: a line from where the particle is back toward where it was
  the frame before, its `length` long (transitions as size, bits 12 on),
  `size` wide, square to the eye's lines to its ends.
- **Cylinder**: rings round its axis, `size2` (transitions as size, bits
  16 on) at its origin and `size` at `length` along; 32 segments, fewer
  the farther its middle is from the eye (`1 − d / 1024`, at least 8); its
  texture wraps once round, from the near ring (1) to the far (0).
- **Decal**: a square `size` (its start) round square to the axis, its
  texture turned by its rotation, laid a tenth of a unit off the surface
  (JA clips it to the surfaces under it; flat here). It stays until the
  hundredth after it is laid, then fades over a second.
- **FxRunner**: plays one of its `playFX` effects at its origin along its
  axis (here up to 64 per settling, read three deep).
- **Sound**: one of its files, at its origin, nobody's (`SOUND.md`).
- **Physics** (`usePhysics` or `expensivePhysics`, on particles, oriented
  particles and tails): each client frame's move is one point trace
  through the level, traced as the particle is drawn (`Spawned::collide`,
  the path kept by 60 Hz frames, so any frame rate draws the same). A
  particle that meets the level bounces, its velocity mirrored in the face
  and scaled by its `bounce`; leaving a floor (normal Z over 0.7) slower
  than 10 units a second it rests there (Q-0076, fixed). Bodies are not
  met. Impact and death effects, lights, flashes, emitters, camera shakes,
  force feedback and `setShaderTime` are not supported (the scene's notes
  list them).
