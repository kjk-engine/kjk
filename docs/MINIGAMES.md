# Minigames (`sim::minigame`, `script::minigame`, `game::minigame_view`)

KOTOR's swoop races (`tar_m03mg`, `manm26mg`, `tat_m17mg`) and the Ebon
Hawk's turret (`m12ab`, every space fight) are modules whose area's ARE has
a `MiniGame` struct. K1 has these four; every other module is not one. TSL has six:
the swoop races of Telos (`211tel`), Nar Shaddaa (`371nar`) and Onderon
(`510ond`), and the turrets of Peragus' hangar (`107per`), Dxun's
approach (`421dxn`) and Onderon (`505ond`); what its executable adds is
under "TSL's rules". Pazaak is no module but a conversation's game:
`PAZAAK.md`.

## The level's rules (`minigame::Rules`)
- `asset_odyssey::minigame` reads the ARE's struct (`Type` 1 the swoop, 2
  the turret; the player, the enemies, the obstacles: models, track, radius,
  hit points, speeds, tunnel, gun banks, `Scripts`) and the LYT's
  `trackcount` and `obstaclecount` places.
- `session::minigame` makes the sim's rules: each track's path is its
  model's `modelhook` node over its `track` clip (offsets from the hook's
  rest, by time), placed where the LYT puts the model; each obstacle stands
  where the LYT puts it, its hits the `hitbump` mesh's box across the ground.
  Its hooks' scripts join the module's library. In the level's digest only
  when there is one (`Level::with_minigame`).

## In play (`minigame::Course`, `Snapshot::minigame`, KJKS 37)
- The install starts a course for a minigame's level (`install::assemble`);
  the next module's install ends it. Entering and leaving are the games' own
  scripts' `StartNewModule` (the announcer, the galaxy map, the turret
  loaders; the race's finish, the last fighter's death).
- First in each step (`world::minigame::steer`, prediction included) the
  course moves by the leading seat's command, and every seat's player is
  parked (its command keeps where it looks):
  - the player's speed goes toward its range at its acceleration (up to the
    maximum, down to it from above; the minimum is no floor); its track
    plays at `speed / MovementPerSec` of its own pace, the enemies' at
    theirs; a track past its end loops (`Num_Loops` −1 for ever) or holds;
  - a swoop's strafe axis pushes it across at `LateralAccel` (scripts raise
    it with speed), the drift settling at a quarter of the push a second
    (`UseInertia`), held within the tunnel; a turret turns by how far the
    view turned since it began, its `Mouse` axes (yaw about z, pitch about x,
    from the ARE's start offset) within the tunnel;
  - fire's press raises the player's `OnFire` (K1's swoop's gear shift),
    or a swoop's `OnAccelerate` where it has one (TSL's races); the jump
    key's press a swoop's `OnBrake` (TSL's jump);
    held, its live gun banks fire along its aim; an enemy's fire at the
    player when it is within its sensing radius and its cone, strayed by its
    inaccuracy from a roll of the tick's (`SimRng::new(tick, …)`), so a
    predicting client shoots the same;
  - a bullet reaching a follower it may hit (its target bits: 1 the player,
    2 the enemies) is spent: that follower's `OnHitBullet`, else the
    default: its `OnDamage`, else the default damage (`SWMG_OnDamage`, as
    both executables have it: the hit's change, the bullet's damage, added
    to its hit points, held at the maximum when it adds; invulnerability
    no part of it); as they run out its `OnDeath` once, else the default
    death (gone); one with none left takes no more hits while its death
    hook runs;
  - the player newly touching a trigger enemy (a pad: spheres, across the
    ground) raises its `OnHitFollower`, then the pad's own (each hearing
    the other as its last follower hit); an obstacle (its box) the
    obstacle's own `OnHitFollower` if it has one, else the player's
    `OnHitObstacle` (both executables: one or the other) [verify: heights
    left out in K1's races; TSL's, below]; K1's `DoBumping` is 0, nothing
    bumps;
  - each live follower's `OnHeartbeat` runs every tick (the swoop's timer
    and finish line, the turret's compass), objects' `OnCreate` once.
