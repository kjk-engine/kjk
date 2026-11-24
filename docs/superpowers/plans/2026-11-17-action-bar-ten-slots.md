# Ten Action Bar Slots Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The action bar has ten slots. Slots 7 to 10 are on keys 7, 8, 9 and 0, start empty, and are hidden on the HUD until the player assigns something to them.

**Architecture:** The sim's bar grows from six stored slots to ten, behind a snapshot bump whose old read leaves slots 7 to 10 empty. The game's key map gets four more slot actions. The HUD layout from KOTOR 2 has six slot controls, so the game copies the sixth control four times to the right when it prepares the layout, and the HUD's view model lists only the slots that are shown.

**Tech Stack:** Rust, Bevy, the repo's probe tests (`kjk sim probes`, `cargo test -p …`).

**Spec:** `docs/superpowers/specs/2026-11-17-action-bar-ten-slots-design.md`

## Global Constraints

- `sim::combat::bar::SLOTS` is 10. `FILLED` stays 4. `settings::PAD_BAR` stays 4.
- Default keys of slots 7 to 10 are `Digit7`, `Digit8`, `Digit9`, `Digit0`. Setting names are `bar_7`, `bar_8`, `bar_9`, `bar_10`. Labels are "Action bar slot 7" to "Action bar slot 10".
- No branch edits a format's version number. The bump goes in `bumps/feat-action-bar-ten-slots.toml`, and its gate is `since::snapshot::FEAT_ACTION_BAR_TEN_SLOTS`.
- The HUD always shows slots 1 to 6 on the keyboard. It shows slots 7 to 10 only up to the last one that holds something.
- Player-facing text follows `docs/player-text.md`: full sentences, no shorthand.

## Review Focus

- An old save (snapshot version 100 to 109) must load with its six slots unchanged and slots 7 to 10 empty. Task 1 tests it.
- A settings file written before this change has no `key.bar_7` line. It must get the default keys. Task 2 tests it.
- A player who already bound 7, 8, 9 or 0 to another action must keep that binding, and the new slot whose default key is taken must start unbound. Task 2 tests it.
- In a conversation, keys 7, 8 and 9 choose replies 7, 8 and 9. They must not also use a bar slot. Task 2 tests it.
- A controller must still see four slots in a cross on the HUD and on the setup screen, even when slots 7 to 10 hold something. Task 3 tests it.

---

### Task 1: Ten stored slots

**Files:**
- Modify: `crates/sim/src/combat/bar.rs` (`SLOTS`, `Bar::decode`, module documentation)
- Modify: `crates/sim/src/snapshot.rs:3042` (`decode_sheet`)
- Create: `bumps/feat-action-bar-ten-slots.toml`
- Test: `crates/sim/src/combat/bar_probes.rs`

**Interfaces:**
- Produces: `pub const SLOTS: usize = 10;`, `pub const KEPT: usize = 6;` (the slots the HUD always shows), `Bar::decode(input: &mut Reader, slots: usize) -> Result<Bar, DecodeError>` reading `slots` slots and leaving the rest empty, `Bar::shown(&self) -> usize` returning `max(KEPT, 1 + index of the last slot that holds something)`.

- [ ] **Step 1:** Add probes: a bar encoded with six slots decodes through `Bar::decode(input, 6)` with slots 7 to 10 empty and the first six unchanged; `Bar::put(9, content)` succeeds and `Bar::put(10, content)` is refused; `Bar::shown()` is 6 for an empty bar, 9 with only slot index 8 filled, 10 with slot index 9 filled; `settle` on a fresh sheet never fills a slot of index 4 or higher.
- [ ] **Step 2:** Run the bar probes and see them fail.
- [ ] **Step 3:** Set `SLOTS` to 10, add `KEPT`, `shown`, the `slots` parameter of `decode`. In `decode_sheet`, pass `SLOTS` when `input.since(since::snapshot::FEAT_ACTION_BAR_TEN_SLOTS)` and 6 otherwise.
- [ ] **Step 4:** Write the bump file with `[formats] snapshot` = "a sheet's action bar has ten slots (`bar::SLOTS`)." and `[migrations] snapshot` = "old saves read six slots, and slots 7 to 10 are empty".
- [ ] **Step 5:** Run the bar probes and `cargo check --workspace`. Commit.

