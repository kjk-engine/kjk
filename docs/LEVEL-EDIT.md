# The level editor (`kjk-level-editor`, `level_edit`)

The level editor opens a KOTOR area and lets the user change it in a window
laid out like a scene editor's. The user can move, turn, copy and delete
the area's creatures, placeables and waypoints, add creatures and
placeables from the install's templates, place Jedi Academy props, and
place boxes of invisible collision. The changes are saved into a project
of the user's own. `kjk play --project NAME` then plays the changed area.
Nothing is ever written into a game install.

This is a prototype. It changes areas that exist. It cannot build new rooms,
because kjk cannot write models, room layouts or walkmeshes.

## The two halves

The editor is a program of its own, `kjk-level-editor`, built from
`crates/level_editor`. It stands on kjk's game crate for the scene, and on
`crates/level_edit` for the project's data. kjk itself carries only what
plays a project: the `--project` option, the project's folder in the
install's files, its boxes and props in the area's collision, and its
props in the area's scene. An editor that lives elsewhere could therefore
be moved out of kjk's repository without changing how kjk plays projects.

## Starting it

```bash
cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
kjk-level-editor                                 # lists the projects and the levels to pick from
kjk-level-editor k1 ebo_m12aa --project hawk     # the Ebon Hawk, project "hawk"
kjk-level-editor k1 my_map --new-map ebo_m12aa   # a new map, lit as the Ebon Hawk
kjk --project hawk play --stage ebo_m12aa --class jedi
kjk --project my_map play --stage my_map --class jedi
kjk --project hawk collision audit k1 ebo_m12aa
```

Started with no game and module, `kjk-level-editor` lists what can be
opened (`picker`). On the left are the projects made so far, the most
recently changed first, each with its game and level. On the right are
every module of KOTOR 1 and of KOTOR 2 that is installed, with the in-game
name of its entry area (its `.are` file's `Name`, else the module's own).
A search box filters the modules by name or by module. A click on a
project opens it. A click on a module picks it, and the project's name below
starts as the module's; Open (or a double-click on the module) opens it.
The names are read on a thread of their own and fill in within a few
seconds. Under them, "New map named" makes a new map (below) whose light,
fog and sound start as the picked level's. A project is opened once in a process, before any install is read,
so the picker starts the editor again as a program of its own on what was
picked, and closes.

