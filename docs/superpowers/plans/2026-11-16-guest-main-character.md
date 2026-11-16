# A Guest as the Main Character Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The owner asked for one Opus subagent to implement the whole plan. The main session reviews the branch afterwards.

**Goal:** Any player can choose the main character in the co-op lobby, and that player answers the story's dialogue and has every power of the main character's player, while the host's machine keeps hosting and saving.

**Architecture:** The simulation is not changed. Its first seat (`LEADER_SEAT`) keeps meaning "the main character's player". The lobby decides which machine sits in that seat, and the session carries a mapping from network peer to simulation seat. The host's machine learns to run the game from a seat other than the first. A new rule hands the first seat to the host when its player is gone.

**Tech Stack:** Rust workspace (crates `session`, `net`, `sim`, `ui`, `game`), the project's probes and approved tests, `cargo xtask lobby`.

**Spec:** `docs/superpowers/specs/2026-11-16-guest-main-character-design.md`. Read it whole first, and the lobby design it builds on, `docs/superpowers/specs/2026-11-15-coop-lobby-design.md`. The handover at the end of `docs/superpowers/plans/2026-11-15-coop-lobby.md` says how the lobby was built and how its runs are driven.

## Global Constraints

- Work in a new worktree on branch `feat/guest-main-character`, cut from `origin/feat/coop-lobby` (pull request 474). If pull request 474 has been merged when you start, cut from a freshly fetched `origin/main` instead.
- Never use a `claude/` branch prefix. No attribution lines in commits. Commit after each task with a short patch-note style title, a noun phrase with no colon. Do not push and do not open a pull request.
- Every `kjk` run passes `--headless` and `--mute`, with `KJK_SETTINGS` set to a scratch copy of the settings and with scratch save folders.
- Do not run any chain. A chain is a walkthrough with one player and does not reach this code.
- The simulation's rules for the first seat are not rewritten. `kjk sim determinism`, `cargo xtask duo` and both existing cases of `cargo xtask lobby` must give the same results as before.
- Nothing new is stored in a save or a snapshot. If you find you need a snapshot field, stop and say so in the handover instead of adding it.
- Text a player reads is plain full sentences. No class or function names from the original executables in shipped text or comments. Game text stays in the game's code page.
- Menus follow KOTOR 2's behaviour and use only KOTOR 2's art.
- No repeated test blocks. Use parameterized tables or shared helpers.
- Before a task is called done: `cargo fmt`, `cargo clippy` on the touched crates, and their tests and probes pass.

## Review Focus

1. The main character's player drops at the same step the host presses Start. The game must start with the host as the main character's player. Test in Task 1 (lobby rule) and Task 6.
2. The main character's player drops in the middle of a story conversation. After the takeover the host must be able to answer the same conversation, and it must not be answered twice. Test in Task 5.
3. The party falls while a guest plays the main character. After the reload the guest must still be the main character's player. Test in Task 4.
4. A story section with a switched character while a guest holds the first seat (`SwitchPlayerCharacter`). The guest must follow the switch and the host must keep its companion. Test in Task 3.
5. The host flies a remote droid because a guest with a droid took the main character. Enemy scaling must not count the host, and the host must still save. Test in Task 3.

---

### Task 1: The main character in the lobby's rules

**Files:**
- Modify: `crates/session/src/lobby.rs`, `crates/net/src/session/lobby.rs` and `msg.rs` (the Choose message), `crates/net/src/wire.rs` (protocol version 8 to 9)
- Test: the probes in `lobby.rs`, `crates/net/src/session/lobby_probes.rs`

**Interfaces:**
- Produces:

```rust
pub enum Pick { Hero, Companion(u8) }          // what a Choose asks for
impl Lobby {
    pub fn choose(&mut self, slot: usize, pick: Pick, swap: bool) -> bool;   // slot 0 allowed now
    pub fn hero_slot(&self) -> usize;                                         // the slot with Playing::Hero
    pub fn seats(&self) -> Vec<(usize /*slot*/, u8 /*sim seat*/, Playing)>;
}
```

