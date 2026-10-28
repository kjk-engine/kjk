# Models' own effects (`game::model_fx`, `game::own_fx`)

What a model's clips key beyond places and turns, read from its own MDL
(`ModelFx::read`): its meshes' alpha and self-illumination, and its
particle emitters. A view's look only; the sim knows none of it beyond a
placeable's look (`sim::look`). Drawn for a minigame's models
(`minigame_view`, MINIGAMES.md), a module's rooms and its placeables, its
creatures' bodies, and TSL's rain and snow (six `fx_rain` or `fx_snow`
models round the camera: GRASS-WEATHER.md, which also holds grass,
lightning and how to see them).

- A placeable's clips are its model's and its supermodel chain's (the
  executable's lookup by name): K1's twelve "Sitting" placeables (a
  creature's model in `AnimationState` 3, which plays `animations.2da`
  row 307 `dead`: the chain's seated pose) sit instead of standing in
  their bind pose; TSL's two stunt Kreias lie.
- A creature's body draws its emitters, keyed by the clips the cast plays
  on it (its own, and a script's over them, in a conversation or out of
  one) and burst by their `detonate` events: the welding droid's sparks,
  flash and smoke under `weld`, a war droid's sparks as it dies and lies.
  K1 has 144 creatures whose bodies carry emitters (TSL 155): droids and
  turrets, and the Manaan diving suit's bubbles. A humanoid's body keys
  them by the KOTOR clips laid over its JA pose (a script's, its gait's).
- What a model's clips key on its emitters, meshes and lights is read
  from its own clips and then its supermodel chain's (a clip of its own
  winning by name): the welding droid's `weld`, which keys its smoke's
  birth rate and its light, is its supermodel `p_t3m3`'s.
- A creature body's lights (K1: 99 creatures, the war droids among them)
  go where their nodes stand each frame and take the colour, radius and
  multiplier its clips key (`own_fx::BodyLights`, beside the scene's
  candidates): the welding droid's `auroralight01`, dark at rest, flickers
  under `weld`.
- `kjk clips k1|tsl` counts them: every placeable's look clip (its own,
  through its supermodel, or missing), every body emitter, and every
  animation a creature's own scripts play and whether its body has the
  clip.

## Emitters (`model_fx`)
- Values are reone's controller numbers on the emitter's node, as the
  clips playing key them over that; each stays where the last clip to key
  it left it (`Values`).
- `Fountain` bears nothing while its birthrate is under one a second,
  whatever its random birthrate (the astro droids' and T3-M4's smoke,
  birthrate 0 and random 3 at rest, starts only when a clip keys a rate);
  otherwise it bears at its birthrate moved, each step, up or down by a
  whole number below its random birthrate, the fraction of a birth kept
  for the next step (QUIRKS Q-0279: the executable's count follows the
  frame rate);
  `Single` keeps one particle (another when it dies, if it loops);
  `Explosion` bursts on a clip's `detonate` event (one at a loop's last
  frame too, as the clip wraps); `Lightning` draws bolts (below).
- An emitter keeps as many particles as its stream holds, none more
  (`particle_limit`: birthrate plus random birthrate, times life, rounded
  up, and the one being born), from the most its rest values and clips give
  any of the three; one whose particles never die (a negative life) and
  that has births keeps the most; an explosion keeps at least one whole
  burst; none keeps more than 4096, a frame guard. TSL's executable
  has no limit (its particles are made as they are born); K1's was not read
  [verify].
