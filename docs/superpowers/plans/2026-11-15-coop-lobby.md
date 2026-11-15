# Co-op Lobby Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The owner asked for one Opus subagent to implement the whole plan. The main session reviews the branch afterwards.

**Goal:** A lobby screen that opens after the host picks a save and before the world loads, where guests gather, choose companions within the story's rules, and the host sets the saber rules and collision mode and starts the game.

**Architecture:** A pure lobby state type decides every choice on the host. A lobby phase in the net server and client carries that state with five new messages before `Welcome`. The host's start path is split into Open (decode save, credentials, lobby) and Start (fix settings, write the party, load the world). A new UI screen draws the state from the view model with KOTOR 2's art.

**Tech Stack:** Rust workspace (crates `sim`, `session`, `net`, `ui`, `game`, `saves`), Bevy, the project's own probes and approved tests, `cargo xtask`.

**Spec:** `docs/superpowers/specs/2026-11-15-coop-lobby-design.md`. Read it first, whole. The layout and words follow the mockup described there.

## Global Constraints

- Work in a new worktree on branch `feat/coop-lobby`, cut from a freshly fetched `origin/main`. The owner's main checkout is about 200 pull requests behind and must not be used or modified.
- Never use a `claude/` branch prefix. No attribution lines in commits.
- Commit after each task. A commit title is a short noun phrase a player could read in patch notes, with no colon. Do not push and do not open a pull request.
- Every `kjk` run passes `--headless` and `--mute`, and sets `KJK_SETTINGS` to a scratch copy of the settings file.
- Do not run the TSL chain. Run the K1 chain once, at the end.
- Text a player reads is written as plain full sentences. No class or function names from the original executables appear in shipped text or comments.
- Game text stays in the game's code page (bytes), never forced to UTF-8. Player names are shown as received.
- Menus follow KOTOR 2's behaviour: hover selects, and keyboard and gamepad reach every control. The lobby layout is recorded in the list of deliberate deviations (`context/ui-survey/` or the file the repo uses for it on `origin/main`; find it with `git grep -n -i "deliberate"`).
- Every drawn element is KOTOR 2's own art. Nothing is hand-drawn.
- Format changes (net protocol, snapshot, save) are numbered through a file in `bumps/` named after the branch, as the existing bump files do.
- The main character is shown by the name given in character creation. A companion is shown by the name the game shows at that point of the save.
- No repeated test blocks. Use parameterized cases or shared helpers.
- Before a task is called done: `cargo fmt`, `cargo clippy` on the touched crates, and the touched crates' tests and probes pass.

## Review Focus

1. A guest disconnects at the moment the host presses Start. The game must start without that guest, their companion must be controlled by the game, and no seat may be left waiting. Test in Task 4.
2. Two guests send Choose for the same free companion in the same tick. One gets it, the other keeps their old companion, and both see the same state. Test in Task 2 and Task 3.
3. The save's party changes its rules between Open and Start because the host used Change Save. Choices made for the old save must not be applied to the new one. Test in Task 2 (`Lobby::reset`).
4. A save with no companions at all (a new game). Every guest slot shows "Remote droid", Choose is refused, and Start works. Test in Task 2 and Task 6.
5. A guest on an older build, or a guest who joins while the host is still making fingerprints. The first is refused with the build-mismatch message. The second waits and then enters the lobby, without the old 10 second failure. Test in Task 3.

---

## File structure

| File | Responsibility |
|---|---|
| `crates/sim/src/script/party_choice.rs` (new) | The one function that says which companions can be chosen. |
| `crates/session/src/lobby.rs` (new) | The lobby state and its rules. Pure, no I/O. |
| `crates/net/src/session/lobby.rs` (new) | The five lobby messages and their wire encoding. |
| `crates/net/src/session/msg.rs`, `server.rs`, client side | The lobby phase before `Welcome`. |
| `crates/game/src/lobby_host.rs` (new) | The host's Open step and the bridge between the lobby server and the view model. |
| `crates/game/src/session_flow.rs`, `lib.rs` | `prepare` split into Open and Start. |
| `crates/game/src/net_join.rs` | The guest's lobby phase. |
| `crates/sim/src/world/drivers.rs` | Seats honour a seeded choice. |
| `crates/ui/src/screens/lobby.rs` (new) | The lobby screen. |
| `crates/ui/src/vm.rs`, `intent.rs`, `strings.rs`, `screens/mod.rs` | View model field, intents, words, registration. |
| `crates/ui/src/screens/party.rs` | Calls the shared choice function. |

