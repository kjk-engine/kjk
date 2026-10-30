# Playtests: a module played headless, end to end (I29)

`kjk playthrough` drives one player through modules goal by goal, with the
games' own scripts running everything else. It answers one question: *does
the engine let the game play as designed?* Each goal not met is a
**blocker**, with a report of what held it. This page is the working
method; the verb's flags and output are in `RUN.md`. Routes live in
`data/routes/<game>/` (`k1/endar_spire.route`, the bunks to Taris, then
`k1/taris_*.route`, `k1/dantooine.route`, `k1/tatooine.route`,
`k1/kashyyyk.route`, `k1/manaan.route`, `k1/korriban.route`,
`k1/leviathan.route`, `k1/unknown_world*.route`, `k1/star_forge*.route`,
`tsl/prologue.route` (the Ebon Hawk to Peragus, one marked stand-in),
`tsl/planets.route` (each later planet's first module, cold: stand-ins
throughout), and the branches, each from the checkpoint its header names); each run's
journal keeps its blocker table.

## Before the first run (minutes that save hours)

1. **Read a walkthrough of the area.** It tells you the intended way:
   which doors are optional, where the spikes are, which terminal opens
   what. Most dead ends come from guessing the route.
   - IGN's K1 walkthrough wiki (one page per area in play order; the
     maintainer keeps an offline copy in `context/walkthroughs/`). Its author plays a male **Scout, default
     build, Dark Side**: its picks, XP and rewards follow that path. The
     text is IGN's: read it, never copy it into routes or docs.
   - Cross-check another source for what IGN's path skips: GameBanshee's
     locations pages (numbered maps, every container, the alternatives,
     e.g. `gamebanshee.com/starwarskotor/locations/starboardsection.php`),
     or any other complete walkthrough.
   - **Other playstyles are the game too.** Where the area offers a choice,
     note each way: light and dark answers, Security vs. a terminal vs. a
     droid, a class's skills (a Soldier has no spikes where a Scoundrel
     does), a fight avoided by talking. Play one way first; then give each
     choice point a route of its own (`data/routes/<game>/<area>_<variant>.route`,
     e.g. `k1/korriban_both.route`), so
     every way is shown to work, not only IGN's.
2. **Check the area's navigation before its story.** These find the places
   where the navigation mesh and the collision disagree, which would
   otherwise surface one at a time, deep in a run:
   ```bash
   kjk collision walk k1 end_m01ab --routes 300   # walks through move_body: how many arrive
   kjk collision nav k1 end_m01ab                 # routes checked leg by leg
   ```
   Fix what they show first (engine rules, see below). Every walk gains.
3. **Look at the area.** A screenshot from a spot takes seconds:
   `kjk play --stage end_m01ab --arrive end_wp_carth01 --headless --cmds 'wait world; wait 2s; screenshot room; quit'`.

## Writing the route

