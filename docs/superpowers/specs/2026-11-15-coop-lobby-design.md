# Co-op lobby

Date: 2026-11-15. Status: the owner picked way 1 and asked for the implementation on 2026-11-15. The decisions under "Decisions made after the design review" are the designer's and were not confirmed by the owner.

Mockup: `scratchpad/lobby-mockup.html` of the design session. The mockup fixes the layout and the words. The real screen is drawn with KOTOR 2's own art.

## Intent

Today the host picks a save and the game loads at once. A guest arrives in the middle of a running game. Nobody sees who else is playing, which character they will get, or which saber rules and collision mode apply.

The owner wants a lobby. Everyone gathers in it before the game starts. The lobby shows the three player slots, who has joined, the saber rules, the collision mode and the save that is about to be played. Each guest chooses the companion they will play. The host starts the game when the group is ready.

The lobby must look like a screen KOTOR 2 could have shipped. The owner's words were "really nice and awesome, no slop".

## Goals

1. The lobby opens after the host picks a save or finishes character creation, and before the world loads.
2. A guest who joins arrives in the lobby without loading a level.
3. The lobby shows three player slots. A slot shows the player's name, their character with its class and level, and their state. The states are Host, Choosing, Ready and Connecting. An empty slot says "Waiting for a player".
4. The lobby shows the save's name, its area, the game (KOTOR or KOTOR 2), the date it was saved and the party.
5. The host changes the saber rules and the collision mode in the lobby. Guests see both values and cannot change them.
6. Each guest chooses a companion from every companion the save has recruited, within the story's rules (below).
7. A guest who chooses a companion that another guest plays swaps characters with that guest.
8. The host presses Start Game. Every player then loads the world at the same time, with the chosen characters in the party.

## Non-goals

- A lobby for a player who joins after the game has started. That player skips the lobby and gets the first free companion, as today.
- Storing time played in saves. The lobby shows the save's date.
- Removing a player from the lobby by the host (a kick).
- Text chat.
- More than three slots.
- A lobby for solo play.
- Looking up the host's public IP address. The game makes no request to an outside service.

## Anti-goals

- The lobby must never let the players start with a party the story would not allow. A companion the story has taken away must not appear in the game because a guest chose them.
- No hand-drawn or invented art. Every frame, button, list and portrait is KOTOR 2's own.
- No second set of rules for which companions can be chosen. The lobby and the in-game party selection screen must use one shared function, so they cannot disagree.

## Approaches

**Way 1, chosen: the lobby runs before the world loads.** The host decodes the save, opens the lobby and answers guests at once. The lobby has its own network messages. Start Game makes every player load the world. The saber rules and the collision mode can change freely because nothing is loaded. The chosen characters are written into the party before the world is made. This is the larger change: the network code gets a waiting state and new messages, and the host's start path is split around the Start button.

**Way 2, rejected: the lobby is drawn over a loaded, paused world.** The network change is small, because the pause message already exists. But every player sits through a loading screen before they see the lobby. Changing the saber rules or the collision mode forces everyone to load again. The player count is fixed before anyone joins.

**Way 3, rejected: a lobby on the host only.** The host sees who has connected, and guests see a waiting message. Guests could not choose a character or see the settings, which is most of what the owner asked for.

The owner picked way 1 on 2026-11-15.

## How it works today (origin/main at 2ae0b572)

- `session_flow::prepare` runs the whole host path on one thread: it fixes the collision, decodes the save and calls `host_session`.
- `host_session` (`crates/game/src/lib.rs`) binds the port and opens Steam, loads the world, then makes the credentials with the world-data fingerprints.
- The `net::Server` is created in `Solo::new`, during `sim_host::install`, from the start snapshot and the level. Nothing answers a guest's handshake before that. A guest gives up after 10 seconds (`JOIN_TIMEOUT`).
- After the handshake the server sends `Welcome`, then `Level` and a keyframe snapshot. The guest loads the level from the `Level` message.
- The collision mode is a process global fixed in `prepare`. Guests get it in `Welcome.collision`.
- The saber rules come from the `DuelOptions` resource at install and travel in the snapshot. They also decide `Welcome.stances`.
- `drivers::assign` gives each guest seat the first free party creature in party join order. No message or action pins a seat to a companion. `Kept` (seat to companion number) is the closest existing hook.
- A guest's name travels in `Credentials.name`. The host only logs it. `vm.players` exists and nothing fills it.
- `saves::Meta` has the name, module, area, save date, game and the main character's description. The party is only in `sim::Save.script.party`, which needs a full decode of the save and no world.

