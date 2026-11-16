# Destruction, first playable pass — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Props break and take damage from weapons, grenades and Force powers, playable in the arena's prop hall and in the games' own modules, without flying chunks yet.

**Architecture:** A new `sim::destruction` module owns the catalogue (`data/destruction/props.csv`), the rules (`data/rules/destruction.toml`) and the pure maths (worth of a blow, damage stage). Each placeable's facts carry `destructible: Option<Destructible>`, set at load after the guards; the existing object pipeline (`world/objects.rs`: `Battering` → `objects`) resolves blows on them, and the existing look `DEAD` is "broken". The view plays the model's own clips where it has them, else hides the model, with KOTOR's own effects and sounds.

**Tech Stack:** Rust, bevy_ecs sim (`crates/sim`), session loader (`crates/session`), Bevy view (`crates/game`), probes run by `kjk sim probes --only <name>`.

**Spec:** `docs/superpowers/specs/2026-11-06-destruction-design.md` (sections "Which props, and how", "How much damage it takes", "What breaking looks like", "Rules for collision, nav and story", "Co-op and saves").

## Global Constraints

- Worktree `.claude/worktrees/prop-hall`, branch `claude/prop-hall` (draft PR kjk-engine/kjk#186). Never touch the main checkout.
- Every numeric constant in `crates/sim` is a rulebook rule (`data/rules/*.toml`) or listed in `data/rules/not-rules.txt`; run `cargo xtask rulebook` after editing rules.
- No snapshot, save or pack format change: `ObjectState` (`state`, `damage`) already carries everything. If one turns out necessary, stop and ask.
- KOTOR's own effects, models and sounds only; no new art (owner's standing rule).
- The existing bash rule (locked doors and containers, `BASH_TIMES`) behaves exactly as today.
- Breaking a prop runs no script, gives no XP, makes nobody hostile; the AI never targets props.
- Every game run passes `--mute`. Build with `cargo build --release --bin kjk`.
- Before calling a task done: `cargo clippy -p sim -p session -p game --release --all-targets` clean, `cargo fmt`.
- Commit messages: a plain sentence of the effect (repo style), body explains; no attribution lines.

## Review Focus

