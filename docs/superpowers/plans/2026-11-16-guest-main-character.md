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

---

## Handover

Branch `feat/guest-main-character` in the worktree `~/.config/superpowers/worktrees/kjk/guest-main-character`, cut from `origin/feat/coop-lobby` at 8d595cee (pull request 474 was still open). Nothing is pushed and no pull request is open. Each task has its own commit:

| Task | Commit | Title |
|---|---|---|
| 1 (with the design and this plan) | 30e2bda4 | The main character in the co-op lobby's rules |
| 2 | eb24f58c | The main character on the co-op lobby screen |
| 3 | 0b22e415 | A machine's seat that is not its lobby slot |
| 4 | 7704b3dd | The main character's player for the whole session |
| 5 | e2e586c6 | The host takes over the main character |
| 6 | (this commit) | The main character's player in the lobby run |

### What is done

All six tasks. A guest can swap for the main character in the lobby. The guest then sits in the simulation's first seat, leads the party and answers the story's dialogue. The host's machine plays the companion or the remote droid the guest had, in the seat after it, and still runs, saves and reloads the world. When the main character's player leaves on purpose, the host's player takes the first seat at once. When their connection is lost, the host's player takes it 60 seconds after the host timed them out, and the game's mind plays the companion the host had. The simulation's rules for the first seat are unchanged, and nothing new is stored in a save or a snapshot. The sim hash is unchanged, so there is no bumps file. The network protocol goes from version 8 to 9 (`crates/net/src/wire.rs`), because the `Choose` message names a pick.

### Parts of the story with another character (the owner's addition)

Every chain checkpoint was read for its party table (`kjk gui screen GAME lobby --step "lobby checkpoint PATH"` prints `lobby facts`). These are saved inside a part of the story where the first seat plays another character:

| Checkpoint | Game | The first seat plays | How the save marks it | The section's party, which the lobby offers |
|---|---|---|---|---|
| `prologue_cargo` | KOTOR 2 | T3-M4 | switched to party number 8 | nobody |
| `prologue_garage`, `prologue_hull`, `prologue_hyperdrive`, `prologue_inside` | KOTOR 2 | T3-M4 | switched to 8 | the prologue's droid in party number 7 (named by its template, `c_drdastro001`) |
| `telos_military_base` | KOTOR 2 | the main character (see the review fixes: the walker led as Bao-Dur) | leader 1, main character in the party | every companion who can join |
| `nar_shaddaa_t3` | KOTOR 2 | T3-M4, alone in Vogga's warehouse | switched to 8 | nobody |
| `nar_shaddaa_yacht` | KOTOR 2 | Atton, alone on Goto's yacht | switched to 0 | nobody |
| `nar_shaddaa_bridge` | KOTOR 2 | the main character (see the review fixes: the walker led as Mira) | leader 7, main character in the party | every companion who can join |
| `dxun_sith_tomb`, `at_wave2` | KOTOR 2 | Visas Marr, leading the second party on Dxun | switched to 9 | Atton and Bao-Dur |
| `malachor_remote` | KOTOR 2 | the remote, alone on Malachor | switched to 1 (Bao-Dur's number) | nobody |
| `lev_detention`, `lev_icebreaker`, `lev_key` | KOTOR | HK-47, the rescuer on the Leviathan | switched to 3 | nobody |

Not found in any checkpoint: the Peragus section played as T3-M4 (`peragus_t3` is saved after it ends), Mira's own Nar Shaddaa section at the Jekk'Jekk Tarr (the route saves before and after it), the Onderon part of the second party, and Mira on Malachor. None of them is checked.

Two mechanisms mark these sections. Most use the switch (`Party::switched`, the design's case). Mira on Goto's yacht and Bao-Dur at the Czerka site use the party leader (`Party::leader`, a script's `SetPartyLeader`). The first seat plays the leading companion in that case too (`Snapshot::body`). Before this work the lobby showed the benched main character for both, and offered the section's character and every recruited companion. Now the lobby shows the companion the first seat plays (`lobby_host::story_companion`: the leader, else the switched companion) in the main character's place, with its own name, portrait, class and level, and offers only the section's own party. A leading companion is a party member, so Start keeps it in the party.

`cargo xtask lobby --tsl-checkpoints DIR --k1-checkpoints DIR` checks all 15 checkpoints above (`sections` in `xtask/src/lobby.rs`). The hero case ran on `dxun_sith_tomb` (with a party) and `nar_shaddaa_yacht` (alone, so the guests and the host fly droids), and the hero-drop case on `dxun_sith_tomb`.

### Decisions where the plan left a choice

- **Where the seats live.** The lobby gives each slot its seat (`Lobby::seats`). The lobby's network keeps the list (`solo::OpenNet::seat_slots`), and the session's server applies it when the session begins (`Server::begin` takes it). The server keeps a seat for the host's own player (`Server::host_seat`). The host's player takes the first seat when it is free when it connects, else its own seat. That covers the main character's player leaving at the moment the host presses Start. The seats then live in the server's connections for the whole session, through every install.
- **How the takeover is carried.** In the session's server, not in `drivers.rs` (`Server::host_takes_first_seat`). When the first seat has no player and no token keeps it, the host's in-process connection moves into it. Only which connection's commands fill the first seat changes. Every guest gets those commands in the snapshots, so all machines apply the change on the same step. The host's own client is told directly in the same process (`Client::reseat`), and the host's game follows its client's seat each frame (`Authority::seat`, `OwnSeat`).
- **A deliberate leave can be told apart.** A player who leaves sends a goodbye (`Refusal::Left`). The server keeps a seat only for a player who timed out. So a deliberate leave frees the first seat at once and the host takes it at once. A lost connection keeps it 60 seconds (`RECONNECT_US`) after the 5 second link timeout.
- **The host's machine in the lobby screen.** A host who does not play the main character keeps the Fill button behaviour for free companions (they join the party under the game's control). The host can swap with any player by choosing that player's character, the main character included. The lobby's rules also let a host who plays a companion take a free one, but the screen does not offer that, because a host's press on a free companion fills a place.
- **The row of portraits.** The main character goes first and ten companions after it. The pitch is 58 units instead of 60, so eleven 38-unit portraits fit the 644-unit panel.
- **The lost box's Load** shows on the machine that runs the world (`WorldHere`), whatever its seat.
- **The pilot** (`Driver::Pilot`, the command line's tests) never fills the first seat, so the main character waits for its player or the host.

### "This machine is the host" against "the main character's player"

Changed to read this machine's seat or the host's machine:
- `sim_host::advance`: the held pitch, the conversation for the reply controls, the power choices and the item in hand read this machine's seat (they read the first seat).
- `sim_host::advance`: the pilot's commands never fill the first seat.
- `ui_view::fall`: the lost box's Load reads `WorldHere` (it read the first seat).
- `lib.rs::host_session`: a lobby host's camera seat starts as its lobby seat.

Kept on the first seat, because they are powers of the main character's player: the galaxy map's flag (`script_screens.rs`), the leader controls (`leader_view.rs`), forcing a ready check (`party_calls.rs`), the stance picks and dressing for companions nobody plays (`stance_sets::may_pick`, `character.rs`), the join requests to the story's conversation (`dialogue_view::asking`), and the two notices in `loot.rs`, now worded "Only the main character's player equips companions nobody plays." and "Only the main character's player can go now, after 5 seconds."

Left as they are: the slow motion of a perfect block (played alone only), the scripted aim of `kjk play --aim` (a test tool), the transcript's and barks' player name (the story's PC), character creation and the arena's setup, and the `map_or(Seat(0))` fallbacks where `OwnSeat` is missing (it is always set when a world runs).

### Departures from the design or the plan

- **The party leader** marks two sections, as the table above says. The design names only the switch. The lobby treats both the same way. A save where the player chose a leader with the leader key is treated the same too: the lobby shows that companion in the main character's place and keeps the party as it is. The owner may want only script-set leaders to count, but the save does not tell the two apart.
- **Task 4 needed no code.** The plan names `session_flow.rs` and `save_view.rs`. Loading another save while hosting is refused already ("A hosted game loads from the main menu."), and the reload after a fall is an install on the same server, which keeps its connections' seats. The task's commit holds the check only.
- **The takeover landed in the Task 3 commit**, because it is part of the same server code that seats the players. The Task 5 commit holds the conversation check. Task 5's net checks are in the Task 3 commit.
- **Test order.** The probes were written together with the code, not run first to see them fail.
- **The host's first command in the first seat picks no reply.** The simulation takes a returning seat's first answer count as its start (an existing rule), so the host's own count does not answer for it. Its next answer counts once. This needed no change.

### Commands run and their results

All `kjk` runs were headless and muted, with `KJK_SETTINGS` set to a scratch copy of the settings and `KJK_SAVES` set to scratch folders. The saves were copies of the owner's `k1-02` and `k1-08`, and checkpoints written from the main checkout's kept KOTOR 2 chain checkpoints (`kjk-artifacts/tsl-chain-checkpoints/checkpoints/tsl/`) into scratch save folders. No chain ran.

The final runs, on the branch as committed:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -q --release --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --release -p approved_tests`: 135 passed. The new network checks are in `coop_lobby_wire` and the new lobby rules in `coop_lobby`.
- `kjk gui probes`: 169 passed, 0 failed (167 before, with two new checks for a guest playing the main character).
- `kjk net probes`: 89 checks, 0 failed.
- `kjk sim probes`: 1425 checks, 2 failed: `fp_regeneration` and "the lab station", the two that failed on the lobby branch before this work. The new checks pass: the lobby's rules (24 cases and 10000 random changes), `lobby host` (the switched and leading characters, the lost box, the player count with the host on a droid), `other_seat_keeps_its_companion_through_a_switch` and `first_seat_away_in_talk_answers_once`.
- `kjk sim determinism`: passed.
- `cargo xtask duo`: passed (the player rebuilt 1951 snapshots, 0 check failures).
- `cargo xtask lobby --saves <copies> --load k1-08 --droid k1-02 --hero tsl-51 --hero tsl-52 --hero-drop tsl-51 --tsl-checkpoints DIR --k1-checkpoints DIR`: passed. Report: `kjk-artifacts/runs/1794809603-lobby/lobby.md`. Save `tsl-51` is a copy of the checkpoint `dxun_sith_tomb`, and `tsl-52` of `nar_shaddaa_yacht`.
  - swap on `k1-08` and droid on `k1-02`: passed as before, every rebuilt snapshot equal to the host's.
  - hero on `dxun_sith_tomb`: Mia swapped for Visas Marr in the lobby, was welcomed into seat 0, and answered Xarga's conversation with `ui reply 1` three times. The host's machine sat in seat 1 and drove Bao-Dur, Kalle drove Atton in seat 2, and the host wrote a save. Both guests rebuilt every snapshot as the host stepped it.
  - hero on `nar_shaddaa_yacht`: Mia played Atton alone and answered Goto's conversation. The host's transcript shows Goto's lines and the picked replies, a few seconds apart, as Mia's console sent them. The host's machine and Kalle were given remote droids. No droid had launched by the dump, 20 seconds in, while the conversation held the world. The host wrote a save. Every rebuilt snapshot matched.
  - hero-drop on `dxun_sith_tomb`: Mia's process was killed about 30 seconds after Kalle joined. About 65 seconds later the host logged "the host plays the main character from=Seat(1) to=Seat(0)". At the end no player drove Bao-Dur (the host's old companion), and Kalle still drove Atton. Kalle rebuilt 2787 snapshots, none unlike the host's 11142, across the takeover.
  - sections: all 15 checkpoints showed the section's character with its portrait, class and level, offered only the section's party, and had exactly one slot on the story's character.

Earlier runs:
- `cargo xtask duo` failed once after Task 3: the joining player gave up after 5 seconds because the host took 6 seconds to load its stage. The host's own player was welcomed into seat 0 as before. The next run passed, unchanged (1959 snapshots rebuilt, 0 check failures).
- `cargo xtask lobby --load k1-08 --droid k1-02` failed once after Task 3, in its own log parsing only: the host's seat list now names each slot's seat. The parser was fixed, and both cases passed with every rebuilt snapshot equal to the host's.

Not run: the two sim probes that failed on the lobby branch (`fp_regeneration` and "the lab station") were not tried on plain `origin/main`, because that needs a separate build of about 20 GB and the disk had 24 GB free. Nothing was checked in a window by a person, and nothing through Steam.

### Screenshots

In `kjk-artifacts/guest-main-character/`: `lobby-{k1,tsl}-hero-{host,guest}-{1280x960,1920x1080}.png`. Mia plays the main character. "host" is the host's view, whose slot shows the companion Carth (KOTOR) or Mandalore (KOTOR 2). "guest" is Mia's own view. The section checks drew each checkpoint's lobby into the lobby run's `sections/` folder too.

### Known small things

- Fixed in review: the "Not available" labels under the Companions row ran together.
- The prologue's droid in party number 7 has no name in the game's data, so the lobby shows its template, `c_drdastro001`.
- `lobby facts` (the gui verb's line for the checks) prints the party table's switch, leader, members and puppets. It is a test aid.

### Review fixes

Two defects were found in review and are fixed in the commit "Main character with a chosen leader, and lobby labels in their columns".

**A leader the player picked hid the main character.** The lobby showed any companion leading the party in the main character's place. Both party-leader checkpoints turned out to be ordinary leads. Their saves contain:
- `nar_shaddaa_bridge`: Mira leads (leader 7), the party is Mira and T3-M4, and the Exile's avatar is in the area, in the party, alive, commandable and 2.1 metres from Mira. The route saves it right after the Exile is freed, and the next line of the route leads as the player again.
- `telos_military_base`: Bao-Dur leads (leader 1), the party is Bao-Dur and Kreia, and the Exile's avatar is in the party, alive, commandable and 1.4 metres from Bao-Dur.

So no checkpoint holds a section where the main character is absent while a companion leads.

The test that tells the cases apart is `lobby_host::main_playable`. It is true when the main character's avatar is in the party (in the area and not benched by a switch) and no script holds it (`SetCommandable`). When it is true, a leading companion is an ordinary companion. In that case the lobby shows the main character, offers every party member including the leader, and Start clears the leader as the game's own change of leader to the main character does (`Party::lead` with the main character's body, and the snapshot's leading body cleared). When it is false, the leading companion is the story's character, as before. A switch always benches the main character, so a switched save keeps its switched character.

The checks:
- The lobby host probes `lobby_offers_a_companion_the_player_chose_to_lead` (main character present) and `lobby_shows_the_leading_companion_while_the_main_character_is_away` (its avatar removed).
- The section check now expects the Exile for both checkpoints, every companion who can join as offered, and no leader after Start (`start_leader` in the lobby facts). This check fails on the earlier code: the earlier run's report shows Mira and Bao-Dur as the story's character (`kjk-artifacts/runs/1794809603-lobby/lobby.md`).

**Labels under the Companions row ran into each other.** Each label is now its own column's width (56 of the 58 units). A long name wraps inside it. A word wider than the column is cut to fit and ends in a full stop ("Canderou.", "Handmaid."), in `Painter::shown_words` for every label that offers shorter words. That is the lobby's and the pazaak screen's. A companion who cannot be chosen shows its name only, without "Not available". KOTOR 2's empty portrait shows it, and the description strip still says the full sentence. The gui probe `lobby_labels_stay_in_their_columns` paints a label in a narrow column and checks that no word is wider than it. `lobby_words_*` checks that the taken companion's label is its name alone. I looked at the new screenshots at both sizes and in both games. The labels stay in their columns. At 1280 by 960 some neighbours stand close ("Bao-Dur" and "Mandalo."), but they do not touch.

The runs after the review fixes, on the branch as committed:

- `cargo fmt --all -- --check`: clean.
- `cargo clippy -q --release --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --release -p approved_tests`: the first run had 134 passed and 1 failed. The failure was `net_rewinds` ("lan_rewinds_on_a_thread session: client 0 prediction"), while other sessions' builds loaded the machine. The fixes touched no network or prediction code. Two reruns had 135 passed and 0 failed each.
- `kjk gui probes`: 170 passed, 0 failed.
- `kjk net probes`: 89 checks, 0 failed.
- `kjk sim probes`: 1426 checks, 2 failed. The failures are `fp_regeneration` and "the lab station", the same two as before this work.
- `kjk sim determinism`: passed.
- `cargo xtask duo`: passed.
- `cargo xtask lobby` with every case (swap, droid, both hero cases, hero-drop and sections): passed. The report is `kjk-artifacts/runs/1794810791-lobby/lobby.md`. `telos_military_base` and `nar_shaddaa_bridge` now show the Exile and offer every companion who can join (Atton, Bao-Dur and Kreia; Atton, Bao-Dur, the Handmaiden, Kreia, Mira, T3-M4 and Visas Marr). In hero-drop, the host took the main character again, and the second guest rebuilt 2418 snapshots with none unlike the host's 9669.

The screenshots were rendered again in `kjk-artifacts/guest-main-character/`:
- `lobby-{k1,tsl}-hero-{host,guest}-{1280x960,1920x1080}.png`;
- `lobby-section-{dxun_sith_tomb,nar_shaddaa_bridge,telos_military_base}-{1280x960,1920x1080}.png`;
- `lobby-section-lev_key-1280x960.png`.

The three earlier section screenshots without a size in their names were removed.