Rules:
- Exactly one occupied slot has `Playing::Hero` at all times. At `new` and `reset` it is slot 0.
- `choose(slot, Pick::Hero, swap)` is a swap with `hero_slot()`. It is refused when `swap` is false. The other player gets what the chooser had: a companion or `Playing::Droid`.
- `choose(0, Pick::Companion(npc), ..)` is allowed only as a swap with a guest who plays `npc`, or as a free pick when slot 0 does not hold the hero. Slot 0 can never give up the hero except by a swap.
- `leave(slot)` of the hero's slot gives the hero to slot 0. The companion slot 0 had becomes free and is not added to the host's places.
- Both slots of a swap lose their Ready mark. Slot 0 has no Ready mark.
- "The hero" means the story's character: the first seat's body in the save. In a save inside a section with a switched character (`Party::switched`, for example KOTOR 2 played as Atton), that is the switched character, and `Summary`'s hero name, portrait, class and level describe that character. The real main character is then not offered anywhere. Check what `lobby_host::summarize` shows for such a save today and fix it if it shows the benched main character.
- A game never starts without the hero. `started` asserts that exactly one occupied slot has `Playing::Hero`, and gives it to slot 0 if none does.
- `seats()`: the hero's slot gets simulation seat 0. The other occupied slots get seats 1 and 2 in slot order.

- [ ] **Step 1: Write failing probes** as one table: a guest takes the hero and the host gets the guest's companion; the host takes it back; a guest with a droid takes it and the host gets the droid; a free-pick request for the hero is refused; the hero's player leaves and the hero returns to slot 0; `seats()` for each of those states; no sequence of `choose`, `fill`, `leave` and `reset` calls leaves the lobby without exactly one hero; a save in a switched-character section shows the switched character as the hero; two guests ask for the hero in one step and the second swaps with the first.
- [ ] **Step 2: Run and see them fail. Implement. Run again.**
- [ ] **Step 3: The Choose message carries a `Pick`.** Update the wire round-trip table and the lobby net probes. Bump the protocol version.
- [ ] **Step 4: fmt, clippy, probes. Commit.**

### Task 2: The main character on the lobby screen

**Files:**
- Modify: `crates/ui/src/screens/lobby.rs`, `crates/ui/src/vm.rs` (`LobbyView`), `crates/ui/src/strings.rs`, `crates/ui/src/intent.rs` (`LobbyIntent::Choose` takes the pick), `crates/game/src/lobby_host.rs` and `lobby_guest.rs` (view and intents), `crates/launcher/src/gui_verbs.rs` (stand-in), `crates/ui/src/probes/lobby.rs`

Exact words:
- Description on the main character's portrait, for a player who does not play it: "{hero} is the main character, played by {player}. Choose them to swap characters with {player}. The main character's player answers the story's dialogue."
- For the player who plays it: "You play {hero}, the main character. You answer the story's dialogue."

- [ ] **Step 1: Write failing probes** (host view and guest view): the main character's portrait is first in the Companions row, labelled with its player's name; both description texts; selecting it emits the swap intent; when a guest plays the hero the host's slot shows the host's companion and still says "Host", and the guest's slot shows the hero's portrait, name, class and level.
- [ ] **Step 2: Implement.** The row now has the hero plus up to ten companions. Keep the sizes from the lobby (38-unit portraits) and check that eleven portraits fit at 1280×960. If they do not, reduce the pitch, not the portrait.
- [ ] **Step 3: Render** host and guest views with a guest playing the hero, both games, 1280×960 and 1920×1080, into `kjk-artifacts/guest-main-character/`. Look at them and fix clipping.
- [ ] **Step 4: fmt, clippy, probes. Commit.**

### Task 3: A machine's seat is not its slot