If `origin/main` has moved a file, follow the code, not this table.

---

### Task 1: The shared choice function

**Files:**
- Create: `crates/sim/src/script/party_choice.rs`
- Modify: `crates/sim/src/script/party.rs` (export), `crates/ui/src/screens/party.rs` or the game-side code that feeds it (call the function)
- Test: probes beside the existing party probes (`crates/edit/src/probes/party.rs`, `companions.rs`, or where `origin/main` keeps them)

**Interfaces:**
- Produces:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice { Open, Unavailable, Required }

/// One answer per recruited companion, in the party selection screen's order.
pub fn choices(party: &Party, party_locked: bool) -> Vec<(u8 /* NPC number */, Choice)>;
```

- [ ] **Step 1: Survey.** Find what state says a companion is recruited, selectable, required, and what says the area forbids a party change, in both games. Read `Party` (`party.rs`), the script routines that set availability and selectability, and what the party selection screen reads today. Write the findings as the module's doc comment in plain sentences.
- [ ] **Step 2: Write failing probes**, one parameterized table: an available companion is `Open`; a companion the story removed is `Unavailable`; a required companion is `Required`; with `party_locked` only current members are `Open` and every other recruited companion is `Unavailable`; a party with no companions returns an empty list.
- [ ] **Step 3: Run them and see them fail.**
- [ ] **Step 4: Implement `choices`.**
- [ ] **Step 5: Make the in-game party selection screen use `choices`** for its availability check. Its behaviour must not change otherwise. Existing party screen probes must still pass.
- [ ] **Step 6: Run probes, fmt, clippy. Commit.**

### Task 2: The lobby state

**Files:**
- Create: `crates/session/src/lobby.rs`
- Test: unit tests in the same file

**Interfaces:**
- Consumes: `Choice` from Task 1 (re-declare a mirror enum in `session` if `session` cannot depend on `sim`; convert in `game`).
- Produces:

```rust
pub const SLOTS: usize = 3;          // slot 0 is the host
pub const COMPANION_PLACES: usize = 2;

pub struct Companion { pub npc: u8, pub name: Vec<u8>, pub portrait: String,
                       pub class: Vec<u8>, pub level: u8, pub choice: Choice }
pub struct Summary { pub save: Vec<u8>, pub area: Vec<u8>, pub game: Game,
                     pub saved: u64, pub hero: Vec<u8>, pub hero_class: Vec<u8>,
                     pub hero_level: u8, pub hero_portrait: String }
pub enum Playing { Hero, Companion(u8), Droid }
pub struct Slot { pub name: String, pub playing: Playing, pub ready: bool, pub delay_ms: u16 }
pub struct Lobby { pub summary: Option<Summary>,      // None while the host is choosing a save
                   pub saber_rules: String, pub collision: Collision,
                   pub companions: Vec<Companion>, pub with_party: Vec<u8>,   // host-filled places
                   pub slots: [Option<Slot>; SLOTS] }

