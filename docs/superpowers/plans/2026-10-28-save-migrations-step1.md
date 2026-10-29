# Save migrations, step 1: old formats read in-tree — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A save made by v0.1.0 or any later build decodes on main, every
bump to a stored format says how old saves are read, and CI proves it on
every PR with synthetic saves.

**Architecture:** `wire::Reader` learns the version it found
(`header_since`), and decoders gate each change on a named version
(`crate::since::<format>::<BUMP_NAME>`). The sim's build script names a
pending bump's version, and `xtask pin` makes it permanent in `since.rs`.
`crates/bumps` refuses a stored-format bump without `[migrations]` or
`breaking`. A code-built *kitchen-sink* save is written as a fixture at every
pin, and an approved test loads every fixture. The save's own first migration
records its pack version, so companions' templates migrate too.

**Tech Stack:** Rust (sim, bumps, xtask, approved_tests), no new
dependencies.

**Spec:** `docs/superpowers/specs/2026-10-28-save-migrations-design.md`
(parts 1 and 2a; parts 3, 4, 2b and 5 get their own plans).

## Global Constraints

- Oldest supported versions: KJKV `59`, KJKS `72`, KJKT `20` (the v0.1.0
  values).
- Formats that need migrations: `save`, `snapshot`, `pack`. Not `install`,
  `replay` or `gathered`.
- `encode` only ever writes the current layout. Loading never rewrites a
  slot.
- No change to the sim hash. The bump file of this work names `save` only,
  with no `sim` line.
- Don't touch `crates/saves` (the safety-net work is in flight there).
- Permanent tests live only in `crates/approved_tests` (AGENT.md). The two
  new approved tests, `old_saves_load` and `bump_files`, are named here for
  the owner's approval. Add their entries to `docs/TESTS.md`.
- No `.kjkv` from a real game goes into the repo. Fixtures are code-built
  only.
- The gate before every commit: `cargo fmt --all -- --check && cargo clippy
  -q --workspace --all-targets -- -D warnings && cargo test --release -p
  approved_tests`.
- Commits: TZ=UTC, no attribution lines, squash-merged later.

## Review Focus

1. **A save from a newer build** (a downgrade) must be refused with
   `DecodeError::Version`, never decoded wrongly or panicking. Task 3 pins
   it.
2. **A truncated or damaged old save** must give a `DecodeError`, not a
   panic. Task 3 pins it.
3. **A companion template that won't decode** must fail the load with an
   error naming the kept companion, not silently drop the companion (today
   `creature_of` hides the error). Task 3 pins it.
4. **The network snapshot path keeps exact versions.** A joiner on another
   build is refused as today, since only stored saves accept ranges. Task 3
   pins it.
5. **Two pending bump files for one format** get distinct, ordered `since`
   constants, and a bump file's mistakes name the file. Task 2 pins it.

---

### Task 1: The kitchen-sink save, its first fixture, and `old_saves_load`

**Files:**
- Create: `crates/sim/src/fixtures.rs` (`pub mod fixtures;` in
  `crates/sim/src/lib.rs`)
- Create: `crates/approved_tests/src/bin/save_fixture.rs`
- Create: `crates/approved_tests/saves/save-59-72-20.kjkv` (written by the bin)
- Modify: `crates/approved_tests/tests/approved.rs` (new test)
- Modify: `docs/TESTS.md` (entry for `old_saves_load`)