### Task 2: Keys for slots 7 to 10

**Files:**
- Modify: `crates/game/src/settings.rs` (`Action`, `BAR`, the bindings table near line 230, any table sized by the number of actions)
- Modify: `crates/game/src/bar_view.rs` (arrays sized by `SLOTS` follow on their own; the conversation gate)
- Modify: `crates/launcher/src/lib.rs:398` (help text)
- Test: `crates/game/src/settings_probes.rs`, `crates/game/src/bar_view.rs` probes

**Interfaces:**
- Consumes: `bar::SLOTS`.
- Produces: `Action::Bar7`, `Action::Bar8`, `Action::Bar9`, `Action::Bar10`; `pub const BAR: [Action; sim::combat::bar::SLOTS]`.

- [ ] **Step 1:** Extend the settings probe "a fresh settings file puts the action bar slots on keys" to ten slots with `Digit7`, `Digit8`, `Digit9`, `Digit0`. Add a probe that a settings file with `key.map = "Digit7"` (any other action on 7) keeps that binding and leaves `bar_7` unbound. Add a bar view probe that slot 7's key in a conversation does not use the slot, beside the existing "slot 3 in a conversation" row.
- [ ] **Step 2:** Run them and see them fail.
- [ ] **Step 3:** Add the four actions and their table rows with a loop or a helper, not four copied blocks, if the table's shape allows it. Follow how a new default key that collides with a player's existing binding was handled for the first six slots (the migration notes near `settings.rs:1055`).
- [ ] **Step 4:** Run the probes and `cargo check --workspace`. Commit.

### Task 3: HUD and setup screen

**Files:**
- Modify: `crates/ui/src/screens/hud.rs` (new `bar_extra(gui: &mut Gui)`, the frame round the row)
- Modify: `crates/game/src/ui_view.rs:1205` (call `bar_extra` where the HUD layout is prepared)
- Modify: `crates/game/src/hud.rs:1018` (count of shown slots)
- Modify: `crates/ui/src/screens/action_bar.rs` (`SETUP_SLOTS` = 10, the row's width, comments)
- Modify: `crates/ui/src/vm.rs:759,784` (comments)
- Test: `crates/ui/src/probes/hud.rs`, `crates/ui/src/probes/menus.rs`

**Interfaces:**
- Consumes: `Bar::shown()`, `settings::BAR`, `settings::PAD_BAR`.
- Produces: `pub fn bar_extra(gui: &mut Gui)` in `ui::screens::hud`. It copies `BTN_ACTION5` and `LBL_ACTION5` to `BTN_ACTION6..=9` and `LBL_ACTION6..=9`, each one slot step to the right of the one before. The slot step is `BTN_ACTION1.left - BTN_ACTION0.left`.

- [ ] **Step 1:** Add HUD probes: with a six-entry `HudView::bar` the HUD draws six frames; with nine entries it draws nine frames, each one slot step apart; with a controller it draws four frames in a cross. Add a setup screen probe that the keyboard sees ten slots in a row and a controller sees four in a cross.
- [ ] **Step 2:** Run them and see them fail.
- [ ] **Step 3:** Implement `bar_extra`, call it before `bar_cross`. In `game::hud`, `shown` is `PAD_BAR` with a controller and `sheet.bar.shown()` otherwise. The frame round the row (`LBL_MOULDING3`) is drawn six slots wide by the layout. Widen it by one slot step for each extra slot shown, or hide the extra width if stretching the picture looks wrong. Decide from a screenshot.
- [ ] **Step 4:** Set `SETUP_SLOTS` to 10 and make the setup row fit the page's width (shrink the gap first, then the slot).
- [ ] **Step 5:** Run the probes, `cargo check --workspace`. Take a headless screenshot of the HUD with slot 9 assigned and of the setup screen. Commit.

### Task 4: Text and pull request

- [ ] **Step 1:** Update every comment and help line that says the bar has six slots (grep "six" beside "slot" in `crates/game`, `crates/ui`, `crates/sim/src/combat/bar.rs`, `crates/launcher`).
- [ ] **Step 2:** `cargo fmt`, `cargo check --workspace --all-targets`. Push the branch and open the pull request titled "Ten action bar slots". CI runs clippy and the tests.
