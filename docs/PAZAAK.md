# Pazaak

KOTOR's card game, played in the sim (`sim::pazaak`) by whoever sat down,
alone, with the party's credits (I30 P20): only that player's screen shows
the table, and the others play on, at a table of their own if they like. A conversation's script starts
a match; the games' own data decides the rest. Journal:
`context/artifacts/2026-10-04-pazaak/` (the rules list with its
evidence, the decisions, screenshots).

## What decides what

| What | From |
|---|---|
| The rules | the talk table's rules page (32842) and tutorial (38627–38649) |
| An opponent's side deck | `pazaakdecks.2da`, the row `PlayPazaak` names (a missing row: row 0) |
| The player's cards | row 0 (the basic deck: two each of +1..+5) and the card items the party holds |
| A card item's card | its base item's class `I_PazCard` (`baseitems.2da`) and its `ModelVariation`: 1–6 ±1..±6, 7–12 +1..+6, 13–18 −1..−6, TSL's 19 ±1/2, 20 Double, 21 Tie Breaker, 22 Flip 2&4, 23 Flip 3&6 (`sim::pazaak::Card`) |
| `pazaakdecks.2da`'s codes | `+N`, `-N`, `*N` (±N), `VV`, `$$`, `TT`, `F1`, `F2` |
| When it starts, what follows | the dialogue's scripts: `PlayPazaak`, its end script, `GetLastPazaakResult` |

`session::pazaak` reads the decks and the cards into the script library
(`Library::with_pazaak`, in its digest).

## A match (`sim::pazaak::Match`)

- `PlayPazaak(deck, end script, max wager, tutorial, opponent)` (K1 364)
  makes the match in the scripts' state (`script::State::pazaak`) for
  whoever asked for what its conversation does (`Conversation::asker`: the
  guest whose pick or click led there, else the PC speaker;
  `Match::player`) and tells that player (`Screen::Pazaak`, notice code
  11); one a player body, several at once (`script::State::pazaak` a
  list): another for a body already playing is refused. The cards
  the player may choose from are fixed then.
- **The wager** (when the call's maximum is above 0): 1 to the maximum
  and the party's credits; none skips the wager screen and the credits.
  Placed, it is held out of the party's credits until the match ends (in
  co-op another seat cannot spend it meanwhile, nor wager it at its own
  table). Quit (or Back on the side deck) leaves: no game, the wager back,
  the end script reads 0.
- **The side deck**: ten of the cards offered. Each side's hand is four of
  its ten at random, for the whole match, each card once.
- **A set**: a fresh 40-card main deck (four each of 1–10), shuffled from
  the authority's random stream. The first set's beginner is a coin; then
  the set's loser, after a tie its beginner again [H]. A turn begins with a
  main card drawn onto the table; at most one hand card may be played;
  then End Turn or Stand. Over 20 at the end of one's turn loses the set
  (a negative card may save a draw first). Exactly 20, or a ninth card in
  play, stands at once. One standing, the other plays on until it stands
  or busts. Both standing: the higher total wins; a tie is replayed and
  counts for no one (TSL's Tie Breaker wins it for its player).
- **The match**: three sets won. The wager held comes back with as much
  again for a win, and stays lost with a loss or a forfeit; the
  result is kept for `GetLastPazaakResult` (1 won, 0 lost), and the end
  script runs for the opponent named, else the call's caller [H], at
  once: each end script reads its own match's result, and a conversation
  it begins is its player's own (the player its asker). Matches are
  decided in their players' seat order, so two wagers after the last
  credits go by seat. Its player's status summary tells the wager won
  ("Credits Received") or lost ("Credits Lost") once the screens close
  (`pazaak_view` → `progress::Told`: the sim settles the credits without
  a status). The table sounds as K1's (`docs/SOUND.md`).
- **Paused** (`Match::paused`; R9, R12): while more than one seat plays
  a body, a match pauses while a party conversation holds every seat, and
  from a hurt until its player is out of combat (`Match::hurt`, each cause
  ending its own pause); its player's table is put away meanwhile (its
  watchers keep theirs, told why: R39). A paused match
  takes no move and its clock stands: resumed, it plays on where it
  stood. One player's match plays on, as the originals' does.
