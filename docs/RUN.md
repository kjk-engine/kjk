# Running and scripting `kjk`

```bash
kjk                                   # opens the window, runs until closed
kjk --cmds 'wait 60f; screenshot first; dump; quit'
kjk --headless --cmds 'wait 60f; screenshot; quit'   # no window
kjk --load quick                      # opens on a save (`quick`, `0001`, …), as the main menu loads it
KJK_PERF=1 kjk --cmds '…'             # also writes a trace
kjk play --mute …                     # no sound (or KJK_MUTE=1); the settings' volumes untouched
KJK_MOVIES=1 kjk play --headless …    # movies played headless too (SOUND.md)
kjk movie import k1 01a               # a movie made ours by ffmpeg, in the store
kjk view k1 c_bantha --headless --cmds 'screenshot; quit'   # a model
```

## Commands

Separated by `;` or new lines. Each one finishes only when what it promises
is true, and the next starts after it. A command that cannot keep its promise
ends the run with exit code 1; a script that does not parse exits with 2.

| command | finishes when |
|---|---|
| `wait 2s`, `wait 500ms`, `wait 30f` | the time, or that many frames, has passed |
| `screenshot [name]` | no render pipeline has been compiling for 3 frames (60 s limit), then `<name>.png` exists (10 s) |
| `film 8s [name]` | as `screenshot`, then every frame for that long (wall time) went to `<name>.rgb` (raw RGB, one frame after another, written on its own thread; about 1.4 GB for 10 s at 1280×720), and `<name>.txt` gives the size (`1280x720`), then each frame's ms since the first |
| `dump [name]` | `<name>.json` is written: frame, elapsed time, entity count, facts such as the render adapter |
| `wait world`, `wait loading`, `wait loading_screen` | the state a plugin publishes is true (no module loading; one loading; its screen up) |
| `module end_m01ab [tag]` | the module is installed (`kjk play`), arriving at the tag's door or waypoint, and the world is up |
| `travel end_m01ab [tag]` | the same install asked for, done as soon as the game takes it: what follows runs while it loads (`wait loading_screen; screenshot`) |
| `camera 20 20 1.5 90 0 [60]` | at once: the camera at x y z (metres, game space) looking along yaw and pitch (degrees), with a vertical field of view if given |
| `ui open inventory`, `ui press BTN_OK`, … | at once: the request handed to the game's screens (`UI.md` lists them) |
| `vfx 2007 end_trask [9] [source]` | at once: a `visualeffects.2da` row played on the creature with that tag, as a script's would be, for that many seconds, a beam from the source's tag |
| `anim 2.5`, `anim run` | at once: animation and game time are held at that second (`kjk play` steps the sim up to it), or run with the clock again |
| `record name 1.5 5.5 [30]` | game time is held at each frame's moment from 1.5 s to 5.5 s in turn (30 a second), each screenshotted (`name-0000.png`, …), then `name.mp4` is made of them with the run's `audio.wav` by `ffmpeg` (none: the frames stay, with a warning) |
| `clip name 9 [12]` | the game, running on, screenshotted 12 times a second of its time for 9 s (later when capturing falls behind), each frame stamped with the game's time it shows, then `name.mp4` of them with the sound (`SOUND.md`) |
| `quit` | always; the run ends with success |

Unnamed screenshots and dumps are numbered (`screenshot-1`, `dump-1`, …).

`kjk play` (and `host`, `join`) opens its window at once on KOTOR's loading screen and loads its
stage on a thread meanwhile (as the main menu's New Game does), drawing the
scene's remembered pipelines while it waits. Its script starts once the
stage is in, so `wait 30f` is 30 frames of the game, as before. A stage that
does not load ends the run with exit code 1 and why.

## The run folder: `kjk-artifacts/runs/<utc time>-<pid>/`

- `kjk.log`: the log, without colours.
- `manifest.json`: git commit (with `-dirty`), build profile, headless or
  not, the script, the render adapter, and each install's fingerprint when
  the fingerprint cache already covers it (`kjk fingerprint` fills it).
- screenshots and dumps the script asked for, and `record`'s videos.
- `audio.wav` from a headless `kjk play`: its sound on the game's clock
  (`SOUND.md`).
- `trace.json` with `KJK_PERF=1`: Chrome trace format, with a span per Bevy
  system per frame. Open it at <https://ui.perfetto.dev>. It is large (about
  15 MB per second of running), so it is opt-in.

## Where a launch's time goes

