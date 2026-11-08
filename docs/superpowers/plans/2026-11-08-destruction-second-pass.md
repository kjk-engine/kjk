# Destruction, second pass (pieces, impacts, damage stages) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the owner's playtest of the prop hall: breakable props fly apart into pieces of themselves instead of vanishing, big and wall-mounted things crumple (dents, scorch, dying screens) instead of breaking, workbenches stay intact, and every hit shows where it landed with shards, a lasting mark and a jolt.

**Architecture:** The sim changes only in its catalogue (classes); everything else is looks-only in the view, driven by the synced `ObjectState` (look `DEAD`, `damage` → `sim::destruction::stage`). Mesh maths that needs no Bevy (shattering a mesh into capped pieces, dents, crossing a segment with triangles) lives in `crates/ragdoll` beside `cut.rs`; flying pieces are rigid bodies in the existing `ragdoll::World` against the level faces it already holds. A new view module `crates/game/src/prop_looks.rs` owns pieces, shards, marks, shakes and stages; `prop_breaks.rs` keeps effects and sounds.

**Tech Stack:** Rust, Bevy view (`crates/game`), rapier via `crates/ragdoll`, `asset_odyssey::model::ModelIr`, probes run by `./target/release/kjk sim probes --only <name>`.

**Spec:** `docs/superpowers/specs/2026-11-06-destruction-design.md`, section "Second pass: what breaking looks like (owner playtest 2026-11-08)"; where it differs from "What breaking looks like", the second pass wins.

## Global Constraints

