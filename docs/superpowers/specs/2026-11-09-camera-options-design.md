# Camera options — research and proposed plan (2026-11-09)

## What players asked for (Discord)

- **Camera zoom thread**:
  - zoom the camera in and out, and zooming all the way in goes to first person;
    keys `-`/`+` because the wheel is busy; on the pad, hold the stick click and move the stick;
  - an over-the-shoulder camera — "directly behind the back feels a bit awkward" —
    as an **option** the player sets, not forced;
  - a camera menu like Movie Duels' (the camera is what keeps one player away for now).
- **Free camera thread**: see your own character from the front as in KOTOR.
  The owner picked **option 1, Mount & Blade style**: hold a key and the mouse orbits the
  camera around the character; let go and it is JA's camera again.

## What kjk has now

- `play_view::follow_camera` is a faithful copy of JA's camera:
  - range 80, aim point 16 over the eye;
  - target damp 0.5, camera damp 0.3 per 50 ms, stiffer on fast turns and steep pitch;
  - a 4-unit box traced, plus kjk's level-first leg (Q-0016).
  - All of these are constants: there is no setting for any of them.
- `CameraMode::Third/First` turned by F1 (`camera.view`); the only other camera setting is
  `camera.invert`.
- The FOV is fixed (`FOV_DEGREES` 64.94 vertical = JA's `cg_fov` 80 at 4:3); Speed widens it.
- The crosshair already copes with a camera off the JA line. `mind::aim_for` sends how far
  the camera sits across the view line, in `Command::aim`, ±63 units. A shoulder offset of
  about 24 units fits, so a side camera's shots still hit what the crosshair is on, in co-op too.
- The mouse wheel moves the target's chosen action (`target_view`); the pad's R3 is Saber style.
- Missing:
  - the body is never faded when the camera presses into it against a wall;
  - no crouch nudge;
  - no saber-lock camera;
  - KOTOR's per-area `camerastyle.2da` is unused in play. Rows: DEFAULT 3.2 m, EbonHawk 1.3 m,
    OutDoor 6.25 m, Combat 3.6 m; viewangle 55.

## What others do

**JA / OpenJK**:
- cvars `cg_thirdPersonRange` 80, `VertOffset` 16, `HorzOffset` 0 (a cheat in MP),
  `Angle` (yaw offset), `PitchOffset`, `TargetDamp` 0.5, `CameraDamp` 0.3,
  `Alpha` (body translucent), `cg_fov` 80.
- Its horizontal offset is added **after** the traces and into the damped state, so it goes
  through walls and drifts. camSP only allows it with damping off for this reason.

**Movie Duels** (open source: [MovieDuels/MovieDuels-SJE-SP-Rend2](https://github.com/MovieDuels/MovieDuels-SJE-SP-Rend2), `code/cgame/cg_view.cpp`; Update 8 shipped today):
- **Camera menu with presets.** "battlemaster" was replaced by a **"KOTOR"** preset;
  "Battlefront" means the horizontal offset. Sliders show numbers (Update 8). The menu ships
  in the mod's assets, not the repo, so the exact slider list is unverified.
- **Shoulder offset done right.** It moves the final camera sideways along the view's right
  and traces from the aim point to there, stopping at the wall. That is the Update 8
  "clipping through walls" fix.
- **Aiming camera** (`g_AimingCinematicCamera`, on by default) while a gunner holds block:
  - range 50, vertical 4, FOV 60;
  - the aim point is 64 units ahead of the eye;
  - over the right shoulder by 24 × `cg_aimShoulder`, scaled to the model's size (0.5–3×);
  - its own `cg_aimCamHorzOffset`, the same on every preset.
- **Saber-lock cinematic camera** (`g_saberLockCinematicCamera`, off by default):
  angle 40.5°, pitch −11.25°, side −25.5, vertical −15.5, range 82.5, FOV 31.
- **Blends.** Both blend in and out over 250 ms with smoothstep. Damping is off during a
  blend, because blending the damping made the camera trail and then snap.
- **Orbit while meditating.** The body keeps facing and the mouse orbits the camera, pitch
  kept between +70° and −60°. On the way out it swings back behind in about 1/6 s.
  This is exactly the owner's option 1, tied to meditation.
- **Long Leap.** 70 further back, aimed 16 lower, eased over 1/4 s.
- **Crouch / meditate nudge.** The aim point goes up 6, traced.
- `cg_smoothCamera` 1: exact `powf` damping. kjk already uses `powf`.

**camSP**:
- tracking Loose / None / Tight (= damping JA, off, snap);
- FOV, range, pitch and yaw pan, vertical and horizontal offset;
- live numbers with a reset per slider;
- hide player / HUD for screenshots.

**JA++ TrueView**: first person from the head model's eye tag, with its own FOV. kjk's F1
first person is already this, with the full body.

**Modern games**:
- shoulder swap with a ~0.2 s ease, traced;
- pull in fast at walls, ease back out slowly;
- fade the body when the camera nears it;
- Jedi: Survivor has view distance, follow strength, shake 0–100 and FOV sliders.

## Proposed plan

Everything below is view-side (`crates/game`). The sim and the network need no change: the
crosshair already travels as `Command::aim`.

### Part 1 — camera settings + presets (the core ask)

- Make `follow_camera`'s constants a `CameraTune`, filled from new settings:

  | setting | default | range | JA name |
  |---|---|---|---|
  | `camera.range` | 80 | 30–200 | `cg_thirdPersonRange` |
  | `camera.height` | 16 | −8–48 | `cg_thirdPersonVertOffset` |
  | `camera.shoulder` | 0 | −40–40 (+ = right) | `cg_thirdPersonHorzOffset` |
  | `camera.pitch` | 0 | −20–20 | `cg_thirdPersonPitchOffset` |
  | `camera.fov` | 80 (horizontal at 4:3, as JA) | 60–110 | `cg_fov` |
  | `camera.tracking` | `ja` | `ja` / `tight` / `none` | damp 0.5/0.3, ×2, snap |
  | `camera.body_fade` | true | — | (none in JA) |

- **Shoulder offset, Movie Duels' way.** Apply it after damping, to the shown camera only,
  traced from the aim point, so it never goes through walls or drifts. Ease side changes over
  0.2 s.
- **Presets** set the sliders at once:
  - **JA** (default; today's camera, unchanged, so chains and approved tests stay put);
  - **Over the shoulder**: range 70, shoulder +22, height 10;
  - **KOTOR**: from `camerastyle.2da` DEFAULT: 3.2 m ≈ 126 units, a little higher, FOV 55;
  - **Close**: range 50, shoulder +16.
- **Body fade.** When the camera comes within about 24 units of the eye, the own body and
  weapons fade (JA's `cg_thirdPersonAlpha` idea), so a wall-pressed camera never shows the
  inside of the head.
- **Wall recovery.** Pull in at once, ease back out over about 0.3 s, so the range does not
  pump along a wall.
- **Menu.** A new **Camera** page in Options (K2 styled like the others), with a number on
  each slider (as Movie Duels Update 8), a preset row and Default (`page_keys`).
- **Console.** `camera range 60` etc. for quick tuning; the dump's `play` facts carry the tune.

### Part 2 — zoom and the first-person crossover

- `Action::ZoomIn` / `ZoomOut`, default `=` / `-` (both unbound now), step 10 units, eased.
  They write `camera.range`.
- **Wheel zoom.** While the orbit key is held the wheel zooms; otherwise it keeps moving the
  target action.
- Zooming in past 30 goes to `CameraMode::First` (the F1 eyes). Zooming out from first person
  comes back at 30. F1 keeps its toggle.
- **Pad.** Holding R3 is a shift for the right stick:
  - R3 tapped (released with the stick still) = Saber style, as now, but on release;
  - R3 held + stick = orbit;
  - R3 held + D-pad up/down = zoom.
  This is a player's suggestion, moved to R3, which is where the saber style lives.

### Part 3 — hold-to-orbit (owner's option 1)

- `Action::Orbit`, default **Left Alt**; pad: hold R3 (above).
- While it is held, the mouse/stick turns an orbit yaw/pitch offset on the camera, not the
  player's view. The body keeps facing and moving as before, and W stays the character's
  forward, as in Mount & Blade.
- Pitch is kept between +70° and −60° over the view; a full circle is allowed, so you can see
  the face. On release it swings back behind in about 1/6 s, with no snap. These are Movie
  Duels' meditation-orbit numbers.
- While orbiting, the crosshair is hidden and `Command::aim` is zero, so attacks go where the
  character faces, not where the camera looks.
- Off in conversations, fixed cameras and cutscenes, as today.

### Part 4 — context cameras (Movie Duels' cinematic blends; each an option)

- One blend system: smoothstep 250 ms, damping off while blending, presets kept aside.
- **Aiming camera** (`camera.aim_cam`, default on, as MD):
  - while a gun is held and the alt attack is aimed;
  - range 50, right shoulder 24 × size, FOV 60, aim point 64 ahead.
  - Check: is kjk's blaster alt a hold? If not, use "gun out + attack held".
- **Saber-lock camera** (`camera.lock_cam`, default off, as MD): MD's numbers above, on
  `sim::combat::saber::lock`.
- **Crouch nudge**: aim point +6, traced (JA's own; kjk lacks it, a faithful fix).
- **Death camera**: turn to face the killer (JA's `STAT_DEAD_YAW`), if kjk has the killer at
  hand.
- **Later, optional:** an area-aware range from the .ARE `CameraStyle`, e.g. Ebon Hawk 1.3 m
  → a tighter camera in the Hawk's corridors, only when the player's range is at default.

### Order, checks, evidence

1. Part 1, then 2, then 3, in one branch, each its own commit, with clips:
   - each preset in end_m01aa and the Ebon Hawk;
   - a shoulder camera at a wall;
   - zoom into first person;
   - an orbit to the face and back.
2. The JA preset must leave today's camera identical. The approved tests and the K1 chain
   stay green with no edits.
3. New approved test (needs the owner's OK): a shoulder camera against a wall stays out of it,
   and a shoulder shot lands on the crosshair.
4. Part 4 after the owner has played 1–3.

## Decided (2026-11-09)

- Default preset: **Jedi Academy** (today's camera, unchanged).
- Orbit key: **Left Alt** held; a pad's R3 held.
- Wheel: zooms only while the orbit key is held; otherwise it still moves
  the target's action.
- Parts 1-3 built on `claude/camera-options`; part 4 (context cameras)
  waits for the owner's play of 1-3.
