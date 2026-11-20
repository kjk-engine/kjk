# Stuns That Last As Long As KOTOR's: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace C11's 3-second cap on stuns, stasis, fear, sleep, droid stun and Force Whirlwind with the effect's own duration. A held target rolls a saving throw each round to break free, with a growing bonus. Repeated holds get shorter.

**Architecture:** `ForceState` (`crates/sim/src/combat/force.rs`) keeps each hold's save, DC, the next roll time and the count of recent holds. A step system rolls due saves on the authority. Every caller of `ForceState::hold` says what kind of hold it is and which save first allowed it. All numbers live in `data/rules/force.toml`.

**Tech Stack:** Rust, the `sim` crate (Bevy ECS step), the `rules` crate (generated from `data/rules/*.toml`), the snapshot wire format.

**Spec:** `docs/superpowers/specs/2026-11-19-crowd-control-holds-design.md`

## Global Constraints

- Work on branch `feat/stun-saves` in a worktree cut from `origin/main`. Its bump file is `bumps/feat-stun-saves.toml`, so the generated gate is `crate::since::snapshot::FEAT_STUN_SAVES`.
- Never edit a format version number or `bumps/SIM_HASH` (bumps/README.md).
- Every rule number goes in `data/rules/force.toml` with a plain `summary`. No number is written in code.
- Every `kjk` run is `--headless --mute`, with `KJK_SETTINGS` and `KJK_SAVES` pointing at scratch copies.
- Do not run clippy, the full approved tests or chains locally. CI runs clippy and the tests. One fresh K1 chain runs before merge.
- Values from the spec: a round is 3000 ms. The struggle bonus is +5 for each earlier roll of the same hold. The longest hold is 30000 ms. A hold counts as recent for 10000 ms after it ends. The second recent hold is 50% and the third 25%, and a fourth is refused. A shortened hold lasts at least 3000 ms, or its full duration if that is shorter. Stun Immunity stays 1000 ms.
- Story holds and kjk's own path keep exactly today's behaviour: `min(ms, 3000)` and then 1000 ms of immunity.
- Covering the ears (`cover_ears`) and KOTOR states (`effects::state`) are not touched.

## Review Focus

- A hold that is refused as the fourth recent hold must not set `hold`, show its visuals or count as recent again. Expect the "Immune" feedback and no state change. Test it in Task 1.
- A fear that ends on damage must also end its save rolls and still count as recent. Test it in Task 2.
- A save loaded from main (an old snapshot) with a hold running must keep its end time and roll no saves. Test it in Task 1's encoding test.
- A hold applied by a script with no save call before it (a scripted EffectStunned in combat) must use the caster's power DC and the save the spec's list gives. Test it in Task 3.
- Two holds landing in the same step on one target (an area power and an on-hit stun) must not stack. As today, the second is refused because the target is already held, and it does not count as recent. Test it in Task 1.

---

### Task 1: The hold model in ForceState

**Files:**
- Modify: `data/rules/force.toml` (the `[crowd]` table at about line 1127, `[crowd.hold_ms]` at about line 1163)
- Modify: `crates/sim/src/combat/force.rs` (`ForceState` at about line 550, `hold` at line 623, `encode` at line 731, `decode` at line 795, `ja` constants at about line 1117)
- Create: `bumps/feat-stun-saves.toml`
- Test: unit tests in `crates/sim/src/combat/force.rs` (its `#[cfg(test)]` module)

**Interfaces:**
- Produces:
  - `pub struct HoldSave { pub index: u8, pub dc: i32 }` in `force.rs`. `index` is `Sheet::save`'s index: 0 Fortitude, 1 Reflex, 2 Will.
  - `pub enum HoldKind { Combat(Option<HoldSave>), Story, Own }`. `Combat(None)` is a combat hold that allowed no save, such as the adhesive grenade.
  - `ForceState::hold(&mut self, now: u64, ms: u64, kind: HoldKind) -> bool`. It replaces today's `hold(now, ms)`.
  - New `ForceState` fields: `save: Option<HoldSave>`, `save_due: u64`, `rolls: u8`, `recent: u8`, `recent_until: u64`.
  - `ja::ROUND_MS`, `ja::STRUGGLE_BONUS`, `ja::LONGEST_MS`, `ja::RECENT_MS`, `ja::SECOND_PERCENT`, `ja::THIRD_PERCENT`, `ja::MOST_RECENT`, read from the rules crate as `ja::STUN_MS` is today.

