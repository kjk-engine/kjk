# Drawing a weapon in Stealth Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A character in Stealth Mode can take its weapon in hand without leaving Stealth Mode, and its first attack from there is a sneak attack.

**Architecture:** The holster key becomes a key that draws the weapon when it is put away and puts it away when it is out. A weapon the player drew stays out until the player puts it away. A lightsaber drawn in Stealth Mode stays unlit, and the blade lights on the swing that ends Stealth Mode. A creature keeps not noticing the attacker during the Sneak Attack Window, so the first blow lands as a sneak attack.

**Tech Stack:** Rust. The rules live in `crates/sim`. The checks are the sim's probes, reported by `kjk combat stealth` (`crates/launcher/src/stealth_verbs.rs`). Rule numbers live in `data/rules/stealth.toml`.

**Spec:** There is no separate design document. The owner asked for the plan directly on 2026-11-12. The "Design" section below is the spec. The request comes from a Discord thread of 2026-11-12. In that thread a player reported the fault, and another player asked for the draw to share the holster key.

## Why sneak attacks fail today

These findings come from reading `origin/main`. Task 1 confirms them with a probe before anything changes.

1. A player has no way to draw a weapon except by attacking. The holster key only puts a weapon away (`Action::Holster`, `world/movement.rs:441`).
2. The game puts a weapon away 8 seconds after the last blow (`Fighter::weapon_away`, `combat/fighter.rs:628`). Stealth Mode cannot be entered in combat. So a character in Stealth Mode almost always has its weapon put away.
3. An attack press ends Stealth Mode in the same step (`world/covert.rs:99`). It opens a Sneak Attack Window of 1.5 seconds (`stealth.mode.opener_ms`).
4. The blow must land inside that window, and the target must still not have perceived the attacker when it lands (`world/harm.rs:611`). An active creature checks what it perceives every 0.3 to 0.7 seconds. A character that is no longer in Stealth Mode is seen at once by anyone in sight of it. A blaster first plays its raise, and a lightsaber first lights and then swings. The creature therefore almost always perceives the attacker before the blow lands.
5. The existing check passes only because its attacker is bare-handed, one metre away, and its guard is kept busy (`world/covert/probes.rs:551-650`).

## Design

1. **Draw or holster key.** The holster key draws the weapon in hand when it is put away. It puts the weapon away when it is out, as it does today. The key works in and out of Stealth Mode. Drawing does not end Stealth Mode.
2. **A drawn weapon stays out.** A weapon the player drew is not put away by the 8 second rule. It goes away when the player presses the key again, when a conversation begins, or when the player restrict mode holds (the envirosuit and the space suit).
3. **Unlit lightsaber in Stealth Mode.** A lightsaber drawn in Stealth Mode is held in the hand with its blade off. It makes no ignition sound and gives no light. A character that enters Stealth Mode with a lit lightsaber turns the blade off and keeps the hilt in hand. Out of Stealth Mode, a drawn lightsaber lights as it does in a fight.
4. **The swing lights the blade.** An attack press in Stealth Mode ends Stealth Mode, lights the blade and starts the swing in the same step. No ignition flourish plays first. A blaster that is already drawn fires without its raise.
5. **The first blow is a sneak attack.** During the Sneak Attack Window, creatures perceive the attacker by the Stealth Mode rules (spot and listen checks), as if it were still in Stealth Mode. The window closes when the first blow lands or when its time runs out. A creature that had already spotted the character in Stealth Mode gets no sneak attack against it, as today.
6. **Window length.** Task 1 measures the time from the attack press to the first hit for a drawn unlit lightsaber, a drawn vibroblade, a drawn blaster and bare hands. The window is set to the slowest of these plus 200 ms, and never below today's 1500 ms.
7. **Not changed.** Powers and grenades still end Stealth Mode and still give no sneak attack unless a weapon hit follows inside the window. Companions under AI control never draw this way. KOTOR's sneak attack rule itself (dice, 10 m reach, from behind, Uncanny Dodge) is untouched.

