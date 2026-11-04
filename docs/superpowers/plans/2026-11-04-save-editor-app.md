# kjk save editor app Implementation Plan (plan 2 of 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A super-compact desktop save editor for kjk (Mac/Windows/Linux, Steam Deck desktop mode) as `crates/save_editor` in the kjk repo, built on the `edit` crate (plan 1) and shipped in the same release package as the game.

**Architecture:** One Rust binary on `eframe`/`egui`. A `model` layer that has no UI (locate, catalog, session with changes and undo, save) sits under a thin `ui` layer, so everything except drawing is unit-tested. A session keeps the opened save untouched and replays its change list onto a fresh copy after every change or undo. Undo is pop + replay, so a change can never half-apply.

**Tech Stack:** Rust stable, `eframe`/`egui` + `egui_extras` (tables with only visible rows drawn), `rfd` (folder picker), the workspace crates `edit`, `sim`, `saves`, `vfs`, `fmt_odyssey` (`.workspace = true`).

**Spec:** `docs/superpowers/specs/2026-11-04-save-editor-design.md`. Plan 1 is `docs/superpowers/plans/2026-11-04-save-edit-api.md`.

## Global Constraints

- MIT. No code, names, art or text from KSE. Nobody working on this reads KSE's source. The app isn't called KSE.
- `crates/save_editor` depends on workspace crates only, never `game`. Paths below are relative to `crates/save_editor/`.
- Never a half-opened save, a silently accepted path, or a made-up name: anything unread says so in words (spec: anti-goals).
- Never write a save that failed `edit::round_trip`. Run `edit::load_and_step` when an install is found, and say it was skipped when none is.
- Back up before the first write of a session. The save screen says "this save will only load in kjk vX.Y or newer" when the save was older.
- **Super compact:** egui `Spacing` item_spacing ≈ (4, 2), row height = body font + 2 px, 14 px icons, no cards or frames around rows. Details for the selected row go in a right side panel.
- Icons are the game's own (item, feat, power), drawn at row height. A missing icon is a blank square of the same size.
- Plain words everywhere. A resref is secondary grey text, never the primary label.

## Review Focus

1. **Two installs of the same game, or none.** Pick the one `vfs::discover` ranks first and show which in the header, with a switch. With none, saves still open, showing resrefs and a banner. Test in Task 2.
2. **A saves folder with one damaged save among good ones.** The list shows it greyed with the reason, the others open, and nothing crashes. Test in Task 2.
3. **The save changes on disk while open** (kjk saved over it). On save, compare the file's mtime/hash with what was opened, and ask before overwriting. Test in Task 7.
4. **A long list, thousands of items with a mod pack installed.** Scrolling and search stay smooth because only visible rows are drawn and search is a precomputed lowercase index. Test in Task 3 (index), plus a manual check in Task 6.
5. **Undo after a change that `edit` refused.** A refused change is never added to the list, so undo goes back to the last real one. Test in Task 4.

---

### Task 1: The crate and its window

**Files:**
- Create: `crates/save_editor/Cargo.toml` (`[[bin]] name = "kjk-save-editor"`), `src/main.rs`, `src/model/mod.rs`, `src/ui/mod.rs`
- Modify: root `Cargo.toml` (member; `eframe`, `egui`, `egui_extras`, `rfd` in `[workspace.dependencies]`), `.github/workflows/ci.yml` if its Linux packages need more for eframe

- [ ] **Step 1:** `main.rs` opens an eframe window titled "kjk save editor vX.Y.Z". The version comes from `env!("CARGO_PKG_VERSION")`, the same workspace version as kjk. `--help` prints a usage line and exits 0, so the release check can start it headless.
- [ ] **Step 2:** `cargo run -p save_editor` shows the window. `cargo clippy --workspace --all-targets --all-features -- -D warnings` is clean.
- [ ] **Step 3:** Commit "An empty save editor window in the workspace".

### Task 2: Finding saves and installs, and checking paths

**Files:**
- Create: `src/model/locate.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct SaveRow { pub id: String, pub name: String, pub module: String, pub saved: String, pub game: Game, pub readable: Result<(), edit::Refusal> }
  pub enum Found { Saves { count: usize, unreadable: usize }, Install { game: Game, override_items: usize }, Nothing(String) }
  pub fn default_saves() -> Option<PathBuf>           // saves::folder(env)
  pub fn installs() -> Vec<(Game, PathBuf)>           // vfs::discover, ranked
  pub fn check_path(path: &Path) -> Found             // what a picked folder is, in words via Display
  pub fn list(root: &Path) -> Vec<SaveRow>
  ```
  `Found`'s `Display` gives the spec's sentences: "kjk saves folder, 12 saves (1 unreadable)", "TSL install, override has 34 items", "not a saves folder: no save.kjkv inside".

