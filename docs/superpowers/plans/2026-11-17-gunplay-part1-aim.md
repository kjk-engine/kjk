# Gunplay Part 1 (Hold to Aim) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A player holds [Aim] to raise any blaster, walk, and shoot with half the spread. The gun's feat attack moves to a new [Feat attack] action. Sniper Shot becomes one steadied shot, used like Power Blast.

**Architecture:** Aiming is sim state on `combat::specials::Shooting` (a raise level, an "aimed" flag and a "steadying" flag). The two new buttons are two new bits of `Command::holds`, so the command's encoding does not change. The view reads the sim's raise level for the pose and the camera. Enemies and companions get the raised pose from the shots the view already sees, with no sim state.

**Tech Stack:** Rust, Bevy. Crates `sim`, `game`, `launcher`, `rules`, `approved_tests`. Rulebook in `data/rules/*.toml`.

**Spec:** `docs/superpowers/specs/2026-11-17-gunplay-design.md` (part 1, "Settled after the builder's review", and "Sniper Shot's balance"). Read it before any task.

## Global Constraints

- Branch `feat/gunplay-aim`. Never push. Never open a pull request. Never merge.
- Never edit `bumps/SIM_HASH` or a format's `VERSION`. The branch's changes are said in `bumps/feat-gunplay-aim.toml` (`bumps/README.md`).
- Every number a game rule plays by lives in `data/rules/*.toml` and the sim reads it as `rules::…`. No literal in the sim. After a rule changes, run `cargo xtask rulebook`.
- Permanent tests live only in `crates/approved_tests`. A layer's checks are probes (`pub fn probes() -> Checks`) that an approved test runs with `probed("name", &…::probes(), at_least)`.
- Every `kjk` run is `--headless --mute`, with `KJK_SETTINGS` and `KJK_SAVES` pointing at scratch copies. Check the real settings file's hash before and after a run.
- Do not run the whole gate or a chain for each step. Per task: `cargo fmt --all -- --check`, `cargo clippy -q -p <touched crates> --all-targets -- -D warnings`, and the approved tests the task names. CI runs the rest on the pull request.
- Run a command that may exceed 2 minutes in the background and wait for its notice. No sleep loops.
- Every text a person reads (comments, rule summaries, commit messages, labels) follows `CLAUDE.md`: full sentences, one fact each, no colons in titles, the actual number.
- No decompiler names, no memory addresses, no game files committed. No hand-drawn effects.
- Commit messages are a short noun phrase, with no attribution lines.
- Spec values, verbatim: spread ×0.5 while aimed; 200 ms in; 150 ms out; Sniper Shot steadies for 500 ms, or 250 ms while aimed; camera field of view 75% of normal; other characters hold the raised pose until 600 ms after their last shot.

## Decisions this plan makes

- **Buttons in the sim.** `holds::AIM` is bit 2 and `holds::FEAT` is bit 3 of `Command::holds`. With a blaster in hand the sim reads `AIM` for aiming and `FEAT` for the feat attack, and it ignores `buttons::ALT_ATTACK`. With anything else in hand it ignores `AIM` and `FEAT`.
- **Feat attack defaults.** Keyboard R. Controller LB with the right trigger. No well-placed input is unbound today. R is [Saber throw and kata], which does nothing with a blaster. LB with the right trigger is the attack made while blocking, which only a blade does. Both are free while a blaster is in hand, the same way the right mouse button is shared by [Aim] and [Alt attack]. The strictly unbound alternatives are the O key and LB with the D-pad's right.
- **Feat attack wins over Attack.** While `FEAT` is held with a blaster, the sim reads [Attack] as not held. LB with the right trigger sends both.
- **Minds.** A creature's mind, and a feat used from the action bar, press "the weapon's other mode". That press sets `ALT_ATTACK` and `holds::FEAT` together.
- **Raise level.** `Shooting::aim` counts from 0 to `IN_MS × OUT_MS`. Each step it rises by `OUT_MS × msec` while [Aim] is held and falls by `IN_MS × msec` otherwise. `Shooting::aiming` turns true at the top and false at 0. This gives 200 ms in and 150 ms out with whole numbers.
- **Slow walk.** While `aim > 0` or the character steadies a Sniper Shot, the body's command is turned into a walking command (the walk button and axes of 64, as a mind's `walking` does) and its up axis is capped at 0. A roll needs a run, so it cannot start.
- **Sniper Shot.** It reuses `Shooting::charging_from` for when the hold began. Nothing is paid at the press. On release after the steady time, the cost is paid, one shot leaves with the special, and the Defense penalty starts. The shot has a spread of 0 and no kick on its aim line. A pair fires both guns for one cost.
- **Saved bindings.** Layout 6. A file's [Alt attack] is kept. [Aim] takes the file's [Alt attack] input when the file has one (a reversed mouse keeps aim on the alternate button), else its default. [Feat attack] takes its default unless another action of the file is on it, and is then left unbound.
- **Replays.** The command's encoding is unchanged, so the replay format is not bumped. Old replays load and may play back differently, which the spec allows.

## File map