## Global Constraints

- Work in a fresh worktree of fetched `origin/main` on branch `feat/stealth-draw`. The main checkout is stale.
- Every `kjk` run passes `--headless` and `--mute`, with `KJK_SETTINGS` pointing at a scratch copy of the settings file.
- Rule numbers go in `data/rules/*.toml`, never in code.
- Player-facing text follows `docs/player-text.md`: one short plain sentence, no fixed key names.
- Edits to approved test files are shown to the owner and wait for approval.
- The change alters sim behaviour. Follow the merge-time pin convention (`bumps/<branch>.toml`, `bumps/SIM_HASH`).
- Before finishing: `cargo fmt`, clippy, the approved tests, the rulebook check, and one fresh K1 chain. No TSL chain.

## Review Focus

1. A character draws in Stealth Mode and then changes weapons. The new weapon should be in hand, and a new lightsaber should be unlit.
2. A character with two lightsabers or a double-bladed one draws in Stealth Mode. Both blades should stay off, and both should light on the swing.
3. A character holding a drawn unlit lightsaber is attacked or spotted, and Stealth Mode ends by combat. The blade should light by the normal fight rule.
4. A first-seat player leads a companion in Stealth Mode and presses the key. The companion's weapon should be drawn, not the player's own body's.
5. A save made with a drawn weapon loads. A save from before this change still loads.

Each of these gets a probe row in the task that owns the code (Tasks 2 and 3).

---

### Task 1: Reproduce and measure

**Files:**
- Modify: `crates/sim/src/world/covert/probes.rs` (beside `opener_run`, line 585)
- Modify: `crates/launcher/src/stealth_verbs.rs` (report the new rows)

**Interfaces:**
- Produces: `fn armed_opener(weapon: Armed, drawn: bool) -> Result<OpenerRun, String>`, where `enum Armed { Fists, Vibroblade, Saber, Blaster }` and `struct OpenerRun { sneaks: usize, press_to_hit_ms: Option<u64>, perceived_at_hit: bool }`. The guard is an ordinary active guard facing away at 1 m (melee) or 8 m (blaster). It is not the busy guard.

- [ ] **Step 1:** Write `armed_opener` and a probe `armed_openers()` with one row per `Armed` value at `drawn = false`. Each row asserts `sneaks == 1`.
- [ ] **Step 2:** Run `kjk combat stealth`. Expected: the saber and blaster rows fail with `sneaks == 0` and `perceived_at_hit == true`. Record each row's `press_to_hit_ms` in this file under Task 4. If a row passes, stop and report, because finding 4 above is then wrong for that weapon.
- [ ] **Step 3:** Commit the failing probe, marked as expected to fail.

### Task 2: Draw or holster key

**Files:**
- Modify: `crates/sim/src/combat/fighter.rs:484-516` (`Sheath`), `:628` (`weapon_away`)
- Modify: `crates/sim/src/world/movement.rs:424-461` (the holster key, conversations, restrict mode)
- Modify: `crates/sim/src/motion/weapon.rs:518-571`, `:635-676` (`sheathes`, `draws`)
- Modify: `crates/sim/src/trace/movement.rs:608`, `:1272` (the sheath names)
- Modify: `crates/game/src/settings.rs:270-272` (the label), `crates/game/src/reticle_view.rs:90`
- Test: `crates/sim/src/world/strikes/probes.rs` (beside the holster rows at line 1098)

**Interfaces:**
- Produces: `Sheath::Drawn`, encoded as 4. "Its player drew it. It is out until its player puts it away."
- `Fighter::weapon_away` returns `false` for `Sheath::Drawn`.