**Interfaces:**
- Produces:
  - `sim::fixtures::kitchen_sink() -> (sim::Level, sim::Save)`: the
    proving ground (`determinism::proving_ground()`), run 600 ticks of
    `determinism::pilot` input with 2 seats, saved. Then every field of
    `Snapshot` and `script::State` that the run left at its default is set
    by hand to a plausible non-default value: a global of each kind, a
    journal entry, a party member with `template_data`
    (`pack::creature_bytes` of one of the ground's creatures), a store's
    goods, a pazaak table, fog, and so on.
  - `sim::fixtures::Versions { save: u32, snapshot: u32, pack: u32 }` and
    `sim::fixtures::current() -> Versions`, which reads the three
    `VERSION` constants.
  - `sim::fixtures::file_name(v: Versions) -> String`, giving
    `"save-{save}-{snapshot}-{pack}.kjkv"`.
  - `sim::fixtures::DIR: &str = "crates/approved_tests/saves"` (from the
    workspace root).
  - `sim::fixtures::restamp(save: &mut Save, level: &Level)`: sets
    `save.level` and `snapshot.place.digest` to the level's. Test-only
    until part 3 re-stamps at load. The doc comment says so.
  - The bin `save_fixture` writes `kitchen_sink().1.encode()` to
    `<workspace>/DIR/file_name(current())` and prints the path.

- [ ] **Step 1: Write the failing test** in `approved.rs`:

```rust
/// Every fixture under `crates/approved_tests/saves` (a code-built save from
/// each pin that moved a stored format) loads on this build: decoded,
/// re-encoded and decoded the same, and made a world on the proving
/// ground (docs/superpowers/specs/2026-10-28-save-migrations-design.md 2a).
#[test]
fn old_saves_load() {
    let dir = approved_tests::workspace().join(sim::fixtures::DIR);
    let (level, _) = sim::fixtures::kitchen_sink();
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the fixtures folder") {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "kjkv") { continue; }
        let bytes = std::fs::read(&path).unwrap();
        let mut save = sim::Save::decode(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let again = sim::Save::decode(&save.encode()).unwrap();
        assert_eq!(again, save, "{}: re-encoding changes it", path.display());
        sim::fixtures::restamp(&mut save, &level);
        save.world(&level, 2, None)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        seen += 1;
    }
    assert!(seen > 0, "no fixtures in {}", dir.display());
    let newest = dir.join(sim::fixtures::file_name(sim::fixtures::current()));
    assert!(
        newest.exists() || approved_tests::stored_bumps_pending(),
        "no fixture at this build's versions: run `cargo run --release -p approved_tests --bin save_fixture`"
    );
    // Fixtures from before a `breaking` bump are refused, by name.
    for entry in std::fs::read_dir(dir.join("retired")).into_iter().flatten() {
        let path = entry.unwrap().path();
        let error = sim::Save::decode(&std::fs::read(&path).unwrap()).unwrap_err();
        assert!(error.to_string().contains("version"), "{}: {error}", path.display());
    }
}
```

`approved_tests::stored_bumps_pending() -> bool`, in
`approved_tests/src/lib.rs` beside `hash_left_to_merge`, says whether any
pending bump file names `save`, `snapshot` or `pack`. A branch with such a
bump is at main + 1, and its fixture is written at pin.

- [ ] **Step 2: Run it to make sure it fails.**
  Run: `cargo test --release -p approved_tests old_saves_load`
  Expected: compile error, no `sim::fixtures`.

- [ ] **Step 3: Implement `fixtures.rs`, the bin and `stored_bumps_pending`.**
  Inside `kitchen_sink`, guard against fields being missed: destructure
  `Snapshot` and `script::State` exhaustively (no `..`), so a new field
  fails to compile here until someone gives it a value. Then
  `debug_assert!` that each field differs from its `Default`. A field
  without `Default` gets a comment saying how it is set.

- [ ] **Step 4: Write the fixture.**
  Run: `cargo run --release -p approved_tests --bin save_fixture`
  Expected: it prints `crates/approved_tests/saves/save-59-72-20.kjkv`.

- [ ] **Step 5: Run the test.**
  Run: `cargo test --release -p approved_tests old_saves_load`
  Expected: PASS.

- [ ] **Step 6: Add `old_saves_load` to `docs/TESTS.md`**, in the style of
  the entries there. Run the gate, then commit:

```bash
git add crates/sim/src/fixtures.rs crates/sim/src/lib.rs crates/approved_tests/saves crates/approved_tests docs/TESTS.md
git commit -m "A code-built save from every format version, and the test that loads them all"
```

---

### Task 2: Bump files state their migration; named pending versions

**Files:**
- Modify: `crates/bumps/src/lib.rs`
- Modify: `crates/sim/build.rs`
- Create: `crates/sim/src/since.rs` (`pub mod since;` in `lib.rs`)
- Modify: `crates/approved_tests/tests/approved.rs` (new test `bump_files`)
- Modify: `bumps/README.md`, `docs/TESTS.md`

**Interfaces:**
- Produces, in `bumps`:
  - `Format.stored: bool`: true for `save`, `snapshot` and `pack`.
  - `Bump.migrations: Vec<(&'static str, String)>`, `Bump.breaking:
    Option<String>`, and `Bump::name(&self) -> String`. The name is the
    file stem uppercased, with `-` and `.` becoming `_`
    (`saber-duel-fixes.toml` gives `SABER_DUEL_FIXES`).
  - `pub fn parse_file(name: &str, text: &str) -> Result<Bump, String>`.
    `pending` uses it, and the test calls it.
  - `pub fn base(root: &Path, format: &Format) -> Result<u32, String>`:
    the `N` of `const <constant>: u32 = N + crate::pending::<NAME>;`. Move
    that line's parsing out of `xtask/src/pin.rs` (`bump_version`) into
    here, and have pin call it.
  - `pub fn pending_since(bumps: &[Bump], format: &str) -> Vec<(String,
    u32)>`: for each bump naming `format`, in `pending`'s order (sorted by
    path), its name and its 1-based place.