| File | Change |
|---|---|
| `crates/sim/src/input.rs` | `holds::AIM`, `holds::FEAT`, `Command::other_mode` |
| `crates/sim/src/moves.rs`, `crates/sim/src/trace/duels.rs`, `crates/sim/src/trace/movement.rs` | `alt` also holds `AIM`; new word `feat` |
| `crates/sim/src/combat/specials.rs` | `Shooting::{aim, steadying}`, aim step, codec, new Sniper Shot in `pull` |
| `crates/sim/src/combat/pair.rs` | Sniper Shot on a pair |
| `crates/sim/src/combat/bloom.rs` | the stance reads the aimed state |
| `crates/sim/src/combat/ai.rs`, `crates/sim/src/world/minds.rs` | a mind's hold for Sniper Shot, `other_mode` |
| `crates/sim/src/world/firing.rs` | aim step, feat trigger, Sniper Shot's spread |
| `crates/sim/src/world.rs` (`command_for`), `crates/sim/src/world/powers.rs`, `crates/sim/src/world/movement.rs`, `crates/sim/src/combat/idle.rs` | slow walk, no jump, old aim speed removed, restrict mode |
| `crates/sim/src/world/aim_probes.rs` (new) | the sim's probes for aiming and Sniper Shot |
| `data/rules/weapons.toml`, `feats.toml`, `minds.toml`, `controls.toml` | rules and texts |
| `bumps/feat-gunplay-aim.toml` (new) | the snapshot bump and its migration |
| `crates/game/src/settings.rs`, `pad.rs`, `sim_host.rs`, `hud.rs`, `settings_probes.rs`, `pad/probes.rs` | `Action::Aim`, `Action::FeatAttack`, layout 6 |
| `crates/game/src/aim_pose.rs` (new), `cast.rs` | the per-wield aim pose, its blend, the raised pose of other characters |
| `crates/game/src/play_view.rs`, `first_person.rs`, `camera_tune.rs` | the camera's aim framing |
| `crates/game/src/aim_fx.rs`, `reticle_view.rs`, `shots.rs`, `combat_fx.rs`, `blaster_view.rs` | Sniper Shot's sight line and scope follow `steadying` |
| `crates/launcher/src/duel_verbs.rs`, `specials_verbs.rs`, `sim_verbs.rs` | bench and verbs on the new buttons, `sniper_balance` |
| `crates/approved_tests/tests/approved.rs` | the new probes wired in |
| `docs/COMBAT.md`, `docs/RULEBOOK.md` (generated) | texts |

## Review Focus

1. A player who switches weapon, holsters, is stunned, is knocked down or starts using an item while aiming or steadying stops aiming at once. The slow walk ends, no shot leaves, and no stamina is spent. Test in Task 2 (`aim_drops`) and Task 3 (`steady_drops`).
2. A co-op guest's own prediction steps the raise level exactly as the host does. Test in Task 2 (`aim_predicts`).
3. A settings file with a reversed mouse, or with another action already on R or the right mouse button, ends with no two actions on one input unless `SHARES` allows it, and keeps its [Alt attack]. Test in Task 4 (`layout_6`).
4. On a controller, LB with the right trigger still attacks while blocking with a blade under the Advanced rules, and LB with A (a bar slot) with a blaster in hand starts no feat attack. Test in Task 4 (`feat_chord`).
5. A snapshot or save written before this branch, with the old Sniper aim bit set, loads with the character not aiming, not steadying and not slowed. Test in Task 1 (`old_snapshot_aim`).

---

### Task 1: The shared contract (buttons, state, rules, bump)

**Files:**
- Modify: `crates/sim/src/input.rs:114-122`, `crates/sim/src/moves.rs:1-50,147-185`, `crates/sim/src/trace/duels.rs:475-505`, `crates/sim/src/trace/movement.rs:70-80`
- Modify: `crates/sim/src/combat/specials.rs:276-421`
- Modify: `data/rules/weapons.toml` (after `[bloom.kick_limit]`), `data/rules/feats.toml:402-454`, `data/rules/minds.toml:535-555`
- Create: `bumps/feat-gunplay-aim.toml`
- Create: `crates/sim/src/world/aim_probes.rs`; wire it in `crates/sim/src/world.rs` (as `bloom_shot_probes` is) and `crates/sim/src/lib.rs` (as `pair_probes` is exported)
- Modify: `crates/approved_tests/tests/approved.rs` (beside the `"dual pistols"` test, line 3781)

**Interfaces (produces):**