- [ ] **Step 1: Add the rules.** In `data/rules/force.toml`, rename `[crowd.hold_ms]` to "Hold Without a Duration" (still 3000 ms). Its summary says that kjk's own powers, where no script runs, and story holds hold for this long. Add `[crowd.round_ms]` "Saving Throw Each Round" 3000 ms, `[crowd.struggle_bonus]` "Struggle Bonus" 5 (type i32), `[crowd.longest_ms]` "Longest Hold" 30000 ms, `[crowd.recent_ms]` "Recent Hold" 10000 ms, `[crowd.second_percent]` 50, `[crowd.third_percent]` 25 and `[crowd.most_recent]` 3. Each entry has `source = "the owner's decision, 2026-11-19"`, a plain summary and the same `applies` list as `hold_ms`. Rewrite the `[crowd]` summary to describe the rules in the spec's words.

- [ ] **Step 2: Write the failing tests** in `force.rs`'s test module:

```rust
#[test]
fn combat_hold_lasts_its_duration() {
    let mut f = ForceState::default();
    assert!(f.hold(0, 9_000, HoldKind::Combat(Some(HoldSave { index: 0, dc: 20 }))));
    assert_eq!(f.stunned_until, 9_000);
    assert_eq!(f.save_due, 3_000);
}

#[test]
fn combat_hold_stops_at_longest() {
    let mut f = ForceState::default();
    f.hold(0, 100_000, HoldKind::Combat(None));
    assert_eq!(f.stunned_until, 30_000);
}

#[test]
fn story_and_own_holds_keep_todays_cap() {
    for kind in [HoldKind::Story, HoldKind::Own] {
        let mut f = ForceState::default();
        f.hold(0, 9_000, kind);
        assert_eq!((f.stunned_until, f.immune_until), (3_000, 4_000));
        assert_eq!(f.save, None);
    }
}

#[test]
fn repeated_holds_shorten_then_refuse() {
    let mut f = ForceState::default();
    let kind = || HoldKind::Combat(None);
    f.hold(0, 12_000, kind());                 // first: 12 s
    assert!(f.hold(13_000, 12_000, kind()));   // second: 50% = 6 s
    assert_eq!(f.stunned_until, 19_000);
    assert!(f.hold(20_000, 12_000, kind()));   // third: 25% = 3 s
    assert_eq!(f.stunned_until, 23_000);
    assert!(!f.hold(24_000, 12_000, kind()));  // fourth: refused
    assert!(f.hold(33_000, 12_000, kind()));   // 10 s after the last ended: full again
    assert_eq!(f.stunned_until, 45_000);
}

#[test]
fn shortened_hold_never_below_a_round() {
    let mut f = ForceState::default();
    f.hold(0, 4_000, HoldKind::Combat(None));
    f.hold(5_000, 4_000, HoldKind::Combat(None)); // 50% would be 2 s
    assert_eq!(f.stunned_until, 8_000);
    let mut g = ForceState::default();
    g.hold(0, 2_000, HoldKind::Combat(None));
    g.hold(3_000, 2_000, HoldKind::Combat(None)); // full 2 s is under a round: stays 2 s
    assert_eq!(g.stunned_until, 5_000);
}

#[test]
fn refused_hold_changes_nothing() {
    let mut f = ForceState::default();
    for t in [0, 4_000, 8_000] { f.hold(t, 3_000, HoldKind::Combat(None)); }
    let before = f.clone();
    assert!(!f.hold(12_000, 3_000, HoldKind::Combat(None)));
    assert_eq!(f, before);
}

#[test]
fn second_hold_while_held_is_refused() {
    let mut f = ForceState::default();
    f.hold(0, 9_000, HoldKind::Combat(None));
    assert!(!f.hold(0, 3_000, HoldKind::Combat(None)));
    assert_eq!(f.stunned_until, 9_000);
    assert_eq!(f.recent, 1);
}

#[test]
fn old_snapshot_hold_rolls_no_saves() {
    // Encode with the fields an old snapshot has (no FEAT_STUN_SAVES), decode now.
    // Expect: stunned_until kept, save None, save_due 0, rolls 0, recent 0.
}
```