```bash
kjk launch-report                      # end_m01aa, 3 launches
kjk launch-report --stage course --runs 5
```

Each launch is a headless `kjk play` that dumps `launch` (`dump launch`
in any script does too): the loader's phases in ms (the JA rig, the KOTOR
stage's installer, archives, plan and its parts, scene; the app's engine
and game plugins; the scene's spawn; the pipeline warm-up), and moments
since the process began (loaded, app built, world up, scene spawned,
pipelines warmed). The report tables each one's median, fastest and
slowest. A phase is added with `launch_times::timed` or `record`.

## The whole-game sweep

```bash
kjk sweep                       # K1 and TSL, every module
kjk sweep tsl --only 1          # one game, the modules starting so
kjk sweep k1 --skip render,audit --seconds 20 --jobs 6
```

Every module is built (`ModuleIr`), its scripts stepped headless for
`--seconds` (`kjk script sweep`: gaps, faults, and the scripts the game
names but does not have, which run nothing as in the original), a
640×360 frame rendered from its entry as a `kjk tour` `entry` stop (a child `kjk module` run that
also dumps the scene's facts: missing models, textures and clips, refused
creatures, room walkmeshes that do not read; rooms the game gives no
walkable face, a sky or a backdrop, are listed as seen only and are not a
fault), and its containment audit run as `kjk
collision audit` does by default. `--jobs` modules are built, rendered
and audited at once while the scripts step. The run folder holds:

- `index.html`: tallies per game, then every module worst first with its
  frame, its audit map and what is wrong, filterable by game and area,
  then the bug lists as tables. The score orders it: a module that does
  not build 1000, whose scripts do not install 500, a failed render or
  audit 300 and 200, script faults 100 and up, a near-black frame 80,
  escapes on foot 50 and up and by jump 20 and up, missing assets, then
  one per script gap reached and per script named but missing. A game whose install is missing is left
  out with a warning; the others are still swept.