```rust
// crates/sim/src/input.rs
pub mod holds {
    pub const BLOCK: u8 = 1;
    pub const KICK: u8 = 1 << 1;
    /// Hold a blaster up to aim.
    pub const AIM: u8 = 1 << 2;
    /// A blaster's feat attack.
    pub const FEAT: u8 = 1 << 3;
}
impl Command {
    /// Presses the weapon's other mode, whatever is in hand: the alternate
    /// attack and a blaster's feat attack.
    pub fn other_mode(&mut self);
}

// crates/sim/src/combat/specials.rs
pub struct Shooting {
    // … existing fields; `aiming` now means "in the aimed state" …
    /// How far the gun is raised to aim, from 0 to `Shooting::AIM_FULL`.
    pub aim: u32,
    /// Steadying a Sniper Shot.
    pub steadying: bool,
}
impl Shooting {
    pub const AIM_FULL: u32;                       // rules IN_MS × OUT_MS
    pub fn aim_step(&mut self, held: bool, msec: u32);
    pub fn aim_share(&self) -> f32;                // 0.0 ..= 1.0
    pub fn slowed(&self) -> bool;                  // aim > 0 || steadying
}

// rules (generated from the toml)
rules::weapons::aim::IN_MS: u32 = 200
rules::weapons::aim::OUT_MS: u32 = 150
rules::weapons::bloom::STANCE[1] = 0.5            // "aiming"
rules::feats::specials::sniper_shot::STEADY_MS: [u64; 2] = [500, 250]   // labels: "from the hip", "while aiming"
// removed: rules::feats::specials::sniper_shot::AIM_SPEED, rules::minds::items::SNIPER_AIM_MS
```

`since::snapshot::FEAT_GUNPLAY_AIM` is the gate the bump file gives (`bumps/README.md`, "Reading old saves"; see how `FEAT_DYNAMIC_CROSSHAIR` is used at `specials.rs:399`).

- [ ] **Step 1: Read `bumps/README.md`, `crates/sim/src/since.rs` and `data/rules/README.md`.** Find how a pending bump file's gate constant is made and how a removed rule is retired.

- [ ] **Step 2: Write the failing probes** in `aim_probes.rs` (`pub fn probes() -> Checks`, in the style of `combat/bloom_probes.rs`):

```rust
// aim_level: 25 steps of 8 ms held reach the aimed state; not before.
let mut s = Shooting::default();
for _ in 0..24 { s.aim_step(true, 8); }
assert!(!s.aiming && s.aim > 0 && s.slowed());
s.aim_step(true, 8);
assert!(s.aiming && s.aim == Shooting::AIM_FULL && s.aim_share() == 1.0);
// 150 ms after the release it leaves the aimed state; not before.
for _ in 0..18 { s.aim_step(false, 8); }
assert!(s.aiming);
s.aim_step(false, 8);
assert!(!s.aiming && s.aim == 0 && !s.slowed());

// aim_round_trips: a Shooting with aim = 12345, aiming, steadying encodes and decodes to itself.
// old_snapshot_aim: bytes of the version before FEAT_GUNPLAY_AIM with flag bit 1 set
//   decode to aim == 0, !aiming, !steadying (model: bloom_probes::old_snapshots, line 192).
// interrupt_drops_aim: interrupt() leaves aim == 0, !aiming, !steadying.
// moves_words: Script::parse("1s alt") holds ALT_ATTACK and holds::AIM;
//   Script::parse("1s feat") holds holds::FEAT and not ALT_ATTACK.
// other_mode: Command::default().other_mode() sets ALT_ATTACK and holds::FEAT.
```

Probes return `Result<String, String>`, not asserts. The asserts above are the conditions.

- [ ] **Step 3: Wire the probes** into `approved.rs` as `probed("aim", &sim::aim_probes::probes(), 6)` in a test named `aim`. Run `cargo test --release -p approved_tests --test approved aim`. Expected: it does not compile.

- [ ] **Step 4: Add the rules.** In `weapons.toml`, a section `[aim]` titled "Aiming" with `[aim.in_ms]` (200, u32, ms, range 0 to 2000) and `[aim.out_ms]` (150, same). The section summary says, in plain sentences, what holding [Aim] does (walk speed, no run, roll or jump, spread multiplied, every blaster, players only). Change `[bloom.stance]` to `value = [2.5, 0.5, 0.6]` and reword its summary to "while the character aims" (no Sniper Shot). In `feats.toml`, remove `[specials.sniper_shot.aim_speed]`, add `[specials.sniper_shot.steady_ms]` (`type = "u64"`, `value = [500, 250]`, labels "from the hip" and "while aiming"), and rewrite the Sniper Shot summary to the new behaviour (spec, part 1, the Sniper Shot bullet). In `minds.toml`, remove `[items.sniper_aim_ms]` and reword `[items.charge_past_ms]` so it covers a Power Blast's charge and a Sniper Shot's steadying. Fix every reader of a removed rule so the workspace compiles (`specials.rs:74` `AIM_SPEED`, `powers.rs:2056-2060`, `ai.rs:376`, `launcher/src/specials_verbs.rs:369`): the proper behaviour comes in Tasks 2 and 3, so here `ai.rs` uses `STEADY_MS[0] + CHARGE_PAST_MS` and the two speed readers are deleted. Run `cargo xtask rulebook`.

- [ ] **Step 5: Implement the interface.** `aim_step` raises by `OUT_MS × msec` or lowers by `IN_MS × msec`, clamped to `0..=AIM_FULL`, then sets `aiming`. `interrupt` also clears `aim`, `aiming` and `steadying`. `encode` writes `steadying` as flag bit 6 and `aim` as a `u32` after `fired_at`. `decode` accepts flags up to 127, and before `FEAT_GUNPLAY_AIM` reads `aim = 0`, `aiming = false`, `steadying = false`. In `moves.rs`, `alt` also sets `holds::AIM`, and `feat` sets `holds::FEAT`; update the module documentation. Do the same for the word parsers in `trace/duels.rs` and `trace/movement.rs`.

