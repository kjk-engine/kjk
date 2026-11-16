# Destruction, third pass (damage that reads, bolts that land) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the owner's second playtest: bolts land on every destructible prop as big as its model, damaged props lose chunks, grow hanging panels, keep KOTOR's fire, smoke and sparks and pile up soot and cracks, and the three faults in the playtest log are gone.

**Architecture:** One sim change: a destructible prop's facts gain its model's rest box (`ObjectData::body`), filled at module load (`session::objects::destructible`) and by the arena's prop hall, and `objects::shot_bodies` meets every destructible prop by it. Everything else is looks-only in the view: a new `crates/game/src/prop_damage.rs` owns stage looks (chunks off, hanging panels, emitters, soot and cracks), reusing `prop_looks`' `Fractures`, debris and marks; `prop_looks.rs` (1,729 lines) does not grow further.

**Tech Stack:** Rust, Bevy view (`crates/game`), `crates/session` loader, `crates/ragdoll` (`shatter`, debris), probes run by `./target/release/kjk sim probes --only <name>`.

**Spec:** `docs/superpowers/specs/2026-11-06-destruction-design.md`, section "Third pass: damage that reads, bolts that land"; where it differs from the second pass, the third wins.

## Global Constraints

- Worktree cut from local `claude/prop-hall` at this plan's commit (it includes the uncommitted-on-origin arena grenade kit commit); push fast-forward to `origin/claude/prop-hall` at the end; draft PR kjk-engine/kjk#186 stays draft, no auto-merge.
- No snapshot or save format change. `ObjectData::body` is a fact rebuilt at load, not saved state; the level digest changes as with any fact change (saves re-stamp as usual). If a format change turns out necessary, stop and ask.
- Every numeric constant in `crates/sim` is a rulebook rule; run `cargo xtask rulebook` after touching `crates/sim` or `data/rules`.
- KOTOR's own meshes, textures, emitters and sounds only. Emitters come from the games' own effect models (the effect placeables the catalogue lists as intact, `visualeffects.2da` rows). Crack marks only from a texture the games or the JA marks already used have; none found → no cracks, recorded as a ruling.
- Same looks on every machine: seeds from model resref, object index and stage via `ragdoll::shatter::seed_of`/FNV-1a, never `std` hashers or platform trig.
- Frame budget: no frame over 16.7 ms with a frag grenade in the densest hall row; at most 24 live emitters within 30 m of a player; pieces cast no shadows.
- Every game run passes `--mute`. Build with `cargo build --release --bin kjk`.
- Before a task is done: `cargo clippy -p ragdoll -p sim -p session -p game -p launcher --release --all-targets` clean, `cargo fmt`.
- Commit messages: a plain sentence of the effect (repo style), body explains; no attribution lines.

## Review Focus

1. A prop first seen already damaged (save, joiner, coming back): drawn with its chunks already gone and its panels already hanging, no debris flying, no burst. Test in Task 3.
2. Two seats or two runs: the same prop at the same stage loses the same chunks and hangs the same panels. Test in Task 3.
3. A thin prop (flag, sign, banner) still never blocks a shot wider than it is drawn; a prop whose model has only emitters or dummies gets no body. Test in Task 1.
4. A prop broken while some of its pieces were already knocked off: only the remaining pieces fly; no piece is drawn twice. Test in Task 3.
5. Leaving the area with emitters running: every emitter and hanging panel despawns with the generation (no leak, no orphan sound). Test in Task 4.

---

### Task 1: Bolts meet every destructible prop by its model's box