- Hooks queue as the scripts' other events (`Pending`), the thing as the
  caller (`minigame::FIRST_ID` + its number: the player 0, the enemies, the
  obstacles), the hit they hear of as the event's number (`Course::facts`,
  numbered on across steps, the latest 64 kept, so a hook run late hears
  its own). A course that does not fit its level's rules (a stale save's)
  is not stepped, read or drawn.

## The routines (`script::minigame`)
The 25 the game calls, and the rest of their families: the player's speed,
range, acceleration, tunnel, offset; a follower's invulnerability
(`StartInvulnerability` for its `Invince_Period`); lateral acceleration, speed blur, camera clip;
`GetPlayer`, `GetEnemy(Count)`, `GetObstacle(Count)`, `GetPosition`; hit
points, as both executables have them (`SetFollowerHitPoints` only to
more than none, nothing heard, no death: a race's crash script setting
its swoop's to 0 leaves them; `AdjustFollowerHitPoints` a change heard
through the follower's damage hook, else the default damage; its third
argument never read); K1's executable refuses a negative minimum speed,
maximum speed and acceleration, TSL's only a negative maximum;
`SetPlayerInvincibility` and `GetPlayerInvincibility` stop the script
that calls them (both executables' handlers take an object first, which
the prototype never pushes: QUIRKS.md Q-0257); the step's hits (`GetLastFollowerHit`,
`GetLastObstacleHit`, `GetLastBulletHit…`, `GetLastHPChange`); the default
handlers (`OnDamage`, `OnBulletHit`, `OnDeath`, `OnObstacleHit`);
`PlayAnimation` (an overlay over the rest, else in place of the last one
that was not on each of the follower's models that has it: one a model
lacks leaves that model's playing, as the turret's compass `HudRot_*` on
`mgf_hud01` leaves its hull bar's `Health*` on `mgf_hud02`, the rules
knowing each model's clips; at most 128 kept, room for every name the
scripts play; a looping clip asked for again while it
plays goes on where it is, now over the rest: the swoop's heartbeat asks
for its `camshake` and `cDistL` every tick) and `RemoveAnimation`; `Is…`. They read the
course as the step's scripts left it and change a copy (`Out::minigame`)
the world keeps after them.

