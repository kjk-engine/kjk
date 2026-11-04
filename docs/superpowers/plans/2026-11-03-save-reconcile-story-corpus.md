# Save reconcile, story restore and release corpus — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A save made by any earlier release loads on main correctly, or
falls back to a story restore, and every release is checked against real
saves before its tag.

**Architecture:** Three parts of the save migrations design, each its own
PR off `main`, in this order: part 3c (reconcile at load: object roster,
nav snap, script code digests), part 4 (`story.json` and the story
restore), part 2b (`kjk saves capture|check` and the release checklist).
3c bumps `snapshot` and `save` once with a migration; 4 and 2b move no
hash.

**Tech Stack:** Rust workspace; `crates/sim` (snapshot, install, save,
script state), `crates/saves` (slot folders, serde), `crates/game` (load
screen), `crates/launcher` (verbs), `crates/bumps`.

**Spec:** `docs/superpowers/specs/2026-10-28-save-migrations-design.md`
(sections 2b, 3c, 3d, 4). Read it first; this plan only records what the
survey of 2026-11-03 decided on top of it.

## Global Constraints

- Every format change goes through a bump file `bumps/<branch>.toml` with
  `[formats]` and `[migrations]` lines; never edit a `VERSION` or the sim
  hash pin by hand (`bumps/README.md`).
- Old reads stay, gated with `input.since(crate::since::<area>::<NAME>)`.
- Logs use the target `save-migrate` (as 3b's `Restamp` lines do).
- The player sees nothing when reconcile succeeds (spec 3d).
- `story.json` is keyed only by the games' identities (resrefs, tags, GIT
  order, global names), never `SimId`; its `schema` only grows.
- The story restore never writes over the slot (spec 4b step 4).
- Before done: `cargo clippy --workspace -- -D warnings`, the approved
  tests (`cargo test -p approved_tests`), and `kjk chain k1 --mute`
  (29/29) for 3c. New approved tests need the owner's OK by name.
- House style: comments say what is, in the codebase's voice; no session
  narration.

## Decisions from the survey (deviations from the spec, on purpose)

1. **Origin keys live in a roster, not on each entity.** An object's
   number (`Kind::Object(kind, n)`) is already its place in the module's
   placements in GIT order, then what play made (`crate::instance`). So
   the snapshot and each `Stashed` keep a `roster: Option<Vec<Origin>>`,
   the placements as installed, and loading maps old numbers to new ones.
   No per-entity field.
2. **No separate component digests.** Reconcile runs every check whenever
   the combined digest moved (3b already detects that). The checks are
   cheap, and running them all is what the spec does for old saves anyway.
3. **Saves without a roster (every save before 3c) are trusted as today**
   (logged once), instead of matching by tag and position: entities carry
   no tag to match by. Their numbers are checked only for kind: an entity
   whose number now names a placement of another kind is dropped (logged).

## Review Focus

1. A placement inserted early in the GIT: every later placement and every
   object made in play shifts by one; script locals, door states and lock
   states must follow their objects, not their numbers.
2. A save made in the middle of a `DelayCommand` cutscene whose script
   changed: must stop and offer the restore, never resume.
3. A party member standing where the walkmesh moved: lands within 2 m,
   never inside a wall or falling.
4. A slot with `save.kjkv` but no `story.json` (every save before part 4)
   whose load fails: the old refusal text, no restore button.
5. A corpus save from a release before a `breaking` bump: `saves check`
   reports it as expected-refused, not failed.

---

## Part 3c — reconcile at load (PR 1, branch `claude/save-reconcile`)

### Task 1: The roster and the number map

**Files:**
- Create: `crates/sim/src/reconcile.rs` (+ `mod reconcile;` in `crates/sim/src/lib.rs`)
- Modify: `crates/sim/src/snapshot.rs` (`Snapshot`, `Stashed`: field, encode, decode)
- Modify: `crates/sim/src/install.rs` (record the roster at first install and at stash)
- Create: `bumps/claude-save-reconcile.toml`