- A particle is born within the emitter's size, heads off its z by up to
  half its spread and anywhere round it, as fast as its velocity up or down
  by up to its random one (the executable's birth), moves, then falls at
  its mass × 9.81 m/s² (the executable's step), spins, lives its life
  expectancy (0: dies at once). Size, colour and alpha go start, middle, end; frames walk the
  texture's grid round and round at the emitter's `fps` (104), else once
  over the life, from a random frame with header flag 0x20. A life of −1
  is for ever: size, colour and alpha go round with the frames' run,
  else hold their start (reone; Manaan's birds flapping).
- Header flags: 0x40 keeps them in the emitter's space, else where born
  (0x80 adds its velocity); 0x10 bounces them off what their way meets
  (`model_fx::Surfaces`; the executable traces the rooms, then every
  model's meshes but the emitter's own): turned back off the surface's
  normal, slowed by the bounce coefficient, put 5 mm out, and lying 2 cm
  out, still, once slower than √0.5 m/s. Script effects meet the level and
  the bodies (`particle_surfaces`: capsules between the hit locations'
  nodes [verify: the executable's are mesh triangles]), the minigames their
  thing's floor, rooms' and placeables' emitters nothing yet; 0x1 sends
  them to its reference node (its child: point-to-point), on a curve from
  where each was born over its life with 0x2 (`p2p_bezier2`/`3` standing
  its inner points off along the emitter's z [verify the curve]), else
  pulled there by `grav` (116). The beams' flame spray is one.
- A `Lightning` emitter makes its bolt anew every `lightningDelay`
  (reone's reading): `lightningSubDiv` + 1 segments from a random point of
  its size to its reference node (its child), each strayed across by half
  its `lightningRadius`, drawn `lightningScale` wide facing the camera; the
  beams, which set neither, by their particles' starting size [verify].
- A clip's scale keys on a node scale it (the pose, `fx_model`).
- Quads face the camera (or the ground, or the emitter). A `Motion_Blur`
  particle is a streak, as the K1 executable draws one: each keeps a tail
  point that follows it, closing `min(dt, blurLength) / blurLength` of
  the gap a step, and its camera-facing quad runs from the tail to it
  along that gap as the screen sees it, its size wide (the welding
  droid's sparks; reone draws an emitter-facing quad five times as long)
  [verify the executable's fade of the tail's alpha, not kept].
  `Aligned_to_World_Z` stands them up: their side is
  the camera's (turned by the particle's spin), their height world z (down
  for an upside-down camera): the star map's glyphs and `plc_holoart` turn
  to every walker yet stay upright, and are edge-on from above.
  `Aligned_to_Particle_Dir` (the surf's waves and splash) lays each long
  along where its particle is going, as wide as it is long, unspun, its
  side the emitter's x turned square to that [verify: the executable keeps
  each particle's heading as a quaternion, and its roll about the heading
  is not read]. Quads are added by their texture's alpha (`Lighten`:
  KOTOR's flares and sparks are white, their shape in alpha) or blended in
  encoded values as the game did (`scene::encoded_view`); a chunk emitter
  draws its model; `fx_distortion` with punch-through is a heat haze
  (`heat_haze`) [verify by eye]; any other punch-through emitter is drawn
  whole, unblended, where its alpha is above nothing, as the executable
  draws it (blending off, its alpha test at its default `GL_GREATER` 0).
  `Aligned_to_World_Z` and `Aligned_to_Particle_Dir` follow TSL's
  executable (K1's [verify]).

## Rooms and placeables (`own_fx`, `viewer::OwnFx`)
- `viewer::module_scene` reads each room's and placeable's MDL once for
  both its model and its effects; a room with `animloop…` or
  `scriptloopNN` clips or emitters, and a placeable with clips or
  emitters, gets an `OwnFx` (and
  its emitters' textures join the scene's). A placeable made in play
  (`made`) gets one as it is spawned; `own_fx` binds each as its model
  comes and drops one whose model left.
- Every `animloop…` clip loops at once from the scene's start, on the
  animations' clock (`anim` holds it), each over the rest pose; a node two
  key shows the later one's [verify: reone's order is a hash map's]. Only
  nodes under the model's animation root move (reone).
- A room's `scriptloopNN` clip a script plays on it (`PlayRoomAnimation`:
  the snapshot's `AreaSound::room`, the room by `sim::room_key` of its
  name, one at a time) loops from when it began, on the sim's clock, after
  its `animloop`s, so its keys show over theirs: its nodes, its meshes'
  alpha and self-illumination, its emitters' values and `detonate`
  events, in every module (`own_fx`). Several rooms' emitters are keyed by
  such a clip alone, none at rest: Peragus's sparks and ship smoke (TSL
  `103pert` `scriptloop01`: `fx_Star` sparks on a blast door; `104perg`
  `scriptloop02`/`03`: the ship's and fuel smoke), the Ravager's flames
  and smoke (`851nih02` `scriptloop07`/`08`; the Omen smoke of `851nih46`
  and `853nih01`, `scriptloop01`–`06`: 01 and 04 hold it off, 02 and 05
  build it up, 03 and 06 hold it), the turret's sky planes (K1
  `m12ab_01a`). Where no sim runs (`kjk module`) none plays.
- A placeable plays the clip its look picks, as KOTOR's executable plays
  them (its `animations.2da` placeable rows; read from the Mac executable,
  2026-10-06). Its look (`ObjectState::state`, `sim::look`): its
  `AnimationState` as placed (0 `default`, 1 `open`, 2 `close`, 3 `dead`,
  4 `on`, 5 `off`), then a script's `PlayAnimation` /
  `ActionPlayAnimation` (200 on, 201 off, 202 open, 203 closed, 204–213
  `animloop01`–`10`), a container opened (open) and let go (closed),
  broken by blows (a container open, any other dead). Each look loops its
  clip; a change plays its way there once first: to on `off2on`, to off
  `on2off`, to open `close2open`, to closed `open2close` (from open only),
  to dead `die`, back to default the way it came; none to a loop. A look
  its model has no clip for is not taken up (the executable drops it);
  where the scene is built `default` stands in. A blow while it is closed
  plays `damage` once. Each change sounds as the executable's do
  (`placeableobjsnds.2da` by `SoundAppType`: opened, closed, used for on
  and off, destroyed; `ambience`).
- The Star Forge's captive Jedi (`sta_m45ad`, `plc_endcorps`): `default`
  floats the body in its stasis field (19 emitters: `blueupstream`, the
  `OmenEmitter`s' bolts, sparks, `Energy_stream`, the `Bfuel` ribbons on
  their turning dummies); Malak's drain (`k_psta_cutdrainN`, a 201) plays
  `on2off` (the field flares, `SuperNova` bursts) and the pod stands empty
  (`off`).
- The particles hang under the model's root, so a room's culling hides
  them; a hidden model's still step (at most 0.25 s), unseen, and bounce
  off nothing [verify]. A lightning or point-to-point emitter aims at its
  reference node as posed (`model_fx::aim`, shared with `fx_model`).
  `dump`'s `room_fx` and `placeable_fx`: each model's clips, look and what
  it plays, `posed` (how many of its clips are bound to its nodes), lit
  meshes, emitters, particles and whether it is shown.

```bash
kjk play --stage tat_m17mg --headless --cmds 'wait 3s; screenshot sand; dump; quit'
kjk module k1 manm26mg --headless --cmds 'anim 2; camera 215 830 14 90 3 60; screenshot birds; quit'
```
Tatooine's track (`m17mg_01c`): 35 fountains of blowing sand (`fx_sand`).
Manaan's (`m26mg_01c`): 72 single gulls (`LMA_birdsheet`) flown by
`animloop1` (63.8 s) and `animloop2` (6.7 s).

```bash
kjk play --stage sta_m45ad --arrive sta_way_captive2 --run k_psta_cutdrain2 --headless \
  --cmds 'wait world; camera 215.5 270 2.6 48 2 55; wait 1s; screenshot drained; dump; quit'
```

## Scripts' visual effects (`script_fx`)
- A visual effect a script applies (`EffectVisualEffect` on an object or
  at a place, `EffectBeam`) is an event on the character it lands on
  (`Outcome::Visual`, with how long it lasts and a beam's source; a
  minigame's thing hands it to the party's leader at the thing's place).
  One on a door or placeable, which has no fighter, the module keeps
  (`sim::visuals`, the snapshot's `visuals`) until it ends, a fleeting
  one for `SEEN_MS` (1 s) so every view sees it; a new module begins
  with none. The executable looks on a placeable for nodes none of K1's
  placeable models has (`<name>_impact`, `_hand`, `_head`; read from the
  Mac executable), so its models, and a beam's ends to or from it, stand
  at its root [verify an impact's: the executable skips one it finds no
  node for]. The Star Forge's drain beams run between invisible
  placeables (`bp_malak_target`, `bp_jedi_target`) and from Malak to a
  captive.
- Its row of `visualeffects.2da` is found by its label (the effect's
  number). Its models (`Imp_Root_M_Node` at the feet, `Imp_Impact_Node` at
  the middle, `Imp_HeadCon_Node` at the head; a place given puts them
  there) play `impact` once (else their first clip) and `SoundImpact`
  sounds. One applied for a while or for good keeps them on the character
  (following it): `duration` (else their own clip, the shields' `default`)
  round and round while it lasts, `SoundDuration` held, then `cessation`
  once, its end models (`Ces_*_Node`) and `SoundCessastion` as it ends.
- A beam (`ProgFX_Duration` 608 to 621) draws the model the game's
  executable picks for it (`BEAMS`, read off the Mac build: lightning
  small and large, the droids' disable, destroy and stun, the death
  field, drain, flame spray, stun, cold and both ion rays, Force Storm's
  arc, the shock) at its source's hand, its +Y (the way every beam
  model's emitters fire) toward the character, its lightning and
  point-to-point emitters' reference nodes put on the character; a source
  channelling a power JA's rules drive is `force_view`'s. The second ion
  ray's rings (`fx_lensring`, punch-through, on the emitter's plane) are
  dark: the texture is near black where its alpha is above nothing [verify
  by eye].
- A glow (`ProgFX_Impact` or `ProgFX_Duration` 1401 to 1426) is the
  executable's: the character's solid meshes drawn again over themselves,
  each vertex 0.02 m out along its normal, with `fx_tex_01`…`26`
  (`fx_tex_13` skipped) or `fx_tex_stealth` by the meshes' own UVs (a
  flip-book at its rate), blended by the texture's TXI from the engine's
  table (additive `SRC_ALPHA, ONE`, punch-through `ONE, ZERO`, else alpha)
  (`glow_shell`). It lasts as the effect does; one an effect begins with,
  `FLASH_MS`: the executable's 1 s for every effect's beginning. The energy
  shields (1413 to 1421) are faint: their textures are (the blue's
  brightest texel is (8, 4, 57)).
- The rest of what the executable draws itself, by `ProgFX` code:
  1200 and 1201 put `v_fizzle_imp` at the hand and middle, 1202
  `v_fresist_imp` at the head (a Force power fizzled, resisted); 1500
  (Knight and Master Speed) eases the player's view to 135° over 0.75 s
  (half a cosine) and puts it back 4.75 s on (`OwnScreen`, `play_view`)
  [verify: the game's measure is its own camera's]; 1601 and 1602 (Force
  Speed) blur the player's view as far as the swoop's ratio 1 does
  (`speed_blur`); 1700 to 1702 hang `v_medal_dur` and Revan's masks from
  `medalhook`, `revmask1hook`, `revmask2hook` (the ceremony's clothes,
  Revan's models; none without the node); 1800 hides the character and
  shows the scene behind its shape, bent (`glow_shell`: the executable
  draws it into the stencil only and bends there [verify by eye: how
  far]). It has nothing for 2000 and 2001; 1300 (a MIRV) is a projectile.
- One lasting on a character is kept in the sim too (a `Modifies::Visual`
  among its lasting effects, by row and id; scripts see it as
  `EFFECT_TYPE_VISUALEFFECT`): a script removing it, or clearing the
  character's effects, ends it at once, its end models then; a character
  first seen (a new scene, a seat joining late) shows those it carries.
- Each model is hidden its first two frames, until its keyed meshes have
  materials of their own, and goes once it has ended and its clips and
  particles are done.
- The console's `vfx ROW TAG [SECONDS] [FROM]` plays one by hand (`player`
  the first seat's body), for checking them.
