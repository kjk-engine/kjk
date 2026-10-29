# The party as one: in combat, the break, the ready check

`sim::party` and
`game::party_calls`; in the snapshot: `in_combat`, `paused_by`, `ready`.

## In combat (`Snapshot::in_combat`, `world::InCombat`)
- True after a step when a living character outside the party (no
  `Avatar`, no `mind.leader`) is fighting (a `target`, `Task::Engage`/`Chase`,
  or `combat_until` within 8 s) within `ranges.2da`'s `PartyCombat` (35 m in
  K1, `ModuleData::ranges`) of a party member who is not dead (a downed one
  counts) and to whom it is hostile (`module::relation`: its faction's
  regard ≤ 10). Never a plot one (it cannot fall: the Sand People's wall
  turrets on Tatooine would hold the party in combat for good) [verify].
  `party::keepers` lists them; `SimWorld::combat_keepers` by id.
- With friendly fire on (`Area`, `Full`), also when a party member not dead
  was struck by another within 8 s (`Fighter::last_attacker`): a duel is
  a fight. With it off (co-op's default) only hostiles count.
- `party::combat`, each step after the hurts; without a module only the
  duel case; false after an install. KOTOR's own rule and line [verify].

## The break (P10)
- `Action::Break` (any seat) freezes the world for everyone, refused in
  combat (`Notice::InCombat`); one sent while a break holds is dropped.
  Its caller or seat 0 ends it (`Action::Resume`; anyone else:
  `Notice::NotYourBreak`). Solo too. `party::breaks` runs first; while a
  break holds a step moves its tick and nothing else (`party::running`).
- The host steps no ticks while one holds, as for a movie (`Waited`,
  `Frozen`: sounds pause, beds go on). A waiting resume steps once, the
  other actions held back for the step that ends the break. The network
  holds its players meanwhile (message 14 `Hold`, `COOP.md`).

## The ready check (P16)
- A walk into an exit (a trigger, an open doorway, a door run into or
  used, from anywhere within reach) proposes the move (`ReadyCheck`). A
  seat is ready when it walked in or used it, is its tenant, stands within
  `script::DOOR_REACH` of an exit door's shape, or sends `Action::Ready`;
  a seat with no body counts as ready. Alone, the party goes at once.
- The party goes when everyone is ready, or when seat 0 sends
  `Action::Force` after `FORCE_AFTER_MS` (5 s; else `Notice::CannotForce`).
- The asker gone, the lowest seat still there asks; none: it lapses. In
  combat a walk in is refused, told once, asks again after; a fight holds
  a proposal (forcing too). A script's `StartNewModule` skips it.
- While more than one seat's body stands : a seat in a locked conversation, a pazaak match, or a party
  conversation it is not the unlocked listener of is kept unready
  (`party::kept`; its ready is refused and its bit cleared each step),
  and the banner says why ("waiting for player 2 (playing pazaak)",
  `Snapshot::kept_from_exit`). A seat readying from an unlocked
  conversation leaves it (a Leave, so its `EndConverAbort` runs once)
  and from a store ends its trade (`party::Leaving`). The check reads
  the conversations and matches as the last step left them, as
  prediction does. The host's Force still goes, ending the rest with the
  module (`DIALOGUE.md`, `PAZAAK.md`).

## Screens and keys
- Z calls or ends a break (during another's, it asks to end it and is
  told it is not its own); Y is ready (at the host, ready and after 5 s:
  go now). The in-game menu's autopause button is Break/Resume (greyed in
  combat or for another's break). The banner, KOTOR's combat message over
  everything: "Paused by player N", or "player N wants to go to M. Ready:
  …; waiting for …".
- Console: `ui break|resume|ready|force`, `wait paused|ready_check`, the
  dump's `calls`, log lines `calls: …` (the duo report lists them).
  `kjk sim probes --only party` (20); `kjk net session --only hold`.
  The ready check beside conversations, stores and matches is
  `dialogue_coop`'s (`sim::coop_talk`, its `exits` phase).
  Approved tests `party_calls` and `hold_and_resume`.
- Tab makes the next party member lead (seat 0 only; `leader_view`); a
  seat with no party member free flies a remote droid (`COOP.md`), and
  one whose body is down watches the next with N.
- Switched to another character (`SwitchPlayerCharacter`, `world::switch`),
  seat 0 leads through a companion the same way (`world::lent`): the one
  switched to stands as the seat's own character would, its portrait takes
  the lead back, and a switch either way makes the one played lead (TSL's
  prologue: T3-M4 waits in the garage's airlock while 3C-FD works the
  security console). A module installed in the step a script switched back
  carries the seat's own character, the one given back left behind unless
  it is a member (`SimWorld::settle_switch`; probes `leads_a_companion_while_switched`,
  `install_after_switching_back`).
