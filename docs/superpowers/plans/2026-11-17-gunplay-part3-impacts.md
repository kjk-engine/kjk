# Gunplay Part 3 (Impacts on Enemies) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A bolt that hits an enemy leaves a burn where the bolt met the body, lights the spot in the bolt's colour, raises a puff of KOTOR's own smoke, and makes the enemy flinch away from the shot. A killing shot throws the ragdoll along the bolt's path.

**Architecture:** Everything is view only. `combat_fx` already sees every bolt it draws and every hit event. It keeps each drawn bolt's line for a short time, and for each new bolt hit it writes one record (`bolt_hits::BoltHit`) into a resource that the other views read: `body_marks` lays the burn and asks for the smoke, `cast` adds the flinch, and `ragdolls` takes the push. Which bolts burn is one pure function with named constants. The sim's hit event and every sim rule stay as they are.

**Tech Stack:** Rust, Bevy. Crates `game` (`kjk_game`), `launcher` (`kjk`), `approved_tests`.

**Spec:** `docs/superpowers/specs/2026-11-17-gunplay-design.md` ("Part 3: impacts on enemies", "Settled after the builder's review", "What would make this a failure", "Not goals", "The test that exists"). Read it before any task. The owner's ruling on fast-firing blasters (2026-11-17) is copied into the Global Constraints below.

## Global Constraints

- Branch `feat/gunplay-impacts`. Never push. Never open a pull request. Never merge.
- Never edit `bumps/SIM_HASH` or a format's `VERSION`. Part 3 is view only and changes no sim state, so it adds no bump file. If a task finds it must touch the sim, stop and report.
- Every number a game rule plays by lives in `data/rules/*.toml` and the sim reads it as `rules::…`. No literal in the sim. After a rule changes, run `cargo xtask rulebook`.
- Permanent tests live only in `crates/approved_tests`. A layer's checks are probes (`pub fn probes() -> Checks`) that an approved test runs with `probed("name", &…::probes(), at_least)`.
- Every `kjk` run is `--headless --mute`, with `KJK_SETTINGS` and `KJK_SAVES` pointing at scratch copies. Check the real settings file's hash before and after a run.
- Do not run the whole gate or a chain for each step. Per task: `cargo fmt --all -- --check`, `cargo clippy -q -p <touched crates> --all-targets -- -D warnings`, and the approved tests the task names. CI runs the rest on the pull request.
- Run a command that may exceed 2 minutes in the background and wait for its notice. No sleep loops.
- Every text a person reads (comments, rule summaries, commit messages, labels) follows `CLAUDE.md`: full sentences, one fact each, no colons in titles, the actual number.
- No decompiler names, no memory addresses, no game files committed. No hand-drawn effects. Every texture, model and sound comes from KOTOR 1, KOTOR 2 or Jedi Academy.
- Commit messages are a short noun phrase, with no attribution lines.
- Spec values, verbatim: a white-to-red flash for 0.6 s; Jedi Academy's ember texture glowing for 3.5 s; Jedi Academy's body burn texture for 8 s with a 2 s fade; smoke for about 1 s; at most 6 bolt burns on one body.
- The owner's ruling, verbatim in meaning: a bolt leaves a burn when it is a critical hit, a Power Blast, a Sniper Shot, the killing shot, or deals at least 15% of the target's maximum vitality. Any other bolt leaves a burn at most once every 500 ms per body. A seventh bolt burn replaces the oldest bolt burn. Bolt burns never push out lightsaber burns. A hit on the head leaves no burn, and no burn may reach the head's mesh. Burn size is small for a pistol, larger for a rifle, and largest for a Power Blast. The light flash, the smoke and the flinch happen on every bolt hit, whether or not it leaves a burn.

## Decisions this plan makes