**Files:**
- Modify: `crates/sim/src/module.rs:256` (beside `destructible`: `pub body: Option<[[f32; 3]; 2]>`, least and greatest corners in the prop's own frame, metres), `crates/sim/src/world/objects.rs:162-175` (`shot_bodies`), the body builder `bodies_where` (an oriented box from `body` and the facing when present), `crates/session/src/objects.rs:248` (fill `body` from the model), `crates/sim/src/world/arena.rs` + `crates/game/src/prop_hall.rs` (arena props carry `body` from `model_box`), `crates/game/src/shot_fx.rs` / `prop_looks::impact` (a bolt that ended on a body: the struck face of the box)
- Test: `crates/sim/src/world/destruction_probes.rs`

**Interfaces:**
- Produces: `ObjectData::body`; `pub fn model_body(ir: &ModelIr) -> Option<[[f32; 3]; 2]>` in `asset_odyssey::model` (rest bounds of meshes only: no emitters, no dummies, no lights); `shot_bodies` keeps every unbroken destructible prop that has a `body`, both classes, and drops the class filter.

- [ ] **Step 1: Write the failing probes:** `bolt_meets_console` (a damage-class console with `body` `[[-0.5,-0.2,0],[0.5,0.2,1.6]]` facing 90°: a party bolt and an enemy bolt aimed at it end on it and `ObjectState.damage` rises); `bolt_passes_beside_flag` (a flag with body 0.05 m thick: a bolt 0.2 m to its side passes); `bolt_no_body` (a destructible prop with `body: None` is not met, as today).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only destruction`; expect the three to fail.
- [ ] **Step 3:** Implement; the session loader reads each destructible placeable's model through the resolver it already has for the module (or is given one), once per model.
- [ ] **Step 4:** Run probes; expect pass; `cargo xtask rulebook` whole. In the hall, shoot a console, a swoop, a statue and a jar with a blaster rifle; screenshot each: an impact where the bolt met it.
- [ ] **Step 5:** Commit: "Blaster bolts land on every destructible prop, as big as its model".

### Task 2: The playtest log's three faults

**Files:**
- Modify: `crates/game/src/prop_looks.rs` (`cuttable` :254, `base` :105, the piece despawn path)

**Interfaces:**
- Produces: `base(pieces: &[Piece], height: f32, floor: f32) -> Vec<usize>`: lowest quarter, or lowest point within 0.05 m of `floor`, and never empty for a non-empty set.

- [ ] **Step 1: Write the failing probes** in "prop looks": `prop_pieces_base_legs` (four tall leg pieces touching the floor and a top piece: base holds the four legs); `prop_pieces_base_never_empty` (one floating set: base holds its lowest piece); `prop_cut_plant` (`cuttable` of `plc_firixa01`'s model gives at least one `Plain`, uncapped: load it through the probe's resolver, as the other model probes do, or a synthetic IR shaped like it if the probes have no game data).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only "prop looks"`; expect failures.
- [ ] **Step 3:** Implement. For the double despawn, find the second despawn (a piece despawned by fade and again by eviction or the generation reset) and despawn once; check the hall log shows no `Entity despawned` warning after a grenade.
- [ ] **Step 4:** Run probes; expect pass.
- [ ] **Step 5:** Commit: "Legged furniture leaves its legs standing, plants break apart, pieces go once".

### Task 3: Chunks knocked off and hanging panels

**Files:**
- Create: `crates/game/src/prop_damage.rs` (+ plugin beside `PropLooksPlugin`; probes registered as `("prop damage", kjk_game::prop_damage::probes)` in `crates/launcher/src/sim_probes.rs`)
- Modify: `crates/game/src/prop_looks.rs` (expose what `prop_damage` needs: `Fractures`, piece spawning, debris launch, dents on piece meshes; move stage handling for scorch out of it)

**Interfaces:**
- Consumes: `Fractures`, `base`, `fling`, `sim::destruction::stage`, `ragdoll::shatter::{Piece, seed_of, dent}`.
- Produces:
  - `pub(crate) fn knocked(pieces: &[Piece], base: &[usize], size: Size, stage: u8, seed: u64) -> Vec<usize>`: the pieces gone by `stage` (cumulative: stage 2's holds stage 1's); medium 1–2 then +2, large 2–3 then +3, small 1 then +1; never base pieces; outermost and highest first, ties by seed.
  - `pub(crate) fn hangs(piece: &Piece, middle_of_prop: [f32; 3]) -> Option<([f32; 3], [f32; 3], f32)>`: for a flat outer piece (thinnest side < 1/3 of the others), its hinge edge (two points, the edge nearest the prop's middle) and its angle 25–70° (seeded); `None` otherwise.
  - Behaviour: from the first knocked piece the prop's root is hidden and its remaining pieces drawn in place with the prop's own (copied) materials and its dents; knocked pieces fly as debris when the stage rises live, absent when first seen; hanging ones turn about their hinge and stay; at stage 2 one stage-1 hanger may fall (seeded). On break, only remaining pieces fly.
- [ ] **Step 1: Write the failing probes:** `damage_knocked_cumulative` (stage 2's set ⊇ stage 1's; sizes give the counts above); `damage_knocked_same` (same seed, same set; another object index, another set); `damage_knocked_never_base`; `damage_hangs_flat_only` (a 1×1×0.05 m piece hangs, a cube doesn't; angle within 25–70); `damage_first_seen` (the decision for a prop first seen at stage 2: knocked absent, hangers hanging, no debris, no burst); `damage_break_remaining` (breaking after stage 2 flies only the pieces not knocked).
- [ ] **Step 2:** Run `./target/release/kjk sim probes --only "prop damage"`; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, saber a console, a desk, a swoop and a kiosk through both stages; screenshot each stage up close: gaps with capped edges, a panel hanging, chunks on the floor.
- [ ] **Step 5:** Commit: "Damaged props lose chunks and hang torn panels, the same on every machine".

### Task 4: Lasting fire, smoke and sparks

**Files:**
- Modify: `crates/game/src/prop_damage.rs`; `crates/game/src/prop_breaks.rs` (the first pass's smoulder moves here or is replaced)

**Interfaces:**
- Produces:
  - `pub(crate) fn emitters(material: Material, stage: u8, broken: bool) -> &'static [Emitter]` where `Emitter` names a KOTOR effect model or `visualeffects.2da` row: electronic stage 1 sparks, stage 2 arcs + smoke; metal stage 1+ smoke; wood, cloth, organic stage 1 smoke, stage 2 small fire + smoke; stone stage 1+ dust; broken base: smoke for 5–10 s (seeded). The builder picks the exact models from the games' effect placeables and `visualeffects.2da` and lists them in one table with a comment per row.
  - `pub(crate) fn lit(candidates: &[(u32, f32 /*distance m*/)], most: usize, reach_m: f32) -> Vec<u32>`: nearest first, within `reach_m`, at most `most` (24, 30 m).
  - Emitters sit at a seeded point on the prop's upper surface (or the hit's mark nearest the top), parented to the prop, and despawn with the area generation.
- [ ] **Step 1: Write the failing probes:** `emitters_by_material` (each material and stage gives the table's kinds; stage 0 none); `emitters_capped` (30 candidates: 24 nearest within 30 m, none beyond); `emitters_leave_with_area` (after a generation change the live set is empty).
- [ ] **Step 2:** Run the probes; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, grenade a mixed row and wait 10 s; screenshot: smoke, sparks, small fires on the wrecks; frame log: no frame over 16.7 ms.
- [ ] **Step 5:** Commit: "Wrecked props keep burning, smoking and sparking in KOTOR's own effects".

### Task 5: Soot and cracks build up

**Files:**
- Modify: `crates/game/src/prop_damage.rs`, `crates/game/src/prop_looks.rs` (marks: kept count by class, the soot layer)

**Interfaces:**
- Produces:
  - Marks kept: 24 for damage class, 8 for breakables (`kept_marks(class) -> usize`).
  - Each mark lays a soot mark under it: 3× its radius, faint, same texture family as the scorch.
  - `pub(crate) fn cracks(material: Material, screen: bool) -> bool`: stone and screens; the crack texture found in the games or the JA marks (search texture names for crack, broken glass, shatter); none → `cracks` always false and a ledger ruling.
  - `DARKENED` becomes `[1.0, 0.9, 0.8]`.
- [ ] **Step 1: Write the failing probes:** `marks_kept_by_class`; `soot_under_mark` (the soot's radius is 3× the mark's); `cracks_stone_and_screens`; `darkened_subtle`.
- [ ] **Step 2:** Run the probes; expect failures.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run probes; expect pass. In the hall, shoot a statue and a console 15 times each; screenshot: soot spreading, cracks, colour still the prop's own.
- [ ] **Step 5:** Commit: "Scorch spreads into soot and stone and screens crack as hits pile up".

### Task 6: Checks, clips and the PR

- [ ] **Step 1:** Run `./target/release/kjk chain k1 --mighty --skill security=2` in the background; expect 29/29 (bolts now meet consoles and statues, so fights may change; story globals and journal must not).
- [ ] **Step 2:** Run the whole probe set (`./target/release/kjk sim probes`); expect 0 failed.
- [ ] **Step 3:** Clips (K1 and TSL halls): a blaster pass along consoles and statues, a saber working a console and a desk to wrecked, grenades into a mixed row with emitters burning after; send to the owner with absolute paths.
- [ ] **Step 4:** `docs/COMBAT.md` "Destruction" updated (bodies, damage looks); the design's status line; `cargo xtask rulebook`, clippy, fmt; push to `claude/prop-hall`; update PR 186's description (stays draft).
- [ ] **Step 5:** Commit: "COMBAT.md tells how damage shows and where bolts land".