**Files:**
- Modify: `crates/net/src/session/server.rs` (the host's in-process peer and seat reservation), `crates/game/src/solo.rs`, `crates/game/src/lobby_host.rs` (`started`, Start), `crates/game/src/sim_host.rs` (the local input reads and other uses of the first seat as "this machine"), `crates/game/src/ui_view.rs` (defeat screen), `crates/game/src/script_screens.rs`, `crates/sim/src/snapshot.rs` (`players_driving`) only if it counts the host as always driving
- Test: game and net probes, `cargo xtask lobby`

**Interfaces:**
- Consumes: `Lobby::seats()` from Task 1.
- Produces: the session server accepts a seat for its in-process peer and for each kept lobby connection; each client's own seat is its simulation seat; a single `hosting` test (the existing `solo.hosting()` / `WorldHere`) is what "this machine is the host" code reads.

- [ ] **Step 1: List the sites.** Grep the game crate for `Seat(0)`, `LEADER_SEAT` and `OwnSeat` comparisons. Sort each into "this machine is the host" or "this player is the main character's player". Write the list in the handover. Only the first kind changes.
- [ ] **Step 2: Write failing checks:** a session started from a lobby where slot 1 holds the hero gives the guest's connection seat 0 and the host's peer seat 1; the host's own input goes in as seat 1; the host still saves and autosaves; the defeat screen's host button shows on the host's machine and not on the guest's; the enemy scaling count is the number of players with a body, with the host not counted when it flies a droid.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Switched character check:** a probe where the first seat is a guest's and a script switches the played character. The guest's body follows the switch and the host keeps its companion.
- [ ] **Step 5: Reword the two notices** in `crates/game/src/loot.rs` to name the main character's player: "Only the main character's player equips companions nobody plays." and "Only the main character's player can go now, after 5 seconds." Grep for other player-facing texts that say "host" for a first-seat power and reword them the same way.
- [ ] **Step 6: fmt, clippy, probes, determinism, duo. Commit.**

### Task 4: The role lasts the whole session

**Files:**
- Modify: where the session keeps the peer-to-seat mapping (Task 3), `crates/game/src/session_flow.rs`, `crates/game/src/save_view.rs` (`ReloadRequest` path)

- [ ] **Step 1: Write failing checks:** after the host loads another save during the session, and after the reload that follows a party wipe, the guest who played the main character still has seat 0 and the host still has its guest seat. A save written during such a session loads alone with the host as the main character's player.
- [ ] **Step 2: Implement.** The mapping lives in the session and is applied to every world the session installs.
- [ ] **Step 3: fmt, clippy, probes. Commit.**

### Task 5: The host takes over the main character

**Files:**
- Modify: `crates/sim/src/world/drivers.rs` or the session server, as the design leaves to the builder; `crates/net/src/session/server.rs` (a deliberate leave); `crates/game/src/solo.rs`
- Test: sim probes beside the driver probes, net probes

Rules (from the design):
- A lost connection of the first seat's player: the seat is kept for 60 seconds (`KEEP_MS`, the time a guest's companion is kept). The game keeps running. The first seat sends no input.
- A return within 60 seconds gives the seat back.
- After 60 seconds, or at once on a deliberate leave when the host can tell, the host's machine takes seat 0. The companion the host played is controlled by the game. Every machine applies the change on the same step.
- A player who returns after the takeover joins as an ordinary late guest.
- When the host itself is the first seat's player, none of this applies.

- [ ] **Step 1: Find out** whether the host can tell a deliberate leave from a lost connection. Write the answer in the handover. If it cannot, the 60 second rule covers both.
- [ ] **Step 2: Write failing checks** as one table: drop and return at 30 seconds keeps the seat; drop for 61 seconds moves the host to seat 0 and frees the host's old companion to the game; a deliberate leave moves the host at once (if detectable); a return after the takeover gets a guest seat; a drop during a story conversation, after which the host answers it once; the takeover gives identical state on the host and a guest (compare hashes).
- [ ] **Step 3: Implement.** The takeover must be part of the deterministic input that guests receive, not a local decision on each machine.
- [ ] **Step 4: fmt, clippy, probes, determinism. Commit.**

### Task 6: The whole thing together

- [ ] **Step 1: Add two cases to `cargo xtask lobby`**, as cases of the same runner and not copies. Case "hero": a guest takes the main character in the lobby, the host starts, the guest's machine leads and answers a story conversation, the host's machine plays a companion and writes a save, and every snapshot the guests rebuild matches the host's. Case "hero-drop": the same start, then the guest's process is killed, and after 60 seconds of game time the host plays the main character. Add the script flags the cases need to `kjk join --lobby-script`.
- [ ] **Step 2: Review Focus.** Check that each of the five lines has its test, and add any that is missing.
- [ ] **Step 3: Run everything:** workspace `cargo fmt --check` and `cargo clippy -D warnings`, the approved tests, `kjk gui probes`, `kjk net probes`, `kjk sim probes`, `kjk sim determinism`, `cargo xtask duo`, and all four cases of `cargo xtask lobby`.
- [ ] **Step 4: Update `docs/COOP.md`** with the rules for the main character's player.
- [ ] **Step 5: Commit. Append a handover** to this plan file: what is done and not done, every decision where the plan left a choice, every departure from the design with its reason, every command with its real result, and the screenshot paths.