- **Base.** The branch starts from the tip of `feat/gunplay-aim`, because part 3 reads part 1's `Shooting::steadying` to know a Sniper Shot. If part 1 is merged by then, it starts from `main`.
- **One record per bolt hit.** `combat_fx::hits` already handles each new event once. For a blaster hit it now also pushes a `BoltHit` into the resource `BoltHits`. Records are kept for `ragdolls::KEPT_MS` (350 ms) so that a system that runs before `combat_fx` in a frame still sees them the next frame.
- **Which bolt hit.** `combat_fx` keeps a `Trail` for every bolt it draws (its `Path` as drawn, its owner, weapon, `power` flag and Sniper Shot flag) until 500 ms after it was last drawn. For a hit by shooter S on body V, the trail chosen is S's trail last drawn within 250 ms of the event's snapshot time whose line passes nearest V's hit location node, if that distance is under 1.5 m. With no such trail the line runs from S's chest to the node, as the bolt-burn test did.
- **Sniper Shot.** A trail takes the `special` of the `shots::Shot` of the same shooter whose `ms` is nearest the missile's `from_ms`, within 50 ms. A trail whose missile has `power` set is a Power Blast whatever the shot says.
- **Killing shot.** The struck entity's `fighter.hp` in the same snapshot is 0 or less.
- **The 500 ms gate** is measured in snapshot time (`Snapshot::time_ms`), not frame time, so two machines and a replay agree. It counts from the body's last bolt burn of any kind.
- **Separate counts.** Bolt burns are counted apart from lightsaber marks. `MOST_RECORDS` keeps counting only lightsaber marks. When `ALL_RECORDS` is passed, the oldest bolt burn goes before any lightsaber mark.
- **Head.** A hit whose location is `Location::Head` lays no burn. The trace includes the head's triangles, and when the first triangle the bolt meets belongs to the head, no burn is laid. A burn is never made from a mesh under the body's head node, nor from a body triangle with a corner weighted 0.5 or more to the head node or a joint under it.
- **A guest's own bolts.** The spark and the sound that a guest's prediction already plays stay as they are. The light, the smoke, the flinch and the burn wait for the authority's event, because they need its damage.
- **Flinch.** It turns the hit location's first node (`cast::spot(location).nodes`) and, for a hit on the trunk, `torsoupr_g` by 40% of that. The top of the node moves along the bolt's way. The angle rises to `FLINCH_DEG` = 6° in 40 ms and eases back to 0 by 300 ms. Several flinches add up, to at most 12°.

## File map

| File | Change |
|---|---|
| `crates/game/src/bolt_hits.rs` (new), `bolt_hits/probes.rs` (new) | `BoltHit`, `BoltHits`, `Trail`, the burn rule, burn size, trail choice, flinch curve |
| `crates/game/src/combat_fx.rs` | trails, the `BoltHit` records, the light flash |
| `crates/game/src/body_marks.rs`, `body_marks/probes.rs` (new) | three-layer bolt burns, the `gore` fix, the cap, the trace, the head rule, the smoke |
| `crates/game/src/cast.rs` | the flinch, added after breathing |
| `crates/game/src/ragdolls.rs` | the killing shot's push along the bolt |
| `crates/game/src/lib.rs`, `runtime.rs` | `pub mod bolt_hits`, `pub mod body_marks`, the resource |
| `crates/approved_tests/tests/approved.rs` | tests `bolt_hits`, `bolt_burns`, `armour_burns` |
| `docs/COMBAT.md`, `KNOWN-ISSUES.md` | texts |

## Review Focus

1. A repeating blaster emptied into one body for 10 s leaves at most 6 bolt burns on it, and the body's lightsaber burns are all still there. Test in Task 3 (`bolt_cap_spares_saber`).
2. A bolt that comes in from above or below and meets the head first leaves no burn, even when the sim's location says chest. Test in Task 4 (`trace_meets_head_first`).
3. A droid, which has no hit location nodes, still takes a burn on the skin the bolt meets, and the fallback with no trail and no node still finds skin. Test in Task 4 (`no_node_no_trail`).
4. Two bolts on the same body within one snapshot, the first heavy and the second light, give exactly one burn from the gate and one from the heavy rule, never two from the gate. Test in Task 1 (`gate_same_snapshot`).
5. The `gore` fix keeps lightsaber marks on the skin. A saber burn after the fix draws no triangle farther than 5 cm from the skin. Test in Task 3 (`saber_off_skin`) and the `off_skin_cm` dump fact in Task 7.

---

### Task 1: The burn rule and the hit record

