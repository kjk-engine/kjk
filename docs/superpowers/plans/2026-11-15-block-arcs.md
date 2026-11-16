# Block Arcs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three marks of targeted blocks with a sliding aim arc and a swing arc, and draw no crosshair while they are shown.

**Architecture:** The sim's `aim.rs` exposes the two crosshair offsets that the rule already computes. The game's block reticle view turns them into one angle and puts it in the HUD's view model. The HUD draws two arcs from that view. No rule, rule number, snapshot layout or trace changes.

**Tech Stack:** Rust, the project's own HUD quads (`ui::screens::hud`), probes instead of unit tests.

**Spec:** `docs/superpowers/specs/2026-11-15-block-arcs-design.md`. It replaces the section "The reticule" of `docs/superpowers/specs/2026-11-15-targeted-blocks-design.md`, whose other sections and constraints still apply.

## Global Constraints

- Work only in the worktree `targeted-blocks`, on the branch `feat/targeted-blocks`. Do not push. Do not open a pull request.
- No rule, no rule number, no snapshot layout and no trace changes. The Jedi Academy trace and the Advanced trace with the flag off stay identical to the base `b2c7c0fc`.
- The aim angle is computed from the same two offsets that `aim::side_of_offsets` reads.
- The colour of the swing arc follows the rule's own verdict (the crosshair's side equals the swing's side), not the drawn angle.
- Only approved tests may be `#[test]`. Every new check is a probe. Do not add or edit an approved test.
- Every `kjk` run uses `--headless` and `--mute`, with `KJK_SETTINGS` and `KJK_SAVES` set to a scratch copy under `/private/tmp/claude-501/`. The owner's real settings file must keep SHA-1 `a1b76fbcae4c51fd55447102f8188fcccf9c8008`.
- The gate before every commit: `cargo fmt --all -- --check && cargo clippy -q --workspace --all-targets --all-features -- -D warnings`.
- Commit titles are short noun phrases without colons and without attribution lines. Comments are plain full sentences. No repetitive code: probes are table-driven.
- Never edit `VERSION` or `bumps/SIM_HASH`.

## Review Focus

- The crosshair is exactly on the chest (both offsets 0). The view must give no aim angle, and nothing may divide by zero.
- The crosshair is straight below the chest. The aim arc must be drawn dim at the bottom, and no swing arc may turn green.
- A co-op guest. The angle must come from the presented snapshot, like the sides do.
- The blaster crosshair and the hitmarker when the player does not block. They must be drawn exactly as before.
- A very small or very large window. The arcs scale with the layout's height, like the old marks.

---

### Task 1: Aim arc and swing arc

**Files:**
- Modify: `crates/sim/src/combat/saber/duel/aim.rs` (near lines 70-135)
- Modify: `crates/ui/src/vm.rs` (`BlockReticleView`, near line 1317)
- Modify: `crates/ui/src/screens/hud.rs` (`block_marks`, near line 875)
- Modify: `crates/game/src/ui_view.rs` (near lines 1236-1252)
- Modify: `crates/game/src/block_reticle_view.rs` (the view and its probes)
- Modify: `crates/ui/src/probes/block_reticle.rs` (the drawing probes)
- Modify: `crates/sim/src/trace/duelling_probes/targeted.rs` (one probe for the offsets)
- Modify: `crates/ui/src/strings.rs` (`TARGETED_BLOCKS_HELP`), `docs/SABER-RULES.md` (section 3.2), `docs/superpowers/specs/2026-11-15-targeted-blocks-design.md` (one line under "The reticule" that points to the new spec)

**Interfaces:**
- `aim::crosshair_offsets(line_from: Vec3, line_way: Vec3, chest: Vec3) -> (f32, f32)` returns `(right_deg, up_deg)`. `aim::crosshair_side` calls it and passes the pair to `side_of_offsets`, so there is one computation.
- `aim::offsets(blocker: &MotionState, attacker: &MotionState) -> (f32, f32)` builds the crosshair line and the chest the same way `aim::sides` does. `aim::sides` and `aim::offsets` share that construction.
- `aim::aim_deg(right_deg: f32, up_deg: f32) -> Option<f32>`. None when both offsets are under `targeted::DEAD_DEG`. Otherwise the angle in degrees clockwise from straight up, in `0.0..360.0`: `(0, 5) -> 0`, `(5, 0) -> 90`, `(0, -5) -> 180`, `(-5, 0) -> 270`, `(5, 5) -> 45`.
- `BlockReticleView` gains `pub aim: Option<f32>` (that angle). It keeps `crosshair: Option<Side>` and `swing: Option<Swing>`. It can no longer derive `Eq`.
- `hud::block_arcs(frame: (i32, i32), view: BlockReticleView) -> Vec<Quad>` replaces `hud::block_marks`.

