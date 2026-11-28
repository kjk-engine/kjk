# Console commands

Generated from `crates/commands` by `cargo xtask console` (do not edit
by hand). `kjk commands` prints this list; `kjk commands --json`
prints it as JSON.

## Syntax

- Commands are case-insensitive and separated by `;` or newlines.
- Use double quotes (`"..."`) to include spaces in an argument.
- Arguments in `[BRACKETS]` are optional.
- `WHO` is `me` or a party slot number starting at `1` (defaults to your character).
- Headless `--cmds` scripts accept every command below (`docs/RUN.md`).

## Cheats & Permissions

Commands marked **Cheat.** always work on Debug Mode and combat test
maps. In normal play they require cheats to be enabled:

- **Solo:** enabled when Gameplay -> Console is on, or when running
with `--cmds` or `KJK_CONSOLE=1`.
- **Co-op:** set by the host in the lobby (`Cheats`: `Off`,
`Host only` [default], or `All players`). `warp` and `load` are
always host-only. Players can only edit their own character and
unclaimed companions.
- Using any cheat outside test maps permanently marks the game and
its saves **Cheats used**.

## Menus & Mods

The console also opens at the main menu and in the co-op lobby
(main-menu commands cannot run inside a lobby). Mods can add custom
cheat commands via `kjkconsole.2da` (`docs/MODS.md`); list them with
`mods commands`.

## General

### `help [NAME]`

List available commands. Use "help <command>" for details, or "help all" to include unavailable commands.

Aliases: `?`.

Available: anywhere. Example: `help spawn`

### `find TEXT`

Search commands and creature templates matching TEXT.

Available: anywhere. Example: `find sith`

### `clear`

Clear the console output.

Available: anywhere. Example: `clear`

### `whereami`

Show the current module and your position (in meters).

Available: in-game. Example: `whereami`

### `version`

Show the kjk version.

Available: anywhere. Example: `version`

## Character

### `god`

Toggle god mode (invulnerability) for your character.

Aliases: `invulnerability`, `arena god`.

Available: in-game. Cheat. Example: `god`

### `level CLASS LEVEL [SECOND] [SECONDLEVELS]`

Rebuild your character at total level LEVEL in CLASS, auto-picking feats, powers, and skills. Optionally split SECONDLEVELS into a second class (SECOND). Pass class names or IDs from classes.2da.

Aliases: `setlevel`, `arena level`.

Available: in-game. Cheat. Example: `level 3 20 9 5`

### `attribute NAME SCORE [WHO]`

Set an attribute (strength, dexterity, constitution, intelligence, wisdom, charisma) for yourself or party member WHO.

Aliases: `arena ability`, `setstrength`, `setdexterity`, `setconstitution`, `setintelligence`, `setwisdom`, `setcharisma`.

Available: in-game. Cheat. Example: `attribute strength 18`

### `skill NAME RANKS [WHO]`

Set skill ranks (computer, demolitions, stealth, awareness, persuade, repair, security, treat) for yourself or party member WHO.

Aliases: `arena skill`, `setcomputeruse`, `setdemolitions`, `setstealth`, `setawareness`, `setpersuade`, `setrepair`, `setsecurity`, `settreatinjury`.

Available: in-game. Cheat. Example: `skill security 10`

### `alignment VALUE [WHO]`

Set alignment (0 = full Dark Side, 100 = full Light Side) for yourself or party member WHO.

Aliases: `arena alignment`.

Available: in-game. Cheat. Example: `alignment 100`

### `restore`

Reset your character to the Debug Mode default with full Vitality and Force.

Aliases: `arena restore`.

Available: Debug Mode / combat test maps. Cheat. Example: `restore`

### `down DEATH`

Play death animation DEATH (1-19) on your character. You stand back up after 7 seconds.

Aliases: `arena down`.

Available: Debug Mode / combat test maps. Cheat. Example: `down 4`

### `giveitem TEMPLATE [COUNT]`

Add an item (TEMPLATE) to party inventory, optionally specifying COUNT.

Available: in-game. Cheat. Example: `giveitem g_w_lghtsbr01 2`

### `givecredits AMOUNT`

Add AMOUNT credits to the party (capped at 9,999,999).

Available: in-game. Cheat. Example: `givecredits 500`

### `addexp AMOUNT`

Add AMOUNT XP to the party.

Aliases: `addxp`.

Available: in-game. Cheat. Example: `addexp 2000`

### `addlevel`

Give the party enough XP for your next level.

Available: in-game. Cheat. Example: `addlevel`

### `heal [WHO]`

Restore full Vitality and Force to party member WHO, or to everyone you control.

Available: in-game. Cheat. Example: `heal`

## World

### `kill [TAG]`