**Files:**
- Create: `crates/game/src/bolt_hits.rs`, `crates/game/src/bolt_hits/probes.rs`
- Modify: `crates/game/src/lib.rs` (`pub mod bolt_hits;`), `crates/game/src/runtime.rs` (init `BoltHits` beside `BodyMarksPlugin`, line 218)
- Modify: `crates/approved_tests/tests/approved.rs` (a test `bolt_hits` beside `grunts`, line 313, in the same `#[cfg(feature = "rig")]` form)

**Interfaces (produces):**

```rust
// bolt_hits.rs
pub(crate) const HEAVY_PERCENT: i32 = 15;        // of the target's maximum vitality
pub(crate) const BURN_GAP_MS: u64 = 500;          // per body, snapshot time
pub(crate) const MOST_BOLT_BURNS: usize = 6;      // per body
pub(crate) const KEPT_MS: u64 = 350;              // how long a record is kept

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BurnSize { Pistol, Rifle, PowerBlast }

/// What one bolt hit was, as the view saw it.
#[derive(Clone, Copy, Debug)]
pub struct BoltHit {
    pub victim: SimId, pub by: Option<SimId>, pub sequence: u16,
    pub ms: u64,                 // the event's snapshot time
    pub location: Location,
    pub critical: bool, pub power_blast: bool, pub sniper_shot: bool, pub killing: bool,
    pub damage: i32,             // hundredths of a point of vitality, as the event has it
    pub max_hp: u16,             // whole points, `Sheet::max_hp`
    pub one_handed: bool,        // the gun that fired it (`Weapon::one_handed`)
    pub from: Vec3, pub way: Vec3,   // the bolt's line as drawn (game space, metres; way is a unit vector)
    pub node: Option<Vec3>,      // the hit location's node as posed (game space, metres)
    pub tint: Vec3,              // `weapons::handling(blaster).flash_tint`
}
#[derive(Resource, Default)]
pub struct BoltHits(pub Vec<BoltHit>);

/// Whether this hit leaves a burn, given when the body's last bolt burn was laid.
pub(crate) fn burns(hit: &BoltHit, last_burn_ms: Option<u64>) -> bool;
/// Whether the hit is heavy enough to burn whatever the gate says.
pub(crate) fn always_burns(hit: &BoltHit) -> bool;
pub(crate) fn burn_size(one_handed: bool, power_blast: bool) -> BurnSize;
```

`always_burns` is true for a critical hit, a Power Blast, a Sniper Shot, a killing shot, or a hit whose damage is at least 15% of `max_hp`. The damage is in hundredths and `max_hp` in whole points, so the test in whole numbers is `damage >= HEAVY_PERCENT * i32::from(max_hp)`. A body with `max_hp` of 0 counts no hit as heavy by this test. `burns` is false for `Location::Head`, true when `always_burns`, and otherwise true only when `last_burn_ms` is `None` or at least `BURN_GAP_MS` before `hit.ms`.

- [ ] **Step 1: Write the failing probes** in `bolt_hits/probes.rs` (model on `crates/game/src/fight_rooms/probes.rs`), one table each, driven by one helper that builds a `BoltHit` from a plain hit and a closure of changes:

```text
burn_rule_table   (parameterised) with max_hp 100 and last burn 100 ms before:
                  plain 10.00 damage → false; 15.00 damage → true; 14.99 → false;
                  critical → true; power_blast → true; sniper_shot → true; killing → true;
                  plain with last burn 500 ms before → true; 499 ms → false; no last burn → true.
head_never        every row above with location Head → false.
gate_same_snapshot two hits at the same ms on one body: a heavy one then a plain one.
                  The heavy one burns. The plain one, gated by the heavy one's burn, does not.
zero_max_hp       max_hp 0, plain, last burn 100 ms before → false; no last burn → true.
burn_sizes        (one_handed, power_blast): (true, false) Pistol; (false, false) Rifle;
                  (true, true) and (false, true) PowerBlast.
```

- [ ] **Step 2: Wire them** as `probed("bolt hits", &kjk_game::bolt_hits::probes(), 5)` in a test `bolt_hits`. Run `cargo test --release -p approved_tests --features rig --test approved bolt_hits`. Expected: it does not compile.
- [ ] **Step 3: Implement** the interface above. The module documentation states the owner's ruling in full sentences.
- [ ] **Step 4: Run** the test again. Expected: PASS. Then fmt and `cargo clippy -q -p game -p approved_tests --all-targets -- -D warnings`.
- [ ] **Step 5: Commit** as "Bolt burn rule".

