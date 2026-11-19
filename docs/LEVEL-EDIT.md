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
kjk-level-editor k1 ebo_m12aa --project hawk     # the Ebon Hawk, project "hawk"
kjk --project hawk play --stage ebo_m12aa --class jedi
kjk --project hawk collision audit k1 ebo_m12aa
```

`kjk-level-editor GAME MODULE` opens the module's entry area. `--project
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

## Where projects live

A project is a folder in `projects` in kjk's own folder, beside the saves:
`%APPDATA%\kjk\projects\NAME` on Windows, `~/Library/Application
Support/kjk/projects/NAME` on macOS and `~/.config/kjk/projects/NAME` on
Linux. `KJK_HOME` moves kjk's folder, and `KJK_PROJECTS` names another
folder for the projects alone. A name is 1 to 64 letters, digits, `-` and
`_`.

The folder holds three files.

- `project.toml` names the game, the module, the area and the kjk that
  wrote it.
- `AREA.git` is the area's object list as the editor changed it. It is a
  KOTOR file, and the original games could read it.
- `kjk-area.toml` holds what only kjk understands: the Jedi Academy props
  and the collision boxes. The original games never read it.

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
  typed or dragged. A prop also has its scale, and a box its size. Below are
  Duplicate, Delete and Frame, the editor's last messages, and the
  containment audit's result.
- **The asset picker**, along the bottom, has four tabs: the install's
  placeable templates, its creature templates, Jedi Academy's map objects
  (`models/map_objects/`), and collision shapes (a ledge, a platform, a
  wall, a pillar and a step). A search box filters the tab. Each asset is a
  tile with a thumbnail. An asset dragged into the view is placed where the
  pointer meets the level. A double-click places it where the centre of the
  view meets the level.
- **The view**, in the middle, draws the area as `kjk module` does. Every
  object has a marker: a small diamond in its kind's colour. Creatures are
  red, doors purple, placeables orange, triggers light blue, waypoints
  green, sounds pink, encounters white and stores yellow. Props get a pale
  blue marker. Collision boxes are drawn as see-through cyan boxes. The 24
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
| F5 | save, then start `kjk play` on the project in a window of its own, as a level 6 Jedi |
| F6 | save, then run the containment audit on the project and list its result |
| Escape | select nothing |

While a text field has the keyboard, the keys go to the field. While the
right mouse button is held, they fly the camera.

One drag of a gizmo, or of a number in the details panel, is one edit: one
undo takes all of it back. Only creatures, placeables and waypoints can be
moved, turned, copied and deleted. Doors stay where their walkmesh is.
Triggers, sounds, encounters and stores are shown and can be selected, but
not changed. New objects can be creatures and placeables, made from the
install's `.utc` and `.utp` templates.

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
| `edit add placeable TEMPLATE [X Y Z]`, `edit add creature TEMPLATE [X Y Z]` | adds an object made from the template, at the point or where the view's centre meets the level |
| `edit prop MODEL [X Y Z [SCALE]]` | places a Jedi Academy model (`models/map_objects/...md3`), solid |
| `edit box [CX CY CZ SX SY SZ [YAW]]` | places a collision box: its centre, its size and its yaw |
| `edit collision on`, `edit collision off` | shows or hides the collision |
| `edit save` | saves the project |
| `edit play` | saves, then starts `kjk play` on the project |
| `edit audit` | saves, then runs `kjk collision audit` on the project |
| `edit ui tool select`, `move`, `rotate`, `scale` | picks the tool |
| `edit ui tab placeables`, `creatures`, `props`, `shapes` | shows a tab of the asset picker |
| `edit ui search TEXT`, `edit ui find TEXT` | sets the asset picker's or the outliner's search |
| `edit ui snap on`, `edit ui snap off` | turns snapping on or off |

A dump (`dump NAME`) has an `editor` fact: the project, the counts of each
kind of object, every object with its template, tag, position and facing,
the selection, the props (with their bounds) and boxes, the depths of undo
and redo, the last messages and the audit's result. In play, a dump has a
`project` fact instead: the project's name, and where each of the area's
objects and each player stands.

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

- **No new rooms.** kjk reads models, layouts and walkmeshes but cannot
  write them, so a project changes an area that exists.
- **The game's characters do not use new geometry.** Navigation is baked
  from the walkmesh alone (`docs/NAV.md`). A box or a prop is solid for
  every body, but the game's characters do not path onto it or around it.
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
- **Triggers, doors, sounds, encounters and stores** are shown but not
  edited. Triggers' outlines and doors' destinations are future work.
- **One object at a time.** There is no multiple selection, box selection,
  or copy and paste between areas.
- **No export.** The project's `.git` file is a KOTOR file, but there is no
  step yet that packs a project into a `.mod` file or an `Override` folder
  for the original game.
- **The view is drawn under the panels.** The scene's camera fills the
  window, and the panels cover its edges.

## Future work

These come after the prototype: navigation that includes the added boxes
and props, editing triggers' outlines and doors' destinations, multiple
selection, export to a `.mod` file, new areas made from existing rooms
(which needs layout, visibility and walkmesh writers), markers for vehicle
routes and spawn points for vehicles and jetpacks, and editing an area
together in co-op.

## Checks

- The approved test `level_edit_round_trip` (`docs/TESTS.md`) edits a GIT
  made by the test, saves the project to a temporary folder and reads it
  back. It needs no install.
- The demo (`LEVEL-EDIT-DEMO.md`) shows every behaviour on the Ebon Hawk,
  by hand in a window and as headless runs whose dumps say what changed.