Today's `immune_until` check stays. `immune_until` is set to the hold's end plus 1000 ms, so a hold is refused while another runs and during the 1 second after it. A refused hold changes nothing.

- [ ] **Step 3: Run the tests and see them fail.** Run `cargo test --release -p sim combat::force::tests`. Expect a compile error on `HoldKind`.

- [ ] **Step 4: Implement** the types, fields and `hold` in `force.rs`. Add the new fields to `encode` after `immune_until`, and in `decode` read them only `if input.since(crate::since::snapshot::FEAT_STUN_SAVES)`, else defaults, as `ears_until` does. Write `bumps/feat-stun-saves.toml` with a `sim` line, `[formats] snapshot = "a fighter's hold keeps its save, its next roll and its recent holds (`ForceState::save`)."` and `[migrations] snapshot = "old saves read no save and no recent holds, so a running hold keeps its end and rolls nothing"`. Change every caller to compile: pass `HoldKind::Own` for now. Task 3 gives each caller its real kind.

- [ ] **Step 5: Run the tests and see them pass.** Run `cargo test --release -p sim combat::force::tests`.

- [ ] **Step 6: Commit** with the title "Holds that last their own time, shorter when repeated".

### Task 2: A saving throw each round

**Files:**
- Modify: `crates/sim/src/combat/force.rs` (new methods on `ForceState`)
- Modify: the step's force system in `crates/sim/src/world/force.rs`, or wherever `stunned_until` holds are settled each step. The builder picks the place, next to `combat::effects::settle`.
- Create: a probe group "holds" in `crates/sim/src/world/held_probes.rs` (the file PR 599 added), registered in `crates/launcher/src/sim_probes.rs`

**Interfaces:**
- Consumes: Task 1's `HoldSave`, the fields and `ja::ROUND_MS` and `ja::STRUGGLE_BONUS`.
- Produces:
  - `ForceState::due(&self, now: u64) -> Option<(HoldSave, i32)>`. It returns the save and the bonus (`rolls * STRUGGLE_BONUS`) when `save` is set, the hold runs and `now >= save_due`.
  - `ForceState::struggle(&mut self, now: u64, saved: bool)`. On success it ends the hold now and starts the 1 second of immunity, and the hold still counts as recent. On failure it adds 1 to `rolls` and sets `save_due += ROUND_MS`.

- [ ] **Step 1: Write the failing probes** in the "holds" group:
  - `low_save_holds_full`: a Stun with save +0 against DC 40 holds all 9 s.
  - `high_save_breaks_after_a_round`: a save of +40 against DC 10 ends the hold at 3000 ms, never before.
  - `struggle_bonus_grows`: the second roll is made with +5 and the third with +10. Check it through `due`.
  - `fear_ends_on_damage_and_counts`: a fear hit by damage ends, rolls no more saves, and `recent` is 1.
  - `natural_rolls`: a natural 20 always breaks free and a natural 1 never does (`combat::check`).
- [ ] **Step 2: Run** `kjk sim probes --only holds --mute`. Expect the probes to fail.
- [ ] **Step 3: Implement** `due` and `struggle`. Add the step system, which rolls `streams.authority` d20 + `sheet.save(index)` + bonus against `dc` with `combat::check`, on the authority only. When fear ends on damage, clear `save`.
- [ ] **Step 4: Run** `kjk sim probes --only holds --mute`. Expect every probe to pass.
- [ ] **Step 5: Commit** with the title "A saving throw each round to break free".

### Task 3: Each hold's kind, save and DC

**Files:**
- Modify: `crates/sim/src/script/engine.rs` (the `FortitudeSave | ReflexSave | WillSave` arm at about line 4111, and `carry` at line 1418)
- Modify: `crates/sim/src/script/effects.rs` (`Applied` at line 450, and its encoding if it has one)
- Modify: `crates/sim/src/world/harm.rs` (the hold loop at about lines 140 to 165)
- Modify: `crates/sim/src/combat/gear.rs` (`Struck::Hold` at line 503, `on_hits` at line 527)
- Modify: `crates/sim/src/combat/grenades.rs` (`Secondary::Held`, `afflict` at line 422)
- Modify: `crates/sim/src/world.rs` (`struck_by` at line 533)
- Modify: `crates/sim/src/world/powers.rs` (`whirlwind` at line 1822, and `Cast::crowd`)
- Test: probes in the "holds" group