### Task 2: Bolt hit records and the light flash

**Files:**
- Modify: `crates/game/src/combat_fx.rs` (`Drawn` at 305-354, `draw` at 734-772 and 874-931, `hits` at 1315-1409, the module documentation at 14-22)
- Modify: `crates/game/src/bolt_hits.rs`, `bolt_hits/probes.rs`

**Interfaces (produces):**

```rust
// bolt_hits.rs
/// A bolt as the view drew it, kept for `TRAIL_MS` after it was last drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trail {
    pub owner: SimId, pub base: Vec3, pub velocity: Vec3, pub launch: Vec3, pub fired: f32,
    pub last_ms: f32, pub weapon: sim::combat::sheet::Weapon, pub power: bool, pub sniper: bool,
}
pub(crate) const TRAIL_MS: f32 = 500.0;
pub(crate) const TRAIL_MATCH_MS: f32 = 250.0;
pub(crate) const TRAIL_NEAR_M: f32 = 1.5;
/// The trail of `by`'s bolt that hit at `ms` near `node`: the nearest line within reach.
pub(crate) fn pick_trail<'a>(trails: &'a [Trail], by: SimId, ms: f32, node: Vec3) -> Option<&'a Trail>;
/// The line a trail was drawn along when it met the body: its point there and its way.
pub(crate) fn line_of(trail: &Trail, ms: f32) -> (Vec3, Vec3);

// combat_fx.rs
const HIT_LIGHT_STRENGTH: f32 = 1.2;
const HIT_LIGHT_RADIUS: f32 = 2.5;   // metres
const HIT_LIGHT_MS: f64 = 120.0;
```

`line_of` uses the same sum as `Path::shown` (base, velocity, `Path::off`), so the line is the bolt as drawn. Move the `off` arithmetic into a free function both can call rather than copying it.

Behaviour:

