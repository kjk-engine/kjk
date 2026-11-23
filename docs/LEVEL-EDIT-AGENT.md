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

Two editors open at once would share that folder and take each other's
commands. An editor started with `KJK_EDITOR_LINK=NAME` uses a folder of
its own, `editor-link-NAME`. `NAME` is any word of letters, digits, `-`
and `_` but `1`. An editor that starts again to open another level keeps
its link and its folder. This holds whether the link was turned on by
the toolbar or by `KJK_EDITOR_LINK`.

`KJK_HOME` moves kjk's folder, and the link's folder with it. If `live.json`
is not in the folder, or has not changed for a while as the person works,
the link is off. Ask the person to tick Agent link.

Nothing listens on the network. Only a program that can write to the
person's own folder can send the editor commands.

## Reading what the person has selected

The editor writes `live.json` four times a second, whenever anything in it
changed. It holds these fields:

- `project`, `folder`, `game`, `module` and `area` name the project and
  the level open. `new_map` is true when the level open is a new map of
  the project rather than an installed area. A project can hold several
  levels, new maps and installed areas together. To open another one,
  send `save` and then `ui file level MODULE`. To add a new map to the
  project and open it, send `save` and then `ui file new NAME`. The
  editor starts again on that level, so wait until `module` in
  `live.json` names it before sending the next command.
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
- `details` is what the editor knows of the selection: its kind,
  template, tag, position and facing. For an object, `behaviour` holds
  the scripts it runs, every event it has (`events`, set or not), the
  numbers the editor changes of it (`numbers`: a door's lock, a
  creature's faction and attributes), and for a placeable whether it can
  be used and the items it holds.
- `camera` is where the view stands in the game's space (`at`, in
  metres), the way it looks (`yaw` and `pitch`, in degrees), and the
  `camera` command that puts it back there.
- `messages` are the editor's last messages, the newest last.
- `answers` are the agent's last 200 commands, each with its answer and
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
- `lights_own` is true when the level's lights and lamps placed by hand
  are the project's own, and false while they are the ones kjk's light
  data holds. `kinds_own` names each kind of fitting the project
  changed, and `levels_own` is true when the level's room levels are the
  project's own. A save keeps all of these in the project. `publish`
  writes them into kjk's own light data for every player: do not send it
  unless the person asks.
- `markers_hidden` names the kinds of object whose markers the person has
  hidden. They are still in the level and in `objects.json`.
- `auditing` is true while a containment audit runs. `audit` holds the
  lines of the last audit's result.
- `baking` holds a running bake's label and what it is doing now, and is
  null otherwise. `bake_shown` is the folder of the bake the view shows,
  `bake_kept` is the folder of the bake the project keeps for play
  (`bake keep` keeps the bake shown, `bake drop` takes it out),
  `bake_stale` is true when the kept bake was made before the level's
  lights were changed (`bake renew` bakes again and keeps the new bake),
  `timings` says how long the editor's work takes in milliseconds: a
  frame (`frame_ms`, smoothed over about a second), the longest frame of
  the last 120 (`frame_worst_ms`), the last building of the scene
  (`redraw_ms`) and the last save (`save_ms`),
  and `light_view` is how the view lights the rooms (`Off`, `Live`,
  `LampsOnly` or `Baked`).

The game's space has Z up. Positions are in metres. A facing is in degrees
from +X, turning left.

## Changing the level

