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
starts with no props and no boxes. Opening a project with another game is
refused. Opening a project of the install with another module adds that
level to the project (below, "Several levels in one project"). The editor also
takes `--cmds`, `--headless` and `--size`, as `kjk` does (`docs/RUN.md`).

The Play and Audit buttons start the `kjk` program that lies beside
`kjk-level-editor`, so both are built into the same folder.

A release package (`cargo xtask dist`, `docs/RELEASE.md`) holds
`kjk-level-editor` beside `kjk`. Its `kjk` is a player's build, without the
verbs that check and measure, so the editor greys out Audit and Bake there
(it asks `kjk --help` once as it starts). Everything else works, Try
included.

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
  turned, copied and deleted like objects, with undo. A room turns about
  Z by any angle, with the Rotate tool or its Facing field. The layout
  cannot turn a room, so the room's copy is turned instead. Each part of
  its model is turned round the model's origin, and its walkmesh is turned
  round the room's old place, then moved. The model's vertices, its MDX
  and its lightmaps stay as they are, so a turned room is lit as it was.
  `kjk-map.toml` keeps a turned room's `yaw` in degrees. KOTOR 2's rooms are listed the same way:
  its layouts and room models are in the game's shared data
  (`chitin.key`), as KOTOR 1's are, and a TSL new map of Peragus's rooms
  plays (checked on 2026-11-21).
- **Seams.** Two rooms are walked across where an edge of one's floor lies
  on an edge of the other's (`docs/NAV.md`, room seams). "Meet a seam"
  moves the selected room by the shortest way, 1.5 m at most, that puts an
  edge of its floor on an edge of the same length of another room's. With
  snapping on, a room placed or let go after a drag meets the nearest seam
  by itself.
- **The entry.** The outliner's Entry is where the player starts: a yellow
  map marker that moves and turns like an object.

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

## Lights

The fitted lights (`game::fitted_lights`, `docs/LIGHTING-PLAN.md`) put a
light at every lamp the level art shows, from the rule data of
`data/lights/`. The editor shows them live and edits the lights placed by
hand.

- **The Lighting menu** picks how the view lights the rooms: **Game** (the
  stored lightmaps), **Fitted live** (half the stored light and the fitted
  lights, as `KJK_FITTED_LIGHTS=on` plays), **Lamps only** (none of the
  stored light, so each light's pool shows on its own) and **Baked** (a
  bake of `kjk detail-bake`, picked from the bakes `tools/light-kinds`
  made of the area). The scene is drawn again in the mode picked.
- **Show lights** (F4) draws every light as a sun in its colour: a large
  one for a light placed by hand, a small one for a light the game places
  at a fitting. The selected light shows its reach as three rings round
  it, one level and two upright, as wide as the light reaches. A light with
  a cone also shows a line the way it shines. The rings are drawn over the
  view, so selecting a light never draws the scene again. The outliner's Lights lists the lights placed by
  hand and the game's own by where they come from: the glowing texture of
  their fitting, or `hidden lamp` for a room's hidden lamp.
- **A fitted light** is the game's and is not changed. Its details say
  where it comes from, its colour, reach, cone and strip. "Copy as a
  placed light" makes an editable copy of it.
- **A light placed by hand** is placed from the asset picker's Lights tab
  (a lamp that shines all ways, a spot straight down, a panel that shines
  one way, a strip), moved and turned with the gizmo (turning turns its
  aim about Z), copied and deleted, with undo. Its details set its colour
  and strength, reach, cone, strip and the reason it is there.

- **A kind of fitting** is changed from one of its lights. A fitted
  light's details show its kind (its glowing texture, on rooms or on
  placeables): whether it is a lamp, its form, strength, reach, colour
  and cone, and a reason. A change lights every lamp of the kind at once,
  in every level of the game, with undo. A room's hidden lamp has no kind.
- **Room levels** (in the Lighting menu) is a window of the area's rooms
  with each room's level from its stored lightmaps, `Stored` (their mean)
  and `Lit` (the mean with each texel counted by its own level), about as
  `kjk detail-bake` measures them, and the level the bake aims the room at
  (`Target`). A target is set from the room's lit level, changed, or
  taken away, with undo. The window says how the module's bake is set in
  `bake.toml`.

An installed level's lights placed by hand are the `[[placed]]` tables of
the light data's `fittings.toml` whose `area` is its module. The light
data is the folder `KJK_LIGHT_DATA` names, else the `data/lights` of the
kjk checkout the editor was built in; the Lighting menu names it. Saving
replaces that area's tables and leaves every other line of the file as it
was, so the change is a diff the lighting's maintainer reviews. A new
map's lights placed by hand are `[[light]]` tables of its `kjk-area.toml`.