**Drawing values (at a 1080-high layout, scaled by height / 1080):**
- Both arcs lie on a circle of radius 27 pixels around the middle of the frame.
- The aim arc is 40 degrees long, centred on `view.aim`, 3 pixels thick, over a dark edge like the old marks. Its colour is `RETICLE_LIGHT`. It is `BLOCK_MARK_UNLIT` when `view.crosshair` is None. It is not drawn when `view.aim` is None.
- The swing arc is 2 pixels thick. Above runs from 315 to 45 degrees. Right runs from 45 to 160 degrees. Left runs from 200 to 315 degrees. Its colour is `BLOCK_MARK_WARNING`, and `BLOCK_MARK_CONFIRM` while `view.crosshair` equals the swing's side. `Swing::Any` draws all three arcs in `BLOCK_MARK_CONFIRM`. No swing arc is drawn when `view.swing` is None.
- The aim arc's quads come after the swing arc's quads, so the aim arc is drawn on top.
- An arc is built from straight pieces of at most 5 degrees each, with one shared helper for both arcs.
- In `ui_view.rs`, while `ui.vm.block_reticle` is Some, the call to `reticle(...)` is skipped. The hitmarker is still drawn.

**Player text:** in `TARGETED_BLOCKS_HELP`, "Marks around the crosshair show the side." becomes "An arc around the centre of the screen shows the side." Reword the marks paragraph of section 3.2 of `docs/SABER-RULES.md` to describe the two arcs with the values above, in plain full sentences.

- [ ] **Step 1: Write the failing probes.**
  - Sim (`targeted` group): a table probe `aim_angle_table` over `aim_deg` with the five rows above, plus `(2.9, 2.9) -> None`, `(0, 0) -> None` and `(-3, 0) -> 270`. A probe that `crosshair_side(f, w, c) == side_of_offsets(crosshair_offsets(f, w, c))` for the rows of the existing `crosshair_side_table`.
  - Game (`block reticle view` group): the existing staged cases also assert `aim`. A crosshair left of the chest gives an angle between 225 and 315. A crosshair on the chest gives None. The guest's view reads the angle from the presented snapshot.
  - UI (gui probes, `block_reticle.rs`): one table over views. No aim and no swing gives no quads. Aim only gives quads all in `RETICLE_LIGHT` (plus edges). Aim with `crosshair: None` gives `BLOCK_MARK_UNLIT`. A left swing with the crosshair left gives `BLOCK_MARK_CONFIRM` and no `BLOCK_MARK_WARNING`. A left swing with the crosshair right gives `BLOCK_MARK_WARNING` and no `BLOCK_MARK_CONFIRM`. `Swing::Any` gives three confirm arcs. Every quad's centre lies between 24 and 30 pixels from the middle at a 1080-high frame, and twice that at 2160.
- [ ] **Step 2: Run the probes and see them fail.** `kjk --headless --mute sim probes --only targeted`, `--only "block reticle view"`, and `kjk --headless --mute gui probes`.
- [ ] **Step 3: Implement** the interfaces and drawing values above. Remove `block_marks` and everything only it used.
- [ ] **Step 4: Run every check.** The three probe runs (expected: no failure). The full `sim probes` (expected: only `fp_regeneration` and "the lab station" fail, as on the base). `cargo test --release -p approved_tests` (expected: all pass, none edited). `cargo xtask rulebook` (expected: no change). The Jedi Academy trace and the Advanced flag-off trace compared with `b2c7c0fc`, the way `.superpowers/sdd/2026-11-15-targeted-blocks/task-3-report.md` describes (expected: identical). The gate.
- [ ] **Step 5: Commit** as "Aim arc for targeted blocks".