- [ ] **Step 1:** Table tests in a temp dir: an empty dir → `Nothing("…no save.kjkv inside")`. A dir with two `kitchen_sink` saves and one truncated → `Saves { count: 3, unreadable: 1 }`, and `list` gives that row `Err(Damaged)` (Review Focus 2). A save patched to a newer format → `Err(Newer)`, whose Display names both versions. A fake install layout (`chitin.key` + `override/` with 2 UTIs) → `Install { override_items: 2 }`. Zero installs → `installs()` is empty and nothing panics (Review Focus 1).
- [ ] **Step 2:** Run `cargo test locate`, FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5:** Commit "Saves and installs are found, and a picked folder says what it is".

### Task 3: Names, icons and search

**Files:**
- Create: `src/model/catalog.rs`, `src/model/icons.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct Entry { pub key: u32 /* row or index */, pub resref: String, pub name: String, pub icon: Option<String>, pub detail: String, pub modded: bool, pub lower: String }
  pub struct Catalog { pub items: Vec<Entry>, pub feats: Vec<Entry>, pub powers: Vec<Entry>, pub classes: Vec<Entry>, pub skills: Vec<Entry>, pub quests: Vec<Entry>, pub data: edit::Data }
  impl Catalog { pub fn load(discovery: &vfs::Discovery, game: Game) -> Result<Catalog, String>; pub fn search<'a>(list: &'a [Entry], query: &str) -> impl Iterator<Item = &'a Entry> }
  pub struct Icons // TPC/TGA → egui TextureHandle at 14 px, cached by resref, loaded lazily on first draw
  ```
  - Item names come from the UTI's `LocalizedName` via `dialog.tlk`. The icon is `i` + baseitems `itemclass` + `_` + 3-digit `ModelVariation`. `detail` holds the item's damage/defence from `ItemFacts`. `modded` means the UTI came from `override/`.
  - Feats come from feat.2da `name`/`icon`, powers from spells.2da `name`/`iconresref`, quests from `global.jrl` (tag → name, entries).
  - Search matches every space-separated word, case-insensitively, against `lower` (name + resref).

- [ ] **Step 1:** Approved test with a K1 install: the "Lightsaber" entry has an icon that decodes to a non-empty image, Force Lightning is in `powers` with an icon, and the quest list is non-empty. The same checks for a TSL install (Force Lightning and a TSL-only item, e.g. a Peragus one), since the spec covers both games. Unit table test for `search` over a made-up list: "saber" finds "Lightsaber" and "Double-bladed Lightsaber". "double saber" finds only the second (every word must match). "g_w_lghtsbr01" finds it by resref. "SABER" is case-insensitive. 10 000 entries searched in under 5 ms (Review Focus 4).
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement with `fmt_odyssey::{gff, tlk, twoda, texture}`. **Step 4:** PASS.
- [ ] **Step 5:** Commit "Every item, feat, power and quest by its own name and icon".

### Task 4: The editing session: changes, undo, warnings

**Files:**
- Create: `src/model/session.rs`

**Interfaces:**
- Produces:
  ```rust
  pub enum Change { Credits(i32), PartyXp(i32), Xp(Who, i32), Quest(String, i32), Number(String, i32), Boolean(String, bool), Text(String, String),
      AddItem { holder: u32, template: String, stack: u16 }, RemoveItem(u32), Stack(u32, u16), Equip(Who, u32, u8), Unequip(Who, u8),
      GrantFeat(Who, u16), RemoveFeat(Who, u16), GrantPower(Who, u16), RemovePower(Who, u16),
      Ability(Who, usize, u8), Skill(Who, usize, u8), Alignment(Who, u8), Hp(Who, i32), Fp(Who, i32),
      Available(u8, String), Join(u8), Leave(u8), Unavailable(u8) }
  pub struct Session { pub opened: edit::Opened, pub edited: sim::Save, pub changes: Vec<(Change, String /* in words */)>, pub warnings: Vec<String> }
  impl Session { pub fn new(opened) -> Self; pub fn apply(&mut self, c: Change, cat: &Catalog) -> Result<(), String>; pub fn undo(&mut self); pub fn characters(&self) -> Vec<edit::Character> }
  ```
  `apply` replays every change in `changes` plus the new one onto a clone of `opened.save` through `edit::Editor`. If any change fails, the new one is refused and nothing changes. On success it is pushed with its words ("Revan: + Lightsaber (equipped, right hand)"), plus any `feat_warning`/`power_warning`.

- [ ] **Step 1:** Tests on `kitchen_sink`: apply credits then an item, undo once, and only the credits remain. A refused `AddItem("nope")` leaves `changes.len()` unchanged, and undo then removes the credits (Review Focus 5). A feat with unmet prerequisites applies and adds a warning.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5:** Commit "Changes replay onto the opened save, so undo is exact".

### Task 5: The compact shell: saves, party strip, search, tabs

**Files:**
- Create: `src/ui/style.rs`, `src/ui/shell.rs`, `src/ui/list.rs`