**Interfaces:**
- Consumes: Task 1's `HoldKind` and `HoldSave`.
- Produces:
  - `Applied::save: Option<HoldSave>`, the last save the target failed against this caster in the same script run.
  - `Struck::Hold { ms, fear, save: HoldSave }`.
  - `Secondary::Held { ms, dc: Option<i32> }`. A grenade's save is always Reflex (index 1).

- [ ] **Step 1: Write the failing probes:**
  - `scripted_stun_uses_its_save`: the script's Stun, after a failed `WillSave` at DC 22, holds with `HoldSave { index: 2, dc: 22 }`.
  - `scripted_hold_without_save_uses_power_dc`: an `EffectStunned` applied by a hostile caster with no save call uses `force::save_dc` of the caster, with Fortitude for stun and Will for fear and sleep.
  - `permanent_or_friendly_hold_is_story`: a permanent hold, or one whose creator is not hostile to the target, holds as today (3 s), with no save and no recent count.
  - `adhesive_holds_its_whole_time`: the adhesive grenade's entangle holds its full duration with no saves.
  - `on_hit_stun_uses_will`: a weapon's on-hit stun carries Will and the property's DC.
  - `whirlwind_uses_reflex`: Force Whirlwind holds with Reflex against the caster's power DC.
- [ ] **Step 2: Run** `kjk sim probes --only holds --mute`. Expect the new probes to fail.
- [ ] **Step 3: Implement.** In the engine, keep a per-run record from target to its last failed save against the running script's caller. `carry` puts it on `Applied` when the carried effect holds. In `harm`, choose `HoldKind::Story` when `applied.until == Some(u64::MAX)` or the creator is not hostile to the target. Otherwise choose `HoldKind::Combat(applied.save.or(the spec's fallback))`. Pass each other caller's save and DC through. `Cast::crowd` keeps `HoldKind::Own`.
- [ ] **Step 4: Run** `kjk sim probes --only holds --mute` and `kjk script impacts k1 --mute`. Expect both to pass.
- [ ] **Step 5: Commit** with the title "Each hold rolls the save that first allowed it".

### Task 4: Feedback, documents and the anti-goal measurement

**Files:**
- Modify: the combat feedback code that prints save lines and "Immune". The builder finds where `powers::saves` results are reported.
- Modify: `docs/COMBAT.md` (C11)
- Create: the probe `full_length_shares` in the "holds" group

- [ ] **Step 1: Feedback.** When a held character breaks free, the feedback shows the save in the form of KOTOR's other saving throw lines, using the game's own talk-table line. A refused fourth hold shows "Immune".
- [ ] **Step 2: Measurement probe.** `full_length_shares` runs 2000 staged holds of each case. The first case is a level 8 player with Fortitude +6 stunned at DC 22 for 9 s. The second is a boss with Fortitude +10 in Stasis at DC 23 for 12 s. The probe prints how often each hold goes its full length. It passes when the shares are within 5 percentage points of 37% and 2%.
- [ ] **Step 3: Run** `kjk sim probes --only holds --mute` and `kjk rules k1 --mute`. Expect the probes to pass and the rulebook to show the new entries.
- [ ] **Step 4: Documents.** Rewrite C11 in `docs/COMBAT.md` to describe the new rules in plain sentences.
- [ ] **Step 5: Commit** with the title "Hold feedback and the rules described".

### Task 5: Branch checks

- [ ] **Step 1:** Run `kjk sim determinism --mute`. The hash changes, and the bump file's `sim` line explains why.
- [ ] **Step 2:** Push, open the PR (title "Stuns that last as long as KOTOR's") and let CI run clippy and the tests. List every approved test whose output changed, for the owner to approve.
- [ ] **Step 3:** Run one fresh K1 chain and post the result on the PR.