A changed kind is saved to the light data twice, so the two files agree:
its `[[kind]]` entry of `k1.toml` or `tsl.toml`, which the game reads, and
a `[[fitting]]` of `fittings.toml` with the same numbers and the reason,
which `tools/light-kinds/kinds.py` reads when it writes those entries
again. An existing `[[fitting]]` for the kind is changed in place, its
other fields kept. The rooms' targets are saved to the module's levels
file (`levels/<module>.txt`), each line kept but the changed rooms'. A
module whose bake aimed at a share of each room's level is set to aim at
the file (`bake.toml`'s `levels`); a room the file has no line for keeps
the share's level in the bake.

### Lamps

A light placed by hand is invisible. A **lamp** is a light fitting that the
player sees: a model of the game drawn where the area had none. The fitted
lights light a lamp's glowing meshes as they light the area's own
fittings, by the kind its glowing texture names. A new lamp therefore
lights its surroundings as every other lamp of its kind does, and a change
to that kind changes the new lamp too.

A lamp is one of two things:

- **A whole model.** A placeable's lamp model is drawn whole. Its glowing
  texture is read as a kind of fitting of placeables.
- **A light mesh.** One mesh of a room's model, such as a ceiling panel or
  a wall strip, is drawn alone. Its middle stands where the lamp is
  placed, and it keeps the way it faced in its room. Its glowing texture is
  read as a kind of fitting of rooms.

The asset picker's **Lamps** tab lists every lamp of the install. The
editor looks through every model that `placeables.2da` names and every
room of the install's layouts the first time the tab is shown, a few
models each frame. The tab says how far the look has got. A placeable model
is listed when one of its meshes glows with a texture that the light data
calls a lamp. A room's mesh is listed when it glows with a texture that the
light data calls a lamp on rooms. A room's row of equal panels is listed
once: a mesh with the same texture as one listed already, whose three
extents are the same to the nearest half metre, is left out. At most 4
meshes of one texture are listed. Each tile shows the lamp's texture and
the model it is from.

A lamp is placed by dragging it into the view or double-clicking it. It is
moved, turned, copied and deleted like an object, with undo. Its
details show its model and mesh, whether its texture is read as a room's
fitting or a placeable's (which can be changed), its pitch and roll, and
the reason it is there. The outliner lists the lamps under Lamps.

A lamp turns on every axis, unlike the games' own objects. The area's
object list keeps a single facing for each of those, so the editor turns
them about Z alone. A lamp is kjk's own, so the Rotate tool shows three
rings for it, turning it about its own axes. A light mesh tipped against
a wall at an angle shows its lit face to the room, and its back, which
has no faces, lies against the wall. The **Pitch** and **Roll** fields set
the same turns by their numbers. Pitch tips the lamp's front down about
its own side. Roll turns it about its own front.

A lamp is drawn a little differently from the place it came from:

- A light mesh leaves behind the lightmap of the room it was taken from,
  which was baked for another place. It is lit at runtime as a placeable
  is.
- A light mesh that does not glow in its model's files glows fully, so it
  looks lit. Such a mesh was bright in its room only because of the
  room's lightmap.
- A lamp casts no shadow, so its own mesh never darkens its light.
- A lamp is not solid. Characters walk through it.

An installed level's lamps are the `[[lamp]]` tables of the light data's
`fittings.toml` whose `area` is its module, saved beside its lights placed
by hand and in the same way. Every scene of the module draws them,
including play without a project. A new map's lamps are `[[lamp]]` tables
of its `kjk-area.toml`.

```toml
[[lamp]]
game = "k1"
area = "danm13"
model = "m13aa_01a"
mesh = "m13aa_01a_lamp2"
on = "room"
at = [102.5, 64.0, 4.2]
yaw = 90.0
pitch = 45.0
reason = "The corridor to the council chamber was dark."
```

`model` is the model's resref. `mesh` names the node of a light mesh and is
left out for a whole model. `on` says which kind of fitting the glowing
texture is read as (`room` or `placeable`). `yaw` is degrees from +X,
turning left. `pitch` and `roll` are degrees, and each is left out when it
is 0. The lamp is rolled about its own X first, then tipped about its own
Y, then turned by its yaw about Z.

### Bakes

**Bake now** (in the Lighting menu, `edit bake`) bakes the area's
lightmaps from its fitted lights as the editor has them, so lights can be
added, kinds and levels changed, and the area baked again until it looks
right. It does what `tools/light-kinds/bake.sh` does, with the editor's
own scene in place of a launch of the game:

1. The project is saved, so the light data holds the editor's lights,
   kinds and levels.
2. The scene's lights are written to a dump (`lights-<label>.txt`), as
   the game writes them for `KJK_FITTED_DUMP` under the live lights
   (`fitted_lights::dump_lights`).
3. `kjk detail-bake --gpu` runs on a thread of its own, set as
   `bake.toml` sets the module (its sun, skylights and even light): a
   module with a levels file bakes once, held to it; a module with a
   share bakes twice, first to measure each room's level and then held
   to the share of it, at most its ceiling.

The editor keeps working while it bakes; the toolbar says how far the bake
is. A bake of the Dantooine enclave takes about 11 seconds, the Ebon
Hawk's two passes about 13. The bake lands where the scripts put theirs,
`target/light-kinds/game/bake-<module>/bake/<label>` in the kjk checkout
(the label is the date and time, ending in `ed`), with its dump, levels
and logs beside it. When it is done, the view shows it (Baked) and the
Lighting menu lists it first. **Compare with the bake before** (`edit bake
compare`) shows the newest bake and the one before it in turn. Walk takes
the bake shown.

**Walk** (F7, in the Lighting menu) saves, then starts play's walk mode
(`kjk play --walk`) on the level, lit as the view is: with
`KJK_FITTED_LIGHTS` and `KJK_DETAIL_BAKE` set from the view, and
`KJK_LIGHT_DATA` set to the folder the lights were saved to.

What the editor needs of the game for this: the lighting mode can change
while a program runs (`fitted_lights::set_mode`, and
`KJK_FITTED_LIGHTS=lamps` for Lamps only), the bake shown can too
(`viewer::set_bake_folder`), the rule data is read from `KJK_LIGHT_DATA`
when it names a folder, and a program can set an area's lights placed by
hand (`fitted_lights::set_placed`) and a game's kinds
(`fitted_lights::set_kinds`). Lights placed by hand are matched to
the module play's scene is named for (`play pmbal in danm13`).

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

### Links between floors

The mesh joins two floors only where they share an edge. A ledge or a
platform a metre up is an island of its own, and the game's characters
have no way onto it. A **link** gives them one. A link is a pair of
points, where a character leaves the floor and where it lands, and it is
one of two kinds:

- A **jump** is crossed either way: up onto the ledge and back down, or
  across a gap.
- A **drop** is crossed downward only: the character steps off the edge
  and falls to where it lands.

With the Nav switch on, the details panel lists the project's links under
**Links between floors**. **Add a jump** and **Add a drop** each take two
clicks on the floor in the view: where characters leave, then where they
land. Each link is drawn in the view as an arc from a ring where
characters leave to a dot where they land.

Each link is checked the way the containment audit moves a player
(`collision::jumps::link_lands`). A jump goes up as high as it needs to,
across, then down. It must reach no higher than a plain jump reaches,
1.68 m, and no farther than a plain jump carries, about 3.6 m. A drop
rises a step over the edge's lip, goes across, then falls. Both must meet
nothing on the way and land on floor a character can stand on, within
0.5 m of the link's end. A link that passes is drawn green and marked
"crossed". One that fails is drawn red and marked "not crossed", with the
reason under it. The mesh also leaves out a link with an end over no
floor, and a link whose ends are on one island already. The Navigation
facts then say how many of the faces not reached on foot the links lead
to.

A character takes a link only where no way leads on foot, so every walk
that works today is the same with links. It walks to where the link
leaves, then jumps toward where it lands (a drop's walker walks off the
edge), and plans on from where it lands (`sim::walk`). The project keeps
its links in `kjk-area.toml` as `[[link]]` tables, each with its `kind`
(`jump` or `drop`), `from` and `to`. Play, scripted moves, followers and
fighters chasing a foe all use them. `kjk --project NAME collision walk
GAME MODULE --from X,Y,Z --to X,Y,Z` walks one character between two
points for real, across the links, and says where it arrived.

## Many objects at once

Several things can be selected at once: objects, Jedi Academy props,
collision boxes, lamps and lights.

- **Ctrl+click** in the view or in the outliner adds a thing to the
  selection, or takes a selected one out of it. The last one added is the
  one whose details show.
- **A drag from an empty part of the view** selects everything whose
  marker lies in the box it draws. With Ctrl held, the box adds to the
  selection.
- The outliner marks every selected row, and the details panel says how
  many things are selected.

With several selected, the gizmo stands in their middle. The Move tool
moves them all by the same amount. The Rotate tool turns them all round
their middle: each one moves round it and turns as much, but a trigger's
or an encounter's outline only moves. Ctrl+D copies them all in place,
and the copies are then selected. Delete deletes them all. Each of these
is one edit, and one undo takes all of it back.

**Ctrl+C** copies what is selected to the clipboard, and **Ctrl+V** in
another editor, or in the same one, pastes it with its middle where the
view looks at the level. A copy holds each object's entry as the level
has it, the templates the objects use (the project's own and the game's),
and the props, boxes, lamps and lights. A paste is one edit, and what it
pastes is selected. A template the level being pasted into reads the same
already stays out of the project. A template whose name the level uses
for something else is pasted under a new name, `kje`, a number and the
name, and its objects point to it. A copy from KOTOR 1 is not pasted into
a TSL project, or the other way round.

A copy does not carry the scripts and conversations its templates name.
A creature copied out of a module whose own archive holds its
conversation or a script of its own has none of them in another module.
Its fields then show those names in orange, and the project can be given
its own scripts and conversations of those names.

## Several levels in one project

Every module of KOTOR holds one area, so a level is a module and its area.
A project of the install can edit several levels, such as the Jedi
enclave on Dantooine (`danm13`) and the courtyard outside it
(`danm14aa`). Each level keeps its own objects, Jedi Academy props,
collision boxes and settings. The templates, scripts and conversations
are the project's, shared by all its levels. Play applies every level's
changes wherever that level is loaded: its objects, its props, its boxes
in the collision, and its boxes' changes to the navigation mesh.

The editor edits one level at a time. **File > Levels of this project**
lists the project's levels. A click on one first asks whether to save the
changes not saved yet, then opens the project on that level. **Add a level** lists the
install's modules, with a filter. A click on one adds it to the project,
made from the install's area, and opens it. A level whose doors and
triggers the project had already changed (`others`, in "Doors and the
ways between levels") keeps those changes when it becomes a level.
`kjk-level-editor GAME MODULE --project NAME` does the same: it opens the
project on that level, and adds the level first when the project does not
have it. A new map's project has one level, the map.

## Where projects live

A project is a folder in `projects` in kjk's own folder, beside the saves:
`%APPDATA%\kjk\projects\NAME` on Windows, `~/Library/Application
Support/kjk/projects/NAME` on macOS and `~/.config/kjk/projects/NAME` on
Linux. `KJK_HOME` moves kjk's folder, and `KJK_PROJECTS` names another
folder for the projects alone. A name is 1 to 64 letters, digits, `-` and
`_`.

The folder holds these files.

- `project.toml` names the game, the first level's module and area, and
  the kjk that wrote it. `levels` lists the project's further levels, each
  as `[module, area]`.
- `AREA.git` is the area's object list as the editor changed it. It is a
  KOTOR file, and the original games could read it.
- `kjk-area.toml` holds what only kjk understands: the Jedi Academy props
  and the collision boxes. The original games never read it. Each further
  level's is `kjk-area-AREA.toml`.
- `kje1_metalbox001.utp` and the like are templates of the project's own
  (below, "An object's behaviour"). They are KOTOR files too.
- `NAME.nss` files are the project's own scripts, and `NAME.ncs` each one
  compiled (below, "The project's own scripts").
- `AREA.are`, for an installed area whose settings the project changes:
  its fog, ambient light and scripts (below, "This level's settings"). A
  new map's is always there.
- `OTHER.git` for each area of another module whose doors or triggers the
  project leads elsewhere, listed in `project.toml` as `others` (below,
  "Doors and the ways between levels").

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
  scripts in `chitin.key`. A script the install has compiled only (most
  of a module's own) is read back into source by kjk's decompiler
  (`script_ncs::decompile`). The viewer shows that source only when it
  compiles back to the script's own bytes. A compiled script keeps no
  names or comments, so its functions, variables and constants are named
  by their type and order (`sub1`, `nLocal1`), and a comment at its top
  says so. **Edit a copy in the project** works on it as on any source.
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
reads a script once and keeps it while the editor runs, and reads it again
after the project's scripts are compiled. The project's own script is read
from its folder each time, so one written since the editor opened shows.

### The project's own scripts

A project can bring scripts of its own. Each is a `NAME.nss` file at the
top of the project's folder, its name 1 to 16 lowercase letters, digits
and `_`, written in the script viewer or in any text editor. Every save
compiles each one that has a
`main` or a `StartingConditional` to `NAME.ncs` beside it, with kjk's
compiler (`script_ncs::compile`, `docs/SCRIPTS.md`). Play and Walk save
first, so they always run what the source says. A file of functions alone
is an include: the scripts that `#include` it are compiled with it, and so
are the install's includes (`k_inc_generic`, `k_inc_debug`). The routines
are the game's, from its `nwscript.nss`.

A script is given to an object as any other, by typing its name into an
event's field. The project's folder is the highest layer of the install's
files, so a script named as one of the game's runs in its place.

A script with a mistake is not compiled. The save says so with its file
and line (for a `;` left out, `kje_broken.nss line 4` and the word it
is missing before), the script viewer shows it in red with its line's
number, and the `.ncs` it had stays as it was. `level_edit::scripts` does
the work.

### Writing a script in the editor

The script viewer is also where a script is written, so that the loop of
writing it, giving it to an object, compiling it and trying it stays in
the editor.

- **Scripts** in the toolbar lists the project's own scripts, each opened
  in the viewer, and makes a new one from the name typed: **New** makes a
  `main`, run on an event, and **New condition** a `StartingConditional`,
  which answers `TRUE` to show a conversation's line. A new script starts
  from a few lines that tell the player it ran.
- A script of the install opens read only, with **Edit a copy in the
  project**. The copy is the project's from then on, and once saved it
  runs in the place of the install's script of that name.
- The Source tab of a script being written is an editor: coloured as the
  viewer colours source, its lines numbered, a mistake's line in red.
  Ctrl+S saves the scripts while it has the keys, and the editor's own
  keys are off meanwhile. A script changed and not saved counts as a
  change, so leaving asks first.
- **Save** writes the scripts changed to the project's folder, and
  **Compile** writes and compiles them all.
- **Try for** saves the project, compiles, and runs the script once in the
  module installed without a window (`kjk try-script`, in every build of
  kjk), for the selected object (as `OBJECT_SELF`, by its tag) or, with
  nothing selected, for the player. It then steps on 3 s, for what the script delays. The Try tab
  shows what happened from the run on, its own run in bold and the
  module's scripts that ran beside it faint: each routine called with its
  arguments and what it gave, what was said to the player, faults, and
  scripts named that the module does not have. A try takes a few seconds,
  most of them the module's install.
- **Give to** lists the selected object's events. One clicked gives it the
  script, as typing its name into the event's field does.
- **Window** opens the viewer in a window of its own over the view, with
  room to write, and **Dock** puts it back under the details.
- While writing, the routine or constant the cursor stands in is looked
  up under the text: its prototype, and what `nwscript.nss` says of it
  (the comment over its prototype, `script_ncs::catalog::notes`). The
  routines and constants that begin the name being typed are listed
  there too, and Tab takes the first.

### Debugging a script

**Debug for** tries the script as Try does, and records the run line by
line: a stop at each source line the script starts, with the functions
called to get there, the locals living there and the script's globals
(`kjk try-script --debug`). The Debug tab steps through the stops,
forward and back, and **Continue** goes on to the next stop at a
breakpoint. A click on a line's number in the gutter sets or clears a
breakpoint (a red dot), and the stop shown is marked there with an arrow.
The run is over before it is read, so stepping back is as cheap as
stepping on; locals of deferred code (`DelayCommand`'s) are not shown.
The compiler writes the notes a run is read with (`NAME.ndb`, beside the
`.ncs`).

### Logic blocks

An object's behaviour on one event can be picked from a list rather than
written. **Blocks** beside an event's script field shows its blocks, run
in order from the top: open, close, lock or unlock a door, start a
conversation, tell the player something, set a global number or boolean,
give the player an item, credits or experience, add a journal entry,
turn a creature hostile, remove an object, take the party to a module,
and guards that stop the rest: only the first time, only if a global
number or boolean is a value. A global is picked from `globalcat.2da`'s:
a global the table does not declare is not kept by the games or by kjk.

The blocks are written as a script of the project (`kjb_N.nss`) whose
header lists them, compiled when the project is saved and tried as any
script; the header is what the editor reads them back from, so the
script holds them and nothing else does. An event that runs another
script offers to replace it with blocks, leaving the old script as it
is. Editing a script of blocks by hand drops its header, and the editor
then shows it as a script written by hand. `level_edit::blocks` does the
work.

## Doors and the ways between levels

The asset picker's Doors tab places doors from the install's door
templates (`.utd`), each drawn with its model. A door is read whole from
the area's object list, so its tag, lock and scripts are its entry's.

A door or a trigger can lead somewhere. The details panel's **Leads to**
sets the level (any installed module, or this one) and where to arrive in
it: a door or a waypoint of that level's entry area by its tag, or the
module's entry. It writes the entry's `LinkedToModule`, `LinkedTo` and
`LinkedToFlags`. kjk takes any door or trigger whose link names a module
for a way out: a door when it is used, a trigger when the party walks into
it. A trigger needs no transition template for kjk.

**Ways in from other levels**, shown when nothing is selected, lead a door
or a trigger of another module's entry area here: pick the level, the door
or waypoint of this level to arrive at, and Lead here on one of its doors
or triggers. The project keeps a copy of that area's object list with the
change (`Project::others`, saved as `OTHER.git`), and the project's folder
being the highest layer of the install's files, kjk plays it wherever that
area is loaded while the project is played. So one project carries a new
map and the way into it from the story: the Ebon Hawk's exit trigger
(`ebn12_ebonexit`) led into a new map takes the party there from the Hawk.
Give back drops the copy, and that area's doors lead where the game has
them again. The project's own area is changed in its own list instead.

## Creatures, stores and encounters

A creature, a store and an encounter have more in their templates than
scripts. The Behaviour section shows these fields under the scripts. A
change to one of them copies the template into the project first, as any
other change to a template does. Each field here is one kjk reads when it
makes the object, so every change shows in play.

A creature's fields:

- **First name** and **Last name**. The game's names come from its talk
  table. A name typed here is kept in the template itself, with no
  talk-table line, and kjk shows it as it shows the game's names.
- **Statistics**: its appearance (its model, size and speed), its faction,
  its race, its six ability scores, its hit points, its Force points, its
  natural defence, its challenge rating, its speed, how far it sees and
  hears, and its alignment. Four check boxes make it a plot creature that
  takes no damage, stop damage at 1 hit point, let its weapons be knocked
  away, and keep its body where it fell. The appearance, faction and race
  are picked from a list of the game's own rows (`appearance.2da`,
  `repute.2da`, `racialtypes.2da`), with a filter.
- **Classes and Force powers**: up to three classes, each with its levels,
  and the Force powers each class knows. A class is picked from
  `classes.2da` and a power from `spells.2da`. A power's button takes it
  away.
- **Feats**, picked from `feat.2da`. A feat's button takes it away.
- **Skills**: the rank of each of the 8 skills.
- **Equipment**: the item in each slot (head, body, hands, both hands, both
  arms, implant, belt, and a creature's own claws, bite and hide). An item
  is typed by its template's name, and the install's items that match are
  listed to click. An empty field leaves the slot empty.
- **Items it carries**, added and removed as a placeable's are. **Drops**
  makes an item drop when the creature dies.

A store's fields:

- its **Name**;
- **Prices**: what it charges for an item and what it pays, each as a
  share of the item's value (100 is the value), and whether it sells,
  buys, or both;
- **Items it sells**, added and removed as a placeable's are. **Never runs
  out** keeps an item in stock however many are bought.

An encounter's fields:

- its **Name**;
- **Spawning**: whether it is active, its difficulty, the most and the
  fewest creatures it spawns, whether only the player's party sets it
  off, whether it resets and after how many seconds, how many times it
  spawns again, and its faction. An encounter resets only when both
  **Resets** and **Can spawn again** are on.
- **Creatures it spawns**: each creature's template, the challenge it
  counts for, and **Once** for a creature it spawns once at most. A
  creature added takes its template's challenge rating.
- **Spawn points**: the places its creatures appear, and the way each
  faces. With none, they appear where the party walks in. Each point is
  set by its numbers or removed. **Place spawn points** adds a point at
  each click on the floor in the view, until Enter or Escape. A selected
  encounter shows its spawn points in the view as pink rings, each with a
  line the way it faces.

New encounters are placed from the asset picker's Encounters tab. A new
encounter gets a square outline 2 metres wide and no spawn points.

## Trigger outlines

A selected trigger or encounter shows its outline over the view, its
corners as rings. A corner dragged moves along the floor under the
pointer. The details panel's **Outline** lists the corners, each set by
its numbers or removed (three are the fewest), adds a corner halfway along
the longest side, and **Draw a new outline**: each click on the floor is
the next corner, in order round it, and Enter or Done ends it (Escape
leaves the outline as it was).

## This level's settings

With nothing selected, **This level** sets the area's own settings:

- its music by day, by night and in battle (`ambientmusic.2da` rows), and
  its ambient sound by day and by night (`ambientsound.2da`) with their
  volumes: the object list's `AreaProperties`;
- its fog (on, where it starts and where it is thick, its colour) and its
  ambient light: the area's `.are`;
- its scripts (`OnEnter`, `OnExit`, `OnHeartbeat`, `OnUserDefined`): the
  `.are` too;
- for a new map, the module's scripts (`Mod_OnModLoad`,
  `Mod_OnClientEntr` and the rest): its own `module.ifo`, kept in
  `kjk-map.toml`. An installed module's stay the game's: every module's
  `module.ifo` has one name, so a copy in the project's folder would stand
  in for every module's.

An installed area's `.are` is copied into the project at the first change
(`Project::are`); a new map's is its own. `level_edit::settings` does the
work.

## Play from the camera

**Play here** (Shift+F5) saves and starts `kjk play` with the player where
the view's centre meets the floor, facing as the camera faces (`kjk play
--at`), to test one corner of a level without walking there. `edit walk
here` does the same for walk mode.

## Conversations

A project can bring conversations of its own, and copies of the game's.
Each is a `NAME.dlg` in the project's folder, which kjk plays in place of
the game's conversation of that name. `level_edit::dialog` does the work,
on the file itself, so every field the editor does not touch stays as it
was.

- **Conversations** in the toolbar lists the project's conversations and
  opens one. **New** makes a conversation with no lines, and **Copy the
  game's** copies the game's conversation of the name typed.
- An object's **Conversation** in its behaviour (a creature, a placeable
  or a door) names the conversation it speaks. **Open** opens the
  project's, **Edit a copy** copies the game's, and **New** makes one for
  the object and gives it to it.

The conversation window shows the conversation as a tree, one opening at
a time. A conversation opens with the first of its starting lines whose
condition holds, tried from the top, so the openings are listed and
numbered in that order. Each opening's header gives its condition, what
that condition reads (the globals and tags its compiled script names,
such as `DAN_JUHANI_PLOT`), and the opening's first words. **Up** and
**Down** on an opening's first line change the order. Under each opening,
each line the other side says (orange) leads to the replies the player
may pick (blue), and each reply leads to the next line. Every line with
lines under it can be folded. A line shown already above is not opened
again; the tree says where it goes on instead. A reply with no text is
shown as Continue, as the games show it. Each row shows the line's
condition and the script it runs. Lines no opening reaches are listed at
the end.

A line clicked in the tree is changed on the right:

- its text. A line the editor writes keeps its text in the conversation
  itself, with no talk-table line, and kjk shows it as it shows the
  game's lines. A line of the game shows its talk-table text, and **Write
  its own text** gives it its own, starting from the game's. New lines
  are text only; they have no voice.
- for a line the other side says, its speaker (an object's tag; empty
  for the conversation's owner) and its listener (empty for the player);
- the script it runs as it shows, and **Write it** opens that script in
  the script window;
- its condition: the script that decides whether the line is said, or
  the reply offered, where it is reached from here. **Write it** opens it,
  or makes a new `StartingConditional` that answers `TRUE`;
- the journal entry it adds: a quest's tag and the entry's number;
- **Add a reply** under a line, or **Add a line** under a reply; **Go on
  at** links it to a line or reply shown elsewhere; **Up** and **Down**
  move it among its siblings (the order replies are offered in); **Remove
  this link**; and **Delete**, which takes it out with every link to it.

The window also sets the scripts run when the conversation ends and when
it is cut short. **Try** saves, then plays the conversation in the level
without a window (`kjk try-conversation`): the player talks to the object
of the tag given and takes the replies by their place (1 the first; the
first when the list runs out). What was said, offered and picked shows
in the window.

## Working with an agent

An agent, such as Claude, can help with a level in two ways.

**Copy** in the toolbar (Ctrl+Shift+C) copies the selected object as
text, to paste to an agent. The text names the project and the level, the
object's kind, and the command that selects it again (`edit select
MetalBox`). It holds everything the details panel knows of the object:
its template, tag, position, facing, scripts and the rest. It ends with
the camera, as a `camera` command. With nothing selected, it copies the
level and the camera.

**Agent link** in the toolbar lets an agent on the same computer see what
is selected and change the level while you watch. It is off until it is
ticked, or until the editor starts with `KJK_EDITOR_LINK=1`. While it is
on, the editor uses the folder `editor-link` in kjk's own folder
(`%APPDATA%\kjk\editor-link` on Windows):

- It writes `live.json` there four times a second, when anything in it
  changed. The file holds the project and the level, whether there are
  changes not saved, the selected object with its details and the command
  that selects it, the camera, the editor's last messages, the answers to
  the agent's last 20 commands, the scripts that did not compile, and
  what the last script tried in the editor printed.
- It runs each line an agent writes to `inbox.txt` there as a command, in
  order, and then deletes the file. A line is one of the editor's own
  commands (the table below), with or without its `edit`; `camera X Y Z
  YAW PITCH`, which moves the view; or `frame`, which looks at the
  selection. Each command's answer is `ok` or the reason it failed, and
  the editor's messages show it too.
- It writes a `README.md` there for agents: the guide in
  `docs/LEVEL-EDIT-AGENT.md`, followed by this document's list of
  commands, as the editor was built with them.

Nothing listens on the network: only a program that can write to the
user's own folder can send the editor commands. The editor saves only on
`save`, and every change an agent makes can be undone as any other.
`level_editor::agent` does the work.

## The editor's window

- **The toolbar**, along the top, has the File menu, picks the tool
  (Select, Move, Rotate, Scale), turns snapping on and sets its steps, and
  has Undo, Redo, Save, Play and Audit, and the Collision and Nav switches.
  It also names the project and says when it has changes that are not
  saved.
- **The File menu** opens another level without starting the editor by
  hand. "Open level..." shows the picker, "Open recent" lists the other
  projects, newest first, and "New map..." asks a name and makes a new map
  whose light, fog and sound are this level's (a new map's own base, for a
  new map). It also saves and quits. A project is opened once in a
  process, so the editor starts again on what was picked and this window
  closes.
- **Leaving with changes not saved** asks first: Save, Don't save or
  Cancel. Opening another level, Quit and closing the window all ask.
  Nothing is saved without asking; Save, Ctrl+S, Play and Audit save.
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
  sound, a store) has its marker's icon instead. An asset dragged into the view is placed where the
  pointer meets the level. A double-click places it where the centre of the
  view meets the level.
- **The view**, in the middle, draws the area as `kjk module` does. Every
  object has a marker: an icon on a dark disc, in its kind's colour, drawn
  over the view. Creatures are red, doors purple, placeables orange,
  triggers light blue, waypoints green, sounds pink, encounters white and
  stores yellow. Props are pale blue and lamps warm yellow. The icon says
  what the thing is:

  | Icon | Marks |
  |---|---|
  | a filled square | a placeable, or a Jedi Academy prop |
  | a speaker | a sound |
  | an arrow down | a waypoint |
  | a map marker | a place a character is put, and a new map's entry. A waypoint is such a place when a word of its tag (between `_`s, with its trailing digits left off) names a party member of either game, the player (`pc`, `party`) or a post (`post`). `dan13_wp_carth_halk` and `POST_dan13_zzshari` are such places |
  | a person | a creature with a conversation |
  | a person with an alert | a creature whose conversation updates the journal: one of its lines has a quest |
  | an alert circle | a trigger, or an encounter |
  | a sun | a light, or a lamp |
  | a dot | a creature with no conversation, a door, or a store |

  A marker stands over a creature's head, at a light's or a lamp's own
  place, and a little above any other thing's place. A marker grows the
  nearer it is and fades the farther it is. Markers more than 80 m away
  are not drawn. The selected one is ringed in yellow and drawn over the
  rest. A click on a marker selects its thing before any model under the
  pointer. A box is drawn see-through in its role's colour: a solid box
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
| left mouse button | select the object, prop or box under the pointer (its marker, else its model); drag a gizmo's handle |
| Ctrl + left mouse button | add the thing under the pointer to the selection, or take it out |
| left mouse button dragged over an empty part of the view | select everything in the box (with Ctrl, add it to the selection) |
| Ctrl+C, Ctrl+V | copy the selection to the clipboard; paste a copy where the view looks |
| Q, W, E, R | the Select, Move, Rotate and Scale tools |
| F | move the camera to the selection |
| Ctrl+D | copy everything selected, in the same place |
| Delete | delete everything selected |
| Ctrl+Z, Ctrl+Y | undo, redo |
| Ctrl+S | save the project |
| Ctrl+Shift+C | copy the selected object, or the level, as text for an agent |
| Enter | place the asset picked in the picker where the view's centre meets the level |
| F2 | show or hide the collision: walkable walkmesh in green, other solid surfaces in orange, the walls along the walkmesh's open edges in blue |
| F3 | show or hide the navigation mesh |
| F4 | show or hide the lights |
| F7 | save, then walk the level in play's walk mode, lit as the view is |
| F5 | save, then start `kjk play` on the project in a window of its own, as a level 6 Jedi |
| Shift+F5 | the same, the player starting where the view looks at the floor |
| Enter, Escape | while drawing an outline: end it, or leave the outline as it was |
| F6 | save, then run the containment audit on the project and list its result |
| Escape | select nothing |

While a text field has the keyboard, the keys go to the field. While the
right mouse button is held, they fly the camera.

One drag of a gizmo, or of a number in the details panel, is one edit: one
undo takes all of it back. Every kind can be moved, copied and deleted,
and every kind but triggers and encounters turned. A door's walkmesh moves
with it; an installed door moved out of its doorway leaves the doorway
open. A trigger's or an encounter's outline moves with it but does not
turn, and an encounter's spawn points move with it. New objects can be
creatures, placeables, doors, waypoints, triggers, sounds, stores and encounters, made
from the install's templates. A new door or waypoint takes its template's
own fields (its tag, name, look, lock and scripts) into its entry. A new
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
| `edit select more TAG` | adds the first object with that tag to the selection |
| `edit move by DX DY DZ`, `edit turn by DEGREES` | moves everything selected by an amount, or turns it round its middle, as one edit |
| `edit duplicate`, `edit delete` | copies, or deletes, everything selected, as one edit |
| `edit copy`, `edit copy to FILE`, `edit paste from FILE` | copies what is selected to the clipboard or a file; pastes a copy from a file where the view looks |
| `edit select prop N`, `edit select box N` | selects the Nth prop or box, from 0 |
| `edit select none` | selects nothing |
| `edit pick X Y` | selects what a click at that point of the window (pixels) would |
| `edit move X Y Z`, `edit nudge DX DY DZ` | moves the selection to a point, or by an amount, in metres |
| `edit turn DEGREES` | turns the selection to face that many degrees from +X, turning left |
| `edit scale S` | draws the selected prop at that scale, from 0.05 to 20 |
| `edit size X Y Z` | gives the selected box that size, in metres |
| `edit duplicate`, `edit delete` | copies or deletes the selection |
| `edit undo`, `edit redo` | takes back the last edit, or makes it again |
| `edit add KIND TEMPLATE [X Y Z]` | adds a `placeable`, `creature`, `door`, `waypoint`, `trigger`, `sound` or `store` made from the template, at the point or where the view's centre meets the level |
| `edit link MODULE [TAG]`, `edit link none` | leads the selected door or trigger to the module, arriving at the door or waypoint of that tag (the module's entry without one), or nowhere |
| `edit entrance MODULE door\|trigger N [TAG]` | leads door or trigger N of the module's entry area here, arriving at the door or waypoint of that tag (this level's entry without one) |
| `edit entrance drop AREA` | gives that area's object list back to the install |
| `edit outline X1 Y1 X2 Y2 X3 Y3 ...` | sets the selected trigger's or encounter's outline: three corners or more, in metres, at its height |
| `edit area music day\|night\|battle ROW`, `edit area ambience day\|night ROW`, `edit area volume day\|night N` | the area's music and ambient sound (2DA rows) and the sound's volume (0 to 127) |
| `edit area fog on\|off`, `edit area fog NEAR FAR`, `edit area fog colour R G B`, `edit area ambient R G B` | the area's fog and ambient light (colours from 0 to 1) |
| `edit area script LABEL SCRIPT`, `edit area module script LABEL SCRIPT` | an area script (`OnEnter`), or a new map's module script (`Mod_OnModLoad`); `-` for none |
| `edit tag TAG` | sets the selected object's tag |
| `edit prop MODEL [X Y Z [SCALE]]` | places a Jedi Academy model (`models/map_objects/...md3`), solid |
| `edit box [CX CY CZ SX SY SZ [YAW]]` | places a collision box: its centre, its size and its yaw |
| `edit script LABEL SCRIPT` | sets the selected object's event script (`OnUsed`, `ScriptHeartbeat`); `-` for none |
| `edit script new NAME`, `edit script new NAME conditional` | makes a script of the project, a `main` or a `StartingConditional`, and opens it in the viewer |
| `edit script edit NAME` | opens a script to be written: the project's own, else a copy of the install's |
| `edit script write`, `edit script compile` | writes the scripts changed in the viewer; compile also compiles the project's scripts |
| `edit script debug NAME`, `edit script debug NAME TAG` | tries the script as `edit script try` does and records it line by line; the stops are under the viewer's Debug tab |
| `edit talk NAME`, `edit talk -` | gives the selected object the conversation, or none |
| `edit conversation new NAME`, `edit conversation copy NAME`, `edit conversation drop NAME` | makes a conversation of the project, copies the game's, or takes one out of the project |
| `edit conversation NAME add`, `edit conversation NAME add line\|reply N` | adds a starting line, or a node under line or reply N (a reply under a line, a line under a reply) |
| `edit conversation NAME text line\|reply N TEXT ...` | sets a node's own text (the words to the end of the command) |
| `edit conversation NAME speaker line N TAG`, `listener line N TAG` | sets a line's speaker or listener; `-` for the owner or the player |
| `edit conversation NAME script line\|reply N SCRIPT`, `quest line\|reply N TAG ENTRY` | sets the script a node runs (`-` for none), or the journal entry it adds |
| `edit conversation NAME condition start I SCRIPT`, `condition line\|reply N I SCRIPT` | sets the condition of the starting list's link I, or of node N's link I; `-` for none |
| `edit conversation NAME link start TO`, `link line\|reply N TO` | links to a line or reply shown elsewhere |
| `edit conversation NAME unlink ...`, `up ...`, `down ...` | takes a link away, or moves it, named as `condition` names it |
| `edit conversation NAME delete line\|reply N`, `end SCRIPT`, `abort SCRIPT` | deletes a node; sets the scripts run as it ends or is cut short |
| `edit conversation try NAME TAG [PICK ...]` | saves, then plays the conversation for the player talking to the object tagged TAG, answering by the picks |
| `edit ui conversation NAME`, `edit ui conversation`, `edit ui conversation select line\|reply N` | opens or closes the conversation window; selects a node in it |
| `edit blocks EVENT add KEY [VALUE ...]`, `edit blocks EVENT remove N`, `edit blocks EVENT clear` | the selected object's logic blocks on the event (`OnUsed`): a block of the kind (`open_door`, `give_item`, `once`; `level_edit::blocks::KINDS`) added with its values, block N removed, or none left |
| `edit script try NAME`, `edit script try NAME TAG` | saves, compiles, then runs the script for the player or the object tagged TAG in the module, headless; its trace is under the viewer's Try tab |
| `edit flag useable on`, `edit flag inventory off` | sets the selected placeable's `Useable` or `HasInventory` |
| `edit item add ITEM`, `edit item remove N` | adds an item template to the selected placeable's, creature's or store's items, or removes its Nth, from 0 |
| `edit item N drops on\|off`, `edit item N infinite on\|off` | sets whether a creature's Nth item drops when it dies, or a store's Nth item never runs out |
| `edit number LABEL VALUE` | sets a number of the selected creature, store or encounter by its template field (`Str`, `Appearance_Type`, `MarkUp`, `MaxCreatures`); a check box is 0 or 1 |
| `edit name LABEL TEXT ...` | sets a name (`FirstName`, `LastName`, a store's `LocName`, an encounter's `LocalizedName`) to its own text |
| `edit class N ROW LEVELS`, `edit class N -` | sets the creature's Nth class (from 0) to a `classes.2da` row and its levels, adds one at the end, or removes it |
| `edit feat ROW on\|off`, `edit power N ROW on\|off` | gives or takes a feat, or a Force power of the Nth class (`feat.2da`, `spells.2da` rows) |
| `edit skill N RANK` | sets the rank of the creature's Nth skill (0 to 7) |
| `edit equip SLOT ITEM\|-` | puts an item in a slot, by its number or its name (`right_hand`), or empties it |
| `edit spawn N TEMPLATE CR [single]`, `edit spawn N -` | sets the encounter's Nth creature, adds one at the end, or removes it |
| `edit spawnpoint add X Y Z [FACING]`, `edit spawnpoint remove N` | adds a spawn point to the selected encounter (facing in radians), or removes its Nth |
| `edit collision on`, `edit collision off` | shows or hides the collision |
| `edit nav on`, `edit nav off` | shows or hides the navigation mesh |
| `edit role solid`, `walkable`, `block`, `floor` | gives the selected box a role |
| `edit navlink jump X1 Y1 Z1 X2 Y2 Z2`, `edit navlink drop ...` | adds a link between floors, from where characters leave to where they land |
| `edit navlink remove N` | takes out the Nth link, from 0 |
| `edit light view game`, `live`, `lamps`, `baked [FOLDER]` | lights the view's rooms so |
| `edit lights on`, `edit lights off` | shows or hides the lights |
| `edit add light point`, `spot`, `panel`, `strip` `[X Y Z]` | places a light by hand |
| `edit select light N`, `edit select fitted N` | selects a light placed by hand, or the game's (from 0) |
| `edit light colour R G B`, `radius R`, `cone C`, `strip HALF`, `reason TEXT` | changes the selected light placed by hand |
| `edit light copy` | copies the selected fitted light as a light placed by hand |
| `edit add lamp MODEL [MESH] [X Y Z]` | places a lamp: the whole model, or (with `MESH`, a node's name) that one mesh of it. `-` in place of `MESH` names the whole model |
| `edit select lamp N` | selects the Nth lamp, from 0 |
| `edit lamp kind room\|placeable`, `edit lamp pitch DEGREES`, `edit lamp roll DEGREES`, `edit lamp reason TEXT` | changes the selected lamp |
| `edit lamps list` | looks through the install at once and lists every lamp the Lamps tab offers |
| `edit ui tab lamps` | shows the Lamps tab |
| `edit walk` | saves, then starts play's walk mode lit as the view is |
| `edit kind strength S`, `reach R`, `colour R G B`, `cone C`, `lamp on`, `lamp off`, `reason TEXT` | changes the kind of the selected fitted light |
| `edit level ROOM LEVEL`, `edit level ROOM none` | sets or takes away the level a room of the bake aims at |
| `edit ui levels on`, `edit ui levels off` | opens or closes the Room levels window |
| `edit bake` | saves, then bakes the area's lightmaps from its lights and shows the bake when it is done |
| `edit bake compare` | shows the newest bake and the one before it in turn |
| `edit save` | saves the project |
| `edit play` | saves, then starts `kjk play` on the project |
| `edit play here`, `edit walk here` | the same, the player starting where the view looks at the floor |
| `edit audit` | saves, then runs `kjk collision audit` on the project |
| `edit ui tool select`, `move`, `rotate`, `scale` | picks the tool |
| `edit ui tab placeables`, `creatures`, `props`, `shapes`, `waypoints`, `triggers`, `sounds`, `stores` | shows a tab of the asset picker |
| `edit ui search TEXT`, `edit ui find TEXT` | sets the asset picker's or the outliner's search |
| `edit ui snap on`, `edit ui snap off` | turns snapping on or off |
| `edit ui view SCRIPT`, `edit ui view` | opens the script viewer on the script, or closes it |
| `edit ui blocks EVENT`, `edit ui blocks` | shows the selected object's blocks on the event, or none |
| `edit ui link on`, `edit ui link off` | turns the agent link on or off |
| `edit ui debug N`, `edit ui break SCRIPT LINE` | the Debug tab's stop N (from 1); sets or clears a breakpoint |
| `edit ui tab doors`, `edit ui way_in MODULE [TAG]` | the asset picker's Doors tab; the level and arrival Ways in shows |
| `edit ui view window on`, `edit ui view window off` | the viewer in a window of its own, or under the details |
| `edit ui view tab source`, `summary`, `listing`, `try` | picks the viewer's tab |
| `edit ui file open [PROJECT]` | opens the picker, or the project, in a new editor (a headless run names it instead) |
| `edit ui file level MODULE` | opens the project on its level MODULE in a new editor, adding the level first when the project does not have it |
| `edit ui file new NAME` | makes the new map NAME from this level in a new editor |
| `edit ui quit` | closes the editor |
| `edit ui confirm save`, `discard`, `cancel` | answers the question asked before leaving with changes not saved |
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
- **Everything but a lamp turns about Z only.** A box has a yaw but no
  pitch or roll, so a ramp is built from boxes laid as steps. A Jedi
  Academy prop has a yaw alone too.
- **Co-op does not check projects.** The co-op fingerprint does not cover
  a project's folder. A host and a guest must play the same project by
  hand, or their worlds differ.
- **Saves made in an edited area** hold the edited area's objects. Loading
  such a save without the project, or with a changed project, may move or
  lose objects. kjk does not warn about this yet.
- **One level at a time.** A project's levels are edited one by one, and
  the editor starts again to move between them. A level cannot be taken
  out of a project yet. A new map's project has one level.
- **No export.** A new map's `.mod` and a project's `.git` file are KOTOR
  files, but there is no step yet that makes them ready for the original
  game.
- **The view is drawn under the panels.** The scene's camera fills the
  window, and the panels cover its edges.

## Future work

These come after the prototype: navigation that includes the added boxes
and props, multiple selection, export of a new map for the original game (its walkmeshes'
neighbours, AI paths and a map picture), turning rooms by 90° steps,
rooms made in Blender, markers for vehicle
routes and spawn points for vehicles and jetpacks, and editing an area
together in co-op.

## Checks

- The approved test `level_edit_round_trip` (`docs/TESTS.md`) edits a GIT
  made by the test, saves the project to a temporary folder and reads it
  back. It needs no install.
- `level_edit_project_scripts` compiles a project's scripts in a folder the
  test makes, and `script_vm` runs scripts compiled from source.
- `level_edit_selection` selects three objects and a prop, moves, turns,
  copies and deletes them each as one undo step, and copies a chair with
  its template into another project, where the template takes a new name.
- `level_edit_nav_links` adds links between two made-up floors, finds the
  first link of a way, keeps routes on foot as they were, writes a mesh
  without links as before, and checks jumps and drops on boxes.
- `level_edit_turned_rooms` turns a made-up room's walkmesh and model,
  finds a point on the same face before and after, and has the map keep
  the room's yaw.
- `level_edit_levels` adds levels to a project, keeps each level's
  objects and kjk-only content apart, saves and reopens them, and refuses
  a level for a new map.
- `level_edit_templates` changes a creature's, a store's and an
  encounter's template beyond its scripts, places a new encounter with
  spawn points, reads the encounter as kjk's module reader reads it, and
  has a project keep the templates.
- `level_edit_conversations` builds and changes a conversation, reads
  it as the walk reads it, and has a project keep it.
- `level_edit_logic_blocks` writes blocks as a script, reads them back,
  and compiles every kind; `script_vm` checks the debug notes and a
  watched run.
- `level_edit_ways_and_settings` places a door, leads doors and a
  trigger's outline, leads another area's door into a new map, changes the
  area's settings, and saves and reads it all back.
- The demo (`LEVEL-EDIT-DEMO.md`) shows every behaviour on the Ebon Hawk,
  by hand in a window and as headless runs whose dumps say what changed.