impl Lobby {
    pub fn join(&mut self, name: String) -> Option<usize>;      // first free slot, default companion
    pub fn leave(&mut self, slot: usize);
    pub fn choose(&mut self, slot: usize, npc: u8) -> bool;     // free pick or swap; false if refused
    pub fn fill(&mut self, npc: u8) -> bool;                    // host toggles a game-controlled place
    pub fn ready(&mut self, slot: usize, on: bool);
    pub fn reset(&mut self, summary: Summary, companions: Vec<Companion>, current: &[u8]);
    pub fn party(&self) -> Vec<u8>;                             // the companions that start in the party
    pub fn seats(&self) -> Vec<(usize, Playing)>;               // slot → what it plays at Start
}
```

Rules, all from the spec's "Choosing and swapping" and "Decisions made after the design review":
- `join` gives the first `Open` or `Required` companion nobody plays, current party members first. With none left, `Playing::Droid`.
- `choose` refuses slot 0, an `Unavailable` companion, and a companion not in the list. A companion another guest plays is swapped. A companion in `with_party` is taken from it. A change clears `ready` for every slot whose `playing` changed.
- `Required` companions are always in `party()`. `party()` never has more than `COMPANION_PLACES` entries. A `choose` or `fill` that would exceed it is refused, unless it replaces the chooser's own companion.
- `leave` frees the slot. Its companion stays in the party as game-controlled only if it is `Required`.
- `reset` drops every guest choice and re-runs the `join` default for each connected guest, in slot order.

- [ ] **Step 1: Write failing tests** as one parameterized table of (starting lobby, operation, expected slots/party/ready): free pick; swap; pick of unavailable refused; slot 0 refused; third companion refused; required companion stays when nobody plays it; leave frees; reset re-defaults; no companions gives droids and `choose` is refused; two `choose` calls for one free companion, the second is a swap request against the first chooser.
- [ ] **Step 2: Run and see them fail.**
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run tests, fmt, clippy. Commit.**

### Task 3: The lobby phase on the wire

**Files:**
- Create: `crates/net/src/session/lobby.rs`
- Modify: `crates/net/src/session/msg.rs` (numbers 16 to 20), `crates/net/src/session/server.rs`, the client session code, the protocol version constant, `bumps/feat-coop-lobby.toml` (follow the existing bump files' form)
- Test: the net crate's session tests

**Interfaces:**
- Consumes: `session::lobby::Lobby`.
- Produces: messages `Lobby = 16` (host to guests, whole state), `Choose = 17` (guest to host, `npc: u8`), `Ready = 18` (guest to host, `on: bool`), `Start = 19` (host to guests), `Closed = 20` (host to guests, reason text).
- Produces on the server: a lobby phase in which an accepted connection receives `Lobby` and no `Welcome`; events `ServerEvent::LobbyJoined{slot,name}`, `LobbyLeft{slot}`, `LobbyChoose{slot,npc}`, `LobbyReady{slot,on}`; calls `send_lobby(&Lobby)`, `start()`, `close(reason)`. After `start()`, the kept connections become the session's seats in slot order when the session server is created, and get `Welcome`, `Level` and the keyframe as today.
- Produces on the client: events `LobbyState(Lobby)`, `Starting`, `Closed(String)`; calls `choose(npc)`, `ready(on)`.

- [ ] **Step 1: Read** `msg.rs`, `server.rs` around `Welcome`, `join_waiting` and `free_seat`, and the guest side in `crates/game/src/net_join.rs` (`attempt`, `JOIN_TIMEOUT`).
- [ ] **Step 2: Write failing tests:** a wire round trip for each message (parameterized); a guest handshakes into the lobby and receives `Lobby`; `Choose` and `Ready` reach the host as events; `Start` then `Welcome` arrive in that order; a guest that drops in the lobby raises `LobbyLeft`; `close` delivers `Closed` with its reason; a guest with the old protocol version is refused with the build-mismatch reason; a guest that connects before the lobby server exists but within the handshake retry window enters the lobby.
- [ ] **Step 3: Implement.** The guest's join timeout covers the handshake only. After `Start`, the guest waits for `Welcome` while the connection is alive.
- [ ] **Step 4: Run tests, fmt, clippy. Commit.**

### Task 4: The host's Open and Start

**Files:**
- Create: `crates/game/src/lobby_host.rs`
- Modify: `crates/game/src/session_flow.rs` (`prepare`, `start`), `crates/game/src/lib.rs` (`host_session`, `HostSpec`), `crates/game/src/solo.rs` (`Solo::new` takes the kept connections), `crates/game/src/settings.rs` (settings written from the lobby), `crates/ui/src/intent.rs`
- Test: game-side probes; a headless three-process run in Task 8

**Interfaces:**
- Consumes: Tasks 1 to 3.
- Produces:

```rust
pub enum LobbyIntent { Choose(u8), Fill(u8), Ready(bool), SaberRules(String),
                       Collision(String), ChangeSave, Invite, Start, Leave }