- [ ] **Step 6: Write `bumps/feat-gunplay-aim.toml`:**

```toml
# The sim's hash moves on purpose.
sim = "a player holds [Aim] to raise a blaster: half the spread, walking speed, no jump; a blaster's feat attack is read from [Feat attack]; Sniper Shot is one steadied shot with no spread"

[formats]
snapshot = "a fighter carries how far its gun is raised to aim and whether it steadies a Sniper Shot (`Shooting::aim`, `Shooting::steadying`)."

[migrations]
snapshot = "old saves read no aim and no steadying, and drop the old Sniper Shot aim flag: the next step sets it again from the buttons held"
```

- [ ] **Step 7: Check the other encoders.** `git grep -n "shooting" crates/net crates/sim/src/snapshot.rs crates/sim/src/save.rs crates/sim/src/replay.rs`. If anything encodes `Shooting` field by field outside `Shooting::encode`, add the two fields there. Report what was found.

- [ ] **Step 8: Run** `cargo test --release -p approved_tests --test approved aim rulebook old_saves_load`. Expected: all pass. Then fmt and `cargo clippy -q -p sim -p rules -p launcher --all-targets -- -D warnings`.

- [ ] **Step 9: Commit** as "Aim and feat attack buttons in the sim".

### Task 2: Aiming in the sim

**Files:**
- Modify: `crates/sim/src/world/firing.rs:93-121,302-340,470-490`, `crates/sim/src/world.rs:1477-1499` (`command_for`), `crates/sim/src/world/movement.rs:82-132` (`restrict_seats`), `crates/sim/src/combat/bloom.rs:92-111`, `crates/sim/src/combat/idle.rs:142-152`
- Modify: `crates/sim/src/world/aim_probes.rs`, `crates/sim/src/combat/bloom_probes.rs:66-110` (`by_stance`: 0.25 becomes 0.5)

**Interfaces:** Consumes Task 1's. Produces no new signature. Behaviour:

- In `firing::fire`, each body's raise level steps once a step. It is held when the body is alive, a seat acts through it (`!creature`, `firing.rs:337`), `last_command.holds & holds::AIM != 0`, a blaster is in hand and the gun is not put away. On every early `continue` of that loop the level drops to 0 (`Shooting::interrupt` where it is already called; `aim_step(false, …)` or a clear on the others). A wrist launcher is no `Delivery::Blaster`, so it never aims.
- `bloom::stance` keeps reading `shooting.aiming`, which is now the aimed state. A shot before the top uses the normal spread.
- `command_for`: when `fighter.shooting.slowed()`, the command becomes a walking command and `up` is capped at 0. Use the same conversion as `world/minds.rs`' `walking(&mut command)`; move that helper to where both can call it.
- `restrict_seats` also clears `holds::AIM` and `holds::FEAT`.
- `idle.rs:151` keeps counting an aiming fighter as busy, by `slowed()`.

- [ ] **Step 1: Write the failing probes** (world probes on the proving ground, harness as `world/bloom_shot_probes.rs` `Range`):

```text
aim_spread      a pistol, still, AIM held 300 ms: bloom::shot_cone == base × 0.5 (within 0.001).
                AIM held 100 ms: shot_cone == base (the normal spread).
aim_walk_spread AIM held and walking forward: shot_cone == (base + PACE_SPREAD[0]) × 0.5.
aim_walks       AIM held with forward = 127 for 1 s covers the same distance (within 3%) as
                WALKING held with forward = 64 for 1 s, and less than half of a plain run.
aim_no_jump     AIM held with up = 127 for 500 ms: the body never leaves the ground.
aim_no_roll     AIM held, forward = 127, crouch pressed: the legs never play a roll clip.
aim_force_speed with Force Speed running, the aimed walk is faster than the plain aimed walk
                by the same factor as a held-[Walk] walk is.
aim_blasters    each of Blaster::{Pistol, Rifle, Repeater, Bowcaster, Disruptor, Ion, Sonic}
                reaches aiming after 200 ms (parameterised, one helper).
aim_not_blades  a lightsaber, a vibroblade and bare hands with AIM held 300 ms: aim == 0.
aim_players_only a creature whose mind's command carries holds::AIM: aim == 0, speed unchanged.
aim_drops       aiming, then each of: weapon switched, holstered, stunned, knocked down,
                a medpac used: aim == 0 on the next step and the walk limit is gone.
aim_predicts    the same inputs stepped with StepReason::Predict for the avatar give the same
                Shooting::aim as the authority's step, tick by tick, over 400 ms.
```

