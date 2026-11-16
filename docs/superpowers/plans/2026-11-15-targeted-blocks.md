# Targeted Blocks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An optional setting under the Advanced combat rules in which a perfect block also needs the crosshair on the side of the attacker that the swing comes from, with a reticule that shows that side.

**Architecture:** One session flag travels beside the combat rules from the settings into the snapshot. Pure functions in a new module `duel/aim.rs` decide the side of a swing, the side of the crosshair and whether they match. The world calls them at the one place where a player's guard is decided. The view calls the same functions to draw three marks around the crosshair.

**Tech Stack:** Rust workspace. `crates/sim` (rules and world), `crates/rules` (numbers from `data/rules/*.toml`), `crates/game` (settings and view), `crates/ui` (options page and HUD drawing).

**Spec:** `docs/superpowers/specs/2026-11-15-targeted-blocks-design.md`. Read it first. This plan does not repeat its rules.

## Where to work

- Another agent is working on branch `feat/advanced-pacing` in the worktree `.claude/worktrees/advanced-combat` of the main checkout. It is tuning numbers in `data/rules/duel.toml`. Do not work in that worktree and do not touch that branch.
- Make a new worktree with a new branch `feat/targeted-blocks` from `feat/advanced-pacing` at `b2c7c0fc`. Use the `superpowers:using-git-worktrees` skill. Copy the spec and this plan into the new worktree (they are untracked files in the worktree above) and commit them with the first task.
- Never use a `claude/` prefix on a branch.

## Global Constraints

- The main checkout is stale. Read code only in your own worktree.
- Every new rule sits inside a branch on `rules.duel()` and the session flag. Under Jedi Academy's rules, and under the Advanced rules with the flag off, nothing may change.
- Rule numbers are in `data/rules/duel.toml`, in a new section `[targeted]`. Touch no other section of that file. Read them in code as `rules::duel::targeted::<KEY>`. After a change run `cargo xtask rulebook`.
- Never edit `VERSION` or `bumps/SIM_HASH`. Add `bumps/feat-targeted-blocks.toml`, modelled on `bumps/feat-advanced-pacing.toml`. Its decode gate is `crate::since::snapshot::FEAT_TARGETED_BLOCKS`.
- Only approved tests may be `#[test]` (`crates/approved_tests/tests/approved.rs`). Every new check is a probe. Do not add or edit an approved test. If one fails because of this work, stop and report it.
- Every `kjk` run uses `--headless` and `--mute`, with `KJK_SETTINGS` set to a scratch copy of the settings file. A run without it writes the owner's real settings.
- The gate before every commit: `cargo fmt --all -- --check && cargo clippy -q --workspace --all-targets --all-features -- -D warnings`.
- Player text follows `docs/player-text.md`: plain sentences, the common term, no key names fixed in text, no class names from the original games.
- Commit messages are short noun phrases without colons, for example "Targeted blocks setting". No attribution lines.
- Movie Battles II is a reference for the idea only. Copy no code and no identifiers from it.
- Do not push and do not open a pull request. The work ends with a report and clips for the owner.

## Review Focus

1. **A camera whose crosshair line does not start at the character's eyes** (third person, the shoulder camera, zoom, first person). The lit mark on the screen and the rule's verdict must agree in each. Probe `crosshair_line_matches_shots` in Task 2 and the clip in Task 5.
2. **An attacker that does not face the blocker** (side-on, or behind and turning). The side comes from geometry, and a side-on swing has no side. Probe `swing_side_table` in Task 2.
3. **Two attackers swinging from different sides at once.** Each blow is judged against its own attacker. The reticule shows the swing that began first. Probe `two_attackers` in Task 3 and `threat_order` in Task 4.
4. **A co-op guest whose own setting differs from the host's.** The guest's reticule and the guest's blocks follow the flag in the snapshot, never the guest's settings file. Probe `guest_reads_snapshot` in Task 4.
5. **An old save, and a setting changed in the middle of a game.** An old snapshot reads the flag as off. A change in the options has no effect until a new game starts, and the options row says so. Probes `old_snapshot_off` in Task 1 and the options probe in Task 1.

---

### Task 1: The setting and the session flag

**Files:**
- Modify: `crates/sim/src/snapshot.rs` (field beside `saber_rules` near line 1371, decode beside the `SABER_MODES` gate near line 2206, encode at the matching place)
- Modify: `crates/sim/src/world/state.rs` (lines 170 and 602, where `saber_rules` is set), `crates/sim/src/install.rs:771`, `crates/sim/src/fixtures.rs:383`, and the world resource that holds the session's rules (`super::Rules` in `crates/sim/src/world/`)
- Modify: `crates/game/src/settings.rs` (key, default near line 889, the page list near line 1002, an accessor beside `saber_rules()` near line 1363)
- Modify: `crates/ui/src/strings.rs`, `crates/ui/src/screens/options.rs` (row after the "Combat rules" row near line 937, and the fixed-in-session list near line 331)
- Create: `bumps/feat-targeted-blocks.toml`
- Create: `crates/sim/src/trace/duelling_probes/targeted.rs`, registered the way `stamina.rs` and `reach.rs` are