## Design

### The flow

1. The host chooses a save or a new game on the "Host a game" screen. A new game goes through character creation first, as today.
2. The game decodes the save on a worker thread and shows the loading screen's status line while it does. It makes the host's credentials from the save's game. The first run can spend minutes on fingerprints, as today.
3. The lobby screen opens. The host's server answers handshakes from this moment. Steam presence is advertised from this moment.
4. Guests join from the Join screen, a Steam invite or a friend's "Join Game". Each accepted guest gets a lobby slot and sees the lobby screen.
5. Players choose characters. Guests mark themselves Ready. The host changes the settings.
6. The host presses Start Game. The host does not have to wait for Ready marks or for full slots.
7. The host fixes the collision mode and the saber rules, writes the chosen party into the save's party, and loads the world. The server then sends each guest `Welcome`, `Level` and the keyframe, as today. Each guest loads the level.
8. A seat plays the companion its player chose.

Change Save sends the host back to the "Host a game" list. The guests stay connected and see "The host is choosing a save." in the Game panel until the host comes back.

Leave closes the lobby for the player who pressed it. When the host leaves, every guest returns to the main menu with the message "The host closed the lobby."

A guest who disconnects frees their slot and their companion.

### The screen

The layout follows the mockup.

- **Title.** "Co-op Lobby".
- **Players panel, left.** Three slots. Each slot has the character's portrait, the player's name, the character's name, the class and level, and the state. A guest's slot also shows the connection delay in milliseconds. The viewing player's own slot is drawn in the selected colour.
- **Game panel, right.** Save, Area, Game, Saved and Party. Then the Saber rules row and the Collision row. Then the address line.
- **Companions row.** One portrait for every recruited companion, in the order of the game's party selection screen. Under each portrait is the companion's name and one of four labels: "Free", the name of the player who plays them, "With the party" for a companion the game controls, or "Not available".
- **Description strip.** It describes the row or portrait under the cursor, as the options pages do.
- **Buttons.** The host has Leave, Invite Friends, Change Save and Start Game. A guest has Leave and Ready.

The screen is a new layout, because KOTOR 2 has no lobby. It is assembled from KOTOR 2's own frame, panel borders, list rows, buttons and portraits. It follows KOTOR 2's menu behaviour: the cursor selects on hover, and the keyboard and the gamepad reach every control. The new layout is added to the list of deliberate deviations from KOTOR 2's UI.

The saber rules row and the collision row reuse the options page's choices, names and descriptions. A choice that needs Movie Duels is offered only when Movie Duels is installed, as on the options page. A change in the lobby is saved to the host's settings.

The address line shows the port and the host's local network address. Invite Friends opens Steam's invite dialog. The button is hidden when Steam is not running.

### Who can be chosen

One function decides which companions can be chosen, and both the lobby and the in-game party selection screen call it. It reads the decoded save's party and story state. It returns, for each recruited companion, one of three answers: can be chosen, not available, or required.

1. A companion the story has made unavailable cannot be chosen. The portrait is darkened. The description strip says "<Name> is not available in this part of the story."
2. A companion the story requires in the party is always in the party. If no player chooses them, the game controls them, and they still take one of the two companion places.
3. In an area where the game forbids changing the party, only the current party's companions can be chosen. Players can still swap among them.
4. In a part of the story with a fixed character, the lobby follows the save. Examples are the lone rescuer on the Leviathan and the KOTOR 2 sections played as Atton, Mira or T3-M4.
5. A player the party has no character for gets a remote droid. Their slot shows "Remote droid". This follows the existing rule that party size alone decides droids.

The host plays the save's main character. A guest cannot take it, because the host answers the story dialogue.

The main character is always shown by the name the player gave in character creation. The lobby never shows a name the story reveals later. The same holds for companions: a companion is shown by the name the game itself shows for them at that point in the save.

### Choosing and swapping

- A guest who joins is given the first free companion that can be chosen, in the current party first. They can change it.
- A guest who chooses a free companion plays that companion. Their old companion becomes free.
- A guest who chooses a companion that another guest plays swaps with that guest at once. The description strip says so before the choice: "<Name> is played by <player>. Choose them to swap characters with <player>."
- A change of character clears the Ready mark of every player whose character changed.
- When there are fewer guests than companion places, the host chooses the companion for each place no player took. The game controls that companion. The default is the save's current party member.
- The party never has more than two companions. A third choice is not possible.