- [ ] **Step 2: Raise the test's count** (`probed("aim", …, 18)`), run it, and see the new probes fail.
- [ ] **Step 3: Implement** the behaviour above.
- [ ] **Step 4: Run** `cargo test --release -p approved_tests --test approved aim "dual pistols" rulebook`, and `cargo run --release -p launcher -- combat specials` if that verb still builds (Task 3 rewrites its Sniper section). Expected: `aim` passes.
- [ ] **Step 5: Commit** as "Hold to aim in the sim".

### Task 3: Feat attack button, the new Sniper Shot, and minds

**Files:**
- Modify: `crates/sim/src/world/firing.rs:105-112,351-368,470-500`
- Modify: `crates/sim/src/combat/specials.rs:1-47,458-556` (`pull` and the module documentation), `crates/sim/src/combat/pair.rs:9-15,81-187`
- Modify: `crates/sim/src/combat/ai.rs:365-379`, `crates/sim/src/world/minds.rs:1252-1254,2191-2193`
- Modify: `crates/sim/src/world/pair_probes.rs:476-535` (`sniper`), `crates/sim/src/world/bloom_shot_probes.rs:267-285`, `crates/sim/src/world/aim_probes.rs`
- Modify: `crates/launcher/src/specials_verbs.rs:250-400`, `crates/launcher/src/duel_verbs.rs:998-1090` (`gun_worth`), `crates/launcher/src/sim_verbs.rs:370-376`, `crates/launcher/src/force_verbs.rs:364`

**Interfaces:** Consumes Task 1's. Produces:

```rust
// specials.rs: `Trigger::alt` now means "the feat attack is held".
// pull(): Sniper Shot
//   press (alt, ready, pool >= cost, not charging): charging_from = Some(now); steadying = true; nothing paid.
//   held: returns None, however long.
//   release: need = STEADY_MS[usize::from(shooting.aiming)];
//            held >= need && afford(pool, cost) -> Some(Shot { special: Some(special), cadence_ms: pace_ms }),
//            timer += refire, expose(); otherwise None. charging_from = None; steadying = false either way.
// `steadying` is false whenever the special is not Sniper Shot or nothing is charging.
```

- `firing.rs`: `let feat = alive && last_command.holds & holds::FEAT != 0;` then `Trigger { attack: pressed(ATTACK) && !feat, alt: feat, … }`. Line 110's edge keeps `alt_held` from `holds::FEAT`. A shot whose `special` is `SniperShot` uses a cone of 0 and a kick of `[0.0; 2]` (the bloom still grows after it through `bloom::on_shot`).
- `pair::pull_special`: Sniper Shot is on while `charging_from.is_some() || (trigger.alt && affordable)`. Its released shot leaves from both guns on the same step for one cost. The old "plain shots from both once the bar runs dry" arm goes.
- Without a special, or without the stamina for one, [Feat attack] fires as [Attack] does. This existing rule stays.
- `ai::choose_special`: Sniper Shot is held for `STEADY_MS[0] + CHARGE_PAST_MS`. The two places in `minds.rs` that put a mind's alternate button into the command call `Command::other_mode`.

- [ ] **Step 1: Write the failing probes** (in `aim_probes.rs`; update `pair_probes::sniper`):

```text
feat_button     a pistol with Power Blast: holds::FEAT held 1.1 s then released fires one
                Power Blast; buttons::ALT_ATTACK held 1.1 s then released fires nothing.
feat_over_attack a pistol with Power Blast: ATTACK and FEAT held together 600 ms fire no plain shot.
steady_fires    Sniper Shot, FEAT held 520 ms, released: exactly one bolt, its Shot.special is
                SniperShot, stamina down by exactly one cost, Defense penalty set.
steady_early    FEAT held 300 ms, released: no bolt, stamina unchanged, weapon timer unchanged.
steady_aimed    aimed (AIM held 300 ms first), FEAT held 260 ms, released: one bolt.
                Not aimed, FEAT held 260 ms: no bolt.
steady_straight 20 seeds, the shooter walking and after 6 plain shots (bloom up): the Sniper
                Shot's bolt direction equals the crosshair line within 0.01°.
steady_walks    while FEAT is held with Sniper Shot the body walks (as aim_walks) and cannot jump.
steady_pair     a pair: one release fires a bolt from each gun on the same step for one cost
                (this replaces pair_probes::sniper's expectations).
steady_broke    stamina below the cost: FEAT held fires plain shots as ATTACK does.
steady_drops    steadying, then stunned or weapon switched: charging_from is None, steadying
                false, no bolt, stamina unchanged.
mind_snipes     a creature with a rifle and Sniper Shot, a target in sight: within 5 s it fires
                a bolt whose special is SniperShot, and it never has aim > 0.
bar_feat_slot   a player's Action::Feat for the Power Blast line with a pistol fires a Power Blast.
```