- Produces, in `sim`:
  - `crate::pending::since::{save,snapshot,pack}::<NAME>: u32 = base +
    place`. `build.rs` always emits all three modules, even when they are
    empty.
  - `crate::since::{save,snapshot,pack}`: modules holding pinned constants
    (none yet), each ending in `pub use crate::pending::since::<format>::*;`.
    Decoders only ever name `crate::since::…`.

Parse rules (in `parse_file`):
- A `[migrations]` table takes stored-format keys only.
- A top-level `breaking = "…"` is allowed.
- Every stored format in `[formats]` needs a `[migrations]` entry, or the
  file needs `breaking`.
- A `[migrations]` key not in `[formats]` is an error.
- Errors use the existing `line N: …` / `<path>: …` shapes.

- [ ] **Step 1: Write the failing test** `bump_files` in `approved.rs`,
  one table of cases checked in a loop:

```rust
/// Bump files say how old saves are read (bumps/README.md): a stored
/// format bumped without `[migrations]` or `breaking` is refused, and
/// pending bumps of one format get ordered named versions.
#[test]
fn bump_files() {
    let cases: &[(&str, &str, Result<(), &str>)] = &[
        ("ok.toml", "[formats]\nsnapshot = \"x\"\n[migrations]\nsnapshot = \"old: none\"\n", Ok(())),
        ("breaks.toml", "breaking = \"why\"\n[formats]\nsave = \"x\"\n", Ok(())),
        ("net.toml", "[formats]\ninstall = \"x\"\n", Ok(())),
        ("bare.toml", "[formats]\npack = \"x\"\n", Err("pack: say how old saves are read")),
        ("stray.toml", "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\nsnapshot = \"z\"\n", Err("snapshot is not bumped")),
    ];
    for (name, text, expected) in cases {
        let got = bumps::parse_file(name, text).map(|_| ());
        match expected {
            Ok(()) => assert!(got.is_ok(), "{name}: {got:?}"),
            Err(part) => assert!(got.as_ref().is_err_and(|e| e.contains(part)), "{name}: {got:?}"),
        }
    }
    let two = [
        bumps::parse_file("b-later.toml", "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\n").unwrap(),
        bumps::parse_file("a-first.toml", "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\n").unwrap(),
    ];
    let mut sorted = two.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(bumps::pending_since(&sorted, "save"), vec![("A_FIRST".into(), 1), ("B_LATER".into(), 2)]);
}
```

- [ ] **Step 2: Run it.** `cargo test --release -p approved_tests bump_files`.
  Expected: compile error (`parse_file`, `pending_since`).

- [ ] **Step 3: Implement** the `bumps` changes, `build.rs` (calls
  `bumps::base` for each stored format and writes the `since` modules
  beside the counts), `since.rs`, and pin's use of `bumps::base`.

- [ ] **Step 4: Run it again.** Same command. Expected: PASS. Also check
  `cargo build -p sim` with a scratch `bumps/zz-try.toml` that bumps
  `pack` without a migration: the build fails, naming the file. Delete the
  scratch file.

