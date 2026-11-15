# Co-op over the network: seats, bodies, holds, saves

On `NET.md`'s network. The break and the
ready check: `PARTY.md`; the party's fall: `FALLING.md`.

## Seats and bodies
- `kjk host` holds 3 seats (`--seats`; one more is refused "the host's 3
  seats are all taken"). On a module the guests play the party's
  companions (`--guests companions`); on the course, the duel floor and
  the ring a player each, and 2 seats unless told (every seat's player
  stands from the start, so a third would stand idle).
- Each authority step (`world::drivers::assign`, first) a guest that sent a
  command takes the first party member no one drives (joining order, the
  standing first); none free, it flies a remote droid (below). One that
  leaves the party or dies frees its guest. A guest whose
  connection ends leaves its companion to its AI, kept for it 60 s
  (`KEEP_MS`, `Snapshot::kept`): no other seat takes it, back it takes it
  first, planning counts it meanwhile.
- The companion stays a creature (`Kind::Object`): its mind's `driver`
  names the seat and the world derives `Avatar(seat)`, so moving, Use,
  containers, locks and exits are its seat's. A story's script or a
  conversation still moves it (`Mind::steered`, predicted as a creature),
  and it pushes idle bystanders aside on KOTOR's walker as an undriven
  companion does: only the party's leader pushes no one (Hendar's gate,
  probe `guest_companion_pushes_in_scenes`);
  its own AI hooks do not (`Engine::own_ai_on_driven`), nor the code they
  defer, assign or queue for it (`Delayed::ai`: K1's `k_ai_master` attacks
  through it, which took a guest's companion off its player whenever
  another party member was hit, probe `own_ai_skipped_when_driven`). Its
  dialogue hook answering a line it heard is its own AI too: K1's
  companions answer the party's combat shouts ("GEN_I_WAS_ATTACKED")
  with an attack, which took a guest's companion out of its player's
  hands for the rest of a round. One PC to scripts.
- What a guest's companion does itself is done by the character its
  player controls, KOTOR's party leader: a script its act raises (a door
  it opens or fails to open, a lock it picks, a thing it uses) sees it
  lead the party (`Pending::leads`; `GetPartyMemberByIndex(0)`,
  `GetIsPC`), the party's leader unchanged after. The Endar Spire's first
  door opens for Trask leading (`k_pend_door01`), so for the guest
  playing Trask too, not only for the host's portrait click.
- Every seat's acting body is on the players' side of a conversation a
  script queues (`dialogue::converse`): a console whose `OnUsed` talks to
  `GetLastUsedBy()` with no DLG named (the Endar Spire's
  `k_pend_comp07`) talks with its own DLG to the guest's companion that
  used it, not with the companion's DLG.
- The first seat drives a creature the same way while a script has
  switched it to one (`SwitchPlayerCharacter`, `world::switch`), its own
  character benched; that creature is the PC then, and a guest driving
  it gives it up (`docs/WORLD-STATE.md`).
- A companion leading the party (`SetPartyLeader`, the portrait click:
  `world::lent`) is the first seat's body while its own character stands;
  the snapshot names it (`Snapshot::leading`, KJKS 36), as clients and
  the host's own screens have no party table.
- Clients find their body with `Snapshot::body(seat)` (taken, lent or
  lost: drawn where the host has it; the target block, the camera and the
  use key go by it); its own figure only while it has one.

## The remote droid
Design: `docs/superpowers/specs/2026-10-29-remote-droid-design.md`; its
numbers: `data/rules/remote.toml` (`kjk rules`).
- The party's size alone decides who flies one: a guest with no party
  member free flies only while the party has fewer characters than
  players (the host's own and each companion in the party table, down or
  standing). With one for every player no guest ever flies: a guest whose
  companion is down keeps it and waits for the revive, and one left
  bodiless while a companion leads watches the party. Phantom seats fly
  regardless (probes `no_droid_while_companion_leads`, `…_down`).
- It is TSL's remote (Bao-Dur's,
  `Droid_SensorBall`): launched from the body it let go, else behind the
  leader; docked the step a companion frees up (`world::remote::launch`,
  after `drivers::assign` and `scene::hold`). Not an entity: a world
  resource and `Snapshot::remotes`, made again each step, never saved.
- It hovers 1 to 3 m over its floor, under a ceiling, through nothing a
  player can't pass, sliding along walls; leashed 20 m to the nearest
  standing member; Use, falling out or getting stuck put it behind the
  leader (`remote::flight`). Nothing sees, blocks, targets or hurts it; it
  opens, uses, talks to and trips nothing (probe `changes_nothing_else`).
- Its kit (`remote::kit`, authority, no dice): attack the shock bolt (3 HP
  on the first body along the view), alternate attack the Sonic Blast (a
  6 m, 60° cone thrown back as a half-strength push, medium or smaller
  knocked down), the medpac key the stim dart (the member under the
  crosshair, else the nearest downed, else hurt: up with 25% or +10%).
- One gate (`remote::gate`): bolt and blast touch only a creature hostile
  to the leader, alive and keeping the party in combat, that no script
  protects (Min1HP, plot, immortal, in a conversation); the dart holds off
  a member fighting a protected one (a duel) and once the party is lost.
  Hits are credited to seat 0's acting body: XP, `GetLastAttacker`.
- While a conversation holds the world or a fade covers the screen it is
  hidden, still and its kit off; a guest who first speaks then gets none
  till it ends. A movie or a break holds the session itself.