The host decides every choice. A guest sends a request, and the host answers with the new lobby state. Two requests for the same companion are resolved in the order the host receives them.

### The network

The handshake is unchanged. After Accept, a guest is in the lobby instead of receiving `Welcome`.

New session messages, numbered after `Explosion` (15):

| Message | Direction | Contents |
|---|---|---|
| Lobby | host to guests | The whole lobby state: the save's summary, the settings, the companions with their answers from the choice function, and each slot's player name, choice, Ready mark and delay. Sent on every change. |
| Choose | guest to host | The companion number the guest wants. |
| Ready | guest to host | The guest's Ready mark, on or off. |
| Start | host to guests | The game is starting. `Welcome`, `Level` and the keyframe follow when the host's world is loaded. |
| Closed | host to guests | The host left or the lobby ended, with the reason as text. |

The guest's join timeout no longer covers the host's world load. It covers the handshake only. After `Start`, a guest waits for `Welcome` for as long as the connection is alive, and shows the loading screen with "The host is loading the game."

The protocol version is bumped. A guest on an older build is refused with today's build-mismatch message.

### The host's start path

`prepare` is split in two.

- **Open.** Decode the save, make the credentials, bind the port, open Steam, create the lobby server. The result is a lobby that the UI reads through the view model.
- **Start.** Take the lobby's final state. Fix the collision mode. Set the saber rules. Write the party. Load the world with three seats. Hand the already-open connections to the session server made in `Solo::new`.

The chosen characters reach the sim in two steps. First, the party's members are set in the save's script state before the world is made, so the story's scripts see an ordinary party. Second, each seat's choice is seeded so that `drivers::assign` gives that seat that companion. `Kept` is the existing hook for this.

Running the game from the command line (`kjk play`, the duo and chain tasks) skips the lobby and behaves as today.

### Decisions made after the design review

These are the designer's calls, not the owner's words. The owner can overrule any of them.

1. **Change Save stays within one game.** The Change Save list shows only saves of the game the lobby was opened for, and a new game of that game. The guests passed the fingerprint check for that game, so they stay connected. To host the other game, the host leaves the lobby and hosts again.
2. **The host uses the Companions row too.** The host selects a free companion to put them in a companion place that no guest took. The label becomes "With the party". Selecting that companion again frees the place. The host cannot change or remove a guest's choice. No network message is needed for this, because the host owns the lobby state.
3. **A guest cannot choose the remote droid.** The droid is only shown as a state. The Choose message carries a companion number and nothing else.
4. **In a part of the story with a fixed character, the host plays that character.** This is today's rule for who leads the party. Guests play the other party members if there are any, and fly remote droids if there are none.
5. **The in-game party selection screen changes in one way only.** It calls the shared choice function for its availability check. It does not learn which player plays which companion.
6. **The lobby opens only from the "Host a game" screen.** Loading another save during a running co-op game, and the reload after the party falls, behave as today.

### What the builder decides

- The exact wire layout of the new messages and how the lobby server and the session server share connections.
- Whether the seeded choice uses `Kept` as it is or a new field beside it. A new snapshot field needs a format bump and a bumps file.
- How the lobby state reaches the view model (`vm.players` is unused today and is the natural place).
- Which story state the choice function reads for "not available", "required" and "party cannot change", in KOTOR and in KOTOR 2. The builder surveys what the games' scripts set and what the party selection screen reads today, and makes the shared function from that.
- Whether enemy scaling counts seats or connected players today, and how to make it count the players present at Start.
- The KOTOR 2 art pieces each part of the screen uses.

## Constraints

- Menus follow KOTOR 2's behaviour. A deviation is written down.
- Game text stays in the game's code page. Player names are shown as received.
- No class or function names from the original executables in shipped text.
- Format changes are numbered at merge time through a bumps file.
- Every `kjk` run in development is headless, muted, and uses a scratch settings file.

## Testing

- UI probes for the lobby screen in the host's view and a guest's view: every row, the greyed settings for a guest, a darkened companion, and the description strip's texts.
- A probe for the choice function for each rule above, on saves from both games.
- Net tests: a guest joins into the lobby, chooses, swaps with another guest, marks Ready, and receives Start. A guest disconnects and the slot opens. The host leaves and guests get Closed.
- A sim probe: after Start, each seat drives the companion its player chose, and no droid flies while the party has a character for every seat.
- A three-client run through the lobby into the first steps of a save, with matching sim hashes.
- The existing duo run and the K1 chain still pass without the lobby.