- [ ] **Step 5: Write a "Reading old saves" section in `bumps/README.md`**:
  - the `[migrations]` and `breaking` keys, with the example from spec 1e;
  - how a decoder gates a change (`if input.since(crate::since::snapshot::NAME)`);
  - the three kinds of change from spec 1b (added, removed, meaning
    changed, the last through `crate::migrate`);
  - that `since` names come from the bump file's name.

  Add `bump_files` to `docs/TESTS.md`. Run the gate, then commit
  (`Bump files say how old saves are read`).

---

### Task 3: Reading a range of versions, and the save's pack version

This is the first real migration, KJKV 59 → 60. It proves the machinery on
Task 1's fixture.

**Files:**
- Modify: `crates/sim/src/wire.rs:141-174` (Reader)
- Modify: `crates/sim/src/save.rs:108`, `:171-207`
- Modify: `crates/sim/src/snapshot.rs:176`, `:1674-1683`
- Modify: `crates/sim/src/pack.rs:37`, `:73-75`, `:228-230`
- Modify: `crates/sim/src/install.rs:177`, `crates/sim/src/script/mod.rs:3400`
  (callers of `creature_of`)
- Create: `crates/sim/src/migrate.rs` (`mod migrate;`)
- Create: `bumps/save-migrations.toml`
- Modify: `crates/approved_tests/tests/approved.rs` (extend
  `old_saves_load`)
- Modify: `.claude/skills/save-compat/SKILL.md`

**Interfaces:**
- Consumes: `crate::since::save::SAVE_MIGRATIONS` (from the bump file,
  through Task 2).
- Produces:
  - `Reader::header_since(&mut self, magic: &[u8; 4], oldest: u32,
    current: u32) -> Result<u32, DecodeError>`. Outside the range it
    returns `DecodeError::Version` as today.
  - `Reader::since(&self, version: u32) -> bool` and
    `Reader::with_version(self, version: u32) -> Self`. `Reader::new`'s
    version is `u32::MAX` (current layout), and `header` sets the version it
    found.
  - `const OLDEST: u32` next to `VERSION` in `save.rs` (59), `snapshot.rs`
    (72) and `pack.rs` (20).
  - `Snapshot::decode_stored(bytes) -> Result<Snapshot, DecodeError>`
    accepts `OLDEST..=VERSION`. `Snapshot::decode` and the `Parts` path stay
    exact, for the network.
  - `Save::decode` accepts `OLDEST..=VERSION`, decodes its snapshot with
    `decode_stored`, and passes its version to the scripts' reader
    (`with_version`). After `SAVE_MIGRATIONS` it reads
    `pack: u32` right after `hz`; before it, `pack = 20`. `encode` writes
    `pack::VERSION` there.
  - `pack::creature_of(bytes: &[u8], version: u32) -> Result<ObjectData,
    DecodeError>`, under the reader name `"a kept companion"`. Its two
    callers keep their `Option` flow with `.ok()?` at the call site, since a
    load has already proven the bytes (below).
  - `migrate::save(save: &mut Save, pack: u32) -> Result<(), DecodeError>`:
    re-encodes every `party.available[..].template_data` from `pack` to the
    current layout (`creature_of(bytes, pack)` then `creature_bytes`), so
    the rest of the sim only ever sees current bytes. It runs at the end of
    `Save::decode`. That's where a template that won't decode fails the load
    (Review Focus 3).
  - `bumps/save-migrations.toml`:

```toml
[formats]
save = "a save records the pack version its companions' templates were written in (`Save` header), so a pack bump migrates them."

[migrations]
save = "saves before it read pack version 20, v0.1.0's"
```

- [ ] **Step 1: Write the failing test.** Make `old_saves_load` a loop
  over the cases below. Each case is a name, a byte change to
  `save-59-72-20.kjkv`, and the expected outcome:
  - the fixture as it is: loads (it already loops over all fixtures; that
    is the first case);
  - KJKV patched to `VERSION + 1` (bytes 4..8): `Err` whose text contains
    `version`;
  - truncated to half its length: `Err`, no panic;
  - the first kept companion's `template_data` replaced by `[0xff; 3]`:
    `Err` whose text contains `a kept companion`. Build this case by
    decoding with the current code, editing, and re-encoding.
  - a snapshot at KJKS `OLDEST` passed to `Snapshot::decode` (the network
    path) while `VERSION` is ahead: refused. Skip it while the two are
    equal, with a comment saying so.