- `bugs.md`: the same lists in Markdown, for handing out: refusals and
  failed stages, script faults by message, script gaps by modules reaching
  them per game, scripts named but missing, containment escapes, the
  slowest audits, missing assets; each missing texture with what is drawn
  in its place (no lightmap, the lightmap alone, nothing, no env map, or
  the body model's own textures: `viewer::stand_in`). On the Steam installs
  every texture listed is one the game does not ship in any archive: TSL
  models naming K1 textures, lightmap names from before the lightmaps were
  packed, body textures `appearance.2da` names.
- `sheet-k1.png`, `sheet-tsl.png`: every frame of a game, worst first.
- `shots/`, `audits/`, `logs/` (each render's output), and `facts.json`.

## Playing a module through: `kjk playthrough`

The working method (walkthrough, nav checks, routes, reading blockers): `PLAYTESTS.md`.

```bash
kjk playthrough k1 end_m01aa 'talk; use end_locker01; to end_door01'
kjk playthrough k1 end_m01aa route.txt --arrive end_wp_flame01 --follow end_trask
kjk playthrough k1 end_m01aa data/routes/k1/endar_spire.route \
  --god --skill security=2     # the Endar Spire, the bunks to Taris (I29)
kjk playthrough k1 @taris_start data/routes/k1/taris_upper_city.route --god
```

`--god` keeps the player above 0 HP (the sheet's immortal flag, which a
script's `SetMinOneHP` leaves alone): a walk that tests what the scripts and the world do, not
the fights' balance. `--mighty` makes the player's blows count ten times
over (fights end quickly; a test aid as `--god` is). The player clicks
past every conversation line that plays out on its own, as a player
skips dialogue (`Pick::Skip`); `--listen` hears every line out.
`--stale` starts from a checkpoint made with other level data than this
build's: its module takes this build's data and the modules it left are
met anew — a dev aid to go on at the chain's top while the chain is
remade, not the run the chain would make. `--skill NAME=RANK`,
`--class`, `--global NAME=N`, `--run SCRIPT` and `--follow TAG` as
`kjk play`'s.

No window: the authority stepped as fast as it goes, a player driven goal
by goal (`launcher::playthrough` lists the goals: `talk`, `pick`, `say`, `party`, `travel`, `to`,
`at`, `use`, `exit`, `fight`, `race` and `turrets` (a minigame played to
its end, `MINIGAMES.md`), `pazaak`, `equip` (`equip TEMPLATE`: that item; with `*` in it, the first item made
that it matches, for what the game rolls at random),
`put`, `save NAME` (a checkpoint; `@NAME` for the module starts from it), `unlock` (Security, with a spike
the party carries if named), `bash` (struck with what is in hand until its lock breaks), `disarm X,Y`
(the mine the party sees nearest there, Demolitions, from where the player stands), `leader`, `solo`, `levelup`, `sell`, `credits`, `wait`, `until`,
`module`, `if` and `while` blocks closed by `end`; and, for what a route stands in for, `global`,
`gold`, `skill NAME=RANK` and `goto MODULE [TAG]`, the module's entry
without a tag). Walks go over the
navigation mesh (`sim::walk`), pressing use on the way; offers are
answered once their line has played (`pick`'s choices, else the first); a
container used is emptied; a party selection screen a script shows is
closed keeping whom it forces in; a door that leads out installs its module and
the route goes on there (an exit the party may not take in a fight, or whose way a door
stayed shut on with foes near, has its foes fought first, as `fight 25`,
as a player must). A level the party's XP allows is taken as a goal
starts, as a player would, in the class taken last (`levelup` names
another); a `use` of a foe still standing attacks it, and a target is
found by its tag or its template. A goal not met within `--goal-seconds` of play is
**BLOCKED**: the line says where the player stood, the conversation that
holds it and every script queue at work (a move's target, its route,
the mind walking it), how far the target is, whether a way leads there
and the doors and placeables on the straight way (locks, DCs); the run
ends there (with `--keep-going` it goes on with the next goal), and exits
1. `save` goals write to `kjk-artifacts/checkpoints/GAME` (`--save-to
DIR`: there). The run folder has `facts.json`
(each goal with its play time and its wall time, `wall_ms`; the
scripts' faults, gaps, missing scripts, the globals),
`transcript.txt`, `trace.txt` (the scripts' first lines) and
`inputs.kjkr` (a recording; `kjk play --replay` draws one made off a
module, not yet one made in a module).

## Playing the routes through: `kjk chain`

```bash
kjk chain k1                                 # every route of data/routes/k1
kjk chain k1 manaan                          # Manaan and every route after it
kjk chain k1 manaan tatooine --alone --jobs 2  # just these two, from their kept checkpoints
kjk chain k1 --quick                         # every route alone, at once
kjk chain k1 --dry-run                       # each route's command and cached outcome
```

A game's routes as the chain they make (`PLAYTESTS.md`): each route a
`kjk playthrough` with the game's chain flags (`data/routes/GAME/
chain.flags`, k1's `--mighty --skill security=2`; `--bare` leaves them
out), its header line's start and flags, and `--mighty`, each `--skill`,
`--keep-going` and `--goal-seconds` when given here, each flag once. A
route starts as soon as the route before it (the one whose `save` makes
its checkpoint) has saved it in this run (one saved in a `while`, or
twice, once that route has ended); one not saved skips it and the routes
after it. The chain stops at its first block: no route starts after one
(`--keep-going` plays on). Routes named start from the checkpoints kept
(`kjk-artifacts/checkpoints/GAME`) as they stood when the run began;
`--alone` (`--quick`) starts every route chosen so, none waiting on
another. Up to `--jobs` play at once (half the cores by default).

The run folder holds a copy of the binary every route runs (let go at
the end), the routes as they were, `starts/` (the kept checkpoints it
started from) and `checkpoints/` (what its routes saved, `--save-to`).
After the run, the checkpoints `data/routes/GAME/saves.list` names are
written as saves `k1-01`, `k1-02`, … for the window's Load screen and
`kjk --load k1-03` (the main checkout always; a worktree with `--saves`;
`PLAYTESTS.md`).
The checkpoints of a route met replace the kept ones; a blocked route's
stay in the run folder. A route met or blocked is cached
(`kjk-artifacts/chain-cache/GAME`, the last three binaries') under a
hash of the binary, the game data's fingerprint, its start checkpoint's
bytes, its route and its arguments; an identical rerun is taken from
there, its checkpoints and log with it (`--no-cache`: every route
played). Each route's line gives how it ended (`met`, `BLOCKED`,
`FAILED`, `skipped`), its wall time, `(cached)` when taken from the
cache, and its summary; the run folder has each route's log and
`facts.json` (each route's command). Exit 0 when every route met all its
goals.

## Headless

`--headless` runs with no window and no windowing system: the camera renders
into a 1280×720 image (`--size 1600x1200` for another), and `screenshot`
reads that image back. Use it for
CI, agents and, later, the dedicated host.
