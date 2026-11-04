# kjk save edit API Implementation Plan (plan 1 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A public, Bevy-free crate `edit` in kjk that opens a save (refusing newer formats by name), lists its characters, applies each editor change through kjk's own rules, and checks an edited save by loading and stepping it. The save editor app (plan 2, `crates/save_editor` in this repo) is built on it.

**Architecture:** `crates/edit` depends on `sim`, `session`, `saves`, `vfs`, `asset_odyssey`, `store`. Edits write the **authority**, `save.script: script::State` (items, gold, journal, xp, globals, party), and the entity's `Sheet`/`Fighter` in `save.snapshot`. The snapshot's `goods`/`progress` views are re-gathered by kjk on the first step, so they're never written. Party changes need creature creation, which is private engine code (`make_npc`), so they're queued as NWScript calls that run when the save loads, the way `launcher::probe::Probe::queue` does today. Each piece kjk keeps private gets a narrow `pub` entry point next to it, never a copy.

**Tech Stack:** Rust (workspace edition), kjk crates, approved tests (`crates/approved_tests`), `sim::fixtures::kitchen_sink()` for saves without game data.

**Spec:** `docs/superpowers/specs/2026-11-04-save-editor-design.md`

## Global Constraints

- No `game` / Bevy renderer dependency in `edit`, `saves`, `store`, `session` (`bevy_ecs` via `sim` is fine).
- Never reimplement a kjk rule in `edit`: call the rule where it lives, making it `pub` if needed.
- Read saves from `sim::SAVE_OLDEST` up to `sim::SAVE_FORMAT`. Refuse newer, by version number, with no partial result.
- Writes go through `saves` (temp folder + rename). `edit` never writes files itself.
- Every edit test round-trips: `Save::decode(&save.encode())` equals the edited save.
- This changes no save format and no sim hash. If a task finds it must, stop and ask (bump files, `docs/RELEASE.md`).
- Run `cargo clippy --workspace` and the approved tests before each commit (`docs/TESTS.md`). Doc comments follow the repo's plain voice.
- Work on branch `claude/save-edit-api` in a worktree. One PR, and plan 2 stacks on it.

## Review Focus