- [ ] **Step 2: Run** `cargo test --release -p approved_tests --test approved aim "dual pistols"` and see them fail.
- [ ] **Step 3: Implement.** Rewrite the module documentation of `specials.rs` and `pair.rs` for the new behaviour.
- [ ] **Step 4: Move the launcher's verbs to the new buttons.** In `specials_verbs.rs` every feat press becomes `holds::FEAT`, and the Sniper Shot section checks the new shot (steady, release, one shot, walk speed) instead of the old aim. In `duel_verbs.rs` `gun_worth`, the feat lines press `holds::FEAT` through `Keys::holding`: Sniper Shot holds for 520 ms and lets go for one step, again and again. Its closing check compares a pair with one pistol as before. `sim_verbs.rs:375` names `steadying` "Sniper Shot steady" and `aiming` "Aim".
- [ ] **Step 5: Run** the approved tests `aim`, `"dual pistols"`, `"feat picks"`, `"action bar"`, `specials`, `rulebook`, and `cargo run --release -p launcher -- combat specials` and `combat duel --only gun_worth` (headless verbs; find the exact spelling with `--help`). Expected: all pass. Raise `probed("aim", …)` to the new count.
- [ ] **Step 6: Commit** as "Feat attack button and steadied Sniper Shot".

### Task 4: Bindings

**Files:**
- Modify: `crates/game/src/settings.rs:29-114` (`Action`), `:138-496` (`ACTIONS`, now 53), `:1051-1115` (layout), `:1676-1683` (`SHARES`), `:1796-1824` (`relayout`)
- Modify: `crates/game/src/pad.rs:123-153` (`PAD`, now 28), `:306-312` (`on_lb`), `:483` (the held-action list)
- Modify: `crates/game/src/sim_host.rs:1137-1201` (`player_command`), `crates/game/src/hud.rs:712`
- Modify: `crates/game/src/settings_probes.rs`, `crates/game/src/pad/probes.rs`
- Modify: `data/rules/controls.toml:269-286` (`[guns]`) and the `[Saber throw and kata]` paragraph at `:246-251`

**Interfaces:** Consumes `sim::holds::{AIM, FEAT}`. Produces:

```rust
// settings.rs
Action::Aim         // "aim", label "Aim", default Input::Mouse(MouseButton::Right), pad alone(LeftTrigger2)
Action::FeatAttack  // "feat_attack", label "Feat attack", default Input::Key(KeyCode::KeyR), pad shifted(RightTrigger2)
SHARES += [Aim, AltAttack], [Aim, Block], [FeatAttack, DuelAltAttack]
KEY_LAYOUT = ("keys.layout", "6")
// sim_host.rs player_command: holds gets AIM while Action::Aim is held and FEAT while Action::FeatAttack is held.
```

- `PadMap::on_lb` does not count `Action::FeatAttack`'s chord. So LB with the right trigger still sends [Attack] under a block, LB stays "not chorded", and [Feat attack] is held as well. The sim picks by the weapon in hand.
- `relayout`, for a file saved with a layout below 6: `key.aim` and `pad.aim` take the file's `key.alt_attack` and `pad.alt_attack` when the file names them and names no aim. Then `place_defaults` runs for `aim` and `feat_attack` on both devices.
- `hud.rs:712` shows [Feat attack]'s label for a blaster's feat prompt.
- `controls.toml` `[guns]`: [Aim] raises the gun (what it does, with the rule references `{weapons.aim.in_ms}` and `{weapons.bloom.stance[1]:bare}`), [Feat attack] makes the feat attack, Sniper Shot is held to steady and released to fire. Use the rulebook's `[Action]` names exactly as the labels above. Then `cargo xtask rulebook`.

- [ ] **Step 1: Write the failing probes** (`settings_probes.rs`, `pad/probes.rs`; follow the probes at `settings_probes.rs:530-570,700-760`):

```text
aim_defaults    defaults: Aim on MouseRight and LeftTrigger2, FeatAttack on KeyR and LB+RightTrigger2;
                no two actions share an input unless SHARES lists the pair.
aim_command     player_command with Aim held sets holds::AIM; with FeatAttack held sets holds::FEAT;
                with AltAttack, Block and Aim all on one held button and a blaster in hand the
                command carries ALT_ATTACK and AIM and no BLOCK.
layout_6        (parameterised) a layout-5 file with: nothing rebound; a reversed mouse
                (key.attack=MouseRight, key.alt_attack=MouseLeft); key.kick=KeyR; key.use=MouseRight.
                Each ends on layout 6 with key.alt_attack unchanged, aim on the alternate button
                where the file named one, and no unshared double binding (feat_attack "none"
                when R is taken).
feat_chord      a pad with LB and the right trigger held: Actions holds Attack, Block and FeatAttack.
                LB with South held: Bar1 held, FeatAttack not held.
```

