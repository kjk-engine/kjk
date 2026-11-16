# A guest as the main character

Date: 2026-11-16. Status: design, waiting for the owner's approval. It builds on the co-op lobby (pull request 474, `docs/superpowers/specs/2026-11-15-coop-lobby-design.md`).

## Intent

In the co-op lobby the host always plays the main character. The owner wants any player to be able to play the main character. Whoever plays the main character answers the story dialogue. The host can then play a companion, and is still the network host and the owner of the save.

## Goals

1. In the lobby, a guest can choose the main character.
2. The player of the main character answers story dialogue and has every other power the game gives the main character's player: the party leader, the galaxy map, returning to the Ebon Hawk, the workbench, the party selection screen, dressing a companion nobody plays, and forcing a ready check.
3. The host keeps the powers of the host's machine: saving, autosaving, loading, the reload after the party falls, starting the game, and the lobby's settings.
4. A host who does not play the main character plays a companion like any guest, with the same camera, display and controls a guest has today.
5. If the main character's player loses their connection, their place is kept for 60 seconds. If they are not back by then, the host takes over the main character, and the game controls the companion the host played.

## Non-goals

- Changing who plays the main character during play. The choice is made in the lobby only.
- Storing the choice in the save. A save still belongs to the host. When the host loads it alone, or hosts it again, the host plays the main character until someone swaps in the lobby.
- Any change to solo play or to hosting from the command line.
- Letting two players share the story's choices.

## Anti-goals

- The simulation's rules for the first seat must not be rewritten. Solo play and today's co-op must produce the same game states as before, step for step.
- The story must never be left without a player who can answer it. The main character is always played by someone.
- A save made in a game where a guest played the main character must load exactly like any other save.

## Approaches

**Way A, chosen: swap who sits in the first seat.** The simulation keeps one seat that means "the player of the main character" (`LEADER_SEAT`, the first seat). About 60 rules read it. Today the host's machine always sits in it. The change lets another machine sit in it. Every rule that reads the first seat then moves to that player without being touched. The work is in the places that assume the host's machine is the first seat.

**Way B, rejected: store who the story player is.** A new field in the game state names the story player, and every one of the 60 rules reads it. The main character's body (`Kind::Avatar(Seat(0))`), its persona and the save format also stop assuming the first seat. The meaning is cleaner, but the save format changes and the risk to solo play is much higher.

**Way C, rejected: the guest moves the main character and the host answers dialogue.** The owner chose against it on 2026-11-15. A player who plays the main character and cannot answer for them would feel wrong.

The owner picked "whoever plays the main character answers" on 2026-11-15, and the disconnect rule on 2026-11-16. Way A is the designer's choice of mechanism.

## How it works today (branch feat/coop-lobby)

- The main character is the only player body that is not a companion creature. It is the first seat's own avatar (`Kind::Avatar(Seat(0))`). Guests drive companion creatures through `Mind::driver` and `Kept`. That mechanism cannot hand the main character to a guest.
- The first seat driving a creature already has a meaning: a story section with a switched character (`SwitchPlayerCharacter`, `world/switch.rs`). So the host cannot simply be given a companion in the first seat.
- The host's in-process peer is always the first network seat (`net/src/session/server.rs`). In the lobby a slot number is a seat number (`session/src/lobby.rs`).
- The host's authority loop reads the first seat as "this machine's seat" for its own controls (`game/src/sim_host.rs`, around the local input reads). The defeat screen's host button and the galaxy map flag test for the first seat (`ui_view.rs`, `script_screens.rs`).
- Saving, autosave, loading and the party-wipe reload are gated on being the network host (`solo.hosting()`, `WorldHere`), not on a seat.
- The camera and display follow `snapshot.body(own seat)`, so they follow whatever seat a machine has.
- `drivers.rs` never releases the first seat when its player stops sending commands, because that player was always the host.
- `Snapshot::players_driving` and `lobby_host::started` count the host as always driving a body.

## Design

### Seats

The simulation is unchanged. Its first seat is the main character's player. At Start, the lobby decides which machine gets which simulation seat:

- The player who chose the main character gets the first seat.
- Every other player gets a guest seat and the companion they chose, or a remote droid, as today.

The host's machine can therefore have a guest seat. The network server, not the simulation, learns that its own in-process peer is no longer always the first seat.

### The lobby

- The main character is the first portrait in the Companions row. Its label is the name of the player who plays it.
- The main character is always played by someone. It is never "Free". Selecting it asks for a swap with the player who has it. The swap happens at once, like any other swap, and clears the Ready mark of both players.
- The description strip says: "<Main character's name> is the main character, played by <player>. Choose them to swap characters with <player>. The main character's player answers the story's dialogue."
- A player who swaps away the main character gets what the other player had. That is a companion, or a remote droid when the other player had no character.
- The host can select the main character to take it back.
- A slot keeps its order on the screen. The host stays in the first slot and keeps the "Host" state. The slot of the main character's player shows the main character's portrait, name, class and level.
- The lobby's slot numbers stop being seat numbers. `Lobby::seats()` returns each slot's simulation seat: the first seat for the slot that plays the main character, and the remaining seats for the other slots in slot order.