1. A guarded prop (plot, container, useable, conversation, any script, tag or template named by scripts, Min1HP, locked) must never take destruction damage from any source, enemies' included. Test in Task 2.
2. A broken prop must stop blocking bodies and bolts, in a real module (collision entities) and in the arena (made props' footprints) alike. Test in Task 3.
3. A damage-only prop must never break, however much damage lands. Test in Task 3.
4. Prediction (`StepReason::Predict`) must not show damage on a client (the pipeline returns early today; keep it). Test in Task 3.
5. A broken prop must not be hurt again (no repeated break sounds or effects from later blows). Test in Task 3.

---

### Task 1: Catalogue, rules and the maths

**Files:**
- Create: `crates/sim/src/destruction.rs` (+ `pub mod destruction;` in `crates/sim/src/lib.rs`)
- Create: `data/rules/destruction.toml`
- Create: `crates/sim/src/destruction/probes.rs` (registered in `kjk sim probes` as the other probe modules are; see `crates/sim/src/world/arena_probes.rs` and where `arena_probes::probes` is listed)

**Interfaces:**
- Produces:
  - `pub enum Class { Break, Damage }`, `pub enum Material { Ceramic, Organic, Wood, Plastic, Metal, Electronic, Stone, Cloth, Debris }`, `pub enum Size { Small, Medium, Large }`, all `Copy + Eq + Debug`.
  - `pub struct Destructible { pub class: Class, pub material: Material, pub size: Size }` (`Copy + Eq + Debug`).
  - `pub fn catalogue(game: &str, appearance: u16) -> Option<Destructible>`: the CSV row for `game` (`k1`/`tsl`) and row; `None` for class `intact` or no row. The CSV is `include_str!`'d and parsed once (`OnceLock`).
  - `pub fn most_hp(d: Destructible) -> i32` (whole points: the size's, times `DAMAGE_TIMES` for class `Damage`) and `pub fn hardness(d: Destructible) -> i32`.
  - `pub fn worth(damage: i32, damage_type: Option<u8>, area: bool, d: Destructible) -> i32`: hundredths of HP a blow of `damage` hundredths takes: `max(0, damage × multiplier(material, type) × (AREA_TIMES if area) − hardness × 100)`; untyped damage uses multiplier 1.
  - `pub fn stage(d: Destructible, damage: u16) -> u8`: 0 intact, 1 at `STAGE_ONE` of `most_hp`, 2 at `STAGE_TWO`, 3 broken (class `Break` at full damage only).
- Rule values to start from (tune in Task 3 against the feel table):
  - Hardness: ceramic 0, organic 0, cloth 0, wood 1, plastic 1, metal 1, electronic 1, debris 2, stone 5.
  - Most HP by size: small 5, medium 20, large 75; `DAMAGE_TIMES` 4 (a damage-only console of medium size has 80).
  - Multipliers (default 1.0): energy (row 12) ×1.2 on metal, wood, plastic; ion (11) ×3.0 on electronic, ×0.2 on stone; electrical (7) ×2.0 on electronic, ×1.5 on organic and cloth; fire (8) and sonic (10) ×2.0 on ceramic, cloth, organic; bludgeoning (0) ×0.25 on metal and stone, ×1.5 on ceramic and wood; slashing (2) ×1.2 on wood and cloth.
  - `AREA_TIMES` 3; `STAGE_ONE` 1/3, `STAGE_TWO` 2/3.

- [ ] **Step 1: Write the failing probes** in `destruction/probes.rs`:
  - `destruction_catalogue`: K1 rows of class break or damage number 92, TSL 137; `catalogue("k1", 54)` (Invisible) is `None`; `catalogue("k1", 39)` is `Break, Metal, Medium`; `catalogue("tsl", 23)` is `Damage, Electronic, Medium`.
  - `destruction_worth`: `worth(350, Some(12), false, chair)` = 320 (3.5 × 1.2 − 1); `worth(350, Some(12), false, stone_pillar)` = 0; `worth(2000, None, true, chair)` = 5900; `stage` of the damage-only medium electronic (80 HP) at 0, 27, 54, 79 points lost is 0, 1, 2, 2; `most_hp` of k1/39 is 20.
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only destruction`; expect the two probes to fail (not compiled in or failing).
- [ ] **Step 3:** Implement `destruction.rs` and `destruction.toml` (chapter "Destruction", sections hardness, hit points, multipliers, area, stages; `source = "ours (destruction design 2026-11-06)"`).
- [ ] **Step 4:** Run the probes; expect `2 checks, 0 failed`. Run `cargo xtask rulebook`; expect `rulebook: whole`.
- [ ] **Step 5:** Commit: "Destruction has its catalogue and rules: what breaks, what it is made of, what a blow is worth".

### Task 2: Facts and guards

**Files:**
- Modify: `crates/sim/src/module.rs` (`ObjectData` gains `pub destructible: Option<destruction::Destructible>`; `unknown` sets `None`; the digest encoding writes it when `Some`)
- Modify: `crates/session/src/objects.rs` (placeables: `destructible` from `catalogue(game, appearance)`) and the module-load pass that has the script library's strings (`crates/session/src/library.rs:780-855`, `Library.strings`) to clear it for guarded placeables
- Modify: `crates/sim/src/world/arena.rs` (`Prop` gains `pub destructible: Option<Destructible>`; `Prop::facts` copies it and sets `hit_points: Some((most_hp, most_hp))`) and `crates/game/src/prop_hall.rs` (fills it from `sim::destruction::catalogue(game, appearance)`)
- Test: `crates/sim/src/destruction/probes.rs`

**Interfaces:**
- Consumes: Task 1's `catalogue`, `most_hp`, `Destructible`.
- Produces: `pub fn guarded(facts: &ObjectData, named: impl Fn(&str) -> bool) -> bool` in `sim::destruction`: true for `toughness.plot`, `container`, `useable`, non-empty `conversation`, any entry in `scripts`, `toughness.min_one_hp`, `lock` locked (`opens == Opens::Locked`), or `named(tag)` / `named(template)`. A destructible placeable's facts have `hit_points = Some((most_hp, most_hp))` (the file's default HP no longer counts for it).
- The instance's appearance: `asset_odyssey::module` already reads it (`instance.appearance`, line ~945); carry it to `object()` if it is not already on `Instance`.
- "Named" must cover the shared script layers a module runs, not only its own NCS/DLG (spec, guard 6): use the strings of every script the module's `Library` holds.

- [ ] **Step 1: Write the failing probe** `destruction_guards`: a bare placeable of catalogue row k1/39 is destructible; each of the eight guards alone (plot, container, useable, conversation `"x"`, a script `("OnHeartbeat","x")`, Min1HP, locked, tag named) makes `guarded` true.
- [ ] **Step 2:** Run `kjk sim probes --only destruction`; expect `destruction_guards` to fail.
- [ ] **Step 3:** Implement `guarded`, the field, the loader and the arena fill.
- [ ] **Step 4:** Run the probes (expect 3 pass). Run `kjk play --stage arena --mute --cmds 'wait 90f; quit'` and check the log has no new errors.
- [ ] **Step 5:** Commit: "Props the scripts don't need are destructible; plot, containers, used and scripted ones stay intact".

### Task 3: Blows break props

**Files:**
- Modify: `crates/sim/src/world/objects.rs` (`hurts`, `objects`, `bashable`, `bodies`), `crates/sim/src/world/bolts.rs:81-120` (any shooter's bolt meets destructible bodies; blasts catch them, scripted grenades' too), `crates/sim/src/world/sabers.rs:317` and `strikes.rs` (sweeps meet them), `crates/sim/src/world/level.rs:467-494` (`states`), the made-placeable collision in `world::move_actors`
- Create: `crates/sim/src/world/destruction_probes.rs`

**Interfaces:**
- Consumes: Tasks 1–2.
- Produces: `ObjectState::breakable(&self, facts: &ObjectData) -> bool` = `bashable(facts) || (facts.destructible.is_some() && !self.broken(facts))`, used by `bashable`/`bodies` in place of `bashable`; `ObjectState::passable(&self, facts) -> bool` = state `collision::GONE`, or `destructible` and look `DEAD`, used by `Level::states` and the made-placeable collision.
- `hurts`: a destructible, unguarded prop is hurt by `Weapon`, `Blast` and `Power` from anyone (no `party` or `ordered` needed); everything else as today.
- `objects`: for a destructible, assess against `stand_in(0, most)` and land `destruction::worth(assessed.damage, assessed.damage_type, area, d)` (no `BASH_TIMES`); `area` is `Source::Blast`, or `area: true` on `Weapon`/`Power`. Class `Damage` lands at most down to 1 HP (as `Min1HP`). A break sets look `DEAD` (`break_open` already does for a placeable without a container).

- [ ] **Step 1: Write the failing probes** in `destruction_probes.rs`, on the proving ground as an arena with props (`arena_probes::arena_with`, make it `pub(super)`): a metal medium chair (k1/20), an electronic damage-only panel (k1/23), a guarded copy of the chair (plot):
  - `destruction_feel`: hits to break, from the expected damage of KOTOR's weapons through `worth` (blaster pistol 1d6, heavy pistol 1d8, vibroblade 2d6+2, lightsaber 2d8+2, fists 1d3+2, frag grenade 20 area), within the design's table: pistol 6–10 on the chair, 1–2 on a small ceramic jar (k1/179); saber 2 on the chair; fists never on the chair; frag breaks small and medium in one blow.
  - `destruction_blows`: blows queued through `Battering::shot` with `Source::Weapon` of a pistol by a non-party creature break the chair after N hits; the plot chair takes nothing; the panel never passes 1 HP left after 500 hits and its `stage` reaches 2; the broken chair is `passable` and `Level::states` gives its entity `collision::GONE`; one more blow on it lands nothing (`Battered` empty); a `StepReason::Predict` step lands nothing.
- [ ] **Step 2:** Run `kjk sim probes --only destruction`; expect the two new probes to fail.
- [ ] **Step 3:** Implement. Tune `destruction.toml` until `destruction_feel` passes, without changing the table.
- [ ] **Step 4:** Run `kjk sim probes --only destruction`, `--only arena`, `--only bash` (the existing bashing probes) and `--only objects`; expect all pass.
- [ ] **Step 5:** Commit: "Blasters, blades, fists and grenades break props and wear down consoles, whoever fires".

### Task 4: Force powers reach props

**Files:**
- Modify: where the authority applies `script::Cast`s (find the consumer of `casts.0` in `crates/sim/src/world`), plus `data/rules/destruction.toml` (`[powers]`)
- Test: `crates/sim/src/world/destruction_probes.rs`

**Interfaces:**
- Consumes: Task 3's `Battering` path with `Source::Power { area, pushes, damage_type }`.
- Produces: `pub fn power_reach(spell: u16) -> Option<PowerReach>` in `sim::destruction`, `pub struct PowerReach { pub shape: Shape, pub damage: i32, pub damage_type: u8 }`, `pub enum Shape { Around { radius_m: f32 }, Line { range_m: f32 } }`, keyed in the rules by `spells.2da` row (K1 and TSL rows of Force Push, Whirlwind, Wave, Storm, Lightning, Throw Lightsaber, Kill, Destroy Droid; ranks share their chain's entry). `Around` catches every destructible within the radius of the caster with a clear line; `Line` catches those within `range_m` along the aim to the target, up to the first solid. Damages: Push/Whirlwind 15 bludgeoning, Wave 25 bludgeoning area, Lightning 10 electrical, Storm 20 electrical area, Throw 20 energy.

- [ ] **Step 1: Write the failing probe** `destruction_powers`: in the arena with three chairs 3, 5 and 9 m ahead, a Force Wave cast breaks the two within its radius and not the third; a Force Push aimed at the 3 m one breaks it alone; a Force Lightning cast takes the panel to stage 2 within three ticks.
- [ ] **Step 2:** Run; expect fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run `kjk sim probes --only destruction` and `kjk combat force` (the 44-power sweep); expect all pass, 44/44.
- [ ] **Step 5:** Commit: "Force powers break props: Wave clears a room, Push and Lightning hit what they're aimed at".

### Task 5: What breaking looks and sounds like

**Files:**
- Modify: `crates/game/src/own_fx.rs:210-235` (looks), `crates/game/src/made.rs` (made props), `crates/game/src/ambience.rs:1110-1150` (sounds), `crates/game/src/combat_fx.rs` (impact effects)
- Test: screenshots and a clip in the hall

**Interfaces:**
- Consumes: `sim::destruction::stage`, `ObjectData::destructible`, look `DEAD`.
- Behaviour:
  - A hit on a destructible plays the impact effect and sound the bolt-miss path already picks by surface family (`docs/COMBAT.md:1045`), by the prop's material.
  - Broken (look `DEAD`): the model's `die` then `dead` clips where it has them (today's path); a model with neither stops drawing, with a KOTOR explosion or spark-burst effect at its middle sized by `Size`, and the `destroyed` sound (`placeablesounds.2da`, or a material's fallback where its row has none).
  - Damage-only stages 1 and 2: a KOTOR spark (electronic, metal) or smoke (stone, wood, cloth) effect stays on the prop, stage 2's stronger; the model's `damage` clip plays once where it has one.
  - Only effects from `visualeffects.2da` and the games' models; rows chosen per material in one table in `combat_fx.rs`.

- [ ] **Step 1:** Run the hall before: `./target/release/kjk play --stage arena --mute --cmds 'wait world; camera 0 12 20 90 -38 75; screenshot before; quit'`.
- [ ] **Step 2:** Implement.
- [ ] **Step 3:** In the hall, break every kind: `arena` console orders or `--cmds` that throw frag grenades along each row (`kjk grenades` verbs show how a scripted throw is issued), then `clip destruction 12` and screenshots from the same cameras; check every break class shows its effect, nothing stays drawn that broke without a `dead` clip, consoles spark. Run the TSL hall (`--stage arena:203tel`) the same way.
- [ ] **Step 4:** Send the owner the clip and before/after shots (absolute paths).
- [ ] **Step 5:** Commit: "Broken props burst in KOTOR's own effects and sounds; damaged consoles spark".

### Task 6: Story safety and docs

**Files:**
- Modify: `docs/COMBAT.md` (a "Destruction" section after bashing: what breaks, the guards, the table, where the rules live), `docs/superpowers/specs/2026-11-06-destruction-design.md` (status: first pass built)

- [ ] **Step 1:** Run `kjk chain k1 --mighty --skill security=2` in the background; expect 29/29. Investigate any failure as a guard miss (spec: fix with a new automatic guard rule, not a per-module exception).
- [ ] **Step 2:** Write the docs.
- [ ] **Step 3:** `cargo xtask rulebook`, clippy, fmt; push; update PR 186's description (title stays the effect: "Props break: weapons, grenades and Force powers destroy what scripts don't need").
- [ ] **Step 4:** Commit: "COMBAT.md tells what destruction breaks and why".
