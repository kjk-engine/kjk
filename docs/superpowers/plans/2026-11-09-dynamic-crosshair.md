# Dynamic Crosshair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Player blasters bloom and kick (in the sim), and the HUD draws a reticle per blaster
family whose gap is the true spread cone, plus hitmarkers on blaster hits.

**Architecture:**
- **Sim:** a new pure module `sim::combat::bloom` holds the maths. Its state lives on
  `combat::specials::Shooting`, in whole millidegrees so the struct stays `Eq` and portable.
  `world::firing` reads and advances it on each player shot.
- **Render:** the reticle and the hitmarker are pure quad builders in `ui::screens::hud`.
  `crates/game` feeds them from the presented snapshot, and the camera takes the sim's kick in
  place of today's screen-only kick.

**Tech Stack:** Rust, Bevy (crates/game), the `sim` crate (deterministic, snapshots), the `ui`
crate (quads), rules from `data/rules/*.toml` (generated `rules::…` constants).

**Spec:** `docs/superpowers/specs/2026-11-09-dynamic-crosshair-design.md`. Read it first. The
numbers table and the "my call" notes there are binding.

## Global Constraints

- Branch `feat/dynamic-crosshair` in its own worktree off fresh `origin/main`. Never `claude/` in
  branch names or commits.
- Every number lives in `data/rules/weapons.toml` with `name`, `value`, `unit`, `range`,
  `summary`, `source = "kjk"` and `see`. No literals in code.