- [ ] **Step 1:** Write probe rows. A holstered character presses the key and the weapon is out 20 seconds later. A second press puts it away. A drawn character whose conversation begins has its weapon put away. A restricted character cannot draw. The leader-companion case from Review Focus 4. A snapshot with `Sheath::Drawn` round-trips, and a byte 4 decodes (Review Focus 5).
- [ ] **Step 2:** Run the probes. Expected: the new rows fail.
- [ ] **Step 3:** Add `Sheath::Drawn`. In `movement.rs`, the key sets `Holstering` when the weapon is out and `Drawn` when `weapon_away` is true. A conversation's `at_rest` and the restrict mode treat `Drawn` as they treat `Fight`. In `weapon.rs`, a gun going to `Drawn` plays `TORSO_RAISEWEAP1` as the attack draw does, `draws` accepts `Drawn` without a target, and an attack press leaves `Drawn` in place.
- [ ] **Step 4:** Change the binding label from "Holster weapon" to "Draw or holster weapon".
- [ ] **Step 5:** Run the probes. Expected: pass. Commit.

### Task 3: Unlit lightsaber in Stealth Mode

**Files:**
- Modify: `crates/sim/src/motion/weapon.rs:635-676` (`sheathes`, `put_away`, `draws`)
- Modify: `crates/sim/src/world/covert.rs:98-123`
- Test: `crates/sim/src/world/covert/probes.rs`

**Interfaces:**
- Consumes: `Sheath::Drawn` from Task 2.
- Produces: `fn dark(&self, arms: &Arms) -> bool` in `weapon.rs`. It is true when the fighter is in Stealth Mode and no attack is pressed. `draws` returns false while `dark` is true.

- [ ] **Step 1:** Write probe rows. A stealthed character draws a lightsaber: `saber.on` is false, `covert.stealth` is true, and the `SABER_ON` event never fires. A character enters Stealth Mode with a lit blade: the blade is off in the next step, the sheath is `Drawn`, and no put-away flourish plays. An attack press: `saber.on` is true and a swing has begun in the same step. Review Focus 1, 2 and 3.
- [ ] **Step 2:** Run. Expected: fail.
- [ ] **Step 3:** Implement `dark`. Entering Stealth Mode with a lit blade sets `saber.on = false` and `sheath = Drawn` without `PUTAWAY`. An attack press under `dark` lights the blade without the ignition wait (`weapon_wait_ms` unchanged).
- [ ] **Step 4:** Run. Expected: pass. Commit.

### Task 4: Sneak Attack Window

**Correction after Task 1 (2026-11-12).** Finding 4 above was wrong. The game already keeps an attacker hidden while the Sneak Attack Window is open (`crates/sim/src/world/scripts.rs:481-488`). Task 1 showed that a first blow from Stealth Mode is a sneak attack when the attacker stands beside the guard and holds the attack key. The real fault is that the window starts at the attack press, and that press must also draw the weapon. A lightsaber or vibroblade takes 1232 ms from the press to the hit, which leaves 268 ms of the 1500 ms window. A player who is a few steps away, or whose first press only draws, runs out of time. Tasks 2 and 3 fix this by letting the player draw beforehand.

This correction overrides the text below where they differ.

- Do not add `Covert::hidden`, and do not change `perceiving.rs` or `scripts.rs`.
- `OpenerRun` gains `first_blow_sneak: bool`. The checks assert `first_blow_sneak` in place of `sneaks == 1`, because the probe's guard never turns and every later blow is also a sneak attack.
- `drawn = true` is produced with the draw key from Task 2, pressed in Stealth Mode before the attack.
- The checks are rows of `kjk combat stealth`, which has Jedi Academy's animations. They are not approved tests.
- Each `drawn = true` row asserts that the character is still in Stealth Mode just before the attack press, that `first_blow_sneak` is true, and that `press_to_hit_ms` is lower than the same weapon's `drawn = false` time.
- The `drawn = false` rows stay as measurements.
- Set `opener_ms` by Design point 6 from the measured `drawn = true` times. Keep 1500 ms if every drawn weapon lands within 1300 ms.
- The rule text for the window stays as it is. Add only the two sentences about drawing to the `[mode]` detail.