// Intent::Lobby(LobbyIntent) in crates/ui/src/intent.rs
```

- Produces: `vm.lobby: Option<LobbyView>` (Task 6 defines `LobbyView`), filled every frame from the lobby.

- [ ] **Step 1: Open.** `SessionIntent::Host` from the menus now runs Open: decode the save on the worker thread, build `Summary` and `Vec<Companion>` from `sim::Save.script.party` and the companions' templates (names, portraits, class, level), call `choices`, make credentials from the save's game (`saves::Meta.game`) without loading a module, bind the port, open Steam and advertise, create the lobby server, open the `lobby` screen. A new game has an empty companion list and the character creation name as `hero`.
- [ ] **Step 2: Lobby intents.** Apply `LobbyIntent`s to the `Lobby`, and server lobby events likewise. Send `Lobby` to guests on every change. `SaberRules` and `Collision` also write the host's settings. `ChangeSave` sets `summary = None`, opens the host list filtered to the lobby's game, and `reset`s on return. `Leave` calls `close("The host closed the lobby.")`.
- [ ] **Step 3: Start.** Fix the collision (`settings::fix_session_collision`) and saber rules from the lobby. Write `lobby.party()` into the save's script party before the world is made. Load with three seats. Pass `lobby.seats()` so each seat's companion is seeded (Task 5). Hand the kept connections to the session server.
- [ ] **Step 4: The command line path** (`kjk play`, `cargo xtask duo`, the chain) keeps today's direct host path. Add a probe or test that proves it does not open a lobby.
- [ ] **Step 5: Probes:** Open on a K1 save with companions yields the expected `Summary` and companion list (name shown is the character creation name); Start after a swap writes the chosen party; a guest that left before Start is not given a seat; `ChangeSave` to another save drops old choices.
- [ ] **Step 6: Run probes, fmt, clippy. Commit.**

### Task 5: Seats honour the lobby's choice

**Files:**
- Modify: `crates/sim/src/world/drivers.rs` (`assign`), `crates/sim/src/world/remote.rs` if needed
- Test: sim probes beside `no_droid_while_companion_leads`

**Interfaces:**
- Consumes: `(seat, NPC number)` pairs from Start.
- Produces: a way to seed them at world creation. Use `Kept` if it fits without a format change. If a new snapshot field is needed, bump the snapshot format through the bumps file.

- [ ] **Step 1: Write failing probes:** with seats 1 and 2 seeded to the second and first party companion, each seat drives its seeded companion, not the join-order one; a seat seeded to a companion that is not in the party falls back to today's rule; no droid launches while the party has a character for every seat; a seat with `Playing::Droid` launches a droid.
- [ ] **Step 2: Implement.** The seed must be part of the deterministic state so the host and guests agree.
- [ ] **Step 3: Run sim probes and the determinism checks, fmt, clippy. Commit.**

### Task 6: The lobby screen

**Files:**
- Create: `crates/ui/src/screens/lobby.rs`
- Modify: `crates/ui/src/screens/mod.rs` (register `"lobby"`), `crates/ui/src/vm.rs` (`LobbyView`), `crates/ui/src/strings.rs`, `crates/ui/src/probes/menus.rs`, the deliberate-deviations list
- Test: UI probes in `menus.rs`

**Interfaces:**
- Consumes: `vm.lobby`, `LobbyIntent`.
- Produces: `LobbyView { host: bool, own_slot: usize, lobby: Lobby (or a mirror with display strings), address: String, steam: bool, movie_duels: bool }`.

Exact words (add to `strings.rs`):
- Title "Co-op Lobby". Panel heads "Players", "Game", "Companions".
- Slot states "Host", "Choosing", "Ready", "Connecting". Empty slot "Open" and "Waiting for a player". Droid "Remote droid".
- Game rows "Save", "Area", "Game", "Saved", "Party". While the host changes save: "The host is choosing a save."
- Companion labels "Free", "With the party", "Not available", or the player's name.
- Description texts: "{name} is not available in this part of the story." / "{name} is played by {player}. Choose them to swap characters with {player}." / "{name} is free. Choose them to play as {name}." / host on a free companion: "Choose {name} to add them to the party. The game controls a companion no player has chosen." / guest on a setting: "The host chooses the saber rules and the collision." The saber rules and collision rows reuse the options page's descriptions.
- Buttons "Leave", "Invite Friends", "Change Save", "Start Game", "Ready". Address line "Friends join at {address}".
- Guest loading line after Start: "The host is loading the game." Closed message: "The host closed the lobby."

- [ ] **Step 1: Pick the art.** Use KOTOR 2's frame, panel borders, list rows, buttons and portrait frames, as the options and party selection screens load them. Write which pieces are used in the module doc.
- [ ] **Step 2: Write failing probes** (parameterized over host view and guest view): every row and label above appears for a sample lobby; the guest's settings rows are greyed and emit no intent; an unavailable companion is darkened and emits no intent; the cursor on another guest's companion shows the swap text; an empty companion list shows "Remote droid" in guest slots; Start Game emits `LobbyIntent::Start` for the host only; a saber rules choice that needs Movie Duels is offered only with it installed.
- [ ] **Step 3: Implement the screen** with the `Screen` trait. Layout as in the spec: Players left, Game right, Companions row, description strip, buttons. Hover selects. Keyboard and gamepad reach every control.
- [ ] **Step 4: Record the layout** in the deliberate-deviations list.
- [ ] **Step 5: Render the screen headless** to PNG in the host view and the guest view, with a save that has an unavailable companion, at 1280×960 and 1920×1080. Save the images under `kjk-artifacts/coop-lobby/` in the worktree. Look at them and fix what is misaligned or clipped.
- [ ] **Step 6: Run probes, fmt, clippy. Commit.**

### Task 7: The guest's side, Steam and the address

**Files:**
- Modify: `crates/game/src/net_join.rs`, `crates/game/src/lib.rs` (`join_session`), `crates/game/src/steam_join.rs`, `crates/net/src/io/steam.rs`
- Test: net and game probes

- [ ] **Step 1: Guest flow.** After the handshake the guest opens the `lobby` screen from `LobbyState`. `LobbyIntent::Choose` and `Ready` go to the host. `Leave` disconnects. On `Starting` the guest shows the loading screen with "The host is loading the game." and continues with today's `Welcome`/`Level` path. On `Closed` the guest returns to the main menu with the reason in the message screen.
- [ ] **Step 2: Invite Friends.** Add a call that opens Steam's invite dialog for the advertised game. `LobbyView.steam` is false and the button is hidden when Steam is not running.
- [ ] **Step 3: Address.** Show the host's local network address and port. Make no request to any outside service.
- [ ] **Step 4: Tests:** a guest receives the lobby, chooses, and sees its own slot change; `Closed` returns to the menu with the reason; the feedback report still redacts addresses (existing probe in `crates/game/src/report/probes.rs`).
- [ ] **Step 5: Run tests, fmt, clippy. Commit.**

### Task 8: The whole thing together

- [ ] **Step 1: A three-process headless run:** a host opens the lobby on a K1 save with at least three recruited companions, two guests join, one swaps with the other, the host starts, and all three reach the level with matching sim hashes and each guest driving its chosen companion. Add it as an `xtask` or extend the duo task, with a flag that drives the lobby without a person.
- [ ] **Step 2: Review Focus cases** not yet covered by a test get one now. Check all five.
- [ ] **Step 3: `cargo xtask duo`** passes without the lobby. The approved tests pass.
- [ ] **Step 4: One K1 chain run.** Report its count.
- [ ] **Step 5: Whole-workspace `cargo fmt --check`, `cargo clippy`, tests and probes.** Review the diff for duplication and narrative comments and fix them.
- [ ] **Step 6: Commit. Write a handover** at the end of this plan file in the worktree: what is done, what is not, what was decided where the plan left a choice, every command run with its result, and the paths of the screenshots.

---

## Handover

Branch `feat/coop-lobby` in the worktree `~/.config/superpowers/worktrees/kjk/coop-lobby`, cut from `origin/main` at 2ae0b572. Nothing is pushed and no pull request is open. Every task has its own commit:

| Task | Commit | Title |
|---|---|---|
| Plan | a0839d0c | Co-op lobby design, plan and mockup |
| 1 | e536ee57 | Shared rule for which companions can be chosen |
| 2 | 5440913e | Co-op lobby rules |
| 3 | 94526507 | Co-op lobby network messages |
| 5 | 038a054e | Seats that play the companions chosen in the lobby |
| 4 | c08fe109 | Co-op lobby opened by the host before the game loads |
| 6 | f96be808 | Co-op lobby screen |
| 7 | 3f17588d | Guests in the co-op lobby |
| 8 | a747bf97 | Co-op lobby run with a host and two guests |

### What is done

All eight tasks. The menu's Host opens the lobby after the host picks a save or makes a new character. Guests join it, choose companions and mark Ready. The host sets the saber rules and the collision, fills the companion places no guest took, and starts the game. The world then loads with three seats, the lobby's connections are kept, and each guest is welcomed into its seat driving the companion it chose. The command line's `kjk host`, `cargo xtask duo` and the chain go straight into the game as before.

### Decisions where the plan left a choice

- **The seeded choice uses `Kept` as it is** (`sim::Save::seat_companions`). No snapshot or save format changes, so there is no bumps file. The net protocol is not one of the bumps formats: `PROTOCOL_VERSION` in `crates/net/src/wire.rs` goes from 7 to 8 with its history line.
- **The party is written as queued party changes** in the save's pending scripts (`lobby_host::queue_party`): `RemovePartyMember` for members no player chose, then `SpawnAvailableNPC` and `AddPartyMember` for chosen companions outside the party. These are the routines the games' scripts call, as the save editor queues them.
- **The lobby server is the session server in a lobby phase** (`Server::lobby`, `Server::begin`). The host's game keeps it open in `solo::OpenNet` and hands it to `Solo::new` through `solo::Listening::Lobby`. `begin` checks each kept guest's credentials again against the world's (its stance sets may differ), and refuses a guest whose seat the world does not have.
- **The screen is built from KOTOR 2's party selection layout** (`partyselect_p`), its pieces moved and copied in `Screen::shape` (`crates/ui/src/screens/lobby.rs` lists which). The companion row holds ten, the most either game has recruited at once. A portrait the story took away is shaded with a translucent black quad. This is a colour, not drawn art. The layout is recorded in `docs/UI.md` as a deliberate deviation.
- **A guest's saber rules and collision rows are greyed but still reachable**, so the description strip can tell the guest that the host chooses them. Back leaves the lobby, as Leave does.
- **The guest's lobby goes through a bridge** between the join thread and the screens (`crates/game/src/lobby_guest.rs`). The join's 10 second timeout now covers the handshake only. After the first lobby state the join waits as long as the connection lasts.
- **Change Save** reads the chosen save on a thread. A save of another game is refused after it is read, with "Choose a save of the same game. To host the other game, leave the lobby and host again." in the feedback line. The host list itself is not filtered by game.
- **The address line** finds the host's local address by "connecting" a UDP socket to a private address, which sends nothing. No outside service is asked.
- **Invite Friends** calls Steam's `ActivateGameOverlayInviteDialogConnectString` with the connect string "Join Game" uses (`net::io::steam::invite`).
- **Test hooks:** `kjk host --load ID --lobby N` opens the lobby with no player and starts once N guests are Ready. `kjk join --lobby-script ready|swap` makes a guest mark Ready, or swap with the other guest first. `kjk gui screen k1|tsl lobby` draws the screen on the games' data, with the step `lobby guest` for a guest's view.

### What is not done or not checked

- Not checked in a window by a person, and not checked through Steam (no Steam run here). Invite Friends is untested.
- On the chain checkpoint `k1-10` (Dantooine, where the save's party is empty), a lobby that puts two companions in the party got only one of them: Bastila did not join. The Dantooine council scene may hold her. This was not investigated. The checkpoint `k1-08` (Taris, party of two) works end to end.
- The spec's question of whether enemy scaling counts seats or connected players at Start was not examined.
- `docs/COOP.md` does not describe the lobby yet.
- A lobby has no join password (the menu's Host never had one).
- Two sim probes fail, `fp_regeneration` and "the lab station". They fail the same way on the `feat/action-bar` build, which contains the same `origin/main` base, so they are not from this branch.

### Commands run and their results

All `kjk` runs were headless and muted, with `KJK_SETTINGS` set to a scratch copy of the settings file and `KJK_SAVES` set to a scratch folder. The chain checkpoints used were copies of the owner's `k1-08` and `k1-10`; the originals were not changed.

- `cargo build --release -p launcher --bin kjk`: builds. (After the disk filled during the session, `cargo clean --release -p game -p launcher` was needed once to clear half-written objects.)
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings`: clean.
- `cargo test --release -p approved_tests`: 135 passed (including `coop_lobby` with 14 checks and `coop_lobby_wire` with 16).
- `cargo test --release -p session -p net -p ui -p sim`: pass.
- `kjk sim probes`: 1405 checks, 2 failed (the two above, both from `origin/main`). The groups `lobby`, `lobby host` and `remote` (with the three seeded-seat cases) pass.
- `kjk gui probes`: all pass, the seven lobby screen checks among them.
- `kjk net probes`: 89 checks, 0 failed.
- `kjk sim determinism`: passed.
- `cargo xtask duo`: passed (1957 snapshots rebuilt, none unlike the host's).
- `cargo xtask lobby --saves <copy> --load k1-08`: passed. Mia joined with T3-M4 and Kalle with Bastila. Kalle swapped, so Mia played Bastila and Kalle T3-M4. Both loaded `tar_m09aa`. Guest A rebuilt 660 snapshots and guest B 682, none unlike the host's 2638. The host's dump shows seat 1 driving Bastila and seat 2 driving T3-M4. Report: `kjk-artifacts/runs/1794767696-lobby/lobby.md`.
- The K1 chain was not run for this work, at the owner's request (Task 8 Step 4 skipped). One run had been started before that request came. It was stopped partway, when its first 16 routes had all met their goals, and its result does not count.