**Interfaces:**
- Produces: `Snapshot.targeted_blocks: bool`.
- Produces: a session accessor in the world, `fn targeted(&self) -> bool`, true only when the session's rules are the Advanced rules and the flag is set. Every later task reads only this.
- Produces: settings key `game.targeted_blocks`, values `"true"` and `"false"`, default `"false"`.
- Produces: strings `TARGETED_BLOCKS`, `TARGETED_BLOCKS_HELP`, `TARGETED_BLOCKS_FIXED` with the texts from the spec's "Player text" section. The fixed text is "Targeted blocks are chosen before a game starts. Change them from the main menu."

- [ ] **Step 1: Write the failing probes** in `targeted.rs`:
  - `flag_round_trip`: a snapshot with `targeted_blocks = true` encodes and decodes to an equal snapshot.
  - `old_snapshot_off`: a snapshot decoded at version `FEAT_TARGETED_BLOCKS.1 - 1` has `targeted_blocks == false`. Model it on the probe at `crates/sim/src/trace/duelling_probes.rs:770`.
  - `ja_ignores_flag`: a world under Jedi Academy's rules with the flag set returns `targeted() == false`.
- [ ] **Step 2: Run** `kjk --mute sim probes --only "targeted"`. Expected: the three checks fail or do not compile.
- [ ] **Step 3: Add the snapshot field, the gate, the bump file and the world accessor.** Follow `saber_rules` at each place it is carried. The value is taken from the settings when a session starts, on the same path as the combat rules, and a guest takes the host's value from the snapshot.
- [ ] **Step 4: Add the settings key, the strings and the options row.** The row is shown on the Sabers page below "Combat rules". In a running session it shows the session's value and the fixed text, as the "Combat rules" row does.
- [ ] **Step 5: Add a gui probe** beside the existing options probes: the Sabers page lists "Targeted blocks" directly after "Combat rules", and in a running session the row shows the fixed text.
- [ ] **Step 6: Run** the sim probes, `kjk --mute gui probes` and the gate. Expected: all pass.
- [ ] **Step 7: Commit** as "Targeted blocks setting".

### Task 2: The sides

**Files:**
- Create: `crates/sim/src/combat/saber/duel/aim.rs`, declared in `crates/sim/src/combat/saber/duel.rs`
- Modify: `data/rules/duel.toml` (new section `[targeted]` only)
- Test: `crates/sim/src/trace/duelling_probes/targeted.rs`

**Interfaces:**
- Consumes: `Quad` and `MoveData.start` (`crates/sim/src/combat/saber/moves.rs`), `duel::unblockable(mv)`, and the function that gives the line a player's blaster shots follow (read `combat::mind::crosshair` and `mind::aim_for`, named in the doc comment of `Command::aim` in `crates/sim/src/input.rs`).
- Produces:

```rust
pub enum Side { Left, Right, Above }

/// The side of the attacker's swing as the blocker sees it. None: no side.
pub fn swing_side(mv: Move, attacker_yaw: f32, blocker_yaw: f32) -> Option<Side>;

/// The side of the attacker that the crosshair line is on. None: no side.
pub fn crosshair_side(line_from: Vec3, line_way: Vec3, chest: Vec3) -> Option<Side>;

/// The attacker's chest: `targeted::CHEST_PERCENT` of its eye height above its origin.
pub fn chest(origin: Vec3, eye_height: f32) -> Vec3;

/// Whether a block in the open window is perfect.
pub fn aimed(swing: Option<Side>, crosshair: Option<Side>) -> bool;
```

- Rule numbers in `[targeted]`: `DEAD_DEG = 3`, `SIDE_ON = 0.3`, `CHEST_PERCENT = 75`, `WINDOW_PERCENT = 200`, `THREAT_M = 4`. Each needs the one-sentence summary the rulebook prints.