**Interfaces:**
- Produces:
  - `pub struct Origin { pub kind: ObjectKind, pub template: String, pub tag: String }`
  - `pub fn roster(placed: &[Arc<ObjectData>]) -> Vec<Origin>`
  - `pub struct Renumber { pub map: Vec<Option<u32>>, pub made_shift: i64, pub added: Vec<u32> }`
    — `map[old]` is the new number of old placement `old` (`None`: gone);
    an object made in play numbered `old >= map.len()` becomes
    `old + made_shift`; `added` are new placements absent from the old roster.
  - `pub fn renumber(old: &[Origin], new: &[Arc<ObjectData>]) -> Renumber`
  - `Snapshot::roster` and `Stashed::roster`: `Option<Vec<Origin>>`

- [ ] **Step 1: Tests in `reconcile.rs`**, one `#[test]` table over cases
  (old roster, new placements → expected `Renumber`): unchanged → identity,
  `made_shift` 0; one inserted at index 2 of 5 → `map = [0,1,3,4,5]`,
  `made_shift` 1, `added = [2]`; one removed at index 1 → `map[1] = None`,
  later ones minus one, `made_shift` -1; two with the same key swapped in
  order → matched by occurrence (n-th of a key ↔ n-th), so identity.
- [ ] **Step 2: Run** `cargo test -p sim reconcile` — fails (not defined).
- [ ] **Step 3: Implement** `renumber`: key = (kind, lowercase template,
  lowercase tag); the n-th occurrence of a key in `old` maps to its n-th in
  `new`. `roster` reads `ObjectData::{kind, template, tag}`.
- [ ] **Step 4: Run** the tests — pass.
- [ ] **Step 5: Store it.** `Snapshot` and `Stashed` gain `roster`; set at
  the install that first plans a module (`roster(level.placed())`) and
  carried into the stash on leaving. Encode after the existing fields;
  decode gated `since(SAVE_RECONCILE)`, old reads `None`. Bump file:
  `[formats] snapshot = "the snapshot and each module left keep the
  placements' roster as installed (`Snapshot::roster`)."`, and
  `[migrations] snapshot = "old saves read no roster: trusted as before"`.
- [ ] **Step 6: Run** `cargo test -p sim` and the save fixtures test
  (`cargo test -p approved_tests old_saves_load`) — pass.
- [ ] **Step 7: Commit.**

### Task 2: Apply the map when a save loads and when a left module is entered

**Files:**
- Modify: `crates/sim/src/reconcile.rs`
- Modify: `crates/sim/src/save.rs` (`Save::world`)
- Modify: `crates/sim/src/install.rs` (the `StashChanged` re-stamp path, ~line 366)
- Test: probes in `crates/sim/src/save.rs`'s tests or `crates/saves/src/probes.rs`, whichever holds the 3b `stale_stamps_load` cases

**Interfaces:**
- Consumes: Task 1's `renumber`, `Renumber`, `Origin`.
- Produces: `pub fn apply(renumber: &Renumber, entities: &mut Vec<EntityState>, script: &mut script::State, party: &BTreeSet<SimId>) -> Report`
  and `pub struct Report { pub moved: usize, pub dropped: Vec<String>, pub added: usize, pub snapped: usize }`
  (`Display`s as one `save-migrate:` line).

- [ ] **Step 1: Find every holder of an object number.** Grep for
  `Kind::Object(` and for where script locals, door/lock state, `last`
  (opener, damager, killer) and dialogue owners key objects. List them in
  the module doc of `reconcile.rs`; `apply` rewrites each. If script
  `ObjectId`s are `SimId`s (not instance numbers), only `Kind::Object`
  numbers move — confirm and say so in the doc.