`kjk-level-editor GAME MODULE` opens the module's entry area at once. `--project
NAME` names the project. Without it, the project is named after the
module. When the project does not exist yet, the editor makes it first: it
copies the area's `.git` file as the install has it, and the project
starts with no props and no boxes. A project edits one area of one module.
Opening a project with another game or module is refused. The editor also
takes `--cmds`, `--headless` and `--size`, as `kjk` does (`docs/RUN.md`).

The Play and Audit buttons start the `kjk` program that lies beside
`kjk-level-editor`, so both are built into the same folder.

`--project NAME` works with every `kjk` verb that loads a module: `kjk
play`, `kjk host`, the collision verbs and the scripted runs. It is one of
kjk's global options, so it may stand before or after the verb.

## New maps

A project can also make a map of its own: a new module, built from the
games' rooms. `kjk-level-editor GAME NAME --new-map BASE` makes one, or
the picker's "New map named" row does. `--title` gives the name the game
shows; the module's name is used when it is left out. The name has
lowercase letters, digits and underscores, 12 at most, and no module of
the install may have it.

A new map starts with no rooms, no objects and the entry at the origin.
Its area's light, fog and sound are the base level's (its `.are` file),
and so are its music and ambient sound (its GIT's area properties).

- **Rooms.** The asset picker's Rooms tab lists every room of the install:
  each room of every area's layout, its thumbnail seen from above. A room
  is placed by dragging or double-clicking it, with its floor's middle where
  the view meets the level, else a few metres ahead. Rooms are moved,
  copied and deleted like objects, with undo. They do not turn, since the
  layout places rooms unturned.
- **Seams.** Two rooms are walked across where an edge of one's floor lies
  on an edge of the other's (`docs/NAV.md`, room seams). "Meet a seam"
  moves the selected room by the shortest way, 1.5 m at most, that puts an
  edge of its floor on an edge of the same length of another room's. With
  snapping on, a room placed or let go after a drag meets the nearest seam
  by itself.
- **The entry.** The outliner's Entry is where the player starts: a yellow
  marker that moves and turns like an object.

A room's walkmesh is in the coordinates of the area it was made for, so a
room placed anywhere else is copied: its model under the name
`<map>_<NN>` (`kjedemo_01`), and its walkmesh moved by how far the room
moved (`level_edit::map`). The editor makes the copies in memory as rooms
are placed, so the view and its collision show the map as it is.

Saving a new map writes, besides the files of every project, its
`kjk-map.toml` (its title, base, rooms and entry), its area's `.are`, and
its module, `modules/<name>.mod`: `module.ifo`, the area's layout and
visibility (every room sees every room), and the rooms' copies. While the
project is active, its modules are in the install's index (`vfs::project`),
so `kjk --project NAME play --stage NAME` plays the map like any module,
and the picker lists it.

## The navigation mesh

The game's characters find their way over the navigation mesh, which play
bakes from the area's walkable faces (`docs/NAV.md`). The player walks on
the collision whatever the mesh holds. The toolbar's Nav switch (F3) draws
the mesh the way play bakes it, with the project's changes: each island in
its own colour, a face under furniture in red, a face under a door or a
placeable in purple. The details panel then says how many faces, islands
and portals the mesh has, and how many faces are not reached from the
entry of a new map (else from the largest island). A character cannot get
to those.

A box's role (`level_edit::area::BoxRole`), picked in the details panel or
with `edit role`, says what it does:

| Role | Solid | The navigation mesh |
|---|---|---|
| Solid | yes | unchanged |
| Solid, walked on (`walkable`) | yes | its top joins the mesh, so characters walk over a platform, a step or a bridge |
| Nav blocker (`block`) | no | the mesh under it is cut away, so characters go round it |
| Nav floor (`floor`) | no | its top joins the mesh, as floor where the walkmeshes leave a gap |

The Collision shapes tab has a platform and a step that are walked on, a
nav blocker and a nav floor. A blocker cuts the faces along its outline,
seen from above (`collision_odyssey::footprints::cut`), and leaves out the
pieces within its height. `kjk-area.toml` writes a box's role as `role`;
a solid box has none, as before boxes had roles. Play, `kjk collision nav`
and the containment audit bake the mesh with the active project's changes
(`level_edit::nav`). An installed module without a project bakes as
before.

## Where projects live

A project is a folder in `projects` in kjk's own folder, beside the saves:
`%APPDATA%\kjk\projects\NAME` on Windows, `~/Library/Application
Support/kjk/projects/NAME` on macOS and `~/.config/kjk/projects/NAME` on
Linux. `KJK_HOME` moves kjk's folder, and `KJK_PROJECTS` names another
folder for the projects alone. A name is 1 to 64 letters, digits, `-` and
`_`.

The folder holds these files.

- `project.toml` names the game, the module, the area and the kjk that
  wrote it.
- `AREA.git` is the area's object list as the editor changed it. It is a
  KOTOR file, and the original games could read it.
- `kjk-area.toml` holds what only kjk understands: the Jedi Academy props
  and the collision boxes. The original games never read it.
- `kje1_metalbox001.utp` and the like are templates of the project's own
  (below, "An object's behaviour"). They are KOTOR files too.

A prop is stored as the path of its model inside Jedi Academy's archives.
The project never holds a copy of a Jedi Academy file, so anyone who plays
an area with props needs Jedi Academy installed.

## How a project reaches play

When a run starts with `--project`, kjk opens the project before it reads
any install (`level_edit::active`).

- The project's folder becomes the highest layer of the install's files
  (`vfs::project`). It sits over `override/` and over every Steam Workshop
  item. So the project's `.git` file replaces the module's own, as a mod's
  file in `override/` would. Only the folder's top level is read, and only
  files named as a KOTOR resource type.
- The collision boxes and the solid props join the area's collision
  (`collision::clip_file::Added`). They are added to the module's clip file
  (`docs/COLLISION.md`) as solid brushes of material 0. A box is one convex
  brush. A prop adds each triangle of its model as a slab, as a room's
  triangle is. Both are solid for every body and every shot.
- The props are drawn in the area's scene (`game::project_props`).
- The area's baked collision in the store is keyed by the project's
  digest, so a project's area is never taken for the module's own.

Without `--project` none of this happens, and kjk reads, builds and plays
every module as before. The sim's hash does not move, and no format
changes.

## An object's behaviour

The games make the objects of an area in two ways.

- **Most kinds are made from a template**: placeables (`.utp`), creatures
  (`.utc`), triggers (`.utt`), sounds (`.uts`), encounters (`.ute`) and
  stores (`.utm`). The area's object list gives such an object a place and
  names its template, and the games read nothing else from the list for it
  (`asset_odyssey::module`, `as_placed`). A trigger's and an encounter's
  outline, and an encounter's spawn points, are the list's too. A
  trigger's own tag, where the list gives one, is kept.
- **Doors and waypoints are read whole from the area's object list.** A
  waypoint's tag, name and map note, and a door's tag and scripts, are the
  list's own.

The template, or for a door or a waypoint the list's entry, says what the
object does:

- its tag, which scripts find it by: a companion arrives at the waypoint
  of a tag, a script opens the door of a tag;
- the scripts it runs on each event: a placeable's `OnOpen`, `OnUsed`,
  `OnHeartbeat` and the rest, a creature's `ScriptHeartbeat`,
  `ScriptOnNotice`, `ScriptDeath` and the rest, a KOTOR 1 trigger's
  `ScriptOnEnter` and `ScriptOnExit`;
- for a placeable, whether the player can use it (`Useable`) and whether
  it is a container (`HasInventory`);
- for a placeable, the items it starts with (`ItemList`).

This is why a container the game placed opens and one placed from a
generic template may not. The Ebon Hawk's metal boxes are made from the
module's own `metalbox001`, which can be used and holds items. The
install's `plc_footlker` is a footlocker for decoration: it cannot be used
and holds nothing.

The details panel's Behaviour section shows the selected object's template,
its tag, its scripts, and a placeable's two flags and items. Each can be
changed there.

- The tag is typed into its field.
- A script's name is typed into its event's field. The fields are the
  events the template or the entry names; the games' templates name every
  event of their kind, empty or not. An event they do not name is one the
  games never read, and is refused. While a field is
  typed in, the install's compiled scripts that match are listed under the
  section, to click. A name the install has no compiled script of is shown
  in orange; it is kept, for a script that comes later.
- "Can be used" and "Holds items" set the placeable's `Useable` and
  `HasInventory`.
- An item is added by its template's name (`.uti`), with matches listed as
  for scripts, and removed with its Remove button.

A door's and a waypoint's tag and scripts, and a trigger's tag, are
changed in the area's object list itself. Any other change to an object made
from a template copies the template into the project first, under a new
name: `kje`, a number, and the template's name, cut to 16 characters
(`kje1_metalbox001`). The object is pointed at the copy, and the copy is
changed. Later changes change the same copy. Duplicates of an object share
its copy. A change to one object whose copy other objects share gives it a
copy of its own first. A copy no object uses any more (an edit undone)
leaves the project, and leaves the folder at the next save. Every change is
one edit, undone and redone as the others are.

Play makes the object from the copy, which the project's folder holds over
the install's files. Nothing else in kjk changes for this.

A companion's place is an example. In the Ebon Hawk, each companion stands
at a waypoint whose tag names them (`pebn_carth`, `pebn_bastila`). A third
place is made by copying a waypoint (Ctrl+D), moving the copy, and typing
its new tag, or by placing a waypoint from the asset picker's Waypoints tab
and typing its tag.

### What a script does

Each script in the Behaviour section has a View button. It opens the script
viewer at the bottom of the details panel, which has three tabs.

- **Source** shows the script's source (`.nss`), coloured, where the
  install has it. KOTOR 1's install holds the source of most of its
  scripts in `chitin.key`.
- **Summary** reads the compiled script (`.ncs`). It lists the engine
  routines the script calls, each with its prototype from the game's
  `nwscript.nss` and how many times it is called, and the strings the
  script uses: tags, templates and the names of globals. A module's own
  scripts are often shipped compiled only, and the summary tells what such
  a script does at a glance.
- **Listing** is the compiled script's whole listing, from kjk's
  disassembler (`script_ncs::disasm`).

The viewer says which layer each file came from: the project, a module or
the install. A script with no source opens on its summary. The viewer
reads a script once and keeps it while the editor runs.

## The editor's window

- **The toolbar**, along the top, picks the tool (Select, Move, Rotate,
  Scale), turns snapping on and sets its steps, and has Undo, Redo, Save,
  Play and Audit, and the Collision switch. It also names the project and
  says when it has changes that are not saved.
- **The outliner**, on the left, lists every object the area places,
  grouped by kind, then the props and the collision boxes. A search box
  filters it by tag and template. A click selects an object, and a
  double-click also moves the camera to it.
- **The details panel**, on the right, shows the selected object's kind,
  template, tag or name, and its position and facing as numbers that can be
  typed or dragged. A prop also has its scale, and a box its size. An
  object of the area has its Behaviour section (above). Below are
  Duplicate, Delete and Frame, the editor's last messages, and the
  containment audit's result. The panel scrolls. The script viewer, when it
  is open, keeps the panel's lower part, and its top edge can be dragged.
- **The asset picker**, along the bottom, has eight tabs: the install's
  placeable templates, its creature templates, Jedi Academy's map objects
  (`models/map_objects/`), collision shapes (a ledge, a platform, a wall, a
  pillar, a step, a nav blocker and a nav floor), and the install's
  waypoint, trigger, sound and store
  templates. A search box filters the tab. Each asset is a tile with a
  thumbnail; an object with no model of its own (a waypoint, a trigger, a
  sound, a store) has its marker instead. An asset dragged into the view is placed where the
  pointer meets the level. A double-click places it where the centre of the
  view meets the level.
- **The view**, in the middle, draws the area as `kjk module` does. Every
  object has a marker: a small diamond in its kind's colour. Creatures are
  red, doors purple, placeables orange, triggers light blue, waypoints
  green, sounds pink, encounters white and stores yellow. Props get a pale
  blue marker. A box is drawn see-through in its role's colour: a solid box
  cyan, a walked-on box green-blue, a nav blocker red and a nav floor
  green. A trigger's or an encounter's outline is drawn as a see-through
  floor of its shape, light blue or white. The 24
  objects nearest the camera are labelled with their tag, the selected one
  in yellow. The selection has a gizmo for the tool: arrows and planes to
  move it, a ring to turn it about Z, and handles to scale a prop or to
  size a box.

The editor runs the scene under Enhanced lighting. egui, which draws the
panels, draws on the scene camera, and only that camera's HDR texture is
one egui's pipeline takes (`ui::attach_egui`).

### Thumbnails

Each tile's thumbnail is drawn on the CPU the first time it is shown: the
model's triangles with their textures, from in front, to one side and a
little above, with one light (`thumbs`). A frame spends at most 12 ms on
them, so a tab fills in over a few frames. Each thumbnail is kept in kjk's
store (`editor/thumbnails`), keyed by the template's or the model's bytes,
so later runs show it at once and a changed file is drawn again.

### Keys and the mouse

| key | what it does |
|---|---|
| right mouse button held, mouse | look around |
| right mouse button held, W, A, S, D, Q, E | fly forward, left, back, right, down and up |
| Shift | fly faster |
| left mouse button | select the object, prop or box under the pointer (its model or its marker); drag a gizmo's handle |
| Q, W, E, R | the Select, Move, Rotate and Scale tools |
| F | move the camera to the selection |
| Ctrl+D | copy the selection, in the same place |
| Delete | delete the selection |
| Ctrl+Z, Ctrl+Y | undo, redo |
| Ctrl+S | save the project |
| Enter | place the asset picked in the picker where the view's centre meets the level |
| F2 | show or hide the collision: walkable walkmesh in green, other solid surfaces in orange, the walls along the walkmesh's open edges in blue |
| F3 | show or hide the navigation mesh |
| F5 | save, then start `kjk play` on the project in a window of its own, as a level 6 Jedi |
| F6 | save, then run the containment audit on the project and list its result |
| Escape | select nothing |

While a text field has the keyboard, the keys go to the field. While the
right mouse button is held, they fly the camera.

One drag of a gizmo, or of a number in the details panel, is one edit: one
undo takes all of it back. Every kind but doors can be moved, copied and
deleted, and every kind but doors, triggers and encounters turned. Doors
stay where their walkmesh is. A trigger's or an encounter's outline moves
with it but does not turn, and an encounter's spawn points move with it.
New objects can be creatures, placeables, waypoints, triggers, sounds and
stores, made from the install's templates. A new waypoint takes its
template's own fields (its tag, name and map note) into its entry. A new
trigger gets a square outline 2 m on a side round where it is placed.

### Commands

The editor's actions are also commands, and `--cmds` runs them as the
window does, so the editor can be checked without a window
(`docs/RUN.md`). Each `edit` command is handed over at once. A redraw of
the scene follows a few frames after the last edit, so a script waits
about 20 frames (`wait 20f`) before a screenshot of the result.

| command | what it does |
|---|---|
| `edit select TAG` | selects the first object with that tag (its own, else its template's) |
| `edit select prop N`, `edit select box N` | selects the Nth prop or box, from 0 |
| `edit select none` | selects nothing |
| `edit pick X Y` | selects what a click at that point of the window (pixels) would |
| `edit move X Y Z`, `edit nudge DX DY DZ` | moves the selection to a point, or by an amount, in metres |
| `edit turn DEGREES` | turns the selection to face that many degrees from +X, turning left |
| `edit scale S` | draws the selected prop at that scale, from 0.05 to 20 |
| `edit size X Y Z` | gives the selected box that size, in metres |
| `edit duplicate`, `edit delete` | copies or deletes the selection |
| `edit undo`, `edit redo` | takes back the last edit, or makes it again |
| `edit add KIND TEMPLATE [X Y Z]` | adds a `placeable`, `creature`, `waypoint`, `trigger`, `sound` or `store` made from the template, at the point or where the view's centre meets the level |
| `edit tag TAG` | sets the selected object's tag |
| `edit prop MODEL [X Y Z [SCALE]]` | places a Jedi Academy model (`models/map_objects/...md3`), solid |
| `edit box [CX CY CZ SX SY SZ [YAW]]` | places a collision box: its centre, its size and its yaw |
| `edit script LABEL SCRIPT` | sets the selected object's event script (`OnUsed`, `ScriptHeartbeat`); `-` for none |
| `edit flag useable on`, `edit flag inventory off` | sets the selected placeable's `Useable` or `HasInventory` |
| `edit item add ITEM`, `edit item remove N` | adds an item template to the selected placeable's items, or removes its Nth, from 0 |
| `edit collision on`, `edit collision off` | shows or hides the collision |
| `edit nav on`, `edit nav off` | shows or hides the navigation mesh |
| `edit role solid`, `walkable`, `block`, `floor` | gives the selected box a role |
| `edit save` | saves the project |
| `edit play` | saves, then starts `kjk play` on the project |
| `edit audit` | saves, then runs `kjk collision audit` on the project |
| `edit ui tool select`, `move`, `rotate`, `scale` | picks the tool |
| `edit ui tab placeables`, `creatures`, `props`, `shapes`, `waypoints`, `triggers`, `sounds`, `stores` | shows a tab of the asset picker |
| `edit ui search TEXT`, `edit ui find TEXT` | sets the asset picker's or the outliner's search |
| `edit ui snap on`, `edit ui snap off` | turns snapping on or off |
| `edit ui view SCRIPT`, `edit ui view` | opens the script viewer on the script, or closes it |
| `edit ui view tab source`, `summary`, `listing` | picks the viewer's tab |
| `edit add room MODEL [X Y Z]` | adds the install's room to a new map, its floor's middle at the point or where the view's centre meets the level |
| `edit select room N`, `edit select entry` | selects a new map's room (from 0) or its entry |
| `edit seam` | moves the selected room to meet the nearest seam |
| `edit entry [X Y Z]` | moves a new map's entry to the point or where the view's centre meets the level |
| `edit ui tab rooms` | shows the Rooms tab of a new map |

The picker takes `edit open GAME MODULE [PROJECT]`, `edit new GAME BASE
NAME` (a new map), `edit search TEXT` and `edit tab k1` or `tsl`.

A dump (`dump NAME`) has an `editor` fact: the project, the counts of each
kind of object, every object with its template, tag, position and facing,
the selection (a placeable's or a creature's with its behaviour), the props
(with their bounds) and boxes, the depths of undo and redo, the last
messages and the audit's result. In play, a dump has a `project` fact
instead: the project's name, where each player stands, and each of the
area's objects with where it stands, whether it can be used, whether it is
a container, how many items it holds and its scripts.

## The containment audit

Audit (or F6, or `edit audit`) saves the project and runs `kjk collision
audit` on it, the audit `docs/COLLISION.md` describes. The audit floods the
area from where the game puts players, walking and jumping at each Force
jump level. It lists each region a body reaches off the walkmesh, as a
perch inside the level or as an escape out of it. The details panel shows
the summary line of each reach, and the whole output is in the dump. A
project's boxes and props are in the level the audit floods, so a box that
closes a gap removes the escape, and a box that makes a new ledge shows as
a new perch.

## Limits of the prototype

- **A new map's rooms are the games' own.** There is no new geometry
  yet, rooms do not turn, and a room is not resized. Rooms from different
  areas carry their own baked light, so a mixed map shows seams in its
  lighting.
- **A new map is played in kjk only.** Its module has no AI paths (PTH),
  no map picture, and walkmeshes whose edges name no neighbouring room,
  which the original game needs. Every room sees every room.
- **KOTOR 2's rooms are not listed yet.** Its layouts are in its modules'
  archives, which the Rooms tab does not read.
- **Props are not in the navigation mesh.** A solid prop is solid for
  every body, but the game's characters do not path onto it or around it.
  A walkable box or a nav blocker placed with it does that. Links for
  jumps and drops between floors are not made yet.
- **Props are lit wrongly.** A KOTOR room is lit by its lightmaps, which a
  Jedi Academy prop does not have. In a dark room the area's ambient light
  alone left a prop nearly black, so a prop gives itself some light of its
  own (`project_props::PROP_GLOW`). It does not match the room around it.
- **Everything turns about Z only.** A box has a yaw but no pitch or roll,
  so a ramp is built from boxes laid as steps.
- **Co-op does not check projects.** The co-op fingerprint does not cover
  a project's folder. A host and a guest must play the same project by
  hand, or their worlds differ.
- **Saves made in an edited area** hold the edited area's objects. Loading
  such a save without the project, or with a changed project, may move or
  lose objects. kjk does not warn about this yet.
- **One area per project.** A module with more than one area is edited at
  its entry area.
- **Doors do not move**, and new doors and encounters are not placed. A
  trigger's outline is not reshaped, and a door's or a trigger's
  destination is not changed. These are future work.
- **One object at a time.** There is no multiple selection, box selection,
  or copy and paste between areas.
- **No export.** A new map's `.mod` and a project's `.git` file are KOTOR
  files, but there is no step yet that makes them ready for the original
  game.
- **The view is drawn under the panels.** The scene's camera fills the
  window, and the panels cover its edges.

## Future work

These come after the prototype: navigation that includes the added boxes
and props, editing triggers' outlines and doors' destinations, multiple
selection, export of a new map for the original game (its walkmeshes'
neighbours, AI paths and a map picture), turning rooms by 90° steps,
rooms made in Blender, markers for vehicle
routes and spawn points for vehicles and jetpacks, and editing an area
together in co-op.

## Checks

- The approved test `level_edit_round_trip` (`docs/TESTS.md`) edits a GIT
  made by the test, saves the project to a temporary folder and reads it
  back. It needs no install.
- The demo (`LEVEL-EDIT-DEMO.md`) shows every behaviour on the Ebon Hawk,
  by hand in a window and as headless runs whose dumps say what changed.