A route is a text file of goals, one per line, `#` comments (the goals:
`launcher::playthrough`'s module notes, `RUN.md`). Rules that keep it working:

- **Name objects, not places.** `to wp_player01`, `use end_pod`, not
  `at 31 140`. Walks are planned on the current navigation mesh each time, so
  a route of tags survives walkmesh changes; coordinates do not.
- **Wait for outcomes, not time.** `until END_TRASK_DLG=2`, `module
  tar_m02af`, `talk`, `gone hendarsfate` (what a script checks is gone),
  `gather` (the companions caught up), a door opening. Avoid `wait Ns`
  except as a pacing choice the route's comment names.
- **Answer conversations with `pick`** before the goal that starts them
  (`pick 2,3` then `use end_comp02`). A pick is used up by every offer,
  including a lone "continue". Without picks an offer takes the first
  reply not yet taken in that conversation, else its last: hubs of
  questions are asked through and left, which shows most of a tree but
  can pick a fight (a guard's "you can't keep me out if you're dead"):
  read the tree (`kjk dialogue show`) and pick where it matters.
  `say WORDS` asks for a reply by what it says: the next offer with a
  reply holding those words takes it, offers before it answer as without
  (sturdier than a count of offers through a hub).
- **One comment per goal, saying why** (what the game expects there).
- Take what the game gives: `use` a container empties it; `equip` wears it.

## Running

```bash
kjk playthrough k1 end_m01aa route.txt --god --mighty --skill security=2
```

- `--god`: the player never drops below 1 HP. Always use it for these runs:
  without it fights decide timing, and every engine change makes the run
  flake. Balance is tested separately.
- `--skill NAME=RANK`, `--class`: a build the route needs, no more than a
  real character could have at that level (a level-up checks it).
- `--mighty`: the player's blows count ten times over, so fights end
  quickly. The k1 routes are written for it and Security 2 (`kjk chain`
  adds both, from `data/routes/k1/chain.flags`).
- **Checkpoints.** `save NAME` in a route writes the world as it stands
  (`kjk-artifacts/checkpoints/GAME/NAME.kjkv`, or the folder `--save-to`
  names); a run given `@NAME` (or `@PATH.kjkv`) for its module starts
  there. Save at the end of each part and before anything
  that took work to reach, and iterate on a segment from its checkpoint: it
  reruns in seconds. A save after a goal not met is refused. A save holds
  the level's digest: after an engine change to the plan, the collision or
  the save format, rerun the routes in order to remake them (the Endar
  Spire in about 40 s of wall time, Taris's in minutes).
- `--arrive TAG` and `--global NAME=N` start a module cold, for a quick
  look; the story's state is the checkpoint's.
- A run ends at its first blocked goal. `--keep-going` tries each goal
  after it too, and each blocked one spends `--goal-seconds` (90 s of
  play; a `fight` its own 900): Manaan with 30 blocked took ten minutes,
  where the whole of Taris met takes two.
- Each run also watches every scripted move (`launcher::motion`):
  `motion.txt` (and `facts.json`'s `motion`) lists a body crossing the
  floor without a moving clip (a slide), turning back and forth or
  stopping and starting (a jitter), or treading in place, with
  `KJK_MOTION_TRACE=TAG,…` writing those bodies' every step. `kjk
  dialogue motion k1 [MODULE…]` does the same over the dialogue sweep's
  runs.
- Exit 0 means every goal was met. The run folder has `facts.json` (each
  goal's play time and its wall time, `wall_ms`, so a goal's cost can be
  measured), `transcript.txt` (lines, notices, XP, Security rolls),
  `trace.txt` and `inputs.kjkr`.

## The chain

Each route starts from the checkpoint another saves, so the routes make a
tree: the main way from the Endar Spire to the Star Forge, and the
branches off it (the Sand People's war, Juhani slain, the dark ending).
`kjk chain` plays it (`RUN.md`): each route a playthrough with the
game's chain flags (`data/routes/k1/chain.flags`: `--mighty --skill
security=2`) and its header's, started as soon as the route before it has
saved its checkpoint, the branches beside the main way.

```bash
kjk chain k1                           # the whole tree: the merge gate
kjk chain k1 manaan                    # Manaan and all after it
kjk chain k1 manaan --alone            # Manaan alone, from its kept checkpoint
kjk chain k1 --quick                   # every route alone at once: a signal in minutes
kjk chain k1 manaan --dry-run          # each route's command, and what the cache holds
```

A route met costs seconds to a few minutes of wall time; it is the
blocked goals, the reruns and the rebuilds that cost hours. So:

- **While working on something, replay the route it touches** from its
  checkpoint (`kjk chain k1 ROUTE --alone`), not the chain.
- **Before a merge, the chain** from the first route the change can
  reach (`kjk chain k1 ROUTE`), or all of it, to both endings, after
  a change to what every route meets: the sim (minds, combat, movement,
  scripts' routines), the harness (`playthrough.rs`), saves, the plan.
  An unchanged sim hash is no proof: its run has no fights' minds in it
  (PR 92's "eyes, not chests" kept the hash and moved the sewers' fight).
  `--quick` is no gate: each route starts from the checkpoint kept from
  the last run that met the route before it, which may be an older
  build's.

### How a chain run works

- **It stops at the first block.** Each route ends at its first blocked
  goal and no route starts after one (`skipped` in the table: the chain
  stopped). Routes already playing finish. `--keep-going` plays every
  goal and every route whose checkpoint was saved.
- **It is its own.** The run folder (`kjk-artifacts/runs/...`) holds a
  copy of the binary every route runs (a rebuild meanwhile changes
  nothing), the routes as they were when it began, the kept checkpoints
  it starts from (`starts/`) and those its routes save (`checkpoints/`).
  A route waiting on another starts once that one's checkpoint is in this
  run's folder, as soon as it is written (a save is written whole), not
  when the route before ends; one saved in a `while` or twice waits for
  that route's end.
- **Checkpoints are kept per route met.** When a route meets all its
  goals, the checkpoints it saved replace the kept ones
  (`kjk-artifacts/checkpoints/k1`), where `--alone`, `--quick` and a
  bare `kjk playthrough k1 @NAME` start. A blocked route's checkpoints
  are not kept: they stay in the run folder (`checkpoints/`), for playing
  on from the last one before the block (`kjk playthrough k1
  @kjk-artifacts/runs/RUN/checkpoints/NAME.kjkv ...`). Per route, not per
  chain, so a fix replays the blocked route from the checkpoint the run
  just made before it.
- **Identical reruns come from the cache.** A route's outcome hangs only
  on the binary, the game's data, its start checkpoint's bytes, the
  route and its flags (an audit found
  every rerun byte-identical). The chain hashes them, keeps each route
  met or blocked with its checkpoints (`kjk-artifacts/chain-cache/k1`,
  the last three binaries'), and answers an identical rerun from there:
  `(cached)` in the table. A route edited reruns that route, and the
  routes after it only if its checkpoints came out different.
  `--no-cache` plays everything.
- A blocked route's log is in the run folder beside `facts.json`.

### Loading a phase in the window

Each game's `saves.list` (`data/routes/k1/saves.list`,
`data/routes/tsl/saves.list`) names the checkpoints that start its
phases, in story order, each with a title. After every chain run in the
main checkout, `kjk chain` writes the kept ones into the game's saves
folder as `k1-01`, `k1-02`, … and `tsl-01`, … (titled `K1-03 Taris: the
Undercity`), so a phase is one Load away:

```bash
kjk chain k1                 # plays (or takes from the cache) and writes k1-01 … k1-19
kjk --load k1-11             # the window, opened on Tatooine
```

They sit beside the owner's own saves (numbered ids, untouched) on the
Load screen. A number whose checkpoint the chain has not kept (its route
never met) keeps the save it had. A run in a worktree writes none, as its
binary is not the one the owner plays, unless given `--saves`. A save
made with other level data than the build's is refused at load ("the
game's data is not what the save was made with"): run the chain again on
the new build. TSL's `cold` saves come from `tsl/planets.route`, whose
`goto`s stand in for the story between Peragus and each planet: the
area plays, its story's earlier state is missing.

### Running the chain (rules for agents)

- **Never rerun a blocked route or chain on an unchanged build.** It
  blocks the same way, byte for byte. Fix the rule, rebuild, then replay
  that route from its checkpoint (`kjk chain k1 ROUTE --alone`); once it
  meets, the chain from it (`kjk chain k1 ROUTE`).
- **Run long chains in the background** and wait for the completion
  notice: no `sleep`/poll loops, and no foreground command with a timeout
  over 10 minutes (the Bash tool's cap kills it, whatever is asked).
- **Never two chains on one machine at once**, across worktrees too: each
  costs the other up to 30% of its wall time.
- Use the chain's defaults (stop at the first block, the chain flags, the
  cache); they need no remembering.

## Reading a BLOCKED goal

The report says what held it. Sort it into one of these:

| looks like | is | where to look |
|---|---|---|
| a conversation paused, a queue with work in it (`#219 … is on Move …`) | a scene waiting on someone | that creature's queue line: its mind, walker, route |
| `no way there … on the straight way: Door … locked, DC 28` | a door the route did not open | the walkthrough; `unlock TAG [SPIKE]`, a terminal, a script |
| `stalled N times`, the player high off the floor | navigation: the mesh sends a body where collision stops it | `kjk collision trace` (below) |
| `no object tagged …` after a module change | the route's goal is for the last module | the route |
| gaps, faults, missing scripts in the summary | an engine routine not bound or failing | `SCRIPTS.md`, `kjk script coverage` |

Tools for the second look:

- `kjk dialogue show k1 MODULE DLG`: the tree, its scripts and conditions.
- `kjk script dis k1 SCRIPT --module MODULE`: what a script calls.
- `kjk collision trace k1 MODULE x,y,z x,y,z [--shape point]`: a box
  starting `START SOLID` is a body that does not fit there. A grid of these
  maps a stuck spot in a minute; a downward point trace gives the floor's
  material (`Nonwalk`, `Metal`).
- `trace.txt` holds the scripts' first 4000 lines and their last 100 000
  (`TRACE_LINES`, `TRACE_TAIL`). For a moment deep in a route, play a
  copy of the route that ends just after it, from the checkpoint before.
- **Bisect** with a worktree per commit (`git worktree add --detach`, its
  build folder seeded), the routes played in order from the Endar Spire
  (a commit before `kjk chain` has none). Each commit's chain makes its
  own checkpoints; never mix checkpoints and binaries across commits
  when judging which one broke.
- A recording made in a module is not drawn yet (`kjk play --replay`); use
  `--stage --arrive` screenshots instead.

## Fixing what it finds

- **An engine rule for every module, never module code** (the owner's rule).
  A fix that names a tag, a module or a scene is wrong.
- Match the reference or the game's data. Mark what is a guess `[verify]`.
- Navigation and collision fixes: compare `kjk collision walk` (300 walks)
  on several modules before and after, from a clean build of the base
  (`git worktree add`, its build folder seeded as `BUILD.md` says). No
  module may lose.
- Gate before committing: fmt, clippy, approved tests with `KJK_SIM_HASH`
  (`TESTS.md`). The sim hash moves only for a change on purpose.
- Log every blocker and its fate in the run's journal table.

## Keeping the chain robust

A route is played anew by every build, and any change to the sim shifts
its fights by seconds: who falls first, where a blow throws a body, which
step a goal ends on. A chain that holds only for one timing breaks at the
next merge. Every block since the chain first ran to both endings came
from a rule that held for one timing and not the next:

| blocked | the timing that met it | the rule fixed |
|---|---|---|
| sewers, `to tar04_gamraidleader@2` | the leader fell before the player reached it, and the other after | a `to` whose target fell walks to where it lay, and re-plans when that place moves (`playthrough`) |
| Manaan, `use man27_flowA` | a walk's use key, pressed for a pressure door, met the flow button beside it; the goal's own press then used the button again, sending the water on twice | a held use acts once, a click with it the same use (`motion_flags::USE_SPENT`, `world::interact`); the harness's key is pressed only for what it would act on (below) |
| Manaan after the holster merge, `use man27_h2o02` | the walk to the flow room's button pressed for a pressure door, and the key met the next room's button (`man27_button04`, beside it), not the goal's: the water went on twice. The guard of the row above covered only the goal's own target | every press of the harness's use key, a walk's for a door or a goal's own, is made only where the sim's own pick (`SimWorld::key_use`, the step's `interact` uses the same) names what it is pressed for; else the key is not pressed (a goal's click still uses its target) |
| Dantooine's ruins, `use man15_door03` | the guard droids, not met during the death seal's door this time, came at the player after it; the fight back walked to where a strafing droid had been, planned again only 3.5 m on, and the blade never came within reach | every chase of a foe plans again as the fight goal's does, a blade's 1 m on (`chase_replan_m`) |
| Manaan's Hrakert station, `use ComputerPanel` | with the chase planned again 1 m on (the row above), the walk up to the heavy sentry droid never stalled against its wide body, 1.7 m off; a blade struck within 1.6 m of a foe's middle only | a blade's reach is measured to the foe's body: `STRIKE_M` plus however much wider than a humanoid's its `appearance.2da` personal space is (`strike_m`) |
| Tatooine, the Sand People's territory | a goal met at a run beside the map's edge; the slide carried the party out | the exit guard brakes a body whose slide ends in an exit it is not to take (`exit_ahead`) |
| Korriban, `use k39_plc_pillcomp` (no final test: one prestige short) | the valley's beasts, spawned during the door console, reached Mekel while the route stood waiting for his word, and killed him | the route fights them first, as a player defends the student the story needs (`fight 25`) |
| the Unknown World's dark way, `exit unk44_tpllckdoor` | the summit's exit trigger began its conversation as the player reached the computer; the computer's, asked by its use script, was refused ("already talking"), yet the use counted | a use is answered by its own conversation (its owner's, or its dialog), not one that plays meanwhile; one refused is pressed again (`playthrough`) |
| the Unknown World's temple, `fight 30` with Bastila | Jolee and Juhani stood within 4 m of the player as her round began; her AI, three foes grouped, chose a power for herself each round, which her 1 FP (her template's) never paid for: she stood all round, parried, and never lost the hit points that end the duel | `GetHasSpell` is a power known and paid for, as K1's exe has it (`script::engine`), so the AI passes over one it cannot cast and fights |
| the Star Forge's dark way, `exit sta45a_west_45b` | the party fought deck one's droids before reaching the dark Jedi's scene; a companion's silent shout ("I was attacked") reached the light Jedi 80 m off, who ran to help; the scene's light Jedi were not there to fall, and its conversation stayed paused | a line is heard within the listener's own hearing (`ranges.2da` `secondaryrange`), as K1's exe hears one only from a speaker its perception has heard (`script::engine::speak`); talk 1000, shout 250, whisper 3, silent talk 35, silent shout 1000 m besides |
| the Star Forge's dark way, `use k45_door_bast1` | the three Dark Jedi holding deck three's door, told to attack, came at a player still fighting troopers; one lost sight of the player behind a railing, and its attack, closing in, ran out after four rounds: it stood idle, and the door it held stayed shut | a script's attack lasts until it reaches its target, as KOTOR's walks to it (reone's), unless its walk can come no nearer (`script::advance`), and its mind keeps that target out of sight (`world::minds`) |
| Dantooine's ruins, `use man15_door03`, `use RakataDoor1@1`, `use dan15_compb` (a third of all first blocks) | the guard droids, strafing with repeaters, came at the player during the seals' scenes; the fight back walked to where a way ended short of a droid and stood there, or swung from 3 m, out of a blade's reach, while the droid held the party in combat for good | every fight, a `fight`'s and any goal's fight back, goes by one rule (`Chase`): a blade swings within the sim's own swing and steps in to its reach (`mind::STRIKE`, `mind::REACH`), a walk arrived short of a foe, or with no way to where it ends, goes on straight; probe `harness_chase` |
| the Star Forge's dark ending, `credits` | a blow threw Malak off the ledge as his drain's run began; the run was judged from the air and dropped | a scripted move's way is judged once its mover lands (`script::advance`) |
| the Star Forge, `exit sta45b_sta45c` (2026-10-17, after `EffectDisguise` became real and Manaan's suit shifted every timing after it) | the party had killed all of deck two's turrets on its way to the exit; two left off the path kept the door to the command centre shut ("cannot be opened while the turrets are active") | the route fights them first (`fight 40`), as a player told so by the door does |
| the Sand People's war, `to tat18_bantha1\|tat18_bantha2\|tat18_bantha3` (Komad's herd) | the herd grazed down the canyon's far side; the walk to it cut across the dune edge, a band of trigger whose warning jumps the player back inside, and was planned the same way again (53 warnings in 900 s). The rule for a herd behind the edge noted only a trigger that owned its conversation (this one's is a placeable its script picks) and judged by the straight line | a trigger the player stood in while a conversation held it and was then jumped out of turned it back: the goal's walks plan round the mesh's faces under it where a way round leads, else through (`TurnedBack`, `sim::walk::faces_under`), and a herd's alternatives are judged by that way; probe group `harness walls` |
| Tatooine, `exit TatooineTransition@1` (Anchorhead to the docking bay) | the walk stalled where a planter's drawn rim overhangs its walkmesh; one timing of the stall's sidestep and backing off climbed the rim and walked off the mesh on its top; the walk approached afresh there found no way from anywhere, so it made for the door itself, which no way leads into, and kept it once back on the mesh | a walk's approach is judged from where it sets out over the mesh: off it, the spot it steps back on to (`Here::approach`, `sim::walk::back_on`); probe `approach_off_the_mesh_from_where_it_steps_back_on` in `harness walls` |
| the Leviathan, `module lev_m40ac` (2026-10-20, after script-made creatures came to stand apart by the safe-location search) | the hangar lift's console answered "You must gather your party" twice: its conversation's `k_plev_chkprox` refuses while either companion stands more than 15 m from the console, and a trooper a trigger spawned as the player walked back fought Carth 27 m off; the route's one retry met the second refusal | a use waits on a companion's fight: while a foe keeping the party in combat fights a companion more than `STRAY_M` (10 m) from the player, the use is not pressed until that fight is fought (as an exit refused in a fight fights its foes first), once a goal (`straggling`); probe group `harness straggle` |
| Manaan's Sith base, `use man27_button03` (2026-10-25, after placeables' footprints came to collide) | two troopers stood 17.6 m from the player, as far as each other; the fight back took the nearer afresh each step, and they took turns; each turn planned the walk to them anew, its stalls forgotten before the third, whose sidestep goes the free way, and the player stood pressed against a door's jamb for 90 s | a fight keeps the foe it fights while that one still fits, as a player keeps their target, a `fight`'s and any goal's fight back (`Chase::pick`); probes `pick_*` in `harness chase` |
| Kashyyyk, `exit KashyyykDoor2` (2026-10-25, after the row above shifted the throne room's fight) | the fight ended with the player at a slanting wall and Freyyr come up beside it; his box's corner pinched against the wall on either side of the player, a pocket no walk, side step or backing off got out of (Freyyr idle for good) | a seat's player meets a body it only touches as one it stands inside, and slides along it while that takes it no nearer the body's middle, as K1's player slides along a creature (`world::met_as`, Q-0267); probe `body_overlap_not_buried` (scene) |
| Tatooine, `exit TatooineTransition@1` (2026-10-27, after companions came to keep formation close behind the player) | the walk stalled at the docking bay door between a mechanic standing idle and a footlocker, its box touching the mechanic's off his box's corner; every step and side step was weighed by how near it took the player to the mechanic's middle, and from his corner each led nearer, so all were taken back and the player stood held for good (with the companions farther back, the walk had come there a moment later and one side step had got out) | a body a seat's player touches or stands inside is weighed by the face of its box it overlaps least, as a trace stopped by the box slides along it: a move keeps its part along that face and is taken back only when it goes deeper across it (`Inside::hold_off`); probe `body_overlap_not_buried` (scene), off a box's corner |
| the Unknown World's south beach, `exit RakataDoor2` (2026-10-25, once placeables collided in every look) | no way led to the Elders' gate: a fence of defence pillars and invisible barriers (`plc_blockage`, placed in their closed look) and the pillars' ring before the gate, which had never collided in that look, so the route had walked through them and never met the Elders' hologram | the route meets the hologram first (`to talktrig`, `talk`): its "you may enter" destroys the ring's barriers (`unk41_holo03`), as a player is let in; the scene the game asks for, which a missing collision had let the route skip |
| TSL's prologue, `unlock med_bin` (2026-10-26, since a body standing inside another stopped being nudged out, 2026-10-21) | the medical bay's floor beside the bed is narrower than T3-M4's box (0.66 m to 0.76 m, the room's sloped wall foot); the walk stalled there, planned again across the bed, and its steps let T3 up onto the top, a nav island of its own, into the Exile lying there. The nudge out of the Exile's body had dropped T3 back on the floor | a walk standing on an island of the mesh its goal is not on steps down onto the goal's island at once, on the goal's side (`sim::walk::down_to`), as one off the mesh steps back on (`playthrough`) |
| Korriban's academy, `to kor35_torturer` (2026-10-26) | an edge wall ran 9,860 km across the academy's corridors from a sliver face's corner (three corners in a line, its outline turning straight back: `tan(pi / 2)` is -2.3e7 in f32, the wall's end set back by it); the walk's way crossed it wherever it went (one such wall in K1, one in TSL) | a turn straight back counts as past a right angle, its set back the most (`edges::half_turn`); every other corner as before; probe `edge_walls_turn_back` |
| the Unknown World's Elders, `fight 30` (2026-10-27) | the Elders and their guards fell one after another (`--mighty`: one blow each), the last 1.3 s before the goal's time was up; each died before it was seen to lose hit points, so no fall held the goal's clock | a foe seen at hand that has fallen since is progress too, as a foe losing hit points is (`playthrough`) |
| Peragus, `bash MedBayDoor`, `bash SecLoc`, `use Hatch_105PERDoor`, `use 102PERDoor` (2026-10-28) | the plasma torch (a stun baton's row) never struck: only bare fists punched, and no blade swings a baton; a hit would have done nothing besides, the door's Hardness 100 turning it whole, so its `OnMeleeAttacked` never ran. The vibroblade's swings passed through the locker between lockers (no brushes), and the walk to pick its lock stops up to 2 m off. The sealed hatches' fail-to-open hint is a conversation of one line, begun and over within a step, never seen playing | a stun baton punches as fists do, a blow of the baton (`world::strikes`); Door Cutting takes all of a door's hit points, and a weapon's blow its hardness turns raises `OnMeleeAttacked` still (`world::objects`); a blade meets a brushless placeable's pick volume (`Step::bash`); a `bash` with a blade or baton steps in while it swings (`playthrough`); a use is answered by its target's own one-line conversation said since the press (`playthrough`); probes `baton_bashes`, `door_cutting_breaks`, `hardness_turned_still_attacked` (`bash_doors`) |
| Peragus's dormitories, after "[Destroy the console]" (2026-10-28) | the reply's script has the Exile attack the shift console's invisible twin (`a_bash_console`: `ActionAttack` on `ShftInvis`), whose `OnMeleeAttacked` opens the dormitory door; a script's attack on the player was dropped (its seat fights), and one on an object did nothing for anyone | a script's attack of a door or placeable is KOTOR's attack of an object: a blow a round within reach, locked or not; a player's waits at the front of its queue until the player stands within reach (`SimWorld::assigned_strike`), and the harness walks there whatever the goal; probes `script_attacks_an_object`, `player_strikes_what_it_was_told_to` (`bash_doors`) |
| TSL's Peragus mining tunnels, `at -1.4 15.8` and `use Shftcom` (2026-10-28) | no way led from the first fuel pump to the cave: the hallway past the damaged droid (`TDROID`) meets the cave's floor only through a sliver face 6 mm wide lying along the hallway's wall; the furniture check's 10 cm column over its points met that wall, so it was `covered`; the dead end's door the route made for (`PeragusDoor1`), which a player cannot go past either, has no floor through it | a face narrower than the column has no floor of its own and is open where two faces it joins are (`collision::nav::NavMesh::mark`, `Face::sliver`); probe `sliver_nav_open` (`surface_probes`) |
| Peragus' escape, `turrets` in 107per (2026-10-28) | Atton, poisoned walking the hangar bay's gas leak, fell to its last dose as his `106percut` began; a knocked-out party member counted as dead, and its conversation ended cut short before `a_106per_movie` set the turret going. Once it ran, the trooper last to board stood on the hangar's render mesh for a minute, past the goal's 90 s, and Atton's `turend` that ends the turret was never answered: the minigame parked the player's whole command, its answer too | a party member knocked out does not end its conversation (`Thing::dead_for_good`); a minigame parks what moves the player, not its answer (`world::minigame::steer`); a global the minigame's scripts set changing holds a `race`'s or `turrets`' clock (`playthrough`); probes `owner_knocked_out_talks_on`, `owner_dead_talks_on` (`talk ends`; since Nar Shaddaa's `miraend`, a dead owner talks on), `talk_answered_while_playing` (`minigame tsl`) |

So:

- **Fix the class, not the case.** The question is never "why this
  route" but "what rule let this happen": an engine rule (as the
  original behaves), or a harness rule (as a player plays). A route
  edited around a block (a longer `wait`, another goal order) hides the
  rule until the next timing finds it. A goal that does what a player
  would (fighting the beasts that come for someone the story needs) is
  the route's to add; one that only waits a timing out is not.