Write commands to `inbox.txt` in the link's folder, one a line. The editor
takes the file within a quarter of a second, deletes it, and runs the lines
in order. Lines written while it runs wait for its next turn. Each command
gets an answer in `answers` in `live.json`. The answer is `ok`, or the
reason the command failed. A command that makes something may say what it
made after `ok`: a conversation's `say` answers `ok: line 3`, the number
of the line it added. The person sees each answer among the editor's
messages too. A `ui` command is answered the same way: `ok` when the
window took it, and the reason when its words are none of the window's.

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
  It lists the game's placeable, creature, door, waypoint, trigger,
  sound, store and encounter templates, its items, scripts and modules,
  the rooms a new map can place, Jedi Academy's models for props, and
  the lamps found so far. Each template and each item is an object with
  `name`, which a command takes, and `shown`, the name the game shows
  for it. A creature also has `creature_level`, the sum of its class
  levels. A template with `from` is another installed mod's: `override`
  when the game reads it from its override folder, `mod` when it reads
  it from the level's mod file. A project that uses it needs that mod
  (`docs/LEVEL-EDIT.md`, "A project built on another mod"). A template
  with `level` is in that level's files only. Adding
  it copies it into the project (`docs/LEVEL-EDIT.md`, "Templates of the
  game's other levels"). The first `catalog` of a session looks through
  every level of the game and takes a few seconds.
- `floormap [METRES]` writes `floor.json`: the walkable floor as places a
  thing can stand on, `METRES` apart (2 when it is left out, from 0.25 to
  20). Each place is `[X, Y, Z]`. The file also has the floor's least and
  most corner, and for a new map each room with the corners of its
  walkmesh's box, and the level's doorways: where its layout hangs a
  door, and the way that door faces. A door added within 1.5 metres of a
  doorway stands in it. One `floormap` replaces the many `floor X Y` questions
  that finding a level's floor took. Send it a few seconds after adding
  rooms, once they are solid.
- `details NAME` writes `details.json`: the details of one thing, without
  selecting it. `NAME` is what `select` takes: a tag, or a kind and a
  number such as `door 0` or `room 2`.
- `conversation NAME show` writes `conversation.json`: the project's
  conversation as its lines and replies, each with its number, its text,
  its script, its journal entry and where it leads.
- `shot view` writes `view.png` as `shot` does, but of the view alone. The
  panels are left out for a few frames, so nothing covers the view's
  edges.
- `properties` writes `properties.json`: every kind of item property the
  game has. Each has its `row`, its `name`, the tables its subtype, its
  value and its parameter are rows of, and the `add_command` that adds
  it. `tables` lists each of those tables' rows by number and name.
- `level` writes `level.json`: what a dump says of the editor. It holds
  every object with its template, tag, position and facing, the props
  and boxes, the lights, lamps and sun, the navigation mesh's facts and
  links, the project's scripts and conversations, the scripts that did
  not compile, and the project's problems.
- `begin` and `end` make the edits between them one undo step, so one
  undo by the person takes the whole change back. A batch left open ends
  with its file. A batch stops at the first edit that is refused. The
  commands after it are answered `skipped` and do not run, because they
  were written for a level in which that edit was made: a `tag` after an
  `add` that failed would tag whatever was selected before. `end` then
  says where the batch stopped. Read the answers, fix the refused line,
  and send the rest again.

The commands act on the selection, so a change usually starts with a
`select` line. A word is ended by a space. Put a value that has a space
in it between double quotes, as in `tag "Metal Box"`. An object is always
selected by the command `objects.json` gives for it.

Write the whole batch into the file at once, and read `answers` after it
was taken. Outside `begin` and `end`, the lines after a line that failed
still ran. Between `begin` and `end` they did not, as said above.

A command that lists something gives the listing in its answer, after
`ok:`. `quest show`, `item properties`, `mod version` and `export` do.
The same text is among `messages`, which keeps only the last few lines.

## Building a level from an empty map

A new map starts with no rooms and no objects. This is the order that
works, with what the commands do not say by themselves.

**Rooms.** `catalog.json` lists every room by its model and the area it
is from. Rooms of one area fit together where that area joins them, so
take rooms of the same area and add each with `add room MODEL home`. Wait
a few seconds, then send `floormap`. Two rooms are joined when
`floor.json` lists a doorway between them and the floor's places run
from one into the other. A doorway at the map's edge that leads to no
room needs nothing: the floor ends there, and the player cannot walk off
it.

**The start.** `entry X Y Z` puts the place the player starts at. Use a
place from `floor.json`.

**Names.** A template's name, an item's name and a conversation's name
have 1 to 16 lowercase letters, digits and `_`. A tag has up to 32
characters. Start your own names with a short mark of your own, such as
`kje_`, so they are not names of the game's.

**A container.** A placeable is a container once it can be used and
holds items: `flag useable on` and `flag inventory on` on the selected
placeable. Some templates are that already and some are not, so send
`details` after adding one and read `useable` and `holds_items`. A
template that holds items brings them along: take out what you do not
want with `item remove N`.

**An item of the project.** `item new NAME BASE` makes an item as a copy
of the game's item `BASE`. Any item serves as a key or a quest item,
because scripts and blocks know it by its tag alone. `item add NAME` puts
it into the selected container or creature, as it puts any item of the
game.

**Text in a command.** A name or a text that ends its command runs to
the end of the line, spaces and all, with or without double quotes:
`item name`, `item text`, `quest new`, `quest entry` and `quest end` take
their text this way. Double quotes are needed only where several texts
stand in one command, as in a conversation's `replies`.

**Events and blocks.** An object runs a script on each of its events.
`details NAME` lists an object's events under `behaviour.events`, set or
not. A placeable has `OnUsed`, `OnInvDisturbed` (an item was put in or
taken out) and `OnDeath`. A door has `OnOpen`, `OnFailToOpen` (it was
tried while locked) and `OnClick`. A creature has `ScriptDeath`,
`ScriptSpawn` and `ScriptHeartbeat`. A trigger has `ScriptOnEnter`.
`blocks EVENT add KIND VALUE ...` gives an event a logic block, so most
behaviour needs no script. `catalog.json` lists every kind of block
under `blocks`, with the values it takes in order. A refused kind is
answered with the list of kinds.

**A door a key opens.** kjk does not read a door's key from the door.
Make the door locked, and open it by blocks for whoever carries the key:

    select kje_door
    number Locked 1
    number KeyRequired 1
    blocks OnFailToOpen add if_has_item kje_key
    blocks OnFailToOpen add unlock kje_door
    blocks OnFailToOpen add open_door kje_door

`if_has_item` takes the item's tag, which is the item's name unless
`item tag` changed it. The player uses the door once, and it opens.
`Lockable` says whether the door can be locked again by hand, and may
stay 0. A door added at a doorway's place from `floor.json` stands in it
already, so `snap door` is not needed after it. A doorway's facing is the
way the door's panel is hung, as the game's own doors are, not the way
one walks through it.

**A quest.** `quest new`, `quest entry` and `quest end` make the quest
and its entries. An entry is added in play by the block
`journal QUEST NUMBER` on some event, or by a conversation's line (`quest
line N QUEST NUMBER`). To start a quest when an item is taken from a
container, put on the container's `OnInvDisturbed`: `if_has_item ITEM`,
`once`, `journal QUEST NUMBER`. To close it when a creature dies, put
`journal QUEST NUMBER` on the creature's `ScriptDeath`. An entry's
number is a whole number. A quest moves on only to a higher
number, so number the entries in the order they happen, such as 10, 20
and 30. `level.json` lists the project's own quests with their entries
under `project_quests`, and its items under `project_items`.

Blocks on an event take the place of the script that event ran. An event
that ran one of the game's scripts loses it. `details` shows what each
event runs before you change it.

**A hostile creature.** A creature fights the player when its faction is
hostile: `number FactionID 1`. `details` shows the creature's numbers
under `behaviour.numbers`.

**Checking without a window.** `script try SCRIPT TAG` and
`conversation try NAME TAG PICKS` answer `ok` at once and run for a few
seconds. Wait until `trying` in `live.json` is null again, then read
`last_try`. An event's blocks are a script the editor names itself, such
as `kjb_339`: `details` gives the name under `behaviour.scripts`, and
`script try` takes it. A trial's player carries nothing, so a block that
begins with `if_has_item` shows only what happens without the item.

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
- `export` saves, then writes the project as a mod of the game to a
  folder. It replaces the export made there before. Send it when the
  person asks for the mod.
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
