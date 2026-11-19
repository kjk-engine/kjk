# The level editor (`kjk edit`, `level_edit`)

The level editor opens a KOTOR area in kjk and lets the user change it. The
user can move, turn, copy and delete the area's creatures, placeables and
waypoints, add creatures and placeables from the install's templates, place
Jedi Academy props, and place boxes of invisible collision. The changes are
saved into a project of the user's own. `kjk play --project NAME` then plays
the changed area. Nothing is ever written into a game install.

This is a prototype. It changes areas that exist. It cannot build new rooms,
because kjk cannot write models, room layouts or walkmeshes.

## Starting it

```bash
kjk edit k1 ebo_m12aa --project hawk          # the Ebon Hawk, project "hawk"
kjk --project hawk play --stage ebo_m12aa --class jedi
kjk --project hawk collision audit k1 ebo_m12aa
```

`kjk edit GAME MODULE` opens the module's entry area. `--project NAME` names
the project. Without it, the project is named after the module. When the
project does not exist yet, the editor makes it first: it copies the area's
`.git` file as the install has it, and the project starts with no props and
no boxes. A project edits one area of one module. Opening a project with
another game or module is refused.

`--project NAME` works with every verb that loads a module: `kjk play`,
`kjk host`, the collision verbs and the scripted runs. It must come before
the verb's own options or after them, as any of kjk's global options.

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

## The editor's screen

The editor draws the area as `kjk module` does, with the free camera. Every
object the area places gets a marker: a small diamond in its kind's colour.
Creatures are red, doors purple, placeables orange, triggers light blue,
waypoints green, sounds pink, encounters white and stores yellow. Props get
a pale blue marker. Collision boxes are drawn as see-through cyan boxes.
The 24 objects nearest the camera are labelled with their kind and tag.

The panel in the top left corner shows the project, whether it has changes
that are not saved, the selected object (its kind, template, tag, position
and facing), the keys and the last messages.

### Keys

| key | what it does |
|---|---|
| right mouse button held, mouse | look around |
| W, A, S, D | fly forward, left, back and right |
| Q, E | fly down and up |
| Shift | fly faster, and move the selection 1 m at a time |
| left mouse button | select the object, prop or box under the pointer (its model or its marker) |
| arrow keys | move the selection 0.25 m forward, back, left or right, as the camera faces |
| Page Up, Page Down | move the selection up or down 0.25 m |
| G | grab the selection: it follows the point under the pointer until the next click |
| `[` and `]` | turn the selection 15 degrees left or right |
| Ctrl+D | copy the selection, in the same place |
| Delete | delete the selection |
| Ctrl+Z, Ctrl+Y | undo, redo |
| Ctrl+S | save the project |
| Tab | show the next list: placeable templates, creature templates, Jedi Academy props, then none |
| up and down arrows, Page Up, Page Down | in a list, pick an entry |
| Enter | in a list, place the picked entry where the screen's centre meets the level |
| B | place a collision box where the screen's centre meets the level |
| 1 and 2, 3 and 4, 5 and 6 | make the selected box 0.25 m shorter or longer along its X, Y and Z |
| `-` and `=` | make the selected prop 10% smaller or larger |
| F2 | show or hide the collision: walkable walkmesh in green, other solid surfaces in orange, the walls along the walkmesh's open edges in blue |
| F5 | save, then start `kjk play` on the project in a window of its own, as a level 6 Jedi |
| F6 | save, then run the containment audit on the project and list its result in the panel |
| Escape | let go of a grabbed selection, close the list |

Only creatures, placeables and waypoints can be moved, turned, copied and
deleted. Doors stay where their walkmesh is. Triggers, sounds, encounters
and stores are shown and can be selected, but not changed. New objects can
be creatures and placeables, made from the install's `.utc` and `.utp`
templates.

### Commands

Every key has a command, and `--cmds` runs them as the window does, so the
editor can be checked without a window (`docs/RUN.md`). Each `edit` command
is handed over at once. A redraw of the scene follows a few frames after
the last edit, so a script waits about 20 frames (`wait 20f`) before a
screenshot of the result.

| command | what it does |
|---|---|
| `edit select TAG` | selects the first object with that tag (its own, else its template's) |
| `edit select prop N`, `edit select box N` | selects the Nth prop or box, from 0 |
| `edit select none` | selects nothing |
| `edit pick X Y` | selects what a click at that point of the view (pixels) would |
| `edit move X Y Z`, `edit nudge DX DY DZ` | moves the selection to a point, or by an amount, in metres |
| `edit turn DEGREES` | turns the selection to face that many degrees from +X, turning left |
| `edit scale S` | draws the selected prop at that scale, from 0.05 to 20 |
| `edit size X Y Z` | gives the selected box that size, in metres |
| `edit duplicate`, `edit delete` | copies or deletes the selection |
| `edit undo`, `edit redo` | takes back the last edit, or makes it again |
| `edit add placeable TEMPLATE [X Y Z]`, `edit add creature TEMPLATE [X Y Z]` | adds an object made from the template, at the point or where the screen's centre meets the level |
| `edit prop MODEL [X Y Z [SCALE]]` | places a Jedi Academy model (`models/map_objects/...md3`), solid |
| `edit box [CX CY CZ SX SY SZ [YAW]]` | places a collision box: its centre, its size and its yaw |
| `edit collision on`, `edit collision off` | shows or hides the collision |
| `edit save` | saves the project |
| `edit play` | saves, then starts `kjk play` on the project |
| `edit audit` | saves, then runs `kjk collision audit` on the project |

A dump (`dump NAME`) has an `editor` fact: the project, the counts of each
kind of object, every object with its template, tag, position and facing,
the selection, the props and boxes, the depths of undo and redo, the last
messages and the audit's result. In play, a dump has a `project` fact
instead: the project's name, and where each of the area's objects and each
player stands.

## The containment audit

F6 (or `edit audit`) saves the project and runs `kjk collision audit` on
it, the audit `docs/COLLISION.md` describes. The audit floods the area from
where the game puts players, walking and jumping at each Force jump level.
It lists each region a body reaches off the walkmesh, as a perch inside
the level or as an escape out of it. The editor shows the summary lines in
the panel, and the whole output is in the dump. A project's boxes and props
are in the level the audit floods, so a box that closes a gap removes the
escape, and a box that makes a new ledge shows as a new perch.

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
- **Collision boxes turn about Z only.** A box has a yaw but no pitch or
  roll. A ramp is therefore built from boxes laid as steps.
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
- **No export.** The project's `.git` file is a KOTOR file, but there is no
  step yet that packs a project into a `.mod` file or an `Override` folder
  for the original game.
- **The editor's panel uses Bevy's own UI and default font**, as the
  first-run setup does, not KOTOR's screens.

## Future work

These come after the prototype: navigation that includes the added boxes
and props, editing triggers' outlines and doors' destinations, export to a
`.mod` file, new areas made from existing rooms (which needs layout,
visibility and walkmesh writers), markers for vehicle routes and spawn
points for vehicles and jetpacks, and editing an area together in co-op.

## Checks

- The approved test `level_edit_round_trip` (`docs/TESTS.md`) edits a GIT
  made by the test, saves the project to a temporary folder and reads it
  back. It needs no install.
- The demo (`LEVEL-EDIT-DEMO.md`) shows every behaviour on the Ebon Hawk,
  by hand in a window and as headless runs whose dumps say what changed.