- In `draw`, every bolt pushed to `blasters.bolts` (host bolts and a guest's own bolts) updates its `Trail` in `Drawn::trails`, keyed by the missile's `SimId` or the own bolt's key. Trails older than `TRAIL_MS` go. A scene swap clears them.
- In `hits`, a blaster hit that is not foretold builds a `BoltHit` with the trail from `pick_trail`, the struck entity's `cast::sheet(entity, &objects).max_hp` and `fighter.hp`, and pushes it to `BoltHits`. A foretold hit builds the record too, but does not play the spark and sound again.
- Each record also pushes a `Pulse` at the hit point (`from + way × the distance to the node`, or the node) in the record's `tint`, with the three constants above. The existing `BOLT_HIT` spark and impact sound stay exactly as they are.
- `BoltHits` drops records older than `KEPT_MS` at the start of each `draw`.

- [ ] **Step 1: Write the failing probes:**

```text
pick_nearest      two trails of one shooter, lines 0.2 m and 0.9 m from the node: picks the 0.2 m one.
pick_reach        the only trail passes 2 m from the node: None. A trail of another shooter: None.
pick_stale        a trail last drawn 300 ms before the hit: None.
line_as_drawn     line_of at a time past CONVERGE equals base + velocity × t (within 1 mm);
                  before CONVERGE it is offset by the launch, as Path::shown draws it.
```

- [ ] **Step 2: Raise the count** to 9 and see them fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `bolt_hits` and the existing combat tests (`git grep -n "combat_fx" crates/approved_tests` names them). Then clippy for `game`.
- [ ] **Step 5: Commit** as "Bolt hit records and light".

### Task 3: Three-layer bolt burns, the cap and the gore fix

**Files:**
- Modify: `crates/game/src/body_marks.rs` (constants 56-78, `Laid` 108-115, `State` 117-124, `mark` 237-334, `lay` 552-622, `gore` 650-725, the module documentation 1-34)
- Create: `crates/game/src/body_marks/probes.rs`; `crates/game/src/lib.rs` (`mod body_marks` becomes `pub mod body_marks`)
- Modify: `crates/approved_tests/tests/approved.rs` (a test `bolt_burns`)

**Interfaces (produces):**

```rust
// body_marks.rs
const BOLT_BURN: &str = "gfx/damage/bodyburnmark2.tga";     // Jedi Academy, 8 s, 2 s fade
const BOLT_EMBERS: &str = "gfx/damage/bodyburnmark_glow.jpg"; // Jedi Academy, 3.5 s
const BOLT_FLASH: &str = "gfx/effects/whiteglow.jpg";       // Jedi Academy, white to red over 0.6 s
const BOLT_LIFE_MS: f32 = 8000.0;
const BOLT_FADE_MS: f32 = 2000.0;
const BOLT_EMBERS_MS: f32 = 3500.0;
const BOLT_FLASH_MS: f32 = 600.0;
const BOLT_DEPTH: (f32, f32) = (-3.0, 4.0);                 // units along the ray
/// Half-width (units), drawn between, for each `BurnSize`.
const BOLT_SIZE: [(f32, f32); 3] = [(1.5, 2.0), (2.0, 2.5), (2.75, 3.25)];
/// The tint of each layer (burn, embers, flash) `age` ms after the hit, `left` ms before it goes.
fn bolt_tints(age: f32, left: f32, flicker: f32) -> [Vec4; 3];
/// Which laid mark to take away so `who` can take one more bolt burn: its oldest bolt burn
/// once it has `MOST_BOLT_BURNS`, else none. Lightsaber marks are never chosen.
fn make_room(laid: &VecDeque<Laid>, who: SimId) -> Option<u32>;
fn gore(mesh: &Mesh, at: &[Vec3], hit: (Vec3, Vec3), axes: (Vec3, Vec3), depth: (f32, f32)) -> Option<Mesh>;
```

`Laid` gains `bolt: Option<BoltStages>`, where `struct BoltStages { materials: [Handle<GlowMaterial>; 3], laid_ms: f32 }` holds the burn's three materials and when it was laid. Each bolt burn has its own three materials, so each cools on its own. `gore`'s depth test uses two bits, one for "in front of the window" and one for "behind it". A triangle is dropped only when all three corners share a bit, so a triangle that crosses the window is kept. This is the fix the bolt-burn test found. Take the texture names, the `heat` colour keys and the gore fix from the test's diff (the uncommitted changes of the `bolt-burn-spike` worktree beside this one, read with `git -C ../bolt-burn-spike diff`). Leave out every `eprintln!`, every `BURN_*` environment variable, `burn_debug`, and the magenta branch in `glow_shell.wgsl`.

In this task the burn is laid at the record's node point along its way, so the three layers can be seen and tested. Task 4 moves it to the exact place.

- [ ] **Step 1: Write the failing probes** (synthetic meshes made in the probe, no install):

```text
gore_crosses_window  a triangle with one corner 5 units in front of the window and two 6 units
                     behind it is kept. A triangle with all three corners in front is dropped.
saber_off_skin       a saber gore on a curved strip of 40 triangles: every kept triangle's middle
                     lies within 5 cm of the strip.
bolt_tints_timing    (parameterised over age) flash: near white at 0 ms, red at 300 ms, zero at
                     600 ms. Embers: above 0 at 3400 ms, zero at 3500 ms. Burn: alpha 1 until
                     6000 ms, 0.5 at 7000 ms, 0 at 8000 ms.
bolt_cap             six bolt burns on one body, then a seventh: make_room names the first.
bolt_cap_spares_saber a body with 16 saber marks and 6 bolt burns: make_room names a bolt burn.
                     A body with saber marks only: make_room names none.
bolt_cap_per_body    six bolt burns on body A: make_room for body B names none.
```

- [ ] **Step 2: Wire** `probed("bolt burns", &kjk_game::body_marks::probes(), 6)` in a test `bolt_burns`. Run it and see it fail.
- [ ] **Step 3: Implement.** `mark` reads `BoltHits`, keeps the records it has laid by `(victim, sequence)`, calls `bolt_hits::burns` with the body's last bolt burn time, and calls `make_room` before `lay`. Past `ALL_RECORDS`, the oldest bolt burn goes first. The dump facts gain `bolt_now` (bolt burns laid now) and `bolt_laid` (all bolt burns laid).
- [ ] **Step 4: Run** `bolt_burns` and `bolt_hits`. Then clippy for `game`.
- [ ] **Step 5: Commit** as "Bolt burns on bodies".

### Task 4: The exact place, and never on the head

**Files:**
- Modify: `crates/game/src/body_marks.rs` (`Bodies::posed` 447-485, `touched` 536-549, the bolt branch of `mark`), `crates/game/src/body_marks/probes.rs`

**Interfaces (produces):**

```rust
// body_marks.rs
/// Each posed mesh also knows whether it is the head's, and which of its vertices are the head's
/// (weighted 0.5 or more to the head node or a joint under it).
struct Posed { /* existing fields */ head_mesh: bool, head_vertex: Vec<bool> }
/// Where a line from `from` along `way` first meets a triangle of `posed`, within `reach` metres:
/// the point and whether that triangle is the head's.
fn first_meeting(posed: &[Posed], meshes: &Assets<Mesh>, (from, way): (Vec3, Vec3), reach: f32) -> Option<(Vec3, bool)>;
/// The vertex of `posed` nearest `point` that is not the head's.
fn nearest_skin(posed: &[Posed], point: Vec3) -> Option<Vec3>;
```

The head node is `Hitboxes[victim][Location::Head]` (`cast::Hitboxes`). `gore` skips a mesh with `head_mesh` set and a triangle with any `head_vertex` corner. The line is traced from 1 m before the record's hit point to 1 m past it. The burn goes where the line first meets the body. When the first meeting is the head's, no burn is laid. When the line meets nothing, the burn goes on `nearest_skin` of the node, or of the body's chest (`mind::chest`) when there is no node.

- [ ] **Step 1: Write the failing probes** (a synthetic body: a box trunk mesh skinned to a trunk joint and a head joint, and a separate head mesh hung under the head node):

```text
trace_meets_front    a line through the trunk's middle meets the trunk's front face, not its back.
trace_meets_head_first a line that passes the head first and then the trunk: meets the head, so no burn.
head_mesh_unmarked   a burn at the neck covers no triangle of the head mesh and no trunk triangle
                     with a corner weighted to the head joint.
no_node_no_trail     no node, no trail: the burn lands on the trunk vertex nearest the chest point.
```

- [ ] **Step 2: Raise the count** to 10, run, and see them fail.
- [ ] **Step 3: Implement.** Use Möller–Trumbore on the posed triangles. Read the mesh's index list once per mesh.
- [ ] **Step 4: Run** `bolt_burns`. Then clippy for `game`.
- [ ] **Step 5: Commit** as "Bolt burns where the bolt met the body".

### Task 5: Burns on armour

**Files:**
- Modify: whatever the cause is in (`body_marks.rs`, `glow_shell.rs`, `glow_shell.wgsl`, `scene.rs` where body meshes are kept readable, or the armour's material)
- Create: `crates/game/src/body_marks/armour_probes.rs` (`pub mod armour_probes` in `body_marks`); find the install the way `crates/game/src/viewer/kept_probes.rs` does, with `probe_kit::not_installed`
- Modify: `crates/approved_tests/tests/approved.rs` (a test `armour_burns` running `probed("armour burns", &kjk_game::body_marks::armour_probes::probes(), 1)`, needing `--features rig` and a K1 install, skipped without one, modelled on `undrawn_creature_named` at line 509)

The bolt-burn test drew burns on cloth and skin and nothing on the Sith trooper (`g_sithtroop01`). Its cause was not found. Use superpowers:systematic-debugging. Check, in this order, and write down what each check shows:

1. Whether `Bodies::posed` returns the trooper's body meshes at all (a mesh with no readable copy is skipped, line 452-459).
2. Whether `gore` keeps any triangle on them (the armour may stand farther out than `BOLT_DEPTH` from the trace's point, or have no index list).
3. Whether the mark meshes are spawned and visible (`InheritedVisibility`, `ViewVisibility`, the parent's `Visibility`; the test's `burn_debug` printed these).
4. Whether they draw but are hidden (the armour's own material drawing over a mark only 1 cm out, `PUSH_M`, or the mark's alpha being cut by the armour's shader).

- [ ] **Step 1: Write the failing probe.** With a K1 install, load `g_sithtroop01`'s body as the scene loads it, pose it at rest, lay a pistol burn on the chest from the front, and check that at least one mark mesh is made, that it is a child of a visible body mesh, and that its triangles lie within 5 cm of the armour. Without an install the check reports it was skipped.
- [ ] **Step 2: Run it** and see it fail. Report which of the four checks failed.
- [ ] **Step 3: Fix the cause**, not the symptom. If the fix touches how body meshes are kept, check that lightsaber marks on the same bodies still draw.
- [ ] **Step 4: Run** `armour_burns`, `bolt_burns`, `undrawn_creature_named`. Then clippy for `game` and `launcher`.
- [ ] **Step 5: Commit** as "Burns on armour". Put the cause, in plain sentences, in the commit body.

### Task 6: Smoke, flinch and the killing push

**Files:**
- Modify: `crates/game/src/body_marks.rs` (the smoke ask), `crates/game/src/script_fx.rs` only if a row needs a lifetime cut (`HookVisual`, line 552)
- Modify: `crates/game/src/cast.rs` (after breathing, 4802-4834, for a retargeted body; the `Look::Clips` branch of the same pose function for a creature or droid)
- Modify: `crates/game/src/ragdolls.rs` (`strike`, 564-601), `crates/game/src/bolt_hits.rs`, `bolt_hits/probes.rs`

**Interfaces (produces):**

```rust
// bolt_hits.rs
pub(crate) const FLINCH_DEG: f32 = 6.0;
pub(crate) const FLINCH_MOST_DEG: f32 = 12.0;
pub(crate) const FLINCH_RISE_MS: f32 = 40.0;
pub(crate) const FLINCH_MS: f32 = 300.0;
pub(crate) const TRUNK_SHARE: f32 = 0.4;
/// How far a flinch has turned the node `age` ms after the hit (degrees).
pub(crate) fn flinch_deg(age: f32) -> f32;
/// The turn of a node for the flinches of `hits` at `now`: about `way × up`, so the node's top
/// moves along each bolt's way, summed and capped at `FLINCH_MOST_DEG`.
pub(crate) fn flinch_turn(hits: &[(Vec3, f32)], now: f32) -> Quat;
/// The way a killing blow throws a body: a bolt's own way made level, else the existing way.
pub(crate) fn blow_way(bolt_way: Option<Vec3>, from_striker: Option<Vec3>, body_velocity: Vec3) -> Option<Vec3>;

// body_marks.rs
const SMOKE_ROW: u16 = /* chosen in Step 1 */;
```

Behaviour:

- **Smoke.** Every bolt hit, burn or not, asks for `SMOKE_ROW` (`HookVisuals`, `Place::At`) where the trace met the body, or at the hit point when it met nothing. The smoke lasts about 1 s.
- **Flinch.** `cast` reads `BoltHits` for each drawn body and turns the nodes named in the decisions above with `turn_in_model` (cast.rs line 4339). It is added after breathing and after the sim's pain clip, so the pain clip plays as before. A dead body does not flinch.
- **Push.** In `ragdolls::strike`, a blaster's killing hit with a record in `BoltHits` (same victim and sequence) throws the body along `blow_way` of the record's `way`. The strength (`blow`, 24 N·s) and the lift (`BLOW_LIFT`) are unchanged.

- [ ] **Step 1: Pick the smoke.** No KOTOR smoke effect is named in the code today. List the `visualeffects.2da` rows of both games whose label holds "SMOKE" (read the table the way `crates/launcher/src/scene_check.rs:403-410` does). Candidates found in the repository to look at as well: the smoke emitters of the astro droids and T3-M4 (`docs/MODEL-FX.md` line 43), the welding droid's smoke under `weld` (supermodel `p_t3m3`), and TSL's Peragus `001ebo17` emitters `smoke` and `smoke01`. Jedi Academy's `volumetric/black_smoke` (`severing.rs:58`) is not KOTOR's, so it is not a candidate. Film the two or three best rows on a body (Task 7's recipe, 2 s each), look at the frames, and pick the one that rises thin and grey for about 1 s. Record the row and why.
- [ ] **Step 2: Write the failing probes:**

```text
flinch_curve      flinch_deg: 0 at 0 ms, 6 at 40 ms, falling at every 10 ms step from 40 to 300 ms,
                  0 at 300 ms and after.
flinch_away       a node at the origin, a point 1 m above it, a bolt with way +X: after
                  flinch_turn at 40 ms the point has moved toward +X.
flinch_capped     five hits at the same ms: the turn is 12°, not 30°.
blow_follows_bolt a bolt way of (0.6, 0.8, -0.3) and a striker offset along +X: blow_way is
                  (0.6, 0.8, 0) normalised. No bolt way: the striker's offset made level, as today.
```

- [ ] **Step 3: Raise the count** to 13, run `bolt_hits`, and see them fail.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `bolt_hits`, `bolt_burns`, and the ragdoll and acting tests (`git grep -n "ragdolls\|acting::" crates/approved_tests`). Then clippy for `game`.
- [ ] **Step 6: Commit** as "Bolt smoke, flinch and killing push".

### Task 7: Clips, tuning and texts

**Files:**
- Modify: the constants of Tasks 2, 3 and 6 as the frames show; `docs/COMBAT.md` (the hits section); `KNOWN-ISSUES.md` if something does not work
- Create: clips under `kjk-artifacts/gunplay-impacts/` (not committed if the folder is ignored; check `.gitignore`)

- [ ] **Step 1: Film,** headless and muted, with scratch settings and saves. The recipe from the bolt-burn test:

```bash
KJK_SETTINGS=<scratch>/settings KJK_SAVES=<scratch>/saves kjk play --stage arena --class soldier@77+sniper --headless --mute \
  --moves "1500ms hand 0; 1s; 2s alt; 3s alt w; 1s; 600ms feat; 1s" \
  --cmds "wait 1s; arena god; arena spawn g_sithtroop01; wait 1500ms; camera 0.45 -10.5 1.3 253 2 36; film 10s hit-pistol-trooper; dump; quit"
```

Film twelve clips: a pistol, a rifle, a repeating blaster held down for 4 s, and a Power Blast, each against a cloth-clad enemy (`g_blkvulkgang01`), a Sith trooper (`g_sithtroop01`) and a droid (`g_wardroid01`). The arena's roster is in `crates/game/src/arena.rs:203-215`. Find how the arena gives a rifle and a repeating blaster (the F keys, `arena.rs`, or `kjk play --help`). Add one clip of a lightsaber hitting the cloth-clad enemy, to check saber burns after the `gore` fix.
- [ ] **Step 2: Look at the frames** of every clip. Check each of these and write down what each frame shows:
  - The burn sits where the bolt met the body, wraps the body and moves with it.
  - The flash turns from white to red in about 0.6 s, the embers glow for about 3.5 s, and the burn fades out by 8 s.
  - The repeating blaster's clip shows no more than 6 burns, and the burns appear no faster than about two a second.
  - No burn is on a face or a head. No burn looks like a flat black smudge.
  - The light flash is in the bolt's colour, the smoke rises for about 1 s, and the enemy flinches away from the shot.
  - The droid keeps its sparks. The killing shot throws the body along the bolt.
  - The dump's `body_marks.off_skin_cm` stays under 5 in the saber clip.
- [ ] **Step 3: Tune** `BOLT_SIZE`, `BOLT_DEPTH`, `HIT_LIGHT_STRENGTH`, `HIT_LIGHT_RADIUS`, `HIT_LIGHT_MS` and `FLINCH_DEG` until the frames look right. Change one value at a time and record each change and its reason. Do not change the owner's ruling numbers or the spec's timings.
- [ ] **Step 4: Bring `docs/COMBAT.md` up to date** in full plain sentences. Remove the line in `body_marks.rs`'s documentation that says bolts leave no burns.
- [ ] **Step 5: Run** `cargo fmt --all -- --check`, `cargo clippy -q --workspace --all-targets --all-features -- -D warnings`, `cargo clippy -q -p launcher --all-targets --no-default-features -- -D warnings`, and the approved tests `bolt_hits`, `bolt_burns`, `armour_burns` once. Fix what fails.
- [ ] **Step 6: Commit** as "Gunplay part 3 documents and tuning".