Kill your current target, or the nearest creature matching TAG. Triggers death scripts normally. Cannot kill party members or plot-essential NPCs.

Available: in-game. Cheat. Example: `kill`

### `setglobal NAME VALUE`

Set global variable NAME to VALUE. For boolean globals, 0 = false and any other value = true.

Available: in-game. Cheat. Example: `setglobal K_SWG_HELENA 1`

### `runscript RESREF [WHO] [PARAMETERS]`

Run script RESREF on the module, or on party member WHO. In KOTOR 2, pass up to 5 integers and a trailing string after WHO (read via GetScriptParameter / GetScriptStringParameter).

Available: in-game. Cheat. Example: `runscript k_sup_gohawk`

### `spawn TEMPLATE`

Spawn a hostile creature (TEMPLATE) in front of you.

Aliases: `arena spawn`.

Available: in-game. Cheat. Example: `spawn g_sithtroop01`

### `clearfoes`

Remove all non-party creatures from the map.

Aliases: `arena clear`.

Available: Debug Mode / combat test maps. Cheat. Example: `clearfoes`

## Session

### `warp MODULE [ARRIVAL]`

Load MODULE (optionally at door/waypoint ARRIVAL). From the main menu, starts a new game there with a default character. Marks the save as Cheats used.

Available: anywhere. Cheat. Example: `warp tar_m02aa`

### `sandbox`

Launch the Debug Mode combat sandbox.

Available: main menu. Example: `sandbox`

### `saves`

List saved games and their IDs, newest first.

Available: anywhere. Example: `saves`

### `load NUMBER`

Load a saved game by ID (run "saves" to list IDs).

Available: anywhere. Example: `load 0003`

### `mods [COMMANDS]`

List active override folders and Steam Workshop mods in load order. Use "mods commands" to list custom mod commands and any skipped kjkconsole.2da rows.

Available: anywhere. Example: `mods commands`

### `host [NUMBER]`

Open a co-op lobby for a new game or saved game NUMBER.

Available: main menu. Example: `host 0003`

### `join ADDRESS [NAME]`

Join a co-op game at ADDRESS (IP, host:port, or Steam friend), optionally setting your NAME.

Available: main menu. Example: `join 192.168.1.20`

## Command scripts

### `wait FOR`

Wait for a duration (2s, 500ms), frame count (30f), or state (world, loading, loading_screen).

Available: --cmds scripts only. Example: `wait world`

### `screenshot [NAME]`

Save the next settled frame as NAME.png in the run folder.

Available: --cmds scripts only. Example: `screenshot hall`

### `film TIME [NAME]`

Capture every frame for TIME (real time) as raw RGB to NAME.rgb.

Available: --cmds scripts only. Example: `film 8s fight`

### `dump [NAME]`

Dump current run state to NAME.json in the run folder.

Available: --cmds scripts only. Example: `dump after`

### `camera WORDS`

Move camera to x y z (meters) with yaw and pitch (degrees), plus optional FOV.

Available: --cmds scripts only. Example: `camera 20 20 1.5 90 0 60`

### `anim SECONDS`

Freeze animation and game time at SECONDS, or resume with "anim run".

Available: --cmds scripts only. Example: `anim 2.5`

### `tick TICK`

Step simulation to TICK and pause, or resume with "tick run".

Available: --cmds scripts only. Example: `tick 400`

### `module MODULE [ARRIVAL]`

Load MODULE (optionally at ARRIVAL) and wait until it finishes loading.

Available: --cmds scripts only. Example: `module end_m01ab`

### `travel MODULE [ARRIVAL]`

Start loading MODULE (optionally at ARRIVAL) without waiting.

Available: --cmds scripts only. Example: `travel end_m01ab`

### `record WORDS`

Record a video: NAME FROM TO FPS (game time in seconds).

Available: --cmds scripts only. Example: `record walk 1.5 5.5 30`

### `clip WORDS`

Record a video from now: NAME SECONDS FPS.

Available: --cmds scripts only. Example: `clip fight 9 12`

### `size WIDTH HEIGHT`

Resize a headless frame to WIDTH x HEIGHT pixels (0 0 simulates a minimized window).

Available: --cmds scripts only. Example: `size 1920 1080`

### `quit`

Exit the run with status 0.

Available: --cmds scripts only. Example: `quit`

### `crash`

Trigger a deliberate panic to test crash reporting.

Available: --cmds scripts only. Example: `crash`

### `ui WORDS`

Send a UI test command (e.g. "ui open inventory", "ui press BTN_OK"). See docs/UI.md.

Available: anywhere. Example: `ui open inventory`

### `vfx WORDS`

Play a visualeffects.2da row on a tagged creature for a duration, with an optional beam source tag.

Available: in-game. Example: `vfx 2007 end_trask 9`
