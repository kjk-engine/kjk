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
- `selection` names everything selected, the same way, with `selected`
  first. It is empty when nothing is selected. Moving, turning, copying
  and deleting act on all of it.
- `select_command` is the command that selects the same thing again,
  such as `edit select MetalBox`. An object whose tag has a space in it,
  or whose tag another object has first, is named by its kind and its
  place in that kind's list, such as `edit select placeable 3`.
- `details` is everything the editor's details panel knows of the
  selection: its kind, template, tag, position, facing, scripts, and for a
  placeable whether it can be used and the items it holds.
- `camera` is where the view stands in the game's space (`at`, in
  metres), the way it looks (`yaw` and `pitch`, in degrees), and the
  `camera` command that puts it back there.
- `messages` are the editor's last messages, the newest last.
- `answers` are the agent's last 20 commands, each with its answer and
  its number `n`. The link numbers every command it answers, from 1,
  since it was turned on. `answered` is the number of the last one. Note
  `answered` before sending a batch of N lines. The batch's answers are
  the ones numbered from that value plus 1 to that value plus N.
- `script_errors` names each of the project's scripts that did not compile
  at the last save or compile, with the reason and the line.
- `last_try` is the last script tried or debugged in the editor, when
  there is one: the script's name, whether it ran without a fault, the
  first 200 lines of what the run printed, and how many debug stops it
  recorded.
- `trying` names the script being tried while a try runs, and is null
  otherwise. A try takes a few seconds, most of them to load the level.
- `recovery_found` is true while the editor offers work that an editor
  that did not close left. `recover` takes it and `recover discard`
  removes it. Leave that choice to the person.
- `markers_hidden` names the kinds of object whose markers the person has
  hidden. They are still in the level and in `objects.json`.
- `auditing` is true while a containment audit runs. `audit` holds the
  lines of the last audit's result.
- `baking` holds a running bake's label and what it is doing now, and is
  null otherwise. `bake_shown` is the folder of the bake the view shows,
  and `light_view` is how the view lights the rooms (`Off`, `Live`,
  `LampsOnly` or `Baked`).

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
- `list` writes `objects.json` into the link's folder. The file lists
  every object, prop, collision box, lamp and light placed by hand of the
  level, and a new map's rooms and entry. Each has its target, its name,
  its position, its facing and the command that selects it. The answer
  says how many things the file lists. Send `list` again after the level
  changed, because the editor does not keep the file up to date.
- `shot` writes `view.png` into the link's folder: a picture of the
  editor's window as the person sees it, with its panels and markers. The
  editor deletes the old picture first and writes the new one after the
  next frame is drawn, so wait until the file is there again, then read
  it. Send `camera` or `frame` in an earlier batch, so the view has
  moved before the picture is taken.
- `ground X Y` answers how high the level's surfaces are at that place,
  in metres, from the top down. Under a roof the first number is the
  roof and the next is the floor under it. The answer is `ok: 8.76` for
  one floor, or `ok: 14.20 8.76` for a roof over a floor. Use it to find
  the Z of a place before `add`, `move` or `box`.
- `floor X Y` answers how high the walkable floor is at that place, from
  the top down, without the furniture, walls and roofs that `ground`
  also finds. It answers `ok: 8.76` where one floor is. Prefer it to
  `ground` for the Z of a thing that stands on the floor. `drop` puts
  what is selected on that floor without a Z at all.
- `frame` moves the view to look at the selection. Its answer is `nothing
  is selected` when there is none.

A `ui` command changes the window on the editor's next frame, so its answer
is `ok` as soon as it is handed over. If the window does not know the
words, it says so among `messages`.

- `catalog` writes `catalog.json`: every name a command can add or set.
  It lists the install's placeable, creature, door, waypoint, trigger,
  sound, store and encounter templates, its items, scripts and modules,
  the rooms a new map can place, Jedi Academy's models for props, and
  the lamps found so far.
- `level` writes `level.json`: what a dump says of the editor. It holds
  every object with its template, tag, position and facing, the props
  and boxes, the lights, lamps and sun, the navigation mesh's facts and
  links, the project's scripts and conversations, the scripts that did
  not compile, and the project's problems.
- `begin` and `end` make the edits between them one undo step, so one
  undo by the person takes the whole change back. A batch left open ends
  with its file.

The commands act on the selection, so a change usually starts with a
`select` line. A word is ended by a space. Put a value that has a space
in it between double quotes, as in `tag "Metal Box"`. An object is always
selected by the command `objects.json` gives for it.

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
- To give a script text of your own, write the text to a file anywhere,
  then send `script text NAME FILE`. The editor takes the file's text as
  the script's, makes the script when the project has none of that name,
  and opens it in its script window. `save` or `script compile` then
  writes and compiles it, and `script_errors` names it if it does not
  compile. Do not write into the project's folder yourself.

## Examples

Place two crates as one change the person can undo at once:

    begin
    add placeable plc_crate 53.1 61.6 8.76
    add placeable plc_crate 54.1 61.6 8.76
    end

Find a template's name before adding it, then read `catalog.json`:

    catalog

Put a footlocker on the floor at a place, then look at it and take a
picture to check it:

    ground 53.1 61.6
    add placeable footlker001 53.1 61.6 8.76
    frame

Then, in a batch of its own once those are answered:

    shot

Find what the level holds before selecting anything, then read
`objects.json`:

    list

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