- Worktree `.claude/worktrees/prop-hall`, branch `claude/prop-hall` (draft PR kjk-engine/kjk#186). Never touch the main checkout. PR stays draft.
- No snapshot, save or pack format change. Hit points, shards, marks, shakes and flying pieces are looks-only, not synced, not saved.
- Every numeric constant in `crates/sim` is a rulebook rule; run `cargo xtask rulebook` after touching `crates/sim` or `data/rules`.
- KOTOR's own meshes, textures, effects and sounds only (pieces and shards are cut from the prop's own mesh and texture; marks use `saber_marks`' textures). No hand-drawn effects.
- Same pieces on every machine: seeds come from the model's resref (and object index for per-instance dents), hashed with a fixed function (FNV-1a 64), never `std` hashers or platform trig (`sim::math` only).
- Frame budget: no frame over 16.7 ms when a frag grenade breaks a row of the hall; pieces cast no shadows; at most 256 flying pieces.
- Every game run passes `--mute`. Build with `cargo build --release --bin kjk`.
- Before a task is done: `cargo clippy -p ragdoll -p sim -p game -p launcher --release --all-targets` clean, `cargo fmt`.
- Commit messages: a plain sentence of the effect (repo style), body explains; no attribution lines.

## Review Focus

1. A model made of several meshes on several nodes, each with its own transform: pieces, dents and marks must be in the model's frame, not each node's local one. Test in Task 2.
2. An alpha-tested or two-sided mesh (plant leaves, flags, vines): shattering must not cap it (caps on a leaf look like solid cardboard), and its pieces still fly. Test in Task 2.
3. A prop already broken when the area loads (save, joiner, coming back): base only, no flight, no burst, no sound. Test in Task 4.
4. Leaving the area mid-flight: every piece body leaves the ragdoll world and every entity despawns (no leak across `SceneSwap::generation`). Test in Task 4.
5. A damage-class mesh that is skinned, dangly or animated: no dent rebuild (it would freeze or tear its motion); scorch and screens still apply. Test in Task 6.

---

### Task 1: Classes the owner asked for

**Files:**
- Modify: `data/destruction/props.csv`, `docs/superpowers/specs/2026-11-06-destruction-props.csv` (same class changes)
- Modify: `crates/sim/src/destruction/probes.rs`

**Interfaces:**
- Produces: the revised catalogue; `catalogue()` unchanged.

- [ ] **Step 1: Write the failing probe** `destruction_classes_revised`: `catalogue("k1", 168)` and `(169)` and `catalogue("tsl", 168)`, `(169)`, `(336)` are `None`; `catalogue("k1", 27)`, `(28)`, `(59)`, `(60)`, `(61)`, `(67)` and `catalogue("tsl", 296)` have class `Damage`; no row of either game with material `Electronic` or size `Large` has class `Break`. Update `destruction_catalogue`'s counts to the new totals.
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only destruction`; expect `destruction_classes_revised` to fail.
- [ ] **Step 3:** Edit both CSVs: rows 168, 169 (both games) and TSL 336 → `intact`; the desks, kiosks and `LargeLocker` above → `damage`; any other electronic or large `break` row → `damage`.
- [ ] **Step 4:** Run the probes; expect all destruction probes to pass. Start the hall (`./target/release/kjk play --stage arena --mute --cmds 'wait world; quit'`); expect no "prop hall is too small" error.
- [ ] **Step 5:** Commit: "Workbenches stay intact, and desks, kiosks and lockers crumple instead of breaking".

### Task 2: Shattering a mesh

**Files:**
- Create: `crates/ragdoll/src/shatter.rs` (+ `pub mod shatter;` in `crates/ragdoll/src/lib.rs`); probes in the same file
- Modify: `crates/ragdoll/src/cut.rs` (its open-loop finding and fan moved to shared functions in `shatter.rs` or a `loops` helper both use; no behaviour change)
- Modify: `crates/launcher/src/sim_probes.rs` (register `("shatter", ragdoll::shatter::probes)`)

**Interfaces:**
- Produces:
  - `pub struct Plain { pub positions: Vec<[f32; 3]>, pub normals: Vec<[f32; 3]>, pub uvs: Vec<[f32; 2]>, pub indices: Vec<u32>, pub capped: bool }`: one mesh in the model's frame; `capped` false for alpha-tested or two-sided meshes.
  - `pub struct Piece { pub meshes: Vec<(usize, Plain)>, pub middle: [f32; 3], pub volume: f32, pub hull: Vec<[f32; 3]> }`: per source mesh index, its share; `hull` the piece's points for a convex collider.
  - `pub fn shatter(meshes: &[Plain], count: usize, seed: u64) -> Vec<Piece>`: `count` seeded points inside the meshes' bounds (deterministic from `seed`), each triangle to the point nearest its centroid, empty cells dropped; each piece's open edge loops of capped meshes closed by a fan from the loop's middle, cap UVs projected on the loop's best-fit plane from the mesh's own texture.
  - `pub fn shards(meshes: &[Plain], near: [f32; 3], count: usize, seed: u64) -> Vec<Piece>`: up to `count` small pieces of the triangles within 0.25 m of `near` (nearest triangles if none), uncapped.
  - `pub fn seed_of(name: &str) -> u64`: FNV-1a 64 of the lowercased name.
  - `pub fn probes() -> sim::probe::Checks` (or the probe type the other `sim_probes` entries return).

- [ ] **Step 1: Write the failing probes:**
  - `shatter_covers_once`: a unit cube (12 triangles, capped) shattered into 6: every source triangle appears in exactly one piece; every piece of a capped mesh has no open edge (each edge shared by two triangles).
  - `shatter_same_everywhere`: two calls with seed `seed_of("plc_chair1")` give byte-identical pieces; `seed_of("PLC_Chair1") == seed_of("plc_chair1")`.
  - `shatter_model_frame`: two meshes given already in the model frame (one offset by +2 m in x) give pieces whose `middle`s span both, none at the origin only.
  - `shatter_uncapped_leaf`: a single quad with `capped: false` gives pieces that add no triangles.
  - `shards_near_point`: shards of the cube near one corner hold only triangles within 0.25 m of it.
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only shatter`; expect failures (not compiled in).
- [ ] **Step 3:** Implement `shatter.rs`; move `cut.rs`' loop and fan code into the shared function, rerun `ragdoll::probes` via `cargo test -p approved_tests ragdoll` (expect unchanged pass).
- [ ] **Step 4:** Run the shatter probes; expect `5 checks, 0 failed`.
- [ ] **Step 5:** Commit: "Meshes shatter into capped pieces of themselves, the same on every machine".

### Task 3: Pieces as rigid bodies

**Files:**
- Modify: `crates/ragdoll/src/world.rs` (debris bodies), probes in `crates/ragdoll/src/shatter.rs` or `probes.rs`

**Interfaces:**
- Consumes: `ragdoll::World` (`add_faces`, `step`).
- Produces:
  - `pub struct DebrisId(u32)` (`Copy + Eq + Ord + Debug`).
  - `World::debris(&mut self, hull: &[[f32; 3]], at: [f32; 3], rotation: [f32; 4], velocity: [f32; 3], spin: [f32; 3]) -> Option<DebrisId>` (`None` when the hull is degenerate); a convex-hull collider, no shadow concerns here.
  - `World::debris_pose(&self, id: DebrisId) -> Option<([f32; 3], [f32; 4])>`, `World::resting(&self, id: DebrisId) -> bool` (asleep or under 0.05 m/s for 0.5 s), `World::remove_debris(&mut self, id: DebrisId)`.
  - Pieces collide with level faces and each other, never with ragdolls' bodies.

- [ ] **Step 1: Write the failing probe** `debris_settles`: a floor of two triangles at z 0; a 0.2 m cube hull dropped from z 1 with spin comes to rest within 3 s of 1/60 s steps with its lowest hull point within 0.02 m of z 0; `remove_debris` makes `debris_pose` `None`.
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only shatter`; expect `debris_settles` to fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run; expect pass.
- [ ] **Step 5:** Commit: "Pieces of broken props tumble and settle against the room".

### Task 4: Breaking into pieces in the view

**Files:**
- Create: `crates/game/src/prop_looks.rs` (+ plugin added where `PropBreaksPlugin` is), probes registered as `("prop looks", kjk_game::prop_looks::probes)`
- Modify: `crates/game/src/prop_breaks.rs` (the hide path, `breaks.hidden`, goes; it tells `prop_looks` of breaks), `crates/game/src/scene.rs:1000` (extract the `MeshIr` → Bevy `Mesh` build of `Spawner::mesh` into `pub(crate) fn bevy_mesh(ir: &MeshIr) -> Mesh` that pieces, shards and dents reuse), `crates/game/src/ragdolls.rs` (the world's owner: lend it to `prop_looks`)

**Interfaces:**
- Consumes: Task 2 `shatter`, `seed_of`; Task 3 debris; `SceneContent.0.models[i].ir` (`ModelIr` with `meshes: Vec<MeshIr>` and node transforms), `instance_models`, `Made::prop_root`, `SceneSwap::generation`.
- Produces:
  - `#[derive(Resource)] Fractures`: per model resref, `Vec<Piece>` once ready; built by `AsyncComputeTaskPool` when a destructible prop's model is first seen. Count by size: small 6, medium 10, large 16. Meshes converted to `Plain` in the model frame (node transforms applied), `capped = transparency_hint == 0 && !two_sided`.
  - `pub(crate) fn base(pieces: &[Piece], height: f32) -> Vec<usize>`: the pieces whose middle is in the lowest quarter of `height`.
  - `pub(crate) fn fling(middle: [f32; 3], from: [f32; 3], blast: bool) -> [f32; 3]`: away from `from`, up and out; 2–4 m/s, 5–8 m/s for blasts, deterministic from inputs.
  - `pub(crate) fn kept(volume: f32) -> bool`: true over 0.001 m³ (bigger than a fist).
  - `pub(crate) fn evict(live: &[(u64 /*born ms*/, bool /*kept*/)], cap: usize) -> Vec<usize>`: indices to fade, oldest unkept first, then oldest kept.
- Behaviour: on `Change::Broke` the prop's root is hidden and its pieces spawned in its place, each a child mesh with the prop's own materials; base pieces stay put; the rest fly as debris (no shadows), unkept ones fade after 10–20 s (seeded), all despawned and removed from the world on area change. A prop seen for the first time already `DEAD` gets its base only. A model with nothing to cut is hidden as before.

- [ ] **Step 1: Write the failing probes:** `prop_pieces_base` (`base` of a 1 m-tall set picks only pieces with middle z < 0.25); `prop_pieces_fling` (`fling` points away from `from` with a positive z, and blast speed > single); `prop_pieces_evict` (cap 3 over 5 live with two kept: evicts the oldest unkept first, never a kept one while an unkept remains); `prop_pieces_already_broken` (the decision function for a first-seen `DEAD` prop returns base-only, no flight, no burst).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only "prop looks"`; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, throw frag grenades along the break rows (as the first pass's clip did: `--moves` route in the memory note `destruction.md`, then `grenade`), screenshot before and 3 s and 15 s after; expect pieces on the floor, bases standing, nothing vanished. Leave the arena for a module and back (`--cmds`): no warnings, piece count 0 after the swap (log it).
- [ ] **Step 5:** Commit: "Broken props fly apart into pieces of themselves; the big ones and the base stay".

### Task 5: Where a hit lands, and what it shows

**Files:**
- Modify: `crates/game/src/prop_looks.rs`, `crates/game/src/shot_fx.rs:271-279` (report a bolt trace that ends on a destructible prop: object index, point, normal), `crates/game/src/prop_breaks.rs` (hit effects at the point, not `middle()`)
- Modify: `crates/ragdoll/src/shatter.rs` (`first_crossing`)
- Reuse: `crates/game/src/body_marks.rs:552` (marks drawn on a mesh's own triangles), `crates/game/src/saber_marks.rs` (mark textures)

**Interfaces:**
- Produces:
  - `pub fn first_crossing(from: [f32; 3], to: [f32; 3], triangles: &[[[f32; 3]; 3]]) -> Option<([f32; 3], [f32; 3])>` in `shatter.rs`: the first point along the segment where it crosses a triangle, and that triangle's normal facing `from`.
  - `pub(crate) enum Blow { Bolt, Blade, Blunt, Blast }` and `pub(crate) fn impact(prop: Index, ...) -> (point, normal, Blow)`: bolts from the `shot_fx` report this frame (exact); else the blade or hand segment of a creature within 3 m of the prop as drawn this frame, crossed with the prop's triangles (`first_crossing`); else the point on the prop's body box facing the nearest creature within 10 m or the last explosion within its radius.
  - `pub(crate) fn shake(size: Size, class: Class, since_ms: f32) -> f32` degrees: small 6°, medium 3°, decaying to 0 at 150 ms; 0 for large or damage class.
- Behaviour on `Change::Hit`: 3–6 shards (`shards`, seeded by object index and damage) fly along the normal and fade within 2 s for wood, ceramic, stone, plastic, debris; metal and electronic play KOTOR's sparks, organic and cloth its dust splash (first pass's rows), all at the point; a mark (bolt/power: scorch, `Blade`: burn, `Blunt`: dark streak) is laid on the prop's own triangles near the point, newest 8 kept per prop; the root jolts by `shake` away from the blow.

- [ ] **Step 1: Write the failing probes:** `prop_hit_crossing` (`first_crossing` of a segment through a unit quad at z 0 returns z 0 and normal facing `from`; a segment that misses returns `None`); `prop_hit_shake` (small at 0 ms is 6, at 150 ms 0; large and damage class are 0 always); `prop_hit_marks_capped` (the ninth mark on a prop drops the oldest).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only "prop looks"`; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, saber three props (wood, metal, electronic), shoot two, Push one, and screenshot each up close; expect shards or sparks where the blade or bolt met it, a mark there that stays, and a visible jolt in a clip.
- [ ] **Step 5:** Commit: "Every hit on a prop lands where it struck: shards of it, sparks, a lasting mark, a jolt".

### Task 6: Damage stages from the prop's own mesh

**Files:**
- Modify: `crates/game/src/prop_looks.rs`, `crates/ragdoll/src/shatter.rs` (`dent`)
- Reuse: `crates/game/src/model_fx.rs:1417` (`own_materials`: a per-instance material copy, with `AnimatedMaterials::copied`), `scene::bevy_mesh` (Task 4)

**Interfaces:**
- Consumes: `sim::destruction::stage`, `MeshIr { self_illum, uv_scroll, texture, .. }`, `TextureFacts::cycle`.
- Produces:
  - `pub fn dent(positions: &mut [[f32; 3]], normals: &[[f32; 3]], centre: [f32; 3], radius: f32, depth: f32)` in `shatter.rs`: moves vertices within `radius` of `centre` inward along their normals by `depth × (1 − (d / radius)²)`.
  - `pub(crate) fn dents(seed: u64, stage: u8, size: Size) -> Vec<(usize /*mesh*/, [f32; 3] /*centre*/, f32 /*radius*/, f32 /*depth*/)>`: per stage 2 (medium) or 4 (large) more dents at seeded surface points; radius 0.15 m (medium) / 0.3 m (large), depth 0.04 m / 0.08 m.
  - `pub(crate) fn screen(mesh: &MeshIr, facts: Option<&TextureFacts>) -> bool`: `self_illum` above 0.1 in any channel, or `uv_scroll`, or a texture `cycle`.
  - `pub(crate) fn darkened(stage: u8) -> f32`: 1.0, 0.75, 0.45 (stages 0, 1, 2+).
- Behaviour: when a destructible prop's stage rises (or is first seen above 0), its materials are copied once and tinted by `darkened`; for damage class, dents are applied to its static meshes from the `ModelIr` and their `Mesh3d` handles swapped for rebuilt ones (never skinned, dangly or animated meshes); screens flicker at stage 1 (self-illum toggling at seeded 80–300 ms) and at stage 2 go dark (self-illum 0, animation stopped, tint 0.2) with one spark burst. Collision never changes.

- [ ] **Step 1: Write the failing probes:** `prop_stage_dent` (a flat grid dented at its middle: the centre vertex moves `depth` against its normal, a vertex at `radius` doesn't move); `prop_stage_dents_same` (`dents` with the same seed and stage are identical; stage 2 holds stage 1's dents plus more); `prop_stage_screen` (a mesh with `self_illum [0.5, 0.5, 0.5]` is a screen, one with zero and no scroll or cycle is not); `prop_stage_skips_moving` (the dent-eligibility function refuses skinned, dangly and animated meshes); `prop_stage_dark` (`darkened` is 1.0, 0.75, 0.45, 0.45 for 0–3).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only "prop looks"`; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, saber a console (`CompPanel2_wall`, `InfoTerminal`), a desk and a swoop through both stages; screenshot each stage up close; expect dents, darker material, a flickering then dead screen, sparks.
- [ ] **Step 5:** Commit: "Consoles, desks and machinery crumple: dents, scorch, screens that flicker and die".

### Task 7: Checks, clips and the PR

**Files:**
- Modify: `docs/COMBAT.md` ("Destruction": pieces, impacts, stages), the design's status line

- [ ] **Step 1:** Run `./target/release/kjk chain k1 --mighty --skill security=2` in the background; expect 29/29.
- [ ] **Step 2:** Frame budget: in the hall, frag grenade into the densest break row with the frame-time log the FPS work used; expect no frame over 16.7 ms; if over, lower the piece cap or counts, not the frame rate.
- [ ] **Step 3:** Clips: K1 and TSL halls, a saber pass along a row, a grenade, a Force Wave, a console worked through its stages; send to the owner (absolute paths).
- [ ] **Step 4:** Docs; `cargo xtask rulebook`, clippy, fmt; push to `claude/prop-hall`; update PR 186's description (stays draft).
- [ ] **Step 5:** Commit: "COMBAT.md tells how props break apart and crumple".