- **Ended as a quit**: its player died or left the party (at once); its
  seat dropped (`Match::away`: paused while the seat is held for it,
  `KEEP_MS`, then quit, unless a seat drives it again); the end script
  runs, reading 0. A module left, a script's call or an exit, ends every
  match (`State::leave_matches`): the wager back, `GetLastPazaakResult`
  0, the end script not run (its module is going, and several of the
  games' book a loss or take credits). A match decided while its end
  shows settles as decided whatever befalls it.
- **Pacing** [H]: the opponent acts 0.9 s into its turn; a set's result
  shows 2.5 s, the match's 3 s.
- **The opponent's play** [H] (KOTOR's is not in its data,
  `Match::choice`): a bust rescued with the hand card nearest 20; against
  a standing player, stand once past it (or tied from 17), else play past
  it if a card can; otherwise play only to reach 19 or 20, and stand from
  18. `Match::advice` is the same play for the player's side: the
  playthrough's `pazaak` goal and its headless player use it.
- TSL's specials [H]: ±1/2 adds ±1 or ±2, Double the last card again, Flip
  2&4 and 3&6 turn the sign of the player's own table cards of those
  values (and count 0).

## Watching a match (R9a)

Other players can watch a match (`Match::watchers`, the bodies in the
order they came; `Snapshot::watching` the one a seat watches). One player
alone has no one to watch it, so nothing changes alone.