- [ ] **Step 2: Tests** on the test level (the one 3b's probes use), one
  per rule from spec "How we know it works": a GIT entry inserted (door's
  open state and a local follow it); a GIT entry removed (its entity
  dropped, logged; a party member standing for it kept); a new GIT entry
  (installed as on first arrival). Each asserts the `Report` too.
- [ ] **Step 3: Run** — fail.
- [ ] **Step 4: Implement** `apply`; call it from `Save::world` after
  `restamp` returns `Some` and the snapshot has a roster, and from the
  stash path when `Restamp::of` matched. New placements (`added`) get
  their entities as the first install makes them (reuse install's
  per-placement fn; don't copy it). Then store `roster(level.placed())`.
  Without a roster: the kind check from decision 3.
- [ ] **Step 5: Run** — pass; `cargo test -p sim`.
- [ ] **Step 6: Commit.**

### Task 3: Bodies off the walkmesh

**Files:**
- Modify: `crates/sim/src/reconcile.rs`, `crates/sim/src/save.rs`, `crates/sim/src/install.rs`

**Interfaces:**
- Produces: `pub fn onto_ground(level: &Level, entities: &mut [EntityState]) -> usize` (count moved)

- [ ] **Step 1: Test:** a creature and the player 1 m off the walkable mesh
  → moved to the nearest walkable point; one 5 m off → to the module's
  nearest waypoint; one on the mesh → untouched (bit-equal position).
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement** with the nav mesh's nearest-point query (use the
  one the movement code already has; `docs/NAV.md`). 2 m is
  `rules` data if the rulebook has a save section, else a `const
  SNAP_M: f32 = 2.0` with a comment naming spec 3c.
  Characters only (avatars, creatures); run where Task 2's `apply` runs.
- [ ] **Step 4: Run** — pass. **Step 5: Commit.**

### Task 4: Deferred code into changed scripts

**Files:**
- Modify: `crates/sim/src/script/state.rs` (`Delayed`, `Action::Code` holders: `code: Option<[u8; 32]>`)
- Modify: `crates/sim/src/script/mod.rs` (`Library::program` gains a digest per program, computed once at load)
- Modify: `crates/sim/src/reconcile.rs`, `crates/sim/src/save.rs`, `crates/sim/src/install.rs` (`Error`)
- Modify: `bumps/claude-save-reconcile.toml` (`save` format line + migration)

**Interfaces:**
- Produces:
  - `Program::digest(&self) -> [u8; 32]` (BLAKE3 of its bytes)
  - `pub fn stale_code(library: &Library, script: &script::State) -> Vec<String>` — the scripts whose saved continuations no longer match
  - `InstallError::CodeChanged { scripts: Vec<String> }`, refusal text: "the save was made while <script> was running, and that script changed since"

- [ ] **Step 1: Test:** a save with a `Delayed` whose `code` is another
  digest than its program's → `Save::world` is `Err` naming the script;
  with `code: None` (old save) → loads; same digest → loads.
- [ ] **Step 2: Run** — fail.
- [ ] **Step 3: Implement.** Set `code` wherever a `Delayed` or a queued
  `Action::Code` is made. Encode/decode gated `since(SAVE_RECONCILE)`;
  bump: `[formats] save = "deferred code keeps its script's digest
  (`Delayed::code`)."`, `[migrations] save = "old saves read `code =
  None`: trusted as before"`. Check in `Save::world` and the stash path
  before anything else.
- [ ] **Step 4: Run** — pass; `cargo test -p sim`; `old_saves_load`.
- [ ] **Step 5: Commit.**

### Task 5: Prove it on the real game, open PR 1

- [ ] **Step 1:** `kjk chain k1 --mute` — 29/29 (the bumps make the
  chain re-run its checkpoints).
- [ ] **Step 2:** Load an owner save and a kept checkpoint from before the
  branch (`kjk-artifacts/checkpoints/k1/*.kjkv`) on the branch build:
  `kjk.log` shows one `save-migrate:` line each, play goes on.
- [ ] **Step 3:** Hand-made inserted placement: patch one module's GIT in a
  scratch override (an extra placeable at index 0), load a save in it, check
  doors/containers kept their states (screenshot before/after).
- [ ] **Step 4:** clippy, approved tests; propose the new approved tests to
  the owner by name; update the spec's 3c status line; open the PR.

## Part 4 — story record and restore (PR 2, branch `claude/story-restore`, after PR 1 merges)

### Task 6: `Story` and its writer

**Files:**
- Create: `crates/sim/src/story.rs` (`Story` and `Story::of`)
- Modify: `crates/saves/src/lib.rs` (`write_files` writes `story.json` in the same atomic write; `meta.json` `formats` gains `story`)

**Interfaces:**
- Produces:
  - `#[derive(Serialize, Deserialize)] pub struct Story` with the spec 4a
    fields: `schema: u32` (`STORY_SCHEMA = 1`), `game`, `build`, `saved`,
    `place: StoryPlace { module, position, facing, waypoint }`, `globals`,
    `party: Vec<StoryCharacter>`, `items`, `journal`, `galaxy`,
    `reputation`, `unescapable`, `transit`, `visited: BTreeMap<String, StoryModule>`.
    Unknown fields ignored (`#[serde(default)]` on all).
  - `StoryModule { spawns_ran: bool, destroyed: Vec<OriginKey>, doors: Vec<(OriginKey, u8, u8)>, locals: Vec<(OriginKey, u8, u32, i32)> }`
    (a local: its object, 0 boolean / 1 number as `State`'s key, index, value)
    with `OriginKey { index: u32, template: String }` (index in GIT order, from the roster).
  - `pub fn of(save: &Save, level: &Level) -> Story`
  - `saves::read_story(root: &Path, id: &str) -> Option<Story>`

- [ ] **Step 1: Test:** `Story::of` on the kitchen-sink fixture save
  (part 2a's generator) — party, items, journal, globals by name; a JSON
  round trip is equal.
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** A visited module
  without a roster (old save) gets `spawns_ran` and globals only.
- [ ] **Step 4: Run** — pass. **Step 5: Commit.**

### Task 7: Story restore

**Files:**
- Modify: `crates/sim/src/story.rs` (`Story::restore`)
- Modify: the game's load path in `crates/game` (where a load's `Err` becomes the refusal text today; grep `safety`'s refusal)

**Interfaces:**
- Consumes: Task 6's `Story`.
- Produces: `pub fn restore(&self, level: &Level, seats: u8) -> Result<(SimWorld, Vec<String>), String>` (the world and what was lost, for the log)

- [ ] **Step 1: Round-trip test (spec 4c):** for each fixture save,
  `Story::of` → `restore` → `Story::of` again: equal ignoring `build`,
  `saved`, and `place.position` within the 2 m snap.
- [ ] **Step 2: Run** — fail. **Step 3: Implement** spec 4b steps 1-3:
  fresh install of `place.module` as an arrival, party at the position or
  the waypoint, then globals, party sheets, items, journal, galaxy,
  reputation; visited modules become stash entries a later install
  applies (destroyed stay gone, doors and locks, locals).
- [ ] **Step 4: Run** — pass.
- [ ] **Step 5: The prompt.** When a load fails and `read_story` is
  `Some`, the load screen shows the spec 4b text with the save's
  `meta.json` version and build, **Restore** / **Cancel** (an existing
  message-box layout; pad: Menu accepts). No `story.json`: today's
  refusal unchanged (Review Focus 4). Screenshot both.
- [ ] **Step 6: Commit;** clippy, approved tests; open PR 2.

## Part 2b — real saves from every release (PR 3, branch `claude/save-corpus`, after PR 2)

### Task 8: `kjk saves capture` and `kjk saves check`

**Files:**
- Create: `crates/launcher/src/saves_verbs.rs`
- Modify: `crates/launcher/src/lib.rs` (a `Saves` verb group beside `Store`, `Report`)
- Modify: `docs/RELEASE.md` (checklist: capture, check, mirror to the private archive)

**Interfaces:**
- Consumes: `saves::write_encoded`, `Save::decode`, `Save::world`, `Story::restore`.
- Produces:
  - `kjk saves capture <tag> [--game k1|tsl] [--from <slot or .kjkv>...]` →
    `kjk-artifacts/save-corpus/<tag>/<name>/` slot folders; default
    sources: the chain checkpoints named in `data/routes/<game>/` for
    Endar Spire, Taris, mid-Korriban (K1) and Peragus (TSL), from
    `kjk-artifacts/checkpoints/<game>/`.
  - `kjk saves check [<corpus>] [--mute]` → per save: formats migrated,
    stamps moved, `Report` counts, 300 headless ticks, re-entering one
    left module, story restore round trip; exit non-zero on any failure.
    A save refused because its release predates a `breaking` bump
    (`bumps/BREAKING.md`) is "expected", not a failure (Review Focus 5).

- [ ] **Step 1: Test** the verb's table printer and the "expected"
  classification on two synthetic slot folders (one current, one
  older than a fake `breaking` entry).
- [ ] **Step 2: Run** — fail. **Step 3: Implement.** **Step 4: Run** — pass.
- [ ] **Step 5: Seed the corpus.** Build tag `v0.5.0` in a scratch
  worktree, write its checkpoints (`kjk chain k1 --mute` from that tag
  only up to Korriban if the route allows it, else the whole chain),
  `saves capture v0.5.0 --from` them; also the owner's current save if
  they agree. Then `kjk saves check` on main: all pass. Paste the table
  into the PR.
- [ ] **Step 6: Commit;** clippy; open PR 3.