1. **A creature with no `sheet` in the snapshot** (its sheet is still the module's data, `module::ObjectData.sheet`). Editing it must work: take the module's sheet first, or refuse with "load this save in kjk and save once". Test in Task 4.
2. **An away companion** (in `Party.available[npc].kept`, not in the module). Its gear and sheet live in `Kept { sheet, worn }`. Every character edit must reach it too. Tests in Tasks 6 and 7.
3. **A modded item whose UTI fails to read, or a template missing from the install.** `add_item` returns `Err` naming the template, and nothing changes. Test in Task 5.
4. **Removing a feat whose `passive::learn` changed the sheet** (Toughness → HP, etc.). Removal must undo exactly that. Test in Task 7 over every feat row.
5. **Stepping an edited save panics or refuses.** `check` reports the error text, never unwinds into the editor. Test in Task 9.

---

### Task 1: Saves folder out of `game`

**Files:**
- Modify: `crates/store/src/lib.rs` (add `kjk_folder`)
- Modify: `crates/saves/src/lib.rs` (add `folder`), `crates/saves/Cargo.toml` (+ `store`)
- Modify: `crates/game/src/settings.rs:693` (`folder` calls `store::kjk_folder`), `crates/game/src/save_view.rs:55` (calls `saves::folder`)

**Interfaces:**
- Produces: `store::kjk_folder(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf>`: macOS `$HOME/Library/Application Support/kjk`, Windows `%APPDATA%\kjk`, else `$XDG_CONFIG_HOME/kjk` or `$HOME/.config/kjk`, then `carry_old_name`. `saves::folder(var: impl Fn(&str) -> Option<String>) -> Option<PathBuf>`: `KJK_SAVES`, else `kjk_folder(var)/saves`. The env is passed in so tests don't touch the process env.

- [ ] **Step 1:** Unit tests in `store`: `kjk_folder` with a fake `var` for each OS branch (`#[cfg(target_os)]` arms for the running OS), and with no `HOME` gives `None`. In `saves`: `KJK_SAVES` wins, else `<kjk_folder>/saves`.
- [ ] **Step 2:** Run `cargo test -p store -p saves` and expect FAIL (no functions).
- [ ] **Step 3:** Move the body of `game::settings::folder` into `store::kjk_folder`. `game` keeps its `pub(crate) fn folder()` as `store::kjk_folder(|k| std::env::var(k).ok())`, and `save_view::folder` calls `saves::folder`.
- [ ] **Step 4:** Run `cargo test -p store -p saves` and `cargo check -p kjk_game`, expect PASS.
- [ ] **Step 5:** Commit "The saves' folder is found by the saves crate, so tools without the game find it too".

### Task 2: The `edit` crate: open a save, or say why not

**Files:**
- Create: `crates/edit/Cargo.toml`, `crates/edit/src/lib.rs`, `crates/edit/src/open.rs`
- Modify: root `Cargo.toml` (member + `edit.workspace` dep), `crates/sim/src/lib.rs` (re-export `save::OLDEST` as `SAVE_OLDEST`)

**Interfaces:**
- Produces:
  ```rust
  pub enum Refusal {
      Newer { found: u32, reads_up_to: u32 },
      Older { found: u32, reads_from: u32 },
      Damaged(String),            // DecodeError's Display
      NotASave(String),           // missing save.kjkv / meta.json, io error
  }
  impl std::fmt::Display for Refusal // plain words, e.g. "made by a newer kjk (format 74); this reads up to 71"
  pub struct Opened { pub id: String, pub save: sim::Save, pub meta: saves::Meta }
  pub fn open(root: &Path, id: &str) -> Result<Opened, Refusal>
  pub fn open_bytes(bytes: &[u8]) -> Result<sim::Save, Refusal>
  ```
  `DecodeError::Version { found, .. }` maps to `Newer` when `found > SAVE_FORMAT`, else `Older`.

- [ ] **Step 1:** Tests in `crates/edit/src/open.rs`: `kitchen_sink()` save bytes → `Ok`. The same bytes with the version field set to `SAVE_FORMAT + 3` → `Refusal::Newer { found: SAVE_FORMAT + 3, reads_up_to: SAVE_FORMAT }`. Version `SAVE_OLDEST - 1` → `Older`. The bytes cut in half → `Damaged(_)`. An empty dir → `NotASave(_)`. `Display` of `Newer` contains both numbers. Use one `#[test]` table over cases, per the house rule on repetition.
- [ ] **Step 2:** Run `cargo test -p edit` and expect FAIL.
- [ ] **Step 3:** Implement with `saves::read` + `Save::decode`. Patch the version in tests by locating it the way `wire::header_since` reads it (right after `MAGIC`).
- [ ] **Step 4:** Run `cargo test -p edit`, expect PASS.
- [ ] **Step 5:** Commit "A save opens for editing, or is refused by its format version".

### Task 3: Game data for edits: every item and the classes

**Files:**
- Modify: `crates/session/src/items.rs` (pub entry), `crates/session/src/lib.rs` (re-export)
- Create: `crates/edit/src/data.rs`

**Interfaces:**
- Produces in `session`: `pub fn item_catalog(source: &dyn asset_odyssey::module::Source, templates: &[String]) -> (script::Catalog, Vec<(String, String)>)`, the catalog plus `(template, why)` for each UTI that failed. It wraps the private `items::facts` with a fresh `Tables`.
- Produces in `edit`:
  ```rust
  pub struct Data { pub catalog: Catalog, pub classes: levels::Classes, pub unreadable: Vec<(String, String)> }
  impl Data { pub fn load(discovery: &vfs::Discovery, game: vfs::Game) -> Result<Data, String> }
  ```
  Templates are every UTI name from `vfs::Resolver::names(ResType::UTI)` (`crates/vfs/src/resolver.rs:546`), override included. Classes come from `session::classes::classes(&source)`.

- [ ] **Step 1:** Approved test (needs an install, like the other install tests in `crates/approved_tests`): `Data::load` for K1 has `g_w_lghtsbr01` in the catalog with a weapon in its facts, and `classes` has the Jedi classes. A fake override UTI dropped in a temp override dir shows up by name.
- [ ] **Step 2:** Run it, expect FAIL.
- [ ] **Step 3:** Implement. Build the `Source` the same way `launcher::levels::Installers` reaches one, but without `game` (`vfs::Discovery` → resolver → `asset_odyssey` source).
- [ ] **Step 4:** Run it, expect PASS.
- [ ] **Step 5:** Commit "Tools can read every item and class of an install without the game".

### Task 4: Characters in a save

**Files:**
- Create: `crates/edit/src/characters.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Who { Player(Seat), Companion { npc: u8, id: SimId }, Away { npc: u8 } }
  pub struct Character { pub who: Who, pub template: String, pub sheet: Sheet, pub hp: i32, pub fp: i32 } // hp/fp in hundredths, as Fighter
  pub fn characters(save: &Save, level: Option<&sim::Level>) -> Vec<Character>
  ```
  The order is players by seat, then members in join order, then available-but-away by npc. A companion entity with `sheet: None` takes `ObjectData.sheet` from `level`. Without a level it's left out and marked by `pub fn unsheeted(save) -> Vec<u8>` (Review Focus 1).

- [ ] **Step 1:** Tests on `kitchen_sink()` after `fill()`: the player first, the companions `fill` made available appear as `Away`, and the count matches `party.available.len() + seats`. One case of a member entity with `sheet: None` and no level → listed by `unsheeted`.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "The characters of a save, here or away".

### Task 5: Credits, XP, journal, globals, inventory

**Files:**
- Create: `crates/edit/src/edits.rs`
- Modify: `crates/sim/src/script/journal.rs:114` (`set_quest` → `pub`), `crates/sim/src/script/xp.rs:193` (`give_xp` stays; add `pub fn set_xp(&mut self, holder: u32, xp: i32)`)

**Interfaces:**
- Produces, on `pub struct Editor<'a> { save: &'a mut Save, data: &'a Data }`:
  - `set_credits(i32)` writes `script.gold`
  - `set_party_xp(i32)` uses `set_xp(PARTY, _)`, and `set_xp(who: &Who, i32)` sets one character's. Levels aren't edited directly: with enough XP, the player levels up in kjk's own level-up screen, which applies every per-level rule (`Classes::take`). This is my call, and it keeps derived BAB, saves and HP right.
  - `set_quest(tag: &str, entry: i32)` uses `State::set_quest(tag, entry, true)`
  - `set_number/set_boolean/set_string(name, value)` use `script::state::set_global`
  - `add_item(holder: u32, template: &str, stack: u16) -> Result<u32, String>` uses `State::make_item(&data.catalog, ..)`
  - `remove_item(id: u32) -> Result<(), String>` (refuses a worn item: unequip first)
  - `set_stack(id, n: u16)`

  Each returns `Result<_, String>` with the reason in plain words.

- [ ] **Step 1:** One table test over edits on `kitchen_sink()` + a tiny in-test `Catalog` of two items: apply, round-trip, then build the world (`save.world(&level, 2, None)`), step once with `pilot`, and assert `snapshot.goods.gold`, `progress.journal`, `progress.xp` and the global show the edit. This proves the views re-gather. Also `add_item("no_such_item")` → `Err` containing the name, with the save unchanged (Review Focus 3).
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "Credits, XP, quests, globals and items edited through the scripts' state".

### Task 6: Equip and unequip

**Files:**
- Modify: `crates/edit/src/edits.rs`
- Modify: `crates/sim/src/script/items.rs:363` (`equip` stays private; add `pub fn equip_now(&mut self, catalog: &Catalog, item: u32, who: u32, slot: u8) -> Result<Worn, String>` and `pub fn unequip_now(...)`, which move the item and return before/after worn facts, sharing `equip`'s checks: slot fits, `non_equippable`, `passive::may_equip`)

**Interfaces:**
- Produces: `Editor::equip(who: &Who, item: u32, slot: u8) -> Result<(), String>` and `Editor::unequip(who: &Who, slot: u8) -> Result<(), String>`. The sheet becomes `inventory::rewear(sheet, before, after)` and is written to the entity's `sheet`, or to `Kept { sheet, worn }` for `Who::Away`.

- [ ] **Step 1:** Tests: equip a saber on the player → `sheet.weapons[0]` is the saber's `Weapon`. Unequip → back to the fists' sheet, which equals the sheet before. Equip on an `Away` companion → its `Kept.worn` slot is set and `Kept.sheet` re-dressed (Review Focus 2). Equipping a feat-gated item without the feat → `Err` naming the feat.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "Gear is put on and taken off outside play, re-dressed by kjk's rule".

### Task 7: Feats, powers, abilities, skills, alignment, HP/FP

**Files:**
- Modify: `crates/sim/src/combat/passive.rs:140` (add `pub fn unlearn(sheet: &mut Sheet, feat: u16, levels: u8)`)
- Modify: `crates/sim/src/levels.rs:377,399` (`may_feat`, `may_power` → `pub`)
- Modify: `crates/edit/src/edits.rs`

**Interfaces:**
- Produces on `Editor`:
  - `grant_feat(who, feat: u16)` (insert + `passive::learn`, as `GrantFeat` does), `remove_feat(who, feat)` (`passive::unlearn` + clear)
  - `grant_power(who, row: u16)`, `remove_power(who, row)`
  - `set_ability(who, ability: usize, value: u8)`. A CON change moves `max_hp` by `(new_mod − old_mod) × sheet.level`, the rule `Classes::take` uses per level. Check it there before relying on this line.
  - `set_skill(who, row: usize, rank: u8)`, `set_alignment(who, 0..=100)`
  - `set_hp(who, hundredths)`, `set_fp(who, hundredths)`, clamped to max
- Produces for warnings: `pub fn feat_warning(data, &Character, feat) -> Option<String>` and `power_warning(..)`, which return `may_feat`/`may_power`'s refusal in words. These are warnings only (spec).

- [ ] **Step 1:** Test `learn_then_unlearn_is_identity`: for every feat row with a sheet at levels 1, 10 and 20, `unlearn(learn(s)) == s` (Review Focus 4). Edit table tests for each setter, round-tripped, including an `Away` companion. A `feat_warning` for Master Speed on a level-1 soldier → `Some`.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "Feats, powers and stats edited with kjk's own learning rule, prerequisites as warnings".

### Task 8: Party changes, queued for the load

**Files:**
- Move: `crates/launcher/src/probe.rs:40` `program(catalog, calls)` → `crates/sim/src/script/calls.rs` as `pub fn program(..)`. The launcher uses it from there.
- Modify: `crates/edit/src/edits.rs`

**Interfaces:**
- Produces on `Editor`: `make_available(npc: u8, template: &str)`, `join(npc: u8)`, `leave(npc: u8)`, `remove_available(npc: u8)`. Each queues the NWScript routine (`AddAvailableNPCByTemplate`, `AddPartyMember`, `RemovePartyMember`, `RemoveAvailableNPC`) on `State.pending` with the module as caller, the way `Probe::queue` does. `pub fn pending_party(save) -> Vec<String>` lists what's queued, in words, for the editor's change list.

- [ ] **Step 1:** Test: `make_available(bastila, "p_bastilla")` (her party number) + `join` on `kitchen_sink()`, then world + step a few ticks, and `party.members` contains her. `leave` → gone. The queued list reads "Bastila joins the party" (the name from the template, or the npc number if there's no catalog).
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "Party changes in a save run through the scripts' own routines when it loads".

### Task 9: Check an edited save

**Files:**
- Create: `crates/edit/src/check.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Checked { RoundTrip, Stepped { ticks: u64 } }
  pub fn round_trip(save: &Save) -> Result<Checked, String>
  pub fn load_and_step(save: &Save, discovery: &vfs::Discovery, ticks: u64) -> Result<Checked, String>
  ```
  `load_and_step` plans the save's level with `session::plan(&source, &Request::new(game, module, arrival, seats).with_players(n), anims)`, then `restamp`, `save.world`, and steps `ticks` with `pilot`. It is wrapped in `std::panic::catch_unwind`, and a panic becomes `Err` with its message (Review Focus 5). If loading the anim table needs `game` today (`game::stage::anims`), move that loader into `session` in this task.

- [ ] **Step 1:** Tests: `round_trip(kitchen_sink)` → `Ok(RoundTrip)`. Approved test with an install: a K1 checkpoint save (`kjk chain` writes them) with a saber equipped → `load_and_step(.., 60)` → `Ok(Stepped { ticks: 60 })`. A save whose `level` names a missing module → `Err` containing the module name.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** Run, PASS.
- [ ] **Step 5:** Commit "An edited save is checked by loading and stepping it, as kjk would".

### Task 10: Document and open the PR

**Files:**
- Create: `docs/EDIT.md` (what `edit` offers, what's authority vs view, why party edits are queued, the version rule (reads up to `SAVE_FORMAT`, refuses newer))
- Modify: `docs/INDEX.md`, `crates/edit/src/lib.rs` (crate docs linking `docs/EDIT.md`)

- [ ] **Step 1:** Write the doc. Run `cargo clippy --workspace -- -D warnings` and the approved tests. Expected: clean. No change to the save format or the sim hash (the approved hash test is unchanged).
- [ ] **Step 2:** Commit, push `claude/save-edit-api`, and open the PR (squash, title = the commit). Plan 2's branch stacks on it.