- The chaos chain (`kjk chain k1 --droids chaos`) is a check run when
  the droid changes, not a gate: two phantom guests fly droids at random
  and press their whole kit, each route from the plain chain's own
  checkpoint (droids end fights sooner, and the routes are walked against
  one timeline). It passes when every route is met with its droids flying;
  what the story does differently goes to `story-changes.txt` for a look:
  dice and timing (random lines, swoop times) and fights won sooner
  (Hendar saved at the Undercity gate) are expected.
- The client: its model turned to its seat's view, yaw and pitch, and its
  kit drawn with KOTOR's own (`remote_view`: the ion blaster's bolt to
  where the shock bolt stopped, `Remote::bolt_to`; the sonic grenade's
  blast; the medpac's heal on the dart's patient). The camera follows the
  own droid from its predictions (`cast::remote_in`, `play_view::OwnRemote`),
  as it does the own body; KOTOR's HUD shows its three buttons with the
  grenades' and medpac's icons, the seconds left, the name of the one last
  fired, and "No one hurt" for a dart with no one to treat.

## The lobby
The menu's Host opens the co-op lobby after the host picks a save or makes
a new character (`game::lobby_host`, the screen `ui::screens::lobby`). The
world is not loaded yet. Guests join it as they would join a game: the
handshake and the data check are the same, and a guest on another build is
refused as before. After the handshake a guest gets the lobby's state
instead of a welcome (`net::session::lobby`, messages 16 to 20).

- The host plays the save's main character. Each guest is given the first
  free companion, the party's own first, and can choose another. A guest
  who chooses a companion another guest plays swaps characters with that
  guest. A guest with no companion left flies a remote droid.
- One rule says which companions can be chosen
  (`sim::script::party_choice`), the same one the in-game party selection
  screen uses. A companion the story has taken away cannot be chosen.
- The host chooses the saber rules and the collision, and fills the
  companion places no guest took. The game controls those companions.
- Start writes the lobby's party into the save as the party changes the
  games' own scripts make (`Save::queue_party`), seeds each guest's seat
  with its companion (`Save::seat_companions`, through `Kept`), and plans
  the save's module for the players present: the host and each guest who
  plays a companion (`Save::plan_for`). The world then loads with three
  seats, and the guests are welcomed into it on the connections they
  joined the lobby with (`Server::begin`).
- The story's own scripts still run after Start. A save made as a module
  starts can have an arrival script that changes the party. Dantooine's
  first arrival (`k_pdan_trig1301`) takes Bastila and Carth out of the
  party for the Jedi Council's scene, whatever the lobby chose. The guest
  who chose such a companion then has none to drive until the story gives
  one back.
- The command line's `kjk host`, `cargo xtask duo` and the chain open no
  lobby. `kjk host --load ID --lobby N` opens one with no player and
  starts once N guests are Ready (`kjk join --lobby-script ready|swap`).

## Over the link
- A module is planned for the players driving a body then
  (`ModuleData::players`, in its digest). A player joining one, at once or
  late, plans it for the announcement's count (`session::Request::of`),
  as for each install after; else it is refused ("this machine builds
  another world than the host: …").
- A join compares the game's tables, modules and overrides
  (`vfs::fingerprint::world_group`). It does not compare `dialog.tlk` or
  the voice files (`streamwaves`, `streamvoice`). Each module's
  announcement compares the lines of game text its scripts may read and
  the lengths of its conversations' voiced lines instead
  (`sim::script::LibraryDigests`). A guest whose copy differs is refused at
  the first module that reads a differing line, and is told which of the
  two differs (`Mismatch::Text`, `Mismatch::Voices`). Each side's log has
  the digests it built ("installed" on the host, "built the host's level"
  on the guest).
- Save copies: message 13 in 64 KiB parts, one only when the
  reliable window is idle; kept by (host id, slot), 10 a host. Holds (movies, breaks): message 14 `Hold`
  carries the held snapshot and stops the players' prediction (each
  forgets what it predicted past it; the host drops the seats' commands,
  as for an install); `Resume` restarts their clocks; keepalives go on.
  A guest reads its input as the host does (`sim_host::PlayerInput`): none
  while a movie is up, and what the skip took is up until let go.
- A reload is an install no client can make: each says its state
  differs and is sent the host's whole at once.
- A seat's command missing past 16 ticks is its idle command
  (`Command::idle`: its last view and selections, nothing pressed), also
  through a hold and an install; none before its first command or after
  its connection ends. The host's own player is in the same process
  (`Link::set_in_process`): neither side times the other out.

## Commands
```bash
kjk net session --only save_copy|hold|reload   # each in virtual time
cargo xtask lobby --saves DIR --load k1-08 --droid k1-02   # the lobby: a host and two guests, a swap, and a guest who flies a droid
cargo xtask duo --stage end_m01aa --arrive entry --guests companions --follow end_trask   # the guest drives Trask
cargo xtask duo --preset exit   # the ready check at end_m01ab's exit, the install went
cargo xtask duo --preset remote   # end_m01ab, the guest flying a remote droid, its kit pressed
kjk sim probes --only remote   # the droid: launch, flight, gate, kit, holds, inert
kjk remote census k1   # every scripted round's module read for a protection the gate honours
kjk chain k1 --droids chaos   # after a plain `kjk chain k1`: each route alone from its checkpoint, two chaos droids; story changes listed in story-changes.txt
cargo xtask duo --preset race   # the swoop race in co-op, a run per seat (MINIGAMES.md); --preset turret: the Ebon Hawk's guns
```