- **Free** (`script::pazaak`'s one rule for both ways in): a seat's body,
  standing, in no conversation (a party one holds every seat), trade or
  match, not in a fight (its own, or a hostile one keeping the party in
  combat within the party's combat reach of it, `party::embattled`: R34),
  and watching none.
- A seat "watching" with no body (I30 P3) is another thing: a body
  watching a match stands by the table.
- **As a match begins** (`PlayPazaak`), every other seat's free body
  within `GATHER_BEYOND_M` (10 m) of its player watches it at once, in
  seat order, and so does each player joined to the conversation whose
  line dealt it, stepping out of that conversation, its mark going with
  it (R35).
- **As that conversation ends** (R37: it remembers the match,
  `Conversation::dealt`), its listener and those still joined, free now
  and within 10 m of the match's player, watch it while it goes on: a
  guest took the dealing reply in the host's own conversation, the host
  watches once it ends. Only as it ends normally (R40): one cut short, by
  its listener's Leave among others, pulls no one in.
- **Later**, a free player watches by using the player's body, or with
  `Action::Watch { player }` (wire code 37, any seat's: a click on a
  pazaak icon on the host's party bar sends it). The use key meets a body
  at a table only for a user free to watch it (R41: `world::watchable`,
  the scripts' rule, `script::WatchRule`, read off the step's own
  conversations, trades, matches and fights, as a client predicting reads
  them, so a loaded save uses alike); otherwise it passes
  through the party's bodies, the companion the host leads through among
  them (R36), to whatever is behind (the dealer across the table). The
  asks are decided in the step's pazaak pass, in seat order, before the
  matches play.
- A watcher is held where it stands, facing the table's player, as a
  listener is (`world::hold_talkers`), the match paused or not: a paused
  match keeps its watchers' table up, told why (R39). It is not busy: a
  save, the exit ready check and benching do not wait on it.
- **It stops** with its Leave (its seat's answer byte's Leave while the
  body the seat acts through stands in no conversation, or
  `Action::Watch { player: u32::MAX }`; such a Leave stops the watching
  alone, never also a conversation the seat's other body stands in), a
  hurt, or its ready at an exit; the match gone (its end, a quit, a
  module change); and, decided after the step's scripts and
  conversations, its seat no longer driving it or it down, a fight
  reaching it (R34's test, R39), a conversation holding it (its own, one
  it joined, a party one's gather), a trade or a match of its own.

Moves are `Action::Pazaak(Move)` (wire code 30), taken from the seat
driving the match's player only (`pazaak::playing`; none driving it, the
first seat's), each seat's to the match it plays (`pazaak::played_by`,
`Snapshot::pazaak_of`: the one whose player it drives, never one held
for a dropped seat). The snapshot carries the matches
(`Snapshot::pazaak`, KJKS 39; its player KJKS 55; a list, paused, away,
hurt and watchers KJKS 56: fewer than the seats, none twice), the save
the scripts' state's (KJKV 28, KJKR 23; 38, 30; 40, 31); a save is
refused while any match is under way (`saves::refusal`: the minigame's
line for the saver's own, "Carth is playing pazaak." for another
player's).

## The screens (`ui::screens::pazaak`, `game::pazaak_view`)

Three KOTOR layouts, K2's skinned (`docs/UI.md`): `pazaakwager`,
`pazaaksetup`, `pazaakgame`. The view opens the one for the match's stage
and closes them at its end; a set's and the match's result show in the
message box (the game's words, 32334–32338) while the match holds them; in
tutorial mode the tutorial's pages open as the side deck and the table
first show (pages longer than the box are cut). Cards are drawn in the
campaign's own art (K1's `lbl_card*`, TSL's `pcards_*`). Each seat's
screens show the match it plays (`Snapshot::pazaak_of`), so two players
sit at two tables at once; a paused match's screens and its tutorial
pages close on its player's screen, and its table comes back at its
stage once it resumes (the pages not shown again).

- Wager: less / more (Left / Right), Wager, Quit (Back).
- Side deck: click a card offered to add it (K1's Add Card adds the one
  focused), a chosen one to take it back, Clear Cards (TSL), Play at ten.
- Table: a hand card plays (click, or 1–4), Flip turns its sign first
  (F1–F4), TSL's Change its size; End Turn (E), Stand (S), TSL's Forfeit.
- Watching (R9a, `Snapshot::watching`): the table only, whatever the
  match's stage (the wager and the side deck are chosen on its player's
  screen), shown as its player sees it (its hand, the opponent's face
  down); "Watching Carth" in place of the player's name (shortened to fit:
  the name, then its first word), a wager or a side deck being chosen
  told over the player's hand ("Carth is choosing a side deck."), a
  set's or the match's result in the third person ("Carth won the set.",
  "The set is tied."), the play buttons faded, nothing sent (a flip is
  not even turned), no tutorial pages; Leave (L, the bound "Leave a
  conversation" key) on TSL's Forfeit button, and the key itself, send
  `Action::Watch` with no player. A paused match keeps the watcher's
  table, Leave live, and tells why where the wager's words go (R39:
  "Carth is away." while the player's seat is dropped, "Carth is hurt."
  while the player is hurt). Its screens close as watching stops and open
  as the watcher's own match's if it deals one. On the host's party bar a
  click on a companion's pazaak icon watches its player's match
  (`hud::watch_at`; the console's `ui watch 2|3`). A name not known yet
  (the scene still coming) is asked again once a snapshot.

## Running it

```bash
# Upper City cantina: Niklos's game for 40 credits, the screens by hand
kjk play --stage tar_m02ae --gold 500 --item g_i_pazdeck --item g_i_pazcard_013=2 \
  --run k_ptar_pazup
# Garouk's deck and lesson, then Niklos, headless to his end script
kjk playthrough k1 tar_m02ae data/routes/k1/taris_pazaak.route
```

Approved test `pazaak` (`sim::pazaak::probes`); watching on screen,
`gui_layouts`' `pazaak_watch_table`, `pazaak_watch_sends_no_move` and
`hud_pazaak_watch`, and `pazaak_watch_view` (`kjk sim probes --only
"pazaak view"`); the save refusal's probes
are `refusal_pazaak`, `refusal_pazaak_other` and `refusal_pazaak_unnamed`
(`save_copy_round_trip`); a match beside a conversation, paused by one,
by a hurt, a drop or a death, and an exit's ready check waiting on it are
`dialogue_coop`'s.

Quirks: Q-0155 (Garouk's lesson asks for "Garouk21"), Q-0156 (the Lower
City cantina's game names an end script that module lacks).