**Files:**
- Modify: `crates/sim/src/world/scripts/perceiving.rs:91` (where a body counts as a sneaker)
- Modify: `data/rules/stealth.toml:29-42` (`mode.opener_ms` value and summary, and the `[mode]` summary)
- Test: `crates/sim/src/world/covert/probes.rs` (Task 1's `armed_openers`)

**Interfaces:**
- Consumes: `armed_opener` from Task 1, the drawn states from Tasks 2 and 3.
- Produces: `Covert::hidden(&self, now: u64) -> bool` in `combat/fighter.rs`, true when `stealth` is set or `opener_until >= now`. Perception uses it in place of `covert.stealth`.

Measured press-to-hit times from Task 1: fists 200 ms, vibroblade 1232 ms, lightsaber 1232 ms, blaster pistol 384 ms (at 6 m). Each weapon started put away, so these times include the draw. In every run the first blow was a sneak attack, and the guard had not perceived the player when it landed. Measured in Task 4 with the weapon drawn in Stealth Mode first: vibroblade 440 ms, lightsaber 440 ms, blaster pistol 136 ms (at 6 m). Bare hands have nothing to draw. In every drawn run the character was still in Stealth Mode before the press, the first blow was a sneak attack, and the guard had not perceived the player when it landed. The slowest drawn weapon lands within 1300 ms, so `opener_ms` stays at 1500 ms.

- [ ] **Step 1:** Extend `armed_openers` with `drawn = true` rows for each weapon, each asserting `sneaks == 1` and `perceived_at_hit == false`. Add a row where the guard had spotted the character before the attack, asserting `sneaks == 0`. Add a row where the blow misses, asserting the guard perceives the attacker once the window ends.
- [ ] **Step 2:** Run. Expected: the new rows fail.
- [ ] **Step 3:** Add `Covert::hidden` and use it in `perceiving.rs`. Set `opener_ms` by Design point 6. Rewrite the rule text: "A character that attacks from Stealth Mode stays hidden for {stealth.mode.opener_ms:s} or until its first blow lands. That blow is a sneak attack on a target that has not yet perceived the character." Add to the `[mode]` detail: "Drawing a weapon does not end Stealth Mode. A lightsaber drawn in Stealth Mode stays unlit until it attacks."
- [ ] **Step 4:** Run. Expected: every `armed_openers` row passes, including Task 1's rows now run with `drawn = true`. Remove the expected-failure mark. Run the rulebook check. Commit.

### Task 5: Look, checks and evidence

**Files:**
- Modify only if needed: `crates/game/src/weapons.rs:1339`, `crates/game/src/blade_fx.rs:307`, `crates/game/src/holster_view.rs`

- [ ] **Step 1:** Film three headless clips in the arena and send them to the owner by absolute path. A scoundrel draws a blaster in Stealth Mode and shoots a guard in the back. A Jedi draws an unlit lightsaber in Stealth Mode, walks up and swings. A Jedi with two lightsabers does the same. "Sneak Attack!" must show in each.
- [ ] **Step 2:** Check in the clips that the hilt sits in the hand with no blade, no glow and no hum, and that the stealth shimmer covers the hilt. Fix the view only where a clip shows a fault.
- [ ] **Step 3:** Run `cargo fmt`, clippy, the approved tests, the rulebook check and one fresh K1 chain. Show the owner any approved test file that changed.
- [ ] **Step 4:** Open the pull request "Drawing a weapon in Stealth Mode" from `feat/stealth-draw`, with the three clips and the pin file.

## Open point for the owner

The plan keeps the hilt in the ordinary standing pose, which is how an unlit lightsaber is held today. A dedicated low "ready" pose would need a new clip choice from Jedi Academy's set and is left out.