### A game never starts without the story's character

The owner stated this rule on 2026-11-16. Someone always plays the main character, and a game cannot start without that.

The one exception is a part of the story where the game itself makes the player play another character, such as a KOTOR 2 section played as Atton alone. In such a save the story's character is that other character. The lobby then shows that character in the main character's place, with that character's own name, portrait, class and level. The rule is the same: someone always plays that character, a player takes it only by a swap, and the game cannot start without it. The real main character is not in the party in such a section and cannot be chosen.

The lobby checks this at Start as well. If no occupied slot plays the story's character, Start gives it to the host before the game loads.

### During play

- The first seat's rules apply to the main character's player with no change. That player leads the party, answers story dialogue, opens the galaxy map and so on.
- The host's machine does the work of a host from whatever seat it has: it runs the simulation, saves, autosaves, loads and reloads after the party falls.
- Screens and buttons that mean "this machine is the host" test for the host, not for the first seat. The known ones are the defeat screen's button and the local input reads in the authority loop.
- Screens that mean "this player is the main character's player" keep testing the first seat. The known ones are the leader controls, the galaxy map, the dialogue request card and the stance picks for companions nobody plays.
- A story section with a switched character works as today. The first seat follows the switch, whoever sits in it.

### When the main character's player disconnects

1. The first seat is kept for that player for 60 seconds, the same time a guest's companion is kept today. The main character stands still. A conversation waits.
2. If the player returns within 60 seconds, they get the first seat back.
3. If they do not, the host's machine moves to the first seat and plays the main character. The companion the host played is controlled by the game.
4. A player who returns after that joins as an ordinary late guest and gets the first free companion, or a remote droid.

The move in step 3 is one deterministic change that every machine applies on the same step.

If the host is the main character's player, nothing changes: the host leaving ends the game, as today.

### Decisions made after the design review

These are the designer's calls, not the owner's words. The owner can overrule any of them.

1. **The game keeps running during the 60 seconds.** The simulation gets no new rule. The first seat sends no input, so the main character stands where they are. Enemies keep fighting, and the main character can be downed like any character whose player is idle. The other players keep playing. The host can still save. An area transition or a ready check that needs the main character's player waits until that player returns or the host takes over.
2. **The main character's player keeps the first seat for the whole session.** A load during the session, and the reload after the party falls, do not put the host back in the first seat. The link from player to seat belongs to the session, not to the loaded world.
3. **A player who leaves on purpose is replaced at once.** If the host can tell a deliberate leave from a lost connection, the host takes over the main character immediately. If it cannot tell, the 60 second rule applies.
4. **In the lobby, the main character goes back to the host when its player leaves.** This also covers a lost connection in the lobby. The companion the host had becomes free again.
5. **Two notices are reworded.** "Only the host equips companions nobody plays." and "Only the host can go now, after 5 seconds." name the host today. Both powers belong to the main character's player, so both texts say "the main character's player".

### Saves

- Nothing new is stored in a save. The save has the main character as the first seat's avatar, as always.
- The count of players for enemy scaling counts every player who has a body, whichever machine has the first seat. A host with a remote droid is not counted.

### What the builder decides

- Where the mapping from network peer to simulation seat lives, and how the host's in-process peer is given a seat other than the first.
- How the takeover in step 3 is carried: a new action from the host, or a rule in `drivers.rs` that reads the kept time for the first seat.
- The list of "is this machine the host" tests that today read the first seat. The survey found three. The builder greps for the rest.

## Who makes the main character

The owner decided on 2026-11-16 that the host makes the main character. In a new game the host goes through character creation, and the lobby opens afterwards, as it does today. A guest who then takes the main character plays the character the host made, with its class, look and name. Character creation does not move.

## Constraints

- All constraints of the lobby design apply: KOTOR 2's menu behaviour and art, plain text, the game's code page, no chain unless a route can be named that would fail.
- Solo play, `cargo xtask duo` and the existing lobby run must give the same results as before this change.

## Testing

- Lobby rules: a guest takes the main character and the host gets the guest's companion; the host takes it back; a guest with a remote droid takes it and the host gets the droid; the main character can never be left unplayed; Ready marks clear for both players; `seats()` gives the first seat to the right slot.
- Screen checks: the main character's portrait is first in the row with its player's name; the description text; the host's slot keeps "Host" while showing a companion.
- A three-process lobby run where a guest plays the main character: the guest's machine leads the party and answers a story conversation, the host's machine plays a companion and still writes the save, and every game state the guests rebuild matches the host's.
- A run of the disconnect rule: the main character's player drops, returns within 60 seconds and keeps the seat; and drops for longer, after which the host plays the main character and the game controls the host's old companion.
- The determinism check, `cargo xtask duo`, and both existing cases of `cargo xtask lobby` still pass.