- [ ] **Step 2: Run it.**
  `cargo test --release -p approved_tests old_saves_load`. Expected: the
  fixture case fails with `save: version 59` once the bump file exists, and
  the new cases fail on compile.

- [ ] **Step 3: Implement** the Reader changes, `OLDEST`, `decode_stored`,
  the save's pack field, `creature_of`'s `Result`, and `migrate::save`.

- [ ] **Step 4: Run the test.** Same command. Expected: PASS.

- [ ] **Step 5: Run the determinism test.**
  `cargo test --release -p approved_tests sim_determinism`. Expected: PASS
  with the pinned hash unchanged. The bump file names no `snapshot` and no
  `sim`.

- [ ] **Step 6: Cut the save-compat skill down.** Step 2 ("Convert the
  format") becomes: "Saves from v0.1.0 on decode on main. If one doesn't,
  that's a bug in a migration: report it with the save's versions." Keep
  step 3 (re-stamp) until part 3 lands. Run the gate, then commit
  (`Saves from v0.1.0 on decode: version-gated reads, and the save's pack
  version`).

---

### Task 4: `xtask pin` makes named versions permanent and writes the fixture

**Files:**
- Modify: `xtask/src/pin.rs`
- Create: `bumps/BREAKING.md` (a heading and one line saying what goes
  there)
- Modify: `.github/workflows/pin.yml` (stage the new fixture)

**Interfaces:**
- Consumes: `bumps::{base, pending_since, Bump::name, Format::stored}` and
  `Bump.{migrations, breaking}` from Task 2, the `save_fixture` bin from Task 1,
  and the `OLDEST` constants from Task 3.
- Produces: after a pin,
  - `since.rs` holds `pub const <NAME>: u32 = <number>;` in each format's
    module, inserted before its `pub use` line;
  - the version history line reads `/// N: <formats text> Old saves:
    <migration text>.`;
  - a `breaking` bump appends `- <tag> N (<file>): <text>` to
    `bumps/BREAKING.md` and sets that format's `OLDEST` constant to `N`;
  - if any stored format moved, `crates/approved_tests/saves/<new name>` is
    written by `save_fixture` and `git add`ed.

- [ ] **Step 1: Change `bump_version`** to return the version, then
  record `(format, bump.name(), version)` for stored formats. Write
  `since.rs` once all bumps are numbered. Add the migration text to the
  history line.

- [ ] **Step 2: Handle `breaking`**: append to `BREAKING.md`, and rewrite
  `const OLDEST: u32 = <n>;` in the format's file (Task 3's constants).
  Move every fixture under `crates/approved_tests/saves/` whose version of that
  format is below `N` into `crates/approved_tests/saves/retired/` (`git mv`).
  `old_saves_load` then checks those are refused.

- [ ] **Step 3: Run `save_fixture`** after `sim_hash` (the bumps are
  applied by then), then `git add` its path. In `pin.yml`, the commit step
  already uses `git commit -qa`, so add `git add crates/approved_tests/saves`
  before it.

- [ ] **Step 4: Dry-run in a scratch worktree.**
  1. `git worktree add --detach <scratchpad>/pin-try HEAD`, with
     `CARGO_TARGET_DIR=<scratchpad>/pin-try-target`.
  2. Add `bumps/try-it.toml` naming `save`, with a migration line.
  3. Gate a dummy read on `crate::since::save::TRY_IT` somewhere harmless,
     then run `cargo xtask pin`.
  4. Check that:
     - `since.rs` has `TRY_IT = 61` (or main + 1);
     - the history line carries "Old saves:";
     - `crates/approved_tests/saves/save-61-72-20.kjkv` exists and is staged;
     - `cargo test --release -p approved_tests old_saves_load` passes.
  5. Remove the worktree and its target dir.

- [ ] **Step 5: Run the gate, then commit**
  (`The pin numbers named versions and writes the format fixture`).

---

## After this plan

Open one PR from `claude/save-migrations` titled "Save migrations, step 1:
old formats read in-tree". Its description lists the two new approved tests
for the owner's approval by name. The pin at merge numbers
`SAVE_MIGRATIONS` as 60 and writes `save-60-72-20.kjkv`. From then on, CI
loads both fixtures on every PR.