Beside them, for the race's timer and the turret's loss: `GetTimeHour`,
`…Minute`, `…Second`, `…Millisecond` (the IFO's `Mod_MinPerHour`, 2 where
unsaid; the day from hour 0 [verify]), `EndGame` (with its death screen,
the turret's loss: the party's loss box),
`PlayRoomAnimation` (a room's `scriptloop0<n>`, looping: the turret's
skybox by `K_TURRET_SKYBOX`; kept with the area's script state as
`AreaSound::room`, the room by its name's hash, and played by the view),
and
`FloatToString` with no decimals keeps its point ("4."; the heartbeat cuts
it off to name `camshake4`, `cDistL4`) [verify in the game].

## TSL's rules
What TSL's executable adds to K1's, from its ARE (`Rules::tsl`: its
`Scripts` structs carry `OnAccelerate`, `OnBrake` and `OnHitWorld`, set
or not) or its hooks; K1's minigames play as before.
- **The keys** (from the hooks): a swoop with `OnAccelerate` runs it on
  the fire key's press (the races' gear shift, `onaccelerate`), its
  `OnBrake` on the jump key's (the jump, `onjump`); `OnHitWorld` is read,
  never raised. The executable takes them from two of its own inputs
  (mouse buttons [verify which] and `keymap.2da`'s `MGshoot`, Space);
  here accelerate is the attack key and the jump our jump key.
- **The jump** (`SWMG_SetJumpSpeed`): the swoop's offset rises at its
  speed, which `JUMP_GRAVITY` (−45.8 m/s²) pulls down, held in the
  tunnel; the speed is not stopped on landing. The races' 30 m/s goes
  some 9.8 m up and lands in about 1.31 s. `SWMG_PlayerApplyForce(v)`:
  the drift across and the lateral acceleration both `v.x` (the
  heartbeat sets the acceleration again a tick on), the jump speed `v.z`;
  `v.y` unread (Nar Shaddaa's push pads, a glancing crash).
- **The offset turns with the track**: the executable hangs the swoop's
  models on the track's hook, and TSL's tracks curve.
- **Hits** along the step's ways: a pad where the player's and its
  spheres' ways come within their radii; an obstacle where the player's
  middle passes into its `hitbump` box (a jump clears it). The bumps
  (`DoBumping`, the races' mines and traffic), up to three passes a step,
  the first raising the hooks: an enemy that is no trigger whose way
  comes within the radii, out of its invulnerability, takes the player's
  `Bump_Damage` through its damage hook and is invulnerable its period;
  each hears of the other (`OnHitFollower`, the player's first); the
  player is pushed out along the line from each, to 1.1 m past touching,
  by their mean, held in the tunnel (the push is the world's, added to
  the track's offset, as the executable adds it); the player out of its
  invulnerability is invulnerable twice its period and, on the first
  pass, takes the most `Bump_Damage` of those it met and holds its speed
  the next step. `Bump_Plane` is read for nothing (the executable works
  it out and throws it away).
- **Routines**: `SWMG_SetLateralAccelerationPerSecond` scaled by the first
  PC's Dexterity modifier, three tenths a point (K1's keeps it as given);
  `SWMG_GetObjectName` a follower's track model's name, an obstacle's
  model's, as their MDLs spell them (`211TEL_MGM05`: the scripts tell a
  mine's track by "MGM"); `SWMG_DestroyMiniGameObject` takes an obstacle
  off the course (gone from the hits and the scene; an enemy stays);
  `SWMG_GetSwoopUpgrade` 0 for every slot (the upgrades are cut: no item
  ships); `SWMG_GetTrackPosition` where a follower's track has it;
  `SWMG_SetFollowerPosition` puts the player back at its track's start,
  its offset and drift none, whatever vector it is given (only dead code
  calls these two).
- **Forfeit**: a race begins `FORFEIT_SWOOPRACE` false; the first seat's
  `Action::Forfeit` (the executable's pause screen's quit) sets
  `GBL_QUIT_SWOOP`, which the races' heartbeats turn into a forfeit. No
  screen sends it yet (the executable's is its pause panel's key, which
  `keymap.2da` does not name [verify]).
- **107per** (Peragus' hangar, a turret with no fighters): each bolt is
  traced each step against the area: a wall stops it; a creature not of
  the party (its body) or a placeable (its pick volume, or its brushes)
  stops it and hears a user-defined event, the bolt's damage its number
  (the troopers' `k_sith_damage` takes it, the barrels' `a_barrelexplode`
  bursts). The executable names the module (`107PER`, ignoring case), so
  this does (`minigame::WORLD_TURRET`). Its ARE gives the bolts 0.2 m/s,
  as the executable flies them [verify in the game: they barely leave
  the barrel]; the hangar's sequence ends by its timer either way. Its
  bolts, and Onderon's (`505ond`) that hit a fighter, go at once: no
  `explode` (`minigame::bolts_burst`).
- **Engines**: a follower's `Sounds` `Engine` (Dxun's fighters') loops
  where it is while it lives.
- Checked by `sim::minigame::tsl_probes` (`kjk sim probes --only
  "minigame tsl"`, no install): the keys, the jump's arc, the push, the
  pads, the obstacles' one hook and heights, the bump's numbers, a jump
  clearing a mine, hit points, a destroyed obstacle, the encoding, the
  routines in a world (TSL's and K1's), the forfeit globals, 107per's
  bolts.

## Drawn (`game::minigame_view`)
- Once a course is presented its models are read off the frame (each
  follower's, each obstacle's, the bullets', the turret's camera rig) and
  spawned under the scene's root.
- Each frame after `play_view`: every thing between the last two ticks,
  turned as its track turns (the turret's rotating models as the view turns
  it); posed by the animations playing on it: the last one not an overlay
  that the model has, over where such clips last left each node (a clip
  keys only what it moves: each `Health` clip drops one more segment of
  the hull bar), then on each node what the last overlay to key it
  keys, added (the swoop's camera pulled back by `cDistL` while `camshake`
  shakes it) [verify]. A node a clip began keying less than its transition
  ago eases there from where it was shown (`cDistL`'s 0.6 s). A clip played
  once is over after its length; a zero-length clip is a pose (the swoop's
  timer digits); a dead follower plays its `die` clip once, then is gone.
- The swoop banks as it drifts: no script plays its `bankl_01`…`10` and
  `bankr_01`…`10`, a chain each turning the bike one step further about
  its forward axis in 0.17 s, so the view goes toward the drift's share of
  the ten steps a step a clip's length, posed part way through the step's
  clip; it leans into the turn (`bankl` toward +x) [verify by eye].
- What the engine plays itself, no script asking (view-side, from what the
  course shows happened): a gun bank's `fire` on its follower's models and
  its gun as a bolt leaves (the turret's recoil), `damage` on a follower
  hit, and a bolt spent before its time stays where it hit playing its
  `explode` (its emitters' burst) [verify]. Each gun bank's gun model
  (`mgg_turret`) hangs on its follower's `gunbank<id>` node, and bolts
  leave its `bullethook0` there along the node's +y (the sim's
  `Bank::muzzle`, `Bank::way`): the turret's barrels are tilted 5° down and
  toed in, so its two bolts meet on the crosshair's line, and the turret
  aims (`Course::aim`, the bot's) along their mean. A turret held at a
  pitch limit takes the view from there (no dead zone turning back).
  Enemies aim at the player's middle moved by its `Target_Offset` (the
  turret's 5 below) [verify]. A track's position keys with bezier handles
  (the fighters') are followed along their curves (`Path::at`; reone and
  xoreos keep only the values); the handles read as in, then out, each
  relative to its key [verify]. A follower's particles outlive its model
  (a fighter's debris after its `die`), none born. A visual effect a script
  puts on a minigame's thing, at once or for a while, goes where the thing
  is, carried by the party's leader, and a script asking where the thing
  is (`GetLocation`) hears its course place: the turret's loss blasts
  (`k_pebo_hawkhit`'s `VFX_FNF_GRENADE_FRAGMENTATION`, one ahead of the gun
  at `Invisible`, one on the Hawk 2 s on), drawn by `script_fx` as every
  script's fire-and-forget visual is (docs/MODEL-FX.md); a permanent one
  stays on the thing. Each bolt keeps
  its pooled copy while it flies and bursts; a copy given to another bolt
  starts new (no clip, pose or particle carried over).
- A room's clip a script plays (`PlayRoomAnimation`) is played by
  `own_fx` as in every module (MODEL-FX.md): over the room's own
  `animloop`s, its emitters keyed with it (the turret's sky planes; their
  lights are not moved).
- What the clips key beyond places and turns (`model_fx`, MODEL-FX.md):
  an animated mesh's alpha and self-illumination, each such mesh with a
  material of its own (one keyed alpha drawn blended; one whose texture's
  TXI says additive added by its alpha, alpha in its texture or not:
  reone; xoreos only without, and the thruster cards `lmg_jet01`/`02` are
  DXT5), and its emitters' particles (`v_damagetat`'s rocks and dust as a
  buoy is hit, `fx_rockchnk` chunks, the turret's tumbling asteroids; the
  swoop's `lmg_distort` a heat haze). These, and every emitter value, stay
  where the last clip to key them left them, as the game's clips are made:
  each keys only what changes (the start lights red, red and yellow, all
  three; gear 3 holds gear 2's thruster glow and lights two more; gear 1's
  flame birthrate lasts through the gears after it); a clip played once
  lands its last keys when it ends. Particles bounce off the thing's ground
  (its first model's lowest point) [verify].
  A model with no clip in place of the others idles in its `ready…`
  clip; an obstacle struck plays its `hit` [verify].
- The speed blur (`SWMG_SetSpeedBlurEffect`; the swoop's heartbeat sets
  `(speed − 149) / 300` past 150): `speed_blur` smears the opaque scene
  toward the screen's middle, none within 0.18 of its height of the
  middle and whole by 0.55, whole by a third of the ratio, its samples
  reaching 0.35 of the way in at a ratio of 1 [verify by eye against the
  game]; every transparent surface (the HUD, the flames) is drawn over it.
- The camera is the rig's `camerahook` node (its −Z ahead, +Y up: the swoop's
  `m17mg_camera`, the turret's `m12ab_camera`), the ARE's view angle and
  clip. The party, parked, is hidden (both executables keep a creature in
  the party out of the scene while a minigame plays, and nothing else:
  whatever else the area places is drawn as play leaves it, Peragus'
  troopers and barrels under its turret), an obstacle a script destroyed
  too, and the game's HUD gives way (`ui_view`): the minigame's HUD is its
  models.

## Heard
- The minigame's own (`minigame_view`, read with its models): each new
  bullet its bank's `Fire_Sound` (the Hawk's `mgs_ebon_fire`, a fighter's
  `mgs_sith_fire`), each hit its shooter's bank's `Collision_Sound`
  (`mgs_sith_hit`, `mgs_ebon_hit`), each death the follower's `Sounds`
  `Death` (`mgs_sith_expl`, a pad's `mgs_accelpad`). The player's own in
  both ears; the others' from where they are, full within 40 m, nothing past
  600 m [verify by ear]. A target's hit sound at most once per the hitting
  gun's `Rate_Of_Fire` (the owner's call, 2026-10-03): the turret's
  fighters land some 30 hits a second as they arrive (0.4 s a gun, nearly
  perfect aim), which drowned the bark "Incoming fighters!".
- The ARE's `Music` (the turret's `mus_bat_sithbs`) plays as the area's
  music; a blank one leaves the area's (`ambience`).
- A script's `SoundObjectPlay` starts a sound object at once from its
  first sound, each time (`ObjectState::plays`): the swoop's gear shifts,
  countdown and buoy hits are one-shot objects with a 25 s interval,
  which an object sounding as placed would wait part of first.
- The area's sound objects play as the scripts switch them (the swoop's
  engine, idle, gears, countdown, damage; the turret's alarm). In a
  minigame one not `Positional` is heard in both ears wherever the camera
  is (xoreos and reone give it no place). `SoundObjectSetFixedVariance`
  fixes a sound object's speed (`ObjectState::pitch`): the swoop's engine
  rises with its gear. `SoundObjectFadeAndStop` fades the object out over
  its seconds (`ObjectState::fade`): the last engine loop eases out as the
  next begins.

A minigame's models, spawned once the course is read under a camera that
never moves, are lit like any other: every material reads the area's
shared light list (`material::AREA_LIGHTS`, kept by `scene::pick_lights`),
so the Ebon Hawk's hull takes the turret room's lights, not black.

## Co-op (M10–M12)
Every seat sending commands plays, a watching guest too (M12).
- **The race** (M10): each seat past the first gets a run of its own
  (`Course::runs`, the course itself the first seat's), a whole course
  stepped beside it by that seat's command: its player, its pads, its
  buoys' clips, its hooks. A seat coming before anyone has finished races
  from its own start (a run's clock is its own). A run's things are their
  own objects (`minigame::id(run, n)`, `RUN_SPAN` on per run), so each
  run's scripts run as its own thing and see its run (`script::minigame`);
  code a run's thing defers or assigns stays that run's (`Event::run`,
  `Delayed::run`), as K1's finish line assigns `StartNewModule` to the
  first PC after a delay (`tar_m03mg`'s heartbeat); any other script that is
  no thing's sees the first seat's.
  - Each run's scripts keep the globals they set apart (`Course::overlay`,
    read back first), so the heartbeats' `TAR_SWOOP_*` do not overwrite one
    another. The first run to call `StartNewModule` has finished first:
    its module and movies are held (`Course::leaving`) until every run has
    finished or `LEAVE_WAIT_MS` (20 s) after it, then taken
    (`Course::leave_due`), its globals made the real ones as the race
    leaves. First to finish is the best time only for runs that started
    together (one joined late started later).
  - Each run keeps the sound objects its scripts set (`Course::voices`:
    the engine, the gears, the countdown), over the world's, and each seat
    hears its own run's.
  - Each seat's view draws its own run from its own rig, HUD and blur
    included, and the others' swoops as ghosts (`GHOST_ALPHA` of
    themselves, blended; made as runs join), on their runs' own clocks;
    their sounds are not heard.
- **The turret** (M11): one hull, the Hawk the one player the scripts
  see (its hit points, its heartbeat, the fighters' target). Each seat
  past the first is a gunner (`Course::gunners`): its own aim from its own
  view's turn, its own copy of the player's gun banks, its bolts the
  player's (numbered by gunner, `Bullet::gunner`). Each seat's view turns
  the gun and its camera by its own aim and draws everyone's bolts.
  - Each gunner sees its own compass and radar: the player's heartbeat
    (`k_heartbeat`) runs again through the gunner's lens (`minigame::lens`:
    ids of its own, `SWMG_GetPlayerOffset` its aim, the clips it plays on
    the player its own, `Gunner::anims`, and the globals it keeps,
    `Gunner::overlay`); its view draws the player's clips with its own over
    them (`Course::lens_anims`: the hull bar stays the hull's).
  - The fighters are scaled for the gunners: each past the first adds the
    module's encounter file's `per_player hp` of their hit points
    (`Rules::per_player_hp`; `m12ab.encounter`, x0.5), one coming
    mid-fight to each fighter still flying.
- Checked by `sim::minigame::probes` (the approved test `minigame_coop`:
  runs, hooks by run, the held module, gunners, scaling (a late gunner
  too), the encoding, two runs alike, a two-seat race whose scripts keep
  their globals apart, each run's sound objects, and the turret's
  heartbeat through a gunner's lens playing its own compass and writing
  nothing else)
  and `cargo xtask duo --preset race` / `--preset turret` (each side draws
  its own run or gun, the player's snapshots rebuilt as the host's).

## Saves
No save is made while a minigame plays, as KOTOR makes none [verify in
the game] (M13): `saves::refusal` refuses it ("You cannot save during a
minigame."), F5 and the save screen with it, and the playthrough's `save`
goal is held. So a party wipe's reload (the turret's `EndGame(TRUE)`, the
party's loss) goes back
to the last save made before the minigame.

## Playing them headless
`kjk playthrough`'s `race` and `turrets` goals (`game::minigame_bot`)
play from the snapshot as a player's hands would: a race presses fire once
the countdown is over (the script shifts when the gear allows) and steers
for the nearest pad ahead round the nearest buoy in the way (a TSL race
read across and along its curving track, its mines steered round too and
jumped when close, what lies off the swoop's height left alone); the turret
turns the view to the nearest fighter ahead of it by the bolts' flight and
holds fire while on it. Each is met when the minigame's script leaves its
module; `turrets` is refused once the game is lost (`EndGame(TRUE)`). A
global the scripts set changing is progress, holding the goal's clock as a
foe losing hit points does (Peragus' turret counts the troopers boarding,
`107PER_MG_EBON`, and ends with Atton's `turend` once all 25 are dead or
aboard). A conversation the scripts start while a minigame plays is
answered as any other: the minigame parks what moves the player, not its
answer (`world::minigame::steer`). `kjk play
--aim race|turrets` hands the scripted player to the same bot, for
filming a minigame played.

```bash
kjk playthrough k1 tar_m03mg race --global TAR_SWOOP_ACCEL=7    # a race, cold
kjk playthrough k1 m12ab turrets --global K_KOTOR_MASTER=10     # after Taris
kjk playthrough k1 @vulkar_out data/routes/k1/taris_swoop.route --god
kjk playthrough tsl 211tel race          # TSL's races, cold: 371nar, 510ond too
kjk playthrough tsl 421dxn turrets       # TSL's turrets: 505ond; 107per to its end talk
kjk play --stage tar_m03mg --global TAR_SWOOP_ACCEL=7 --moves '5.3s; 0.15s attack; …'
kjk play --stage m12ab --global K_KOTOR_MASTER=10 --headless --moves 30s --aim turrets --cmds 'wait 10f; clip fight 16 20; quit'
```

## Not yet
- Not drawn: a room clip's lights, a model's animated light colours (the
  hull bar's red flash, the guns' muzzle lights); `DOF`, `Accel_Secs`,
  `Target_Type`'s bits are read as noted [verify].
- A TSL race's quit on its pause screen (`Action::Forfeit` has no key);
  the swoop upgrade screen (cut content); the bot aims at no 107per
  trooper.
- `ProgFX` 1300 (a MIRV's shot, a projectile) (`script_fx`, MODEL-FX.md).
