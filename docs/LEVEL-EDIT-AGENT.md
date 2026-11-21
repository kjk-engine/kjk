# The level editor's agent link

This guide is for an agent, such as Claude, that helps someone build a level
in kjk's level editor (`kjk-level-editor`). With the editor's agent link on,
the agent can see what the person has selected and change the level while
the person watches. The editor writes this guide into the link's folder each
time the link is turned on, with the full list of the editor's commands
after it. `docs/LEVEL-EDIT.md` describes the editor itself.

## Turning the link on

The link is off until the person ticks **Agent link** in the editor's
toolbar, or starts the editor with `KJK_EDITOR_LINK=1`. While it is on, the
editor uses a folder named `editor-link` in kjk's own folder:

- `%APPDATA%\kjk\editor-link` on Windows,
- `~/Library/Application Support/kjk/editor-link` on macOS,
- `~/.config/kjk/editor-link` on Linux.

`KJK_HOME` moves kjk's folder, and the link's folder with it. If `live.json`
is not in the folder, or has not changed for a while as the person works,
the link is off. Ask the person to tick Agent link.

Nothing listens on the network. Only a program that can write to the
person's own folder can send the editor commands.

## Reading what the person has selected

The editor writes `live.json` four times a second, whenever anything in it
changed. It holds these fields:

- `project`, `folder`, `game`, `module` and `area` name the project and
  the level open. `new_map` is true when the project makes a new map of
  its own rather than changing an installed area.
- `unsaved` is true when the project has changes that are not saved.
- `selected` names the selection the way the editor names its targets,
  such as `Object(Placeable, 3)`, `Prop(0)`, `Box(2)`, `Room(1)` or
  `Light(0)`. It is null when nothing is selected.
- `select_command` is the command that selects the same thing again,
  such as `edit select MetalBox`. It is null when no command can name it,
  for example when its tag has a space in it or another object has the
  same tag first.
- `details` is everything the editor's details panel knows of the
  selection: its kind, template, tag, position, facing, scripts, and for a
  placeable whether it can be used and the items it holds.
- `camera` is where the view stands in the game's space (`at`, in
  metres), the way it looks (`yaw` and `pitch`, in degrees), and the
  `camera` command that puts it back there.
- `messages` are the editor's last messages, the newest last.
- `answers` are the agent's last 20 commands, each with its answer.
- `script_errors` names each of the project's scripts that did not compile
  at the last save or compile, with the reason and the line.
- `last_try` is the last script tried or debugged in the editor, when
  there is one: the script's name, whether it ran without a fault, the
  first 200 lines of what the run printed, and how many debug stops it
  recorded.
- `trying` names the script being tried while a try runs, and is null
  otherwise. A try takes a few seconds, most of them to load the level.

The game's space has Z up. Positions are in metres. A facing is in degrees
from +X, turning left.

## Changing the level

Write commands to `inbox.txt` in the link's folder, one a line. The editor
takes the file within a quarter of a second, deletes it, and runs the lines
in order. Lines written while it runs wait for its next turn. Each command
gets an answer in `answers` in `live.json`. The answer is `ok`, or the
reason the command failed. The person sees each answer among the editor's
messages too.

A line is one of these.

- One of the editor's own commands, listed below, with or without its
  `edit` in front. For example `select MetalBox`, `nudge 0 0.5 0`,
  `turn 90` or `script OnUsed kje_open_door`.
- `camera X Y Z YAW PITCH` moves the view to that place, looking that way.
- `frame` moves the view to look at the selection.

The commands act on the selection, so a change usually starts with a
`select` line. A word is ended by a space, so a command cannot take a tag
or a value that has a space in it. Select such an object by clicking it,
or ask the person to.

Write the whole batch into the file at once, and read `answers` after it
was taken. If a line failed, the lines after it still ran.

## What to be careful with

- The editor saves only on `save`, or when the person saves. Leave saving
  to the person unless they asked for it.
- Every change can be undone in the editor, as the person's own can. The
  `undo` command takes back the last change, whoever made it.
- While the editor is open, change the level through the inbox, not by
  editing the project's files. The editor writes its own state over the
  files on its next save.
- `ui quit` closes the editor, and `ui link off` ends the link. Do not
  send either unless the person asks.
- `play`, `walk`, `audit` and `bake` save first, then start another
  program. `script try` and `script debug` save first too.
- A script of the project is written with `script new NAME`, which opens it
  in the editor's script window, but its text is typed there by the
  person. To give a script text of your own, write `NAME.nss` into the
  project's folder (`folder` in `live.json`) while the person has not
  opened that script, then send `script compile`.

## Examples

Move the selected object half a metre along +Y, turn it to face +Y, and look
at it:

    nudge 0 0.5 0
    turn 90
    frame

Give the metal box a script that runs when it is used, then try that script
for the box in the level without a window, and read the run in `last_try`:

    select MetalBox
    script OnUsed kje_box_used
    script try kje_box_used MetalBox

Let the box give the player 50 credits the first time it is used, without
writing a script:

    select MetalBox
    blocks OnUsed add once
    blocks OnUsed add give_credits 50
    save

Lead a door of this level to another module, arriving at a waypoint there:

    select kje_door
    link end_m01aa end_wp_start
