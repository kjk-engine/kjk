# Module lifecycle (`session`, `sim::install`, `kjk lifecycle`)

A module is installed as a transaction: planned whole, then committed in
one call; its GIT instances are sim entities; doors and triggers lead on.

## Plan, commit, announce
- `session::plan(source, request, anims)`: the module IR, its objects as
  the sim's facts (`sim::module::ModuleData`: kind, tag (lowercase for
  lookups; `cased_tag` keeps the data's capitals for `GetTag`, since
  scripts compare tags as strings), place, facing via
  `sim::math::atan2_deg`, links, door `Opens`/`AnimationState`, trigger
  outline, event scripts kept for I16), the entry area's collision, the
  link overrides (`data/links/<game>/<module>.links`; only with
  `kjk --playtest-links`, `session::Overrides`), a trial arrival for
  the request's `seats`, encounters for its `players` (I30 P21). Typed
  `PlanError`s; nothing running is touched. A module's archives stack on
  the install's base layers, opened once per `Installer`
  (`Resolver::with_module`).
- `Plan::commit(world)` = `SimWorld::install(level, arrival)`: the module
  left goes to the stash (coming back finds it as it was), the one arrived
  in comes from the stash or fresh (new ids, GIT order); each seat's player
  (id, seat) stands at the first door or waypoint with the tag, else the
  IFO entry, dropped to the floor, next seats on free spots around it,
  the view set JA's way. The drop starts a step above the spot, and only
  where that leaves too few spots, from a step higher, up to four: an
  entry on a walkmesh under the floor drawn there stands on the drawn
  floor (Q-0124). An entry with no room for a body that stands over
  walkable floor within 0.5 m (`install::OWN_FLOOR_M`) keeps the first
  seat on it as the game places it, the next seats round it: TSL's Exile
  wakes inside Peragus's kolto tank (101PER, `k_awaken`). A trigger that
  leads out, or an open exit doorway, arrived in already counts the
  player, and a guest's companion as its seat (`ObjectState::tenants`: a
  bit per seat, `MAX_SEATS`); any other trigger counts it on its first
  move inside, so it fires (reone's rule; the Awaken trigger).
- It returns an `Announcement` (`KJKA` v4: game, module, arrival, tick,
  seats, players, digests of module facts, collision, animation lengths, the
  navigation mesh, the hash after). A client plans from it alone (`Request::of`), `Plan::check`s it
  (a `Mismatch` names what differs), commits, `check_after`s. I15 sends it.

## In the sim (every step)
Holding use (`buttons::USE`: a scripted `--moves … use`; in play E is
the target's chosen action, `sim::world::uses`) traces 64 units from the
eye every 100 ms:
a closed door that is not locked (as placed, `Opens`, or as scripts,
keys and Security left it, `ObjectState::lock`) opens away from its
opener (`ObjectData::opening`: a door whose open walkmeshes stand on both
sides of its doorway, `session::doors`, takes state 2 from the side its
first way swings to; every other door, and one a script opens itself, 1;
Q-0025; no K1 or TSL door model opens both ways); one
that leads out asks for the transition, as running into it does. A
locked one fails to open, once while use is held (`WORLD-STATE.md`,
Locks). Entering an open exit door's
doorway (the box a unit larger overlaps the door's own closed shape:
`collision::Level::entity_overlaps`) or a trigger that leads out (over its
outline, on its floor: `ObjectData::contains`, below) asks once. Any module link leads out, its own included (reone
reinstalls it), unless its `LinkedToFlags` is 0 (links to nothing,
xoreos): the Taris hideout's door names its module, but its own `OnOpen`
and the party screen take the player out. The ask goes to the ready check
(`PARTY.md`): the party goes when everyone is ready, and the move is in
that step's snapshot for the host to install; a script's `StartNewModule`
skips the check.

## In `kjk`
- `lifecycle::Installer` plans a module with its scene; `sim_host` stops
  stepping, shows the loading screen (`loadscreen.gui`, `load_<module>`,
  `bluefill`), plans on a thread, commits, swaps `SceneContent` (the old
  scene, effect meshes included, despawned) and fades out. A transition
  that fails to plan (module and arrival) is not retried. `sim view
  --replay` makes the recording's installs, checked against it.
- Console: `module <name> [<tag>]` (`travel` to go on while it loads); `wait world`,
  `wait loading(_screen)`. Where the game's screens are drawn, the loading screen is
  theirs (KOTOR 2's `loadscreen_p`, `ui::screens::loading`).

```bash
kjk lifecycle plan k1 end_m01ab [--arrive TAG]   # host commit, client from the bytes
kjk lifecycle cycle k1 end_m01aa end_m01ab [--times 5 --ticks 250]  # no leaks
```

The cycle's last check loads a save of its end into a level planned from
the save alone and steps both 500 ticks: the same. A save carries what the
last step's queues asked of the next step's minds (`script::State::goals`,
`intents`, KJKV 19): without them a loaded world's fighters let go of their
targets for a step (the Endar Spire's opening fight) and went on
otherwise. It carries the world as the scripts' step last saw it too
(`State::watch`, KJKV 22: each character's HP and life, when its damaged,
attacked and spell-cast-at scripts last ran, the party's items): without
it a loaded world's first step raised no damage or death events and the
throttles started over.

```bash
kjk move run --stage end_m01aa --arrive wp_player01 --moves lifecycle-walk
kjk play --stage end_m01aa --arrive wp_player01 --moves lifecycle-walk --headless \
  --cmds 'anim 1.1; wait loading_screen; screenshot l; wait world; quit'
kjk sim replay run/inputs.kjkr ; kjk sim load run/end.kjkv ; kjk sim view --replay run/inputs.kjkr
```