**Interfaces:**
- Produces: `style::compact(ctx: &egui::Context)` (the Global Constraints' numbers) and `list::rows(ui, entries: &[&Entry], icons: &mut Icons, columns: &[Column], selected: &mut Option<u32>)`, a reusable virtualised table (`egui_extras::TableBuilder::body().rows(row_h, n, …)`) that every page uses.
- Layout: a narrow left panel with saves (one line each, greyed with the reason when unreadable, and a "Choose folder…" button showing `Found` in words). A top strip with the install in use plus a switch, then a party strip of small portraits. Central tabs: Equipment · Inventory · Powers & Feats · Stats · Party · Quests & Flags. One search field, Ctrl/Cmd+F, filters the current tab. A right side panel shows details of the selected row. A bottom bar shows "N changes · Undo · Save…".

- [ ] **Step 1:** Run it on a real saves folder. Screenshot/gif to the owner for the compact feel; this check is visual, by the owner's standing preference.
- [ ] **Step 2:** Commit "A compact shell around the saves and the party".

### Task 6: The pages

**Files:**
- Create: `src/ui/pages/{equipment,inventory,powers,stats,party,quests}.rs`

Each page is a `list::rows` table plus actions that produce `Change`s through `Session::apply`. Errors show as a one-line red note under the search field, never a modal.

- **Equipment:** the slots in rows (icon, slot, item). Clicking a slot opens the item list filtered to what fits it. Unequip.
- **Inventory:** the party's items (icon, name, count, type, a stat, "mod" tag). Add from the catalog with a count, remove, edit the stack.
- **Powers & Feats:** two lists, known on top and the rest below, with a ⚠ icon and a tooltip of the warning.
- **Stats:** abilities, skills, alignment slider, HP/FP, XP, with "level up in kjk once XP allows" shown beside XP.
- **Party:** available and in-party companions; add, join, leave, remove. Queued changes are marked "applies when you load the save".
- **Quests & Flags:** quests by name with their entry picker. Flags (globals) with name, type and value, and an "advanced" note.

- [ ] **Step 1:** Run against a K1 chain checkpoint save, make one change on each page, save, load it in `kjk play --mute`, and confirm in game: a gif to the owner for each of the saber equipped, a power castable, gold, a quest entry and a companion joined.
- [ ] **Step 2:** Check scrolling and search with an install whose override holds 1000+ UTIs (generate copies) (Review Focus 4).
- [ ] **Step 3:** Commit one per page ("Equipment page", …).

### Task 7: Saving safely

**Files:**
- Create: `src/model/write.rs`, `src/ui/save_dialog.rs`

**Interfaces:**
- Produces: `pub fn save(session: &Session, root: &Path, discovery: Option<&vfs::Discovery>) -> Result<Saved, String>`, where `Saved { backup: PathBuf, checked: edit::Checked }`. It refuses if the file on disk changed since it was opened (`Err("kjk saved over this save since it was opened")`, Review Focus 3), unless `force` is passed. The order is: round_trip → load_and_step (when an install is found) → backup to `<root>/../kjk-save-editor-backups/<id>-<YYYYMMDD-HHMMSS>/` → `saves::write`.
- The dialog lists every change in words, the warnings, and the "only loads in kjk vX.Y or newer" line, then shows the result: "Saved. Checked by loading it (60 ticks). Backup: …"

- [ ] **Step 1:** Tests: a good save → backup exists and the file decodes to the edited save. A failed round-trip (inject through a test hook) → nothing written, no backup. The mtime changed → `Err` and nothing written. No install → `checked == RoundTrip`.
- [ ] **Step 2:** Run, FAIL. **Step 3:** Implement. **Step 4:** PASS.
- [ ] **Step 5:** Commit "Saves are written only after they pass the checks, with a backup first".

### Task 8: In the game's package

**Files:**
- Modify: `xtask/src/dist.rs:116-148` (build and stage `kjk-save-editor(.exe)` next to `kjk(.exe)`; on macOS, a second bundle `kjk Save Editor.app` with its own `Info.plist` (bundle id `…save-editor`), the same icon, signed ad hoc like `kjk.app`); the readme text in `dist.rs:8-49` (a short "Save editor" section, plus the macOS quarantine line for the second app)
- Modify: `.github/workflows/release.yml:70-89` (the package check also finds the editor and runs `--help` on it, and on macOS lints and verifies the second app)

- [ ] **Step 1:** Run `cargo xtask dist` locally. The stage folder has both programs (both apps on macOS), and `THIRD-PARTY-LICENSES.html` lists `egui`/`eframe`.
- [ ] **Step 2:** Run the stage's `kjk-save-editor --help` and expect exit 0. On macOS, `codesign --verify --strict -vv "kjk Save Editor.app"` passes.
- [ ] **Step 3:** Commit "The save editor ships in the game's package".
