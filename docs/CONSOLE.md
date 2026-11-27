# Console commands

This page is written from the table of commands in `crates/commands` by
`cargo xtask console`. Do not edit it by hand. `kjk commands` prints the
same list, and `kjk commands --json` prints it as JSON.

The `--cmds` scripts of headless runs accept every command on this page
(`docs/RUN.md`). Commands are separated by `;` or by new lines. Double
quotes group words into one word. A command's name may be typed in upper
or lower case. The launcher checks every line of a script before the run
starts, and a line with an unknown command or a wrong argument ends the
run with exit code 2.

A command that changes the game sends an order to the simulation, as
the keys of the Debug Mode map do. These commands are marked "Changes
the game". On the Debug Mode map and the other combat test levels
they always work. Elsewhere they work only when the session's cheat
rule allows them. The rule is Off by default. Under the rule Host only,
only the host can use them. A run with a command script (`--cmds`), or
with the console setting turned on, plays under the rule Host only.
The first such command outside the combat test levels marks the game
as "Cheats used", and a save keeps the mark. A player
can change only their own character, the companion they lead, or a
companion that no other player controls.

An argument in brackets may be left out. WHO is a party member: `me`, or
a party member's number, where 1 is the player's own character. Without
WHO, a command changes the player's own character.

## General

### `help [NAME]`

Lists the commands that can run now. help NAME describes one command, and help all also lists the commands that cannot run now.

Also accepted: `?`.

It works at any time. Example: `help spawn`

### `find TEXT`

Lists every command and every creature template whose name contains the text.

It works at any time. Example: `find sith`

### `clear`

Empties the console's log.

It works at any time. Example: `clear`

### `whereami`

Prints the module the player is in and where the player's character stands, in metres.

It works while a game is loaded. Example: `whereami`

### `version`

Prints the version of kjk.

It works at any time. Example: `version`

## Character

### `god`

Turns god mode on or off for the player's character. A character in god mode cannot die.

Also accepted: `invulnerability`, `arena god`.

It works while a game is loaded. Changes the game. Example: `god`

### `level CLASS LEVEL [SECOND] [SECONDLEVELS]`

Rebuilds the player's character as a character of the class at this total level. The game makes the choices of every level. With a second class (a specialization), that many of the levels are in the second class. A class is its number in classes.2da or its label.

Also accepted: `setlevel`, `arena level`.

It works while a game is loaded. Changes the game. Example: `level 3 20 9 5`

### `attribute NAME SCORE [WHO]`

Sets an attribute of the player's character, or of the party member WHO, to the score. The attributes are strength, dexterity, constitution, intelligence, wisdom and charisma.

Also accepted: `arena ability`, `setstrength`, `setdexterity`, `setconstitution`, `setintelligence`, `setwisdom`, `setcharisma`.

It works while a game is loaded. Changes the game. Example: `attribute strength 18`

### `skill NAME RANKS [WHO]`

Sets the ranks of a skill of the player's character, or of the party member WHO. The skills are computer, demolitions, stealth, awareness, persuade, repair, security and treat.

Also accepted: `arena skill`, `setcomputeruse`, `setdemolitions`, `setstealth`, `setawareness`, `setpersuade`, `setrepair`, `setsecurity`, `settreatinjury`.

It works while a game is loaded. Changes the game. Example: `skill security 10`

### `alignment VALUE [WHO]`

Sets the alignment of the player's character, or of the party member WHO. 0 is fully dark side. The highest value is fully light side.

Also accepted: `arena alignment`.

It works while a game is loaded. Changes the game. Example: `alignment 100`

### `restore`

Makes the player's character the Debug Mode map's starting character again, with full hit points and Force points.

Also accepted: `arena restore`.

It works on the Debug Mode map and the other combat test levels. Changes the game. Example: `restore`

### `down DEATH`

Knocks the player's character down with death animation DEATH, from 1 to 19. The character gets up where it lies 7 seconds later.

Also accepted: `arena down`.

It works on the Debug Mode map and the other combat test levels. Changes the game. Example: `down 4`

### `giveitem TEMPLATE [COUNT]`

Adds the item of this template to the party's inventory. With COUNT, adds that many of it.

It works while a game is loaded. Changes the game. Example: `giveitem g_w_lghtsbr01 2`

### `givecredits AMOUNT`

Adds this many credits to the party's credits, as the original games' givecredits does. The party never has more credits than the console's limit.