- [ ] **Step 1: Write the failing probes:**
  - `swing_side_table`: with the two fighters facing each other, start quadrants `T`, `TL`, `L`, `BL`, `TR`, `R`, `BR`, `B` give `Above`, `Right`, `Right`, `Right`, `Left`, `Left`, `Left`, `None`. (The attacker's left is the blocker's right when they face each other.) With the attacker turned 90 degrees to either side, every left or right quadrant gives `None` and `T` still gives `Above`. With both facing the same way (the attacker behind the blocker), `L` gives `Left`. A move from `duel::unblockable` gives `None` whatever its quadrant.
  - `crosshair_side_table`: yaw and pitch offsets in degrees `(0, 0)`, `(2, 2)`, `(10, 0)`, `(-10, 0)`, `(0, 10 above)`, `(6, 8 above)`, `(8, 6 above)`, `(0, 10 below)`, `(2.9, 0)`, `(3, 0)` give `None`, `None`, the side the crosshair is on, the other side, `Above`, `Above`, the yaw side, `None`, `None`, the yaw side. Name left and right by the screen: a crosshair to the right of the chest on the screen is `Right`.
  - `aimed_table`: `(None, any) → true`, `(Some(Left), Some(Left)) → true`, `(Some(Left), Some(Right)) → false`, `(Some(Left), None) → false`, `(Some(Above), Some(Above)) → true`.
  - `crosshair_line_matches_shots`: for the third-person camera, the shoulder camera on each side and first person, with an attacker 1.5 meters ahead, the side from the shot line equals the side computed from where the chest and the crosshair are on the screen. Build the commands the way the existing aim probes do.
- [ ] **Step 2: Run** the probes. Expected: fail.
- [ ] **Step 3: Implement `aim.rs`.** `swing_side` takes the horizontal sign of the start quadrant, turns it into a direction with the attacker's right vector, and compares its dot product with the blocker's right vector against `SIDE_ON`. `crosshair_side` measures the two offsets between `line_way` and the direction from `line_from` to `chest`.
- [ ] **Step 4: Run** the probes and the gate, then `cargo xtask rulebook`. Expected: pass.
- [ ] **Step 5: Commit** as "Swing sides and crosshair sides".

### Task 3: The block

**Files:**
- Modify: `crates/sim/src/world/sabers/duel.rs` (`duel_guard_when` near line 270 and its callers)
- Modify: `crates/sim/src/motion/saber.rs` (`duel_block`, the line that sets `perfect_until` near line 510)
- Test: `crates/sim/src/trace/duelling_probes/targeted.rs`

**Interfaces:**
- Consumes: `targeted()` from Task 1, everything from Task 2.
- Produces: no new state. `duel_guard_when` gains the attacker's index, so it can read the attacker's move and facing.

- [ ] **Step 1: Write the failing probes.** Each stages one keyed blocker and one attacker under the Advanced rules, lets the attacker swing, has the blocker hold block and press attack inside the window, and reads the exchange from the existing tap (`duel::tap::Tapped::Exchange`, fields `guard` and `perfect`).
  - `right_side_is_perfect`: flag on, the crosshair on the swing's side. `guard == Guard::Perfect`, `perfect == true`.
  - `wrong_side_is_aimed`: flag on, the crosshair on the other side. `guard == Guard::Active`, `perfect == false`.
  - `centre_is_aimed`: flag on, the crosshair on the attacker's chest. `guard == Guard::Active`.
  - `no_side_is_perfect`: flag on, the attacker plays a spin attack, the crosshair anywhere in front. `perfect == true`.
  - `flag_off_ignores_aim`: flag off, the crosshair on the wrong side. `perfect == true`.
  - `window_doubled`: with the flag on, `perfect_until - aimed_at` is twice the value with the flag off, for Reflex saves 0 and 6.
  - `two_attackers`: two attackers swing from different sides in the same tick, the crosshair on the first one's side. The exchange with the first is perfect and the exchange with the second is not.
  - `creature_guard_unchanged`: flag on, a creature blocks a player's swing. Its guard is `Guard::Npc`, as with the flag off.
- [ ] **Step 2: Run** the probes. Expected: fail.
- [ ] **Step 3: Implement.** In the keyed branch of `duel_guard_when`, when `targeted()` is true and the guard is `Guard::Perfect`, compute `swing_side` for the attacker's current move and `crosshair_side` for the blocker's shot line against the attacker's `chest`. If `aimed` is false, return `Guard::Active`. In `duel_block`, when the session is targeted, the window is `WINDOW_PERCENT` of its normal length. `duel_block` reads the rules through `arms.rules`. Give it the flag on the same path.
- [ ] **Step 4: Run** the probes and the gate. Expected: pass.
- [ ] **Step 5: Check that nothing else changed.** Run the trace on this branch and on `feat/advanced-pacing` at `b2c7c0fc`: `kjk --mute trace m6 random --seed 100 --seeds 40 --n 5000 --out <file>`. Strip ` fenc=`, ` hash=` and ` duel=` from both and compare with `kjk --mute trace-diff`. Expected: identical. Then compare one trace under the Advanced rules with the flag off against the same trace on the base. Expected: identical once the same three fields are stripped.
- [ ] **Step 6: Commit** as "Targeted perfect blocks".

