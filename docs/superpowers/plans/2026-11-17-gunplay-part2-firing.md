# Gunplay Part 2 (Firing Motion) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every shot pushes the shooter's arms and shoulders back and lets them settle, sized by the gun's kick rule. During sustained fire the chest leans toward the target. Players, enemies and companions all show it.

**Architecture:** A new view-only module, `crates/game/src/recoil.rs`, holds a pure core. The core takes the time since each hand's last shot, the gun's kick from `sim::combat::bloom::tune`, and the wield from part 1's `aim_pose::Wield`, and returns a turn for each bone. `cast.rs` keeps a small streak record per body for the lean, and adds the turns after the breathing, the same way `stance.rs` adds breathing over a held pose. The sim is not touched.

**Tech Stack:** Rust, Bevy. Crates `game` (library `kjk_game`), `launcher`, `approved_tests`.

**Spec:** `docs/superpowers/specs/2026-11-17-gunplay-design.md` ("Part 2: firing motion", the "Poses" line of "Settled after the builder's review", and "Where firing motion is decided" in "Approaches considered"). Read it before any task. Part 1's plan, `docs/superpowers/plans/2026-11-17-gunplay-part1-aim.md`, defines `aim_pose.rs`, which this plan builds on.

## Global Constraints

- Branch `feat/gunplay-firing`, cut from `feat/gunplay-aim` once part 1 is done. Never push. Never open a pull request. Never merge.
- Never edit `bumps/SIM_HASH` or a format's `VERSION`. Part 2 changes no sim state, so it adds no bump file. If a task finds it must touch the sim, stop and report.
- Every number a game rule plays by lives in `data/rules/*.toml` and the sim reads it as `rules::…`. No literal in the sim. After a rule changes, run `cargo xtask rulebook`. This part changes no rule. Its view-only tuning numbers are constants in `recoil.rs`, each with a one-sentence comment.
- Permanent tests live only in `crates/approved_tests`. A layer's checks are probes (`pub fn probes() -> Checks`) that an approved test runs with `probed("name", &…::probes(), at_least)`.
- Every `kjk` run is `--headless --mute`, with `KJK_SETTINGS` and `KJK_SAVES` pointing at scratch copies. Check the real settings file's hash before and after a run.
- Do not run the whole gate or a chain for each step. Per task: `cargo fmt --all -- --check`, `cargo clippy -q -p <touched crates> --all-targets -- -D warnings`, and the approved tests the task names. CI runs the rest on the pull request.
- Run a command that may exceed 2 minutes in the background and wait for its notice. No sleep loops.
- Every text a person reads (comments, commit messages, documents) follows `CLAUDE.md`: full sentences, one fact each, no colons in titles, the actual number.
- No decompiler names, no memory addresses, no game files committed. No hand-drawn effects.
- Commit messages are a short noun phrase, with no attribution lines.
- Spec values, verbatim: the size follows the gun's existing `kick` rule; a pistol kicks up at the wrist; a rifle pushes into the shoulder; guns that are not pistols use the rifle's recoil motion, scaled by their own `kick` rule; a pair alternates hands, following which gun the sim fired; the chest leans a few degrees toward the target during sustained fire; breathing continues underneath; the motion is added over any pose, aimed or not.

## Decisions this plan makes