It works while a game is loaded. Changes the game. Example: `givecredits 500`

### `addexp AMOUNT`

Adds this many experience points to the party's experience.

Also accepted: `addxp`.

It works while a game is loaded. Changes the game. Example: `addexp 2000`

### `addlevel`

Gives the party the experience that the player's character needs for its next level.

It works while a game is loaded. Changes the game. Example: `addlevel`

### `heal [WHO]`

Gives the party member WHO full hit points and Force points. Without WHO, heals every party member the player may change.

It works while a game is loaded. Changes the game. Example: `heal`

## World

### `kill [TAG]`

Kills the creature the player has targeted, or the nearest creature with this tag. Its death script runs as for any other death. Party members and creatures that the story needs cannot be killed.

It works while a game is loaded. Changes the game. Example: `kill`

### `setglobal NAME VALUE`

Sets the script global NAME to VALUE. A global that holds true or false becomes true for any value but 0.

It works while a game is loaded. Changes the game. Example: `setglobal K_SWG_HELENA 1`

### `runscript RESREF [WHO]`

Runs the game's script RESREF. With WHO, the party member WHO runs it. Without WHO, the module runs it, as it runs its own scripts.

It works while a game is loaded. Changes the game. Example: `runscript k_sup_gohawk`

### `spawn TEMPLATE`

Calls in a hostile creature of this template in front of the player's character.

Also accepted: `arena spawn`.

It works while a game is loaded. Changes the game. Example: `spawn g_sithtroop01`

### `clearfoes`

Removes every creature that is not in the party.

Also accepted: `arena clear`.

It works on the Debug Mode map and the other combat test levels. Changes the game. Example: `clearfoes`

## Command scripts

### `wait FOR`

Waits for a time (2s, 500ms), a number of frames (30f) or a state (world, loading, loading_screen).

It works only in a command script (--cmds). Example: `wait world`

### `screenshot [NAME]`

Saves the next settled frame as NAME.png in the run's folder.

It works only in a command script (--cmds). Example: `screenshot hall`

### `film TIME [NAME]`

Saves every frame for a length of wall time as raw RGB in NAME.rgb.

It works only in a command script (--cmds). Example: `film 8s fight`

### `dump [NAME]`

Writes the run's state as NAME.json in the run's folder.

It works only in a command script (--cmds). Example: `dump after`

### `camera WORDS`

Puts the camera at x y z (metres) looking along a yaw and a pitch (degrees), with an optional field of view.

It works only in a command script (--cmds). Example: `camera 20 20 1.5 90 0 60`

### `anim SECONDS`

Holds animation and game time at a second, or lets them run again with anim run.

It works only in a command script (--cmds). Example: `anim 2.5`

### `tick TICK`

Steps the world to a tick of the simulation and holds it there, or lets it run again with tick run.

It works only in a command script (--cmds). Example: `tick 400`

### `module MODULE [ARRIVAL]`

Loads a module, arriving at a door or a waypoint, and waits until its world is up.

It works only in a command script (--cmds). Example: `module end_m01ab`

### `travel MODULE [ARRIVAL]`

Asks for a module to load, as module does, and goes on at once while it loads.

It works only in a command script (--cmds). Example: `travel end_m01ab`

### `record WORDS`

Records a video: NAME, FROM and TO in seconds of game time, and frames a second.

It works only in a command script (--cmds). Example: `record walk 1.5 5.5 30`

### `clip WORDS`

Records the running game as a video: NAME, seconds of game time, and frames a second.

It works only in a command script (--cmds). Example: `clip fight 9 12`

### `size WIDTH HEIGHT`

Resizes a headless run's frame to a width and a height in pixels. size 0 0 stands for a minimized window.

It works only in a command script (--cmds). Example: `size 1920 1080`

### `quit`

Ends the run with success.

It works only in a command script (--cmds). Example: `quit`

### `crash`

Makes the program panic, to show what a crash leaves in the run's folder.

It works only in a command script (--cmds). Example: `crash`

### `ui WORDS`

Passes words to the game's screens, such as ui open inventory or ui press BTN_OK. docs/UI.md lists them.

It works at any time. Example: `ui open inventory`

### `vfx WORDS`

Plays a row of visualeffects.2da on the creature with a tag, as a script would, for a number of seconds, with a beam from a second tag.

It works while a game is loaded. Example: `vfx 2007 end_trask 9`