- [ ] **Step 2: Run** the approved tests that run these probes (`git grep -n "settings_probes\|pad::probes" crates/approved_tests`) and see them fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** those tests and `rulebook`. `cargo clippy -q -p kjk_game -p launcher --all-targets -- -D warnings` (use the game crate's real package name), and the no-probes build `cargo clippy -q -p launcher --all-targets --no-default-features -- -D warnings`.
- [ ] **Step 5: Commit** as "Aim and Feat attack bindings".

### Task 5: The aim pose

**Files:**
- Create: `crates/game/src/aim_pose.rs`
- Modify: `crates/game/src/cast.rs:890-1045` (`Raised`, `Figure::raise`), `:1150-1162` (who is held up), `crates/game/src/lib.rs`
- Modify: `crates/game/src/aim_fx.rs:136,240-270`, `reticle_view.rs:47-60`, `shots.rs:100-106`, `combat_fx.rs:1196-1201`, `blaster_view.rs:606,773`
- Modify: `crates/game/src/held_probes.rs` (or a new `aim_pose` probe list beside it), `crates/approved_tests/tests/approved.rs`

**Interfaces:** Consumes `Shooting::{aim_share, aiming, steadying, charging_from}`. Produces:

```rust
// aim_pose.rs
pub(crate) enum Wield { Pistol, Pair, Rifle }       // guns that are not pistols use Rifle
pub(crate) fn wield_of(sheet facts the Figure already has) -> Wield;
/// The corrections added over the held last frame of the shot's clip so the sights meet the
/// eye line: bone name and Euler degrees, at full raise.
pub(crate) fn corrections(wield: Wield) -> &'static [(&'static str, [f32; 3])];
/// Smootherstep of the sim's raise share.
pub(crate) fn eased(share: f32) -> f32;
/// How long another character keeps its guns up after its last shot (ms).
pub(crate) const OTHERS_HOLD_MS: f64 = 600.0;
```

Behaviour:

- A body a seat acts through is held up while `charging_from.is_some() || aim > 0`. Its raise follows the sim's level: the blend toward the held frame is `eased(aim_share())` while aiming, so it takes 200 ms in and 150 ms out and cannot pop. A charge or a steadied Sniper Shot without aim keeps the existing timed raise.
- Any other body with a blaster is held up from each of its shots until `OTHERS_HOLD_MS` after its last one (`Figure::shot_ms`). The sim is not asked.
- The corrections are scaled by the raise and added after the clip, the way `stance.rs` adds breathing. Breathing continues underneath.
- The legs are not touched, so walking while aiming plays the walk.
- Sniper Shot's sight line (`aim_fx`), the disruptor's scope dot (`reticle_view`), and the other readers of the old `aiming` meaning follow `steadying`.

- [ ] **Step 1: Read** `Figure::raise`, `stance.rs`, `Figure`'s `wield`, `off_hand` and `shot_ms`, and `rig_timeline.rs` (how a jolt is measured).
- [ ] **Step 2: Write the failing probes:**

```text
aim_pose_follows   a drawn pistol, sim aim_share stepped 0 → 1 over 200 ms: the right hand's height
                   rises monotonically, ends within 2 cm of the pose at share 1, and no 8 ms step
                   moves the hand more than 1/10 of the whole travel.
aim_pose_wields    (parameterised: Pistol, Pair, Rifle) at full raise the muzzle's height is within
                   6 cm of the eye's height, and for Pair both muzzles are.
aim_pose_walks     aiming and walking: the legs play the walk clip and the feet do not slide more
                   than the walk clip's own slide.
others_raise       a creature that fired at t: held up at t + 500 ms, down by t + 600 ms + the blend.
steady_sight       steadying shows the sight line; aiming alone does not.
```

- [ ] **Step 3: Implement,** then tune `corrections` by looking. Film each wield (Task 8's recipe, 2 s clips), read the frames, and adjust until the sights sit at the eye line with no twisted elbow or wrist. Keep the frames you judged by in `kjk-artifacts/gunplay-aim/`.
- [ ] **Step 4: Run** the approved test that holds these probes, plus `cargo clippy` for the game crate.
- [ ] **Step 5: Commit** as "Aim pose for pistols, pairs and rifles".

### Task 6: The camera

**Files:**
- Modify: `crates/game/src/play_view.rs:1007-1045` (`third_person`), `:1830` (the field of view), `crates/game/src/camera_tune.rs:233-275`, `crates/game/src/first_person.rs`
- Modify: the camera's probes (`git grep -n "camera" crates/game/src/*probes*.rs crates/launcher/src/probes*`)

**Interfaces:** Consumes `Shooting::aim_share` of the watched body and `aim_pose::eased`. Produces:

```rust
// camera_tune.rs
/// The framing while the watched character aims: `share` of the way to the aimed framing.
impl Framing { pub fn aimed(self, share: f32) -> Framing; }
pub(crate) const AIM_FOV: f32 = 0.75;      // of the framing's field of view
pub(crate) const AIM_RANGE: f32 = 0.6;     // of the framing's range
pub(crate) const AIM_SHOULDER: f32 = 1.25; // of the framing's shoulder offset
```

- Third person: after the gun framing is chosen, `framing = framing.aimed(eased(aim_share))`. The camera's damping is unchanged.
- First person: only the field of view narrows, by the same `AIM_FOV` and the same share.
- With no gun in hand the share is 0, so nothing changes.

- [ ] **Step 1: Write the failing probes:**

```text
aim_framing      Framing { range: 80, height: 16, shoulder: 20, fov: 80 }.aimed(1.0) has fov 60,
                 range 48, shoulder 25, height 16; .aimed(0.0) is unchanged; .aimed(0.5) is halfway.
aim_camera_eases the watched body's aim_share stepped 0 → 1 over 200 ms: the camera's field of view
                 falls monotonically to 75% and no frame changes it by more than 15% of the travel.
aim_first_person in first person the field of view is 75% at full aim and the eye does not move.
```

- [ ] **Step 2: Run, implement, run.** Then clippy for the game crate.
- [ ] **Step 3: Commit** as "Aim camera".

### Task 7: Sniper Shot's balance

**Files:**
- Modify: `crates/launcher/src/duel_verbs.rs` (a new section `sniper_balance` beside `gun_worth`, in the `--only` list)
- Modify (only if tuning is needed): `data/rules/feats.toml` (`steady_ms`, `threat_times`, `attack_bonus`, `cost_percent` is shared by all feats so prefer the others), then `cargo xtask rulebook`
- Create: `docs/superpowers/specs/2026-11-17-gunplay-sniper-balance.md` (the measured table and what was tuned)

**Interfaces:** Consumes `Keys::holding(holds::…)`, `Bout`, `gun_worth`'s helpers (`pistol()`, `rifle()`, `dummy`).

The section measures, averaged over 8 seeds and 60 s each:

- Guns: one pistol, a pair of pistols, a rifle.
- Ways of firing: plain from the hip ([Attack] held); plain aimed ([Aim] and [Attack] held); Power Blast at tiers 1, 2, 3 (full charge, released, again); Sniper Shot at tiers 1, 2, 3 (steadied 500 ms from the hip, and 250 ms while aimed).
- Targets: low, middle and high Defense, set as `gun_worth` sets its 75% (a base attack bonus found so the plain shot's chance to hit is 95%, 75% and 40%).
- Ranges: 6 m and 30 m (the `Bout`'s distance; read how 160.0 maps to metres).
- Columns: damage a second, stamina spent a second, damage per point of stamina spent (blank for plain fire), and the gain over plain aimed fire.

Checks the section reports (`report.check`), from the spec:

1. Against high Defense, and at 30 m, Sniper Shot's damage a second is above plain fire from the hip, for every gun and tier.
2. Against low Defense, Sniper Shot's damage a second is not above Power Blast's at the same tier, for every gun.
3. One Sniper Shot deals more damage than plain aimed fire deals in the time that Sniper Shot takes (its steady time plus the gun's refire), against high Defense and at 30 m. This is how this plan reads the spec's "damage per point of the second bar stays above a plain aimed burst of the same length", because plain fire spends no stamina. Sniper Shot's damage per point of stamina is reported beside Power Blast's.

- [ ] **Step 1: Write the section** with the table and the three checks, parameterised over guns, ways, targets and ranges (one loop, no copied blocks).
- [ ] **Step 2: Run it** (`kjk combat duel --only sniper_balance`, headless, in the background if slow). Save the output.
- [ ] **Step 3: If a check fails, tune** `steady_ms`, then `threat_times`, then `attack_bonus`, one at a time, and rerun. Record each change and its reason. Do not change Power Blast or the plain guns. Stop and report if no value in the rules' ranges meets all three.
- [ ] **Step 4: Write the balance document** with the final table (markdown), the checks' results, and every rule changed from the spec's value.
- [ ] **Step 5: Run** `rulebook` and `aim` (the probes use the rule, not a literal, so a tuned steady time must not break them). Commit as "Sniper Shot balance measurement".

### Task 8: Texts, the last checks and the clips

**Files:**
- Modify: `docs/COMBAT.md:760-782,1550-1556`, `docs/controls/` if it names [Alt attack] for guns, `KNOWN-ISSUES.md` if something does not work
- Create: clips under `kjk-artifacts/gunplay-aim/` (not committed if the folder is ignored; check `.gitignore`)

- [ ] **Step 1: Bring the documents up to date** with the new behaviour, in full plain sentences.
- [ ] **Step 2: Run** `cargo fmt --all -- --check`, `cargo clippy -q --workspace --all-targets --all-features -- -D warnings`, `cargo clippy -q -p launcher --all-targets --no-default-features -- -D warnings`, and `cargo test --release -p approved_tests -p launcher --test approved` once (background). Fix what fails.
- [ ] **Step 3: Film,** headless and muted, with scratch settings and saves. The recipe that works (from the bolt-burn test):

```bash
KJK_SETTINGS=<scratch>/settings KJK_SAVES=<scratch>/saves kjk play --stage arena --class soldier@77+sniper --headless --mute \
  --moves "1500ms hand 0; 1s; 2s alt; 3s alt w; 1s; 600ms feat; 1s" \
  --cmds "wait 1s; arena god; arena spawn g_sithtroop01; wait 1500ms; camera 0.45 -10.5 1.3 253 2 36; film 10s aim-pistol; quit"
```

Film five clips: a pistol raised to aim, a pair of pistols raised, a rifle raised, walking while aiming (third-person follow camera, so the camera move shows), and a Sniper Shot (steady, sight line, release). Find how the arena gives a pair and a rifle (`arena.rs:273-285`, the F keys or `arena` commands). Look at frames of every clip before calling it done.
- [ ] **Step 4: Commit** as "Gunplay part 1 documents".