- **What already exists stays.** `cast::gun_aim` already kicks the gun itself through the aim solver (`weapons::Handling::kick`, `kick_degrees`, `kick_back`). That is the wrist's kick for a pistol and it stays unchanged. Part 2 adds the forearms, the upper arms, the collar bones and the chest over it.
- **Size.** A body's motion is its wield's base table times a scale. The scale is the gun's `tune(blaster).kick_up` divided by a reference kick, clamped to 0.5 to 2.0. The reference is `rules::weapons::pistol::KICK[0]` (0.8°) for a pistol or a pair, and `rules::weapons::rifle::KICK[0]` (0.35°) for every other gun. So the repeating blaster (0.2°) shows 0.57 of the rifle's motion, and the disruptor (2.0°) is clamped to 2.0 times it. The sideways part is `kick_side` times `SIDE_PER_DEG`, turned outward from the body on the hand that fired.
- **Timing.** Each hand's share rises over `RISE_MS` (50 ms) and falls to exactly 0 over its wield's settle time (250 ms for a pistol or a pair, 300 ms for a rifle), both with smoothstep. A new shot rises from what the last one left, so rapid fire never passes the single-shot peak.
- **Which hand.** The view already records each hand's shot in `Figure::shot_ms` and `shot_before_ms` from `Shooting::hands` (`cast.rs:1124-1135`). One gun uses whichever hand fired last, as `gun_aim` does today (`cast.rs:3628-3640`). That selection moves into a helper both use.
- **Sustained fire.** A streak is a run of shots with no gap longer than `SUSTAIN_GAP_MS` (1100 ms, above the slowest gun's 1000 ms refire). The lean starts with the streak's third shot, eases in over 300 ms to `LEAN_DEG` (4°), holds while the streak lasts, and eases out over 400 ms once the streak ends.
- **First person.** The body looked through gets the arm motion only. Its chest does not rock or lean, because the eye hangs on the head and the camera already turns with the sim's kick (`play_view::camera_turn`).
- **Which bodies.** Every living body posed on JA's skeleton with a blaster in hand (the `Look::Retargeted` branch of `cast.rs`), whoever controls it. Bodies on their own KOTOR clips (HK-47, droids, `Look::Clips`) get no recoil in this part. Their clips already have a shot.

## File map

| File | Change |
|---|---|
| `crates/game/src/recoil.rs` (new) | the pure core, its constants and its probes |
| `crates/game/src/lib.rs` | `pub mod recoil;` beside `mod stance;` (line 175) |
| `crates/game/src/cast.rs` | `Figure::streak`, the streak update at a shot, `hand_shots`, the turns added after breathing |
| `crates/launcher/src/sim_probes.rs` | `("firing motion", kjk_game::recoil::probes)` beside `"camera kick"` (line 117) |
| `crates/approved_tests/tests/approved.rs` | the test `firing_motion` |
| `docs/COMBAT.md` | a paragraph on firing motion |

## Review Focus

1. A shot the view knows of before the drawn time reaches it (the drawn time runs behind the sim's) shows no turn and no negative turn until its time comes. Test in Task 1 (`recoil_before_shot`).
2. A dead body, or a body with no gun in hand, gets no turns, even within the settle time of its last shot. Test in Task 1 (`recoil_needs_gun`).
3. A pair firing both guns in one shot (part 1's Sniper Shot, `hands == 3`) moves both arms by one shot's amount each, and the chest is not moved twice. Test in Task 1 (`pair_both_hands`).
4. A creature whose own rate of fire is faster than the rise (shots 40 ms apart) shows no jolt and no growth. Test in Task 1 (`recoil_fast_creature`).
5. In first person the chest neither rocks nor leans, so the view does not shake twice. Test in Task 1 (`first_person_arms_only`) and Task 2 (`first_person_no_lean`).

---

### Task 1: The recoil core

**Files:**
- Create: `crates/game/src/recoil.rs`
- Modify: `crates/game/src/lib.rs:175`, `crates/launcher/src/sim_probes.rs:117`
- Modify: `crates/approved_tests/tests/approved.rs` (a new test beside `grunts`, line 313)

**Interfaces:** Consumes `aim_pose::Wield { Pistol, Pair, Rifle }` (part 1), `sim::combat::bloom::tune(blaster) -> Tune` (`kick_up`, `kick_side`), `rules::weapons::{pistol, rifle}::KICK`. Produces:

```rust
// recoil.rs
pub(crate) struct Kick { pub up: f32, pub side: f32 }        // degrees, from bloom::tune
pub(crate) fn kick_of(blaster: Blaster) -> Kick;
/// Each hand's last shot and the one before it (sim ms; right, left).
pub(crate) struct Shots { pub last: [Option<f64>; 2], pub before: [Option<f64>; 2] }
/// What the core needs of one body this frame.
pub(crate) struct Firing { pub wield: Option<Wield>, pub kick: Kick, pub shots: Shots,
                           pub streak: Streak, pub looked_through: bool, pub alive: bool }
/// A turn over the posed bone: about the body's right axis (positive lifts the muzzle or rocks
/// back) and about its up axis (positive turns outward), degrees.
pub(crate) struct Turn { pub node: &'static str, pub pitch_deg: f32, pub yaw_deg: f32 }
/// The share of a shot's motion `since_ms` after it, `gap_ms` after the shot before (0 to 1).
pub(crate) fn share(wield: Wield, since_ms: f64, gap_ms: Option<f64>) -> f32;
/// The kick scale against the wield's reference kick, clamped to [MIN_SCALE, MAX_SCALE].
pub(crate) fn scale(wield: Wield, kick: Kick) -> f32;
/// Every turn of the body at `now_ms`, the lean included (Task 2). Empty when not alive or no wield.
pub(crate) fn turns(firing: &Firing, now_ms: f64) -> Vec<Turn>;
#[cfg(feature = "probes")] pub fn probes() -> sim::probe::Checks;
```

Constants, each with one comment: `RISE_MS = 50.0`, `PISTOL_SETTLE_MS = 250.0`, `RIFLE_SETTLE_MS = 300.0`, `MIN_SCALE = 0.5`, `MAX_SCALE = 2.0`, `SIDE_PER_DEG = 4.0`, and a base table per wield at scale 1, in degrees of pitch and yaw:

| Bone (firing side unless named) | Pistol and Pair | Rifle |
|---|---|---|
| forearm (`rforearm_g` / `lforearm_g`) | 6.0 up | 1.5 up |
| upper arm (`rbicep_g` / `lbicep_g`) | 2.0 up | 2.0 back |
| collar (`rcollar_g` / `lcollar_g`) | 1.5 back | 3.0 back |
| support arm (the other side's bicep and forearm) | none | 1.0 back each |
| chest (`torsoupr_g`) | none | 1.5 back |

`share` is the one algorithm the tests do not settle:

```text
pulse(t)  = 0 if t < 0; smoothstep(t / RISE) if t < RISE; 1 - smoothstep((t - RISE) / SETTLE) up to RISE + SETTLE; then 0
share(s, gap) = 0 if s < 0
              = max(left + (1 - left) * smoothstep(s / RISE), pulse(s + gap))   while s < RISE, left = pulse(gap)
              = pulse(s)                                                     after
```

- [ ] **Step 1: Write the failing probes** in `recoil.rs`. Sample at 8 ms steps. Loop over the wields and the seven blasters with one parameterised helper, not copied blocks.

```text
recoil_settles          for each wield: every turn is 0 at now < shot, rises after it, and is exactly 0 at
                        shot + RISE_MS + that wield's settle time and after.
recoil_before_shot      a shot at 1000 ms sampled at 990 ms: no turn, and no turn below 0 at any sample.
recoil_kick_scales      Rifle wield, the forearm's peak: ion / rifle = 0.6 / 0.35 within 1%; the disruptor's
                        peak = MAX_SCALE × the rifle's; the repeater's = (0.2 / 0.35) × the rifle's within 1%.
                        Over all seven blasters, the peak never falls as kick_up rises.
pair_one_hand           Pair, a right-hand shot only: every left-side node and the chest stay 0.
pair_both_hands         Pair, both hands at once: each side peaks at the one-hand peak, the chest stays 0.
recoil_rapid_fire       repeater, Rifle wield, a shot every 100 ms for 5 s: no turn passes the single-shot
                        peak by more than 1e-4, and the peaks of the last second are within 5% of the first.
recoil_fast_creature    the same with shots 40 ms apart, for each wield.
recoil_no_jolt          in every probe above, no 8 ms step changes a turn by more than 0.25 of that bone's
                        single-shot peak, or by more than 3°.
recoil_needs_gun        alive false, or wield None, with a shot 20 ms ago: no turns.
first_person_arms_only  looked_through true, Rifle: no `torsoupr_g` turn; the arm turns equal the third-person ones.
```

- [ ] **Step 2: Wire them.** Add `pub mod recoil;` to `lib.rs`, the line to `sim_probes.rs`, and a test `firing_motion` in `approved.rs` in the shape of `grunts`: `#[cfg(feature = "rig")] probed("firing motion", &kjk_game::recoil::probes(), 10)`, with the skip line for builds without `rig`. Its doc comment says what is checked and "proposed 2026-11-17, awaiting approval".
- [ ] **Step 3: Run** `cargo test --release -p approved_tests --features rig --test approved firing_motion`. Expected: it does not compile, because the core is missing.
- [ ] **Step 4: Implement** the signatures above. `turns` without Task 2's lean returns only the hand turns for now.
- [ ] **Step 5: Run** the test again. Expected: all 10 pass. Then `cargo clippy -q -p game -p launcher --all-targets -- -D warnings` and `cargo clippy -q -p launcher --all-targets --no-default-features -- -D warnings`.
- [ ] **Step 6: Commit** as "Firing motion core".

### Task 2: The lean during sustained fire

**Files:**
- Modify: `crates/game/src/recoil.rs`

**Interfaces:** Consumes Task 1. Produces:

```rust
/// A run of shots with no gap over SUSTAIN_GAP_MS: when it began, how many shots, when the
/// third came (the lean's start), and the last shot (sim ms).
#[derive(Default, Clone, Copy)]
pub(crate) struct Streak { since_ms: f64, shots: u16, leaned_ms: Option<f64>, last_ms: f64 }
impl Streak { pub(crate) fn shot(&mut self, at_ms: f64); }
/// The chest's lean toward the aim at `now_ms` (degrees, 0 to LEAN_DEG).
pub(crate) fn lean_deg(streak: &Streak, now_ms: f64) -> f32;
```

Constants: `SUSTAIN_GAP_MS = 1100.0`, `LEAN_SHOTS = 3`, `LEAN_DEG = 4.0`, `LEAN_IN_MS = 300.0`, `LEAN_OUT_MS = 400.0`. The lean is `smoothstep((min(now, end) − leaned) / LEAN_IN_MS) × (1 − smoothstep((now − end) / LEAN_OUT_MS))`, where `end = last_ms + SUSTAIN_GAP_MS`. `turns` adds it as a forward pitch on `torsoupr_g`, and adds nothing when `looked_through`.

- [ ] **Step 1: Write the failing probes:**

```text
lean_needs_streak    one shot, and two shots 350 ms apart: the lean is 0 at every sample for 3 s.
lean_sustained       rifle shots every 350 ms for 2 s: 0 before the third shot, LEAN_DEG by the third
                     shot + 300 ms, held while firing, held until the last shot + 1100 ms, 0 by the
                     last shot + 1500 ms, never rising while it eases out.
lean_new_streak      a shot 1200 ms after a streak's last starts a new streak with no lean.
lean_no_jolt         no 8 ms step changes the lean by more than 0.2°.
first_person_no_lean looked_through true with the sustained streak above: no `torsoupr_g` turn.
```

- [ ] **Step 2: Run** `firing_motion` with `at_least` raised to 15. Expected: the new probes fail.
- [ ] **Step 3: Implement** `Streak::shot`, `lean_deg` and the lean in `turns`.
- [ ] **Step 4: Run** the test and clippy as in Task 1. Expected: all 15 pass.
- [ ] **Step 5: Commit** as "Lean during sustained fire".

### Task 3: Recoil on the drawn bodies

**Files:**
- Modify: `crates/game/src/cast.rs:786-894` (`Figure`), `:911-959` (`Figure::new`), `:1123-1137` (shots seen), `:3628-3640` (`gun_aim`'s hand selection), `:4802-4834` (after the breathing)

**Interfaces:** Consumes `recoil::{Firing, Shots, Streak, kick_of, turns}`, `aim_pose::wield_of`, the `turn_in_model` helper (`cast.rs:4339`). Produces:

```rust
// cast.rs
/// Each hand's last shot and the one before, as the motion of `pair` or of one gun reads them.
fn hand_shots(figure: &Figure, pair: bool) -> recoil::Shots;
```

- `Figure` gets `streak: recoil::Streak`, defaulted in `new`. Where a new shot is seen (`cast.rs:1129-1134`), `streak.shot(time_ms as f64)` runs once per shot, not once per hand.
- `gun_aim` reads its `shots` and `befores` from `hand_shots`, so its behaviour does not change.
- After the breathing block and before `lay`, a `Firing` is built from the body's `Wielded` (the first gun's blaster for `kick_of`, `wield_of` for the wield), `hand_shots`, `figure.streak`, `eye.is_some()` and `at.alive`. Each `Turn` is applied with `turn_in_model`, about the body's right axis and up axis taken from the legs' yaw, as the breathing's lean takes its forward axis. The left hand's yaw is mirrored.
- The turns are added whether the body stands, walks, aims or fires from the hip. The breathing's own condition is not changed.

- [ ] **Step 1: Write the failing probe** `hand_shots_one_gun` in `recoil.rs`'s list or beside `cast::muzzle_probes`: a figure with one gun whose last shot is in the left slot gives that shot as `last[0]`, and a pair keeps both slots as they are. Raise `at_least` to 16. Run `firing_motion`. Expected: it fails.
- [ ] **Step 2: Implement** the field, the streak update, `hand_shots` and the application.
- [ ] **Step 3: Run** `firing_motion`, `cargo test --release -p approved_tests --features rig --test approved timeline_jolt_ceilings`, and clippy for `game` and `launcher` (with and without default features). Expected: all pass, and the jolt ceilings are unchanged.
- [ ] **Step 4: Commit** as "Firing motion on every gunner".

### Task 4: Clips, tuning and the text

**Files:**
- Modify: `crates/game/src/recoil.rs` (constants only), `docs/COMBAT.md` (beside the paragraph on kick, near the lines part 1 changed)
- Create: clips under `kjk-artifacts/gunplay-firing/` (not committed if the folder is ignored; check `.gitignore`)

- [ ] **Step 1: Film,** headless and muted, with scratch settings and saves, using part 1's recipe (its Task 8). Equip each gun the way part 1's Task 8 found. Hold [Attack] for 3 s in each clip. Film eight clips of 6 s each: hip fire and aimed fire (hold `alt`) with a pistol, a pair of pistols, a rifle and a repeating blaster. Film one more with a Sith trooper firing at the player, so a creature's motion is seen, and one in first person with the rifle.
- [ ] **Step 2: Look at frames** of every clip, at least one frame every 50 ms around two shots. Check these things. The pistol's muzzle lifts at the wrist and forearm. The rifle's shoulder goes back and the support hand stays on the gun. The pair moves only the arm that fired. The chest leans after the third shot of the rifle and the repeater, and comes back after firing stops. The breathing still moves the chest between shots. There is no pop when aiming starts or stops while firing. The first-person view does not shake more than it does on the branch's base.
- [ ] **Step 3: Tune** the base table and the constants where the frames show too much or too little, one constant at a time, and refilm that clip. After each change run `firing_motion`. The probes use the constants, so a tuned number must not break them. Keep the frames that decided each change in `kjk-artifacts/gunplay-firing/`.
- [ ] **Step 4: Write the text.** Add a paragraph to `docs/COMBAT.md` in full plain sentences. It says that every shot pushes the shooter's arms and shoulders back, that the size follows the gun's kick, that a pair moves only the arm that fired, that the chest leans by 4° after the third shot in a row, and that none of it changes where the shot goes.
- [ ] **Step 5: Run** `cargo fmt --all -- --check`, `cargo clippy -q --workspace --all-targets --all-features -- -D warnings`, `cargo clippy -q -p launcher --all-targets --no-default-features -- -D warnings`, and `cargo test --release -p approved_tests -p launcher --test approved` once, in the background. Fix what fails.
- [ ] **Step 6: Commit** as "Firing motion tuning and text".