- **The harness plays as a KOTOR player.** One press is one use, of
  what it means: the use key is pressed only where the sim says it acts
  on that (`SimWorld::key_use`), never on a guess of the key's reach
  beside it (a guess is right for one room and wrong in the next), and no
  use counted answered by a conversation that is not its own; it never leaves a
  body sliding into a way out; a goal names what is there to the end
  (a fallen target's place, not a vanished index).
- **Ask the sim, don't copy it.** Where the harness needs to know what
  the sim will do (what a press meets, where a slide ends), it asks the
  sim's own function, or one the sim's step shares; a copy of the rule
  in the harness drifts from it. A use is answered by its conversation or
  by the use the sim heard (`SimWorld::uses_heard`) once what it set going
  has played out (`State::busy`); a click is made from within its reach
  as the target block walks up (`sim::target::Walk::near`); a blade
  strikes within the sim's swing (`mind::STRIKE`); a door is planned as
  locked once the sim heard it refuse a press and its scripts are done; a
  fallen creature's remains are waited for while the engine's corpse is
  pending (`State::corpses`); `if`, `while` and `until` read once the
  world has settled (`Settle`: the party's and the last conversation
  partner's deferred code too, as the Leviathan's lift leaves for the
  hangar a tenth of a second after its menu); a fight never goes through
  a locked door to a foe (`Here::open_way`: pressing it is a route's
  `use`, its script the story's); a goal is hopeless early only out of
  combat with nothing deferred before its time would run out, as is one
  whose target cannot answer it as it stands (an `unlock` of a thing not
  locked, a `use` of a placeable not useable: the Harbinger's route named
  the first of seven `MilLowFootLker`, a bare prop); a `while`
  leaves after `QUIET_ROUNDS` rounds in a row that changed nothing.
- **A rule the chain finds gets a probe** (`kjk sim probes`; `use_scripts`
  has `held_use_once`, `key_use_foretold`, `has_spell_paid_for`,
  `shout_heard_within_hearing` and `move_waits_to_land`;
  `fight_through_a_hold` has `attack_keeps_an_unseen_target`; `harness
  walls` has `walk_goes_round_a_trigger_that_turned_it_back`), so it
  holds without the hours of the chain.
- **Green to both endings before a merge**, with the chain's own flags
  (`data/routes/k1/chain.flags`, not `--bare`), and every route met, not
  "met but the branch skipped".

## Not yet

Drawing a module's recording; bashing locked doors.