### Task 4: The reticule

**Files:**
- Modify: `crates/sim/src/combat/saber/duel/aim.rs` (the threat)
- Create: `crates/game/src/block_reticle_view.rs`, modelled on `crates/game/src/reticle_view.rs`, declared in `crates/game/src/lib.rs`
- Modify: `crates/ui/src/vm.rs` (a view struct beside `ReticleView` near line 1286), `crates/ui/src/screens/hud.rs` (a drawing function beside `reticle` near line 805)
- Test: `crates/sim/src/trace/duelling_probes/targeted.rs`, a new file in `crates/ui/src/probes/` beside `reticle.rs`, and `probes()` in the new view file

**Interfaces:**
- Consumes: Tasks 1 and 2. `Duel.swing_began` tells when a fighter's swing began.
- Produces:

```rust
/// One hostile fighter the view offers as a possible threat.
pub struct Near { pub distance: f32, pub in_front: bool, pub swing_began: Option<u64> }

/// The index of the threat among `near`, by the spec's rule. None: no threat.
pub fn threat(near: &[Near]) -> Option<usize>;
```

```rust
/// What the HUD draws around the crosshair while the player blocks.
pub struct BlockReticleView {
    pub crosshair: Option<Side>,   // the lit mark
    pub swing: Option<Swing>,      // None: no swing under way
}
pub enum Swing { Side(Side), Any }
```

- [ ] **Step 1: Write the failing probes:**
  - `threat_order` (sim): of two swinging fighters within 4 meters in front, the one whose swing began earlier is the threat. A swinging fighter at 5 meters is ignored. A swinging fighter behind the player is ignored. With nobody swinging, the nearest fighter in front within 4 meters is the threat. With nobody near, the result is `None`.
  - `view_shown` (game): the view is `Some` only while the player's own character holds block with a melee weapon, the snapshot's flag is set and the rules are Advanced. It is `None` under Jedi Academy's rules, with the flag off, with a blaster in hand, and with block released.
  - `guest_reads_snapshot` (game): with the local setting off and the snapshot's flag on, the view is `Some`. With the local setting on and the snapshot's flag off, it is `None`.
  - `view_sides` (game): for a staged attacker swinging from the blocker's left with the crosshair to its left, `crosshair == Some(Left)` and `swing == Some(Swing::Side(Left))`. For a spin attack, `swing == Some(Swing::Any)`.
  - `marks_drawn` (ui): the drawing function returns three marks for an empty view, in the warning colour for the swing's mark when the sides differ, in the confirm colour when they match, and all three in the confirm colour for `Swing::Any`.
- [ ] **Step 2: Run** the sim and gui probes. Expected: fail.
- [ ] **Step 3: Implement `threat`, the view and the drawing.** The view reads only the snapshot. It builds the crosshair line the same way Task 3 does, so the lit mark and the rule agree. Use the HUD's existing warning and confirm colours and the existing reticule's quads. Draw no new art.
- [ ] **Step 4: Run** the probes and the gate. Expected: pass.
- [ ] **Step 5: Commit** as "Targeted blocks reticule".

### Task 5: Rulebook, clips and the report

**Files:**
- Modify: `docs/RULEBOOK.md` (generated), `docs/SABER-RULES.md` (a short section on targeted blocks, in plain words)

- [ ] **Step 1: Regenerate** the rulebook with `cargo xtask rulebook` and add the section to `docs/SABER-RULES.md`.
- [ ] **Step 2: Run every check.** The gate. `cargo test --release -p approved_tests` (expected: no test edited, all pass). `kjk --mute sim probes` and `kjk --mute gui probes` (expected: no failure that the base branch does not also have). `kjk --mute chain k1` (expected: 29 of 29). The Jedi Academy trace comparison from Task 3 Step 5 (expected: identical).
- [ ] **Step 3: Record clips** in the arena (`kjk play --stage arena`, headless, muted, scratch settings), with the flag on: a perfect block from the left, from the right and from above, a block with the wrong side, and a spin attack. Record one of them with the shoulder camera and one in first person. Send each clip to the owner with its absolute path.
- [ ] **Step 4: Commit** as "Targeted blocks in the rulebook".
- [ ] **Step 5: Report to the owner.** Say what was built, what each check printed, where the clips are, and how to try it (the setting's name and the arena command). List the starting numbers the owner may want to change: the 3 degree dead zone, the doubled window, the 4 meter threat range, the chest height. Do not push. Do not open a pull request.