- Creatures (AI gunners) are untouched, square jitter included.
- `MotionState::view` (the player's command angles) is never written by recoil.
- Blaster shots only get hitmarkers (`Event::weapon == Some(Delivery::Blaster(_))`), and only
  the own avatar's (`Event::by == own`).
- Every `kjk` run is `--headless --mute`.
- Before each commit, `cargo clippy -p <crate> --all-targets -- -D warnings` and
  `cargo test -p <crate>` must pass for the touched crates.

## Review Focus

1. **Two pistols firing in the same step:** both shots add bloom and kick, and the cone the
   reticle shows covers the second shot. Test in Task 2.
2. **Gun put away or swapped mid-bloom:** `Shooting::interrupt` clears bloom and kick, so a
   fresh gun's first shot is true. Test in Task 1.
3. **A widened or narrowed FOV** (Knight/Master Speed, first person, camera options): the gap
   follows the FOV actually used that frame. Test in Task 4.
4. **Old snapshots and saves** (before the new fields) are refused or migrated, never misread.
   Test in Task 1.
5. **A kill by a flechette volley** (several events in one step, the target dead afterwards):
   one mark, red ticks shown. Test in Task 6.

---

### Task 1: Bloom and kick state and maths (sim)

**Files:**
- Create: `crates/sim/src/combat/bloom.rs` (and `pub mod bloom;` in `crates/sim/src/combat/mod.rs`)
- Modify: `crates/sim/src/combat/specials.rs:227-326` (`Shooting` fields, `interrupt`, `encode`, `decode`)
- Modify: `crates/sim/src/snapshot.rs:25-` (bump the KJKS version and add a line to its doc list)
- Modify: `data/rules/weapons.toml`

**Interfaces:**
- Produces, in `sim::combat::bloom`:
  - `pub struct Tune { pub per_shot: f32, pub max: f32, pub decay: f32, pub rest_ms: u32, pub kick_up: f32, pub kick_side: f32 }` (degrees, °/s, ms)
  - `pub fn tune(blaster: Blaster) -> Tune` (from rules)
  - `pub struct Stance { pub moving: bool, pub airborne: bool, pub aiming: bool }` and `pub fn stance(ps: &MotionState, shooting: &Shooting) -> Stance` (moving = horizontal speed above walk; airborne = `ps.ground.is_none()`)
  - `pub fn settled(shooting: &Shooting, tune: &Tune, now: u64) -> f32`: the bloom in degrees at `now`
  - `pub fn cone(base: f32, bloom: f32, stance: Stance) -> f32`: the radius in degrees
  - `pub fn kick_at(shooting: &Shooting, now: u64) -> [f32; 2]`: pitch and yaw degrees, recovered to `now`
  - `pub fn on_shot(shooting: &mut Shooting, tune: &Tune, now: u64, side: f32)`: settles, then adds `per_shot` (capped at `max`) and the kick (`kick_up` up, `side` in `±kick_side`, capped at `kick_max`)
  - `pub fn disc(rng: &mut SimRng, radius: f32) -> [f32; 2]`: a uniform point in a disc (angle, then `sqrt(u) * radius`)
- Produces, on `Shooting`: `pub bloom_mdeg: u16`, `pub kick_mdeg: [i16; 2]`, `pub fired_at: u64` (sim ms of the last shot)

- [ ] **Step 1: Add the rules.** Add these to `data/rules/weapons.toml`:
  - per blaster table (`pistol`, `rifle`, `repeater`, `bowcaster`, `disruptor`, `ion`, `sonic`):
    `[<w>.bloom]`, value `[per_shot, max, decay, rest_ms]` with `labels`;
    `[<w>.kick]`, value `[up, side]`;
  - shared `[bloom]` chapter-level table: `[bloom.stance]` value `[1.5, 2.5, 0.25]` (labels
    moving, airborne, aiming); `[bloom.kick_limit]` value `[4.0, 12.0, 80.0]` (labels max °,
    recover °/s, recover after ms).

  Values are the spec's numbers table. The disruptor's bloom is `[0, 0, 0, 0]`. Check
  `cargo build -p rules` and that `kjk rules` renders the summaries.
- [ ] **Step 2: Write the failing tests** in `bloom.rs` `#[cfg(test)]` as one parameterised table
  `bloom_follows_shots`: rows of (shot times in ms, query time) → expected settled bloom (±0.001).
  Rows, for the rifle:
  - one shot → 0.3;
  - 20 shots 100 ms apart → capped at 3.0;
  - one shot then a query 120 ms later → 0.3 (still resting);
  - one shot then a query 170 ms later → 0.0.

  Plus:
  - `cone_multiplies_stance`: base 1.4 + bloom 0.6, moving and airborne → 2.0 × 1.5 × 2.5 = 7.5.
  - `kick_caps_and_recovers`: 20 bowcaster kicks → pitch −4.0; 80 ms after → −4.0; 413 ms after → 0.0 (4°/12°/s = 333 ms, after the 80 ms wait).
  - `disc_stays_inside`: 10 000 samples ≤ radius and the mean ≈ 0.
  - `interrupt_clears_bloom` (Review Focus 2).
  - `shooting_round_trips` (Review Focus 4): encode then decode a `Shooting` with non-zero new fields gives it back.
- [ ] **Step 3: Run** `cargo test -p sim bloom`. Expected: they fail, because the functions
  don't exist yet.
- [ ] **Step 4: Implement** the module and the `Shooting` fields:
  - store in millidegrees, saturating;
  - extend `encode`/`decode` after `off_ms`;
  - `interrupt` zeroes the three new fields;
  - bump the snapshot version with a doc line: "N: shooting carries its bloom and kick".
- [ ] **Step 5: Run** `cargo test -p sim`. Expected: PASS. Old-version snapshots fail to decode
  with the version error.
- [ ] **Step 6: Commit.** `feat(sim): blaster bloom and kick state, rules and maths`

### Task 2: Shots use the cone, bloom and kick (sim)

**Files:**
- Modify: `crates/sim/src/world/firing.rs:344-447`
- Test: `crates/sim/src/world/aim_probes.rs`, the existing headless aim probes (add a probe beside them)

**Interfaces:**
- Consumes: everything Task 1 produces.
- Produces: `pub fn shot_cone(ps: &MotionState, shooting: &Shooting, blaster: Blaster, now: u64) -> f32` in `sim::combat::bloom`. It is the one call the HUD makes: `cone(primary(blaster).spread, settled(..), stance(..))`.

- [ ] **Step 1: Write the failing probe** `reticle_does_not_lie`. A player with an E-11, then a
  flechette, holds fire for 30 shots at a wall 512 units away. For each bolt (each shard), take
  the angle between its direction and the kicked aim line. It must be ≤ that tick's
  `shot_cone(..)` + 1e-3. Add a second case with a pistol in each hand (Review Focus 1): the
  12th shot's cone > the 1st's.
- [ ] **Step 2: Run it.** Expected: FAIL. The rifle cone is 0 and there is no bloom.
- [ ] **Step 3: Implement.** Players only (`!creature`):
  - **Aim:** the `forward` that `aimed_at` uses and the shot's base `view` add `kick_at(..)` to
    `ps.view` (pitch, yaw).
  - **Spread:** replace the square `range` jitter with `disc(&mut spread, shot_cone(..))`.
    "The first shard flies true" stays for multi-shard weapons.
  - **Advance:** after a shot leaves, call `on_shot(&mut fighter.shooting, &tune(blaster), now, side)`.
    `side` comes from `SimRng::new(tick, id << 8 | 0x40)`.
  - **Creatures:** their path stays byte-for-byte as now.
- [ ] **Step 4: Run** `cargo test -p sim`. Expected: PASS, the new probe included.
- [ ] **Step 5: Determinism and saves.**
  - Run the K1 chain (`kjk chain k1 --headless --mute`). Expected: 29/29.
  - Bump `bumps/SIM_HASH` as the merge-time-pins memory describes.
  - Re-stamp the checkpoint saves the chain uses (the `save-compat` skill).
- [ ] **Step 6: Commit.** `feat(sim): player shots bloom, kick and spread in a disc`

### Task 3: The camera follows the sim's kick (game)

**Files:**
- Modify: `crates/game/src/play_view.rs:1462-1469`
- Modify: `crates/game/src/cast.rs:751, 827, 1257, 3171, 3212` (drop `camera_kick` if nothing else reads it)

**Interfaces:**
- Consumes: `sim::combat::bloom::kick_at(&Shooting, now)`, from the presented snapshot's own fighter.

- [ ] **Step 1: Write the failing test** `camera_takes_sim_kick` (a unit test on the small helper
  below): a presented `Shooting` with `kick_mdeg = [-1200, 300]` at its own time gives
  `shaken = [shake0 - 1.2, shake1 + 0.3]`, watching or talking gives the shake unchanged.
- [ ] **Step 2: Run it.** Expected: FAIL.
- [ ] **Step 3: Implement** `fn own_kick(view, now) -> [f32; 2]` in `play_view.rs`. Use it in
  place of `Figure::camera_kick`, so the kick now turns yaw too. Remove the cast-side
  `camera_kick` and `Handling::camera_degrees` if they are now unused. The gun's own drawn kick
  (`handling.kick`) stays.
- [ ] **Step 4: Run** `cargo test -p game`. Then take one headless capture of 10 rifle shots: the
  screen centre stays on the bolts' line.
- [ ] **Step 5: Commit.** `feat(game): the camera kicks with the sim's recoil`

### Task 4: Reticle geometry (ui)

**Files:**
- Modify: `crates/ui/src/screens/hud.rs:276-285`

**Interfaces:**
- Produces:
  - `pub enum Family { Cross, Prongs, Brackets, Lines, Ring, Dashes }`
  - `pub fn gap_px(cone_deg: f32, vfov_deg: f32, layout_height: f32) -> f32`: `tan(cone)/tan(vfov/2) * h/2`, at least `MIN_GAP` (4.0)
  - `pub fn reticle(gui: &Gui, family: Family, gap: f32) -> Vec<Quad>`
  - `pub fn stroke(centre: [f32; 2], from: f32, to: f32, angle: f32, width: f32, color: [f32; 4]) -> [Quad; 2]` (a dark edge under a light stroke, `spin` = angle)
  - `crosshair` stays for the plain dot.

  Strokes: 2 px, white 0.9 alpha; edge 1 px larger, black 0.5 alpha (scaled by layout
  height / 1080). Shapes as the spec lists them: cross bars 6 px; prongs at −60°, 60° and 180°
  from up; brackets as 3 segments per side spanning ±40°; lines 14 px; ring as 4 arcs of 3
  segments; dashes as 8.

- [ ] **Step 1: Write the failing tests:**
  - `reticle_shapes`, one table over the 6 families: the quad count (dot + strokes, ×2 for the
    edges) and every stroke's inner end at ≥ `gap` from the centre.
  - `gap_matches_fov` (Review Focus 3): `gap_px(2.0, 64.94, 1080)` ≈ `tan(2°)/tan(32.47°)*540`,
    the same cone at fov 90 gives a smaller gap, and `gap_px(0, …)` = 4.0.
- [ ] **Step 2: Run** `cargo test -p ui reticle`. Expected: FAIL.
- [ ] **Step 3: Implement** the four items above.
- [ ] **Step 4: Run** `cargo test -p ui`. Expected: PASS.
- [ ] **Step 5: Commit.** `feat(ui): blaster reticles by family`

### Task 5: Draw the reticle in play (game)

**Files:**
- Modify: `crates/game/src/target_view.rs:272` and `crates/game/src/ui_view.rs:92, 811, 865-866`
- Modify: `crates/game/src/play_view.rs` (export the frame's FOV beside `aim`, for example in a `ViewFov(f32)` resource)

**Interfaces:**
- Consumes: `bloom::shot_cone`, `hud::{Family, gap_px, reticle}`.
- Produces: `vm.reticle: Option<(Family, f32)>` (family and gap, already eased), with `vm.crosshair`
  kept for the dot.

- [ ] **Step 1: Write the failing test** `family_by_blaster`: Pistol and Rifle → Cross,
  Repeater → Prongs, Bowcaster → Brackets, Disruptor → Lines, Ion → Ring, Sonic → Dashes. Plus
  `ease_widens_at_once`: from gap 10 to target 20, one frame gives 20; from 20 to 10 over 16 ms
  gives a value between 10 and 20 (60 ms ease).
- [ ] **Step 2: Run it.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Map `Blaster` to `Family`.
  - Each frame, with the own character holding a blaster and no conversation, menu or cutscene:
    `vm.reticle = Some((family, eased(gap_px(shot_cone(..), fov, layout_h))))`, else `None`.
    `ui_view` draws `reticle` when it's set, otherwise the dot as now.
  - Sniper Shot aiming with the disruptor draws only the dot.
- [ ] **Step 4: Run** `cargo test -p game`, then take headless screenshots, one per family, plus
  rifle hip fire at shots 1, 10 and 20.
- [ ] **Step 5: Commit.** `feat(game): the reticle shows the live spread`

### Task 6: Hitmarkers (ui + game)

**Files:**
- Modify: `crates/ui/src/screens/hud.rs` (mark quads)
- Create: `crates/game/src/hitmarker_view.rs` (registered beside `floaty_view` in the game's plugin)

**Interfaces:**
- Produces:
  - `pub enum Mark { Hit, Strong, Kill { strong: bool }, Turned }` (ui)
  - `pub fn classify(result: Outcome, dead: bool) -> Option<Mark>` (game). Strength order for
    merging: Kill > Strong > Hit > Turned.
  - `pub fn hitmarker(gui: &Gui, mark: Mark, gap: f32, age_ms: f32) -> Vec<Quad>` (ui):
    - X strokes from `gap+6` to `gap+14` at 45°/135°/225°/315°;
    - kill ticks from `gap+18` to `gap+24`;
    - Turned at half length, grey `[0.6, 0.6, 0.6]`;
    - gold `[0.95, 0.72, 0.2]`, red `[0.85, 0.15, 0.1]`;
    - shown 140 ms, scaling to 1.15 and fading over the last 80 ms; empty once older.

- [ ] **Step 1: Write the failing tests:**
  - `classify_table`, one table:
    - Hit, alive → Hit; Critical → Strong; Automatic → Strong;
    - Hit, dead → Kill{strong: false}; Critical, dead → Kill{strong: true};
    - Parried / Deflected → Turned; Refused / Visual → None.
  - `marks_merge` (Review Focus 5): 5 flechette events in one step, the last leaving the target
    dead → one Kill.
  - `hitmarker_fades`: the quad count at age 0 > 0, alpha at 100 ms < alpha at 0, empty at 141.
  - `only_own_blaster_hits`: events with `weapon` a blade, or `by` another seat, give nothing.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - **ui:** `Mark` and `hitmarker`.
  - **game:** `classify` and the merge (in `hitmarker_view.rs`, since it reads sim's `Outcome`).
    The view follows fighters' `next_event` like `floaty_view::Damages`
    (`seen`, `generation` reset on scene swap). It keeps one live `(Mark, since_ms)` and adds
    its quads over the reticle (at the current gap, or `MIN_GAP` if the gun was just put away).
- [ ] **Step 4: Run** `cargo test -p ui -p game`. Expected: PASS. Then take a headless capture of
  a rifle kill: white marks, then the kill frame with red ticks.
- [ ] **Step 5: Commit.** `feat: hitmarkers on the own blaster hits`

### Task 7: Docs, full check and evidence

**Files:**
- Modify: `docs/COMBAT.md`, `docs/UI.md`; regenerate `docs/RULEBOOK.md` with `kjk rules`

- [ ] **Step 1: Write the docs.**
  - `COMBAT.md`: player bloom, the disc spread, the kick (aim only, not the command angles), and
    that creatures keep JA's aim.
  - `UI.md`: the families and the mark colours.
  - `RULEBOOK.md`: regenerate it.
- [ ] **Step 2: Full check.** `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, then the K1 chain `--headless --mute`. Expected: clean, all green,
  29/29.
- [ ] **Step 3: Evidence for the owner.** Send these with absolute paths:
  - one GIF of rifle hip fire showing bloom and kick;
  - a contact sheet of the 6 reticles;
  - a GIF of a kill hitmarker.
- [ ] **Step 4: Commit and open the PR.** Commit `docs: crosshair, bloom and hitmarkers`, then
  open a PR titled "Blasters bloom and kick, and the crosshair shows it". Its description
  carries the spec link, the numbers table and the evidence.
