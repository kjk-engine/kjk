# The level editor (`kjk-level-editor`, `level_edit`)

The level editor opens an area of KOTOR or The Sith Lords and lets the
user change it in a window laid out like a scene editor's. The user can
move, turn, copy and delete the area's creatures, placeables, doors,
waypoints, triggers, sounds, stores and encounters, and add new ones from
the install's templates. The user can also place Jedi Academy props and
boxes of invisible collision, change what an object does, write scripts
and conversations, and light the level. The changes are saved into a
project of the user's own. `kjk play --project NAME` then plays the
changed area. Nothing is ever written into a game install.

This is a prototype. It changes areas that exist, and it makes new maps
from the games' own rooms ("New maps", below). It cannot make a room of
its own, because kjk cannot write a room's model. "Limits of the
prototype" lists what else it does not do.

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

When Jedi Academy is installed, the picker has a third tab, Jedi Academy,
with every level of the game by its name (`t1_sour`, `mp/ffa3`). A level
picked there opens in the rooms tool, to be cut into rooms ("Jedi Academy
levels", below). Its project's name starts as the level's, with `-` where
the level's name has a `/`. Any project can keep a Jedi Academy level's
rooms, a project of KOTOR 1 or KOTOR 2 too. A project that keeps such
rooms is listed on the left once for each level, as "Jedi Academy" and the
level, beside its KOTOR level if it has one. A new map cannot start from a
Jedi Academy level.

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

A new map can also be a further level of a project that exists. **File >
New map...** adds one to the project being edited, and
`kjk-level-editor GAME NAME --project PROJECT --new-map BASE` does the
same. A project can hold several new maps beside levels of the game
("Several levels in one project").

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
- **Jedi Academy rooms.** The Rooms tab also lists every room cut from a
  Jedi Academy level, in this project or in any other ("Jedi Academy
  rooms in a new map" below). Each is shown as its level and its name,
  such as "t1_sour hangar (Jedi Academy)". It is placed, moved and turned
  as a KOTOR room is.

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

### Whose the lights placed by hand are

A level's lights and lamps placed by hand belong to the project once the
project changes them.

- **Until then they are kjk's.** An installed level starts with the
  lights and lamps kjk's light data holds for it: the `[[placed]]` and
  `[[lamp]]` tables of `fittings.toml` whose `area` is its module. The
  light data is the folder `KJK_LIGHT_DATA` names, else the `data/lights`
  of the kjk checkout the editor was built in, else the copy built into
  kjk. The project's files hold none of them, so a later build of kjk
  with better lights for the level reaches the project.
- **A change makes them the project's own.** A save at which the level's
  lights or lamps differ from the light data's writes all of them to the
  project's `kjk-area.toml`, as `[[light]]` and `[[lamp]]` tables under
  `own_lights = true`. They are then the level's whole list. `kjk play
  --project NAME` uses them in place of the light data's, and so does
  every editor, built from source or not. Changed back to what the light
  data holds, they are kjk's again and leave the file at the next save.
- **A new map's are always its own**, in its `kjk-area.toml`.

The Lighting menu says which of the two holds, and `lights_own` in the
agent link's `live.json` says it too.

### The project's own kinds and room levels

The kinds of fitting and the rooms' bake levels follow the same rule as
the lights placed by hand. They are kjk's until the project changes
them, and the project's own from then on.

- **A changed kind of fitting is the project's own.** It applies to this
  project only. Every level the project plays takes it, and no other
  project or player does. Changed back to what the light data holds, it
  is kjk's again.
- **A level's changed room levels are the project's own.** They are that
  level's whole list. A bake from the editor is then held to them, not
  to the light data's levels file or share.

A save writes both to `kjk-lighting.toml` in the project's folder. The
file has a `[[kind]]` table for each changed kind, in the layout of
`data/lights/<game>.toml` with the kind's reason, and a
`[levels.<module>]` table of each room's level. A project that changed
neither has no such file. `kjk play --project NAME` reads the project's
kinds in place of the light data's. The Lighting menu says what the
project keeps as its own, and `kinds_own` and `levels_own` in the agent
link's `live.json` say it too. An editor from a release package changes
and saves both, as an editor built from source does.

### Publishing to kjk's light data

**Publish to kjk's light data** (in the Lighting menu, `edit publish`)
is for whoever maintains kjk's own lighting of the games. It writes
everything of the project's own lighting to the light data, where the
next build of kjk takes it for every player, and then saves the project
without it:

- the level's lights and lamps placed by hand, to `fittings.toml`. That
  area's tables are replaced and every other line of the file is left as
  it was.
- the kinds of fitting the project changed.
- the level's room levels.

Each change is a diff the lighting's maintainer reviews. Publishing needs
a light data folder: kjk built from source, or `KJK_LIGHT_DATA`. A save
alone never writes to the light data.

A published kind is written to the light data twice, so the two files agree:
its `[[kind]]` entry of `k1.toml` or `tsl.toml`, which the game reads, and
a `[[fitting]]` of `fittings.toml` with the same numbers and the reason,
which `tools/light-kinds/kinds.py` reads when it writes those entries
again. An existing `[[fitting]]` for the kind is changed in place, its
other fields kept. The published room levels are written to the module's levels
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

A level's lamps are kept as its lights placed by hand are ("Whose the
lights placed by hand are", above): kjk's until the project changes them,
then the project's own `[[lamp]]` tables. The light data's lamps are
drawn in every scene of the module, including play without a project. A
project's own are drawn in play with that project.

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

1. The project is saved, so the project holds the editor's lights and
   lamps, and the light data its kinds and levels.
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

### The project's bake

A bake from the editor is a trial: it lands in the kjk checkout, and the
next bake stands beside it. **Keep the bake shown in the project** (in the
Lighting menu, `edit bake keep`) makes the bake the Baked view shows the
project's own. It copies the bake's lightmaps into the project's folder,
as `kjk-bake/<module>/<lightmap>.png`, in place of the bake kept before.
The Dantooine enclave's bake is 110 lightmaps, about 3 MB.

**Play without a bake.** A level whose project keeps no bake of it is
still lit in play when its lights or lamps placed by hand are the
project's own. That is every new map with lights, and a level of the game
once the project changed its lights. kjk then lights the rooms live from
those lights, as the editor's Live view does, and says so in its log. A
bake looks better and costs less to draw, so a finished level should
keep one. A level of the game whose lights the project did not change is
lit as the game stores it. `KJK_FITTED_LIGHTS` still chooses the lighting
when it is set.

A kept bake does not follow the lights. A bake the editor makes notes
what it was made from: the level's lights and lamps placed by hand, its
sun, the kinds of fitting and room levels the project changed, and a new
map's rooms. Once any of these differs from the kept bake's note, the
Lighting menu says **The project's bake is older than the level's
lights**. **Bake again and keep** (`edit bake renew`) saves, bakes the
level as it is now, and keeps the new bake in place of the old one. A
bake kept before this version has no note and is never marked.

- **Play is lit with it.** `kjk play --project NAME` draws the level's
  rooms with the project's bake, as the editor's Baked view does. Every
  other level is lit as the game stores it. `KJK_FITTED_LIGHTS` still
  chooses the lighting when it is set, and a bake `KJK_DETAIL_BAKE` names
  is still used in place of the project's.
- **It travels with the project.** A copy of the project's folder has
  the bake, so a player with a release package sees it. Such a player
  cannot make a bake, because a bake needs the `kjk` that has
  `detail-bake`.
- **It is kept at once**, not at the next save, as a bake itself is made
  at once. It is not part of undo.
- **The editor offers it first.** The Baked view of a level whose project
  keeps a bake shows that bake when the editor opens.

**Take the project's bake out** (`edit bake drop`) removes it. The level
is then lit as the game stores it in play. `bake_kept` in the agent
link's `live.json` is the kept bake's folder, or null.

A kept bake is not made again when the lights change. After a change to
a light, a lamp, a kind or a room's level, bake again and keep the new
bake.

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

## The sun

Many areas have a sun: a light in one of the rooms' models that reaches
100 m or more, far over the level. The Dantooine courtyard's stands about
416 m from the level's middle and reaches 2000 m. kjk casts daylight's
shadows from it.

**Sun** at the top of the outliner selects it, and so does its marker, a
sun drawn 40 m from the level's middle the way the sun stands from it.
Its details show where it stands, seen from the level's middle:

- its **bearing**, degrees from east turning left;
- its **height**, degrees over the horizon;
- its **colour**, its **brightness** (its colour times this) and its
  **reach** (100 m or more).

The Rotate tool turns it round the level's middle, its height kept. It is
not moved, copied or deleted. A change makes it the project's sun, which
play uses in place of the game's. **Use the game's sun** gives the game's
back. A level with no sun can be given one with **Add a sun**, which
starts 500 m over the level's middle. The project keeps it in
`kjk-area.toml` as a `[sun]` table: `at` (where it stands, metres),
`colour`, `multiplier` and `radius`.

The sun lights the characters, placeables and doors, and casts their
shadows. The rooms keep the light stored in their lightmaps, so the
ground and the walls do not change with it until the area is baked again
(`kjk detail-bake`).

## How props are lit

A KOTOR room is lit by its lightmaps, which a Jedi Academy prop does not
have, and the area's ambient light alone leaves a prop nearly black. So a
prop is lit by the room's light where it stands.

- **Under Classic lighting**, a prop glows with the light from above that
  the area's light probes read off the lightmap of the floor round it, in
  the room whose floor lies under it. The probes are the ones the game
  makes when an area loads to light characters and placeables. A prop on
  a dark ship's deck therefore stays dim, and one in a bright courtyard is
  lit as the courtyard is. In the Ebon Hawk a crate glows with 0.20 of
  its texture's colour, and on the open ground of Dantooine's courtyard
  with 0.30. Before, every prop glowed with a fixed 0.45.
- **With the live fitted lights**, the glow is half of that, as the rooms
  keep half of their stored light, and the fitted lights light the prop
  too. With the lamps alone it has no glow at all.
- **Under Enhanced lighting**, the probes light a prop as they light a
  placeable, from each side, and the prop does not glow, so the room's
  light is not counted twice.

A prop where no probe with light is near, such as one in an area whose
rooms have no lightmaps, keeps the old fixed glow of 0.45 of its
texture's colour.

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
have it.

**An adventure of several maps.** A project can hold several new maps and
levels of the game together. This is how an adventure is built: a few new
maps that lead to each other, and a level of the game with the way in.

- **File > New map...** adds a new map to the project and opens it. The
  home level's map file is `kjk-map.toml`. A further map's is
  `kjk-map-<area>.toml`. Each map's module is written to
  `modules/<module>.mod` in the project's folder.
- **Add a level** adds a level of the game to a project of new maps, as it
  does to any project. The way in is then placed in that level itself,
  with the level in view: a door or a trigger whose link names the new
  map.
- **A link names any level of the project.** `edit link MODULE [TAG]` on a
  door or a trigger takes a new map of the project as it takes a level of
  the game. The map must be in the project first, so the ways between
  maps are made once every map exists.

The benchmark `tools/level-editor/three-maps/build.py` builds such an
adventure: a hub and two wings as new maps, and Dreshdae with the way in.

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
(`kje88_metalbox00`). The numbers are the project's own. The editor makes
a series from 1 to 250 out of the project's name, and gives the project's
copies only the numbers that leave that series when divided by 251: 88,
339, 590 and on for the series 88. Two mods then bring no file of the
same name, which matters because the games read one file of a name. The
scripts of logic blocks (`kjb_N`) are numbered the same way. A project
made before this keeps the names it has. The object is pointed at the
copy, and the copy is
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
number or boolean is a value, only if the party carries an item of a
tag. A door that a key opens is made with the last one: the door is
locked and takes a key (`Locked` and `KeyRequired` set to 1), and its
`OnFailToOpen` event has three blocks: `if_has_item` with the key's tag,
`unlock` with the door's tag, and `open_door` with the door's tag. A global is picked from `globalcat.2da`'s:
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

## Templates of the game's other levels

A level reads the game's shared templates and the templates in its own
files. Many creatures, sounds and stores are in one level's files only.
The creatures of Korriban's tombs are an example. A new map has no files
of its own, so it reads only the shared templates.

The editor looks through every level of the game the first time it is
asked for such a template. This takes a few seconds. After that,
`edit add KIND NAME` takes a template that only another level has, and
copies it into the project. The copy brings what the template names and
the edited level does not have:

- the items the creature, the placeable or the store carries,
- its conversation,
- the creatures of an encounter,
- the compiled scripts of all of these.

The template, the items and the conversation become the project's own,
and one undo takes them out again with the object. The compiled scripts
are written to the project's folder as the game has them, and an undo
leaves them there. The editor says what it copied and from which level.

In the window, the asset picker's **Other levels** check box adds these
templates to the list, each with its level after its name. Over the
agent link, `catalog.json` lists them with the name the game shows and
the level they are in.

A door and a waypoint are read whole into the level, so no template is
copied for them. Models and textures that only another level's files
hold are not copied.

`edit item new NAME BASE` also takes a base item that only another level
has.

**A creature made tougher.** `edit fights like TEMPLATE` makes the
selected creature fight as the creature of that template does. It takes
that creature's classes with their levels and Force powers, its feats,
skills and special abilities, its six attributes, its hit points and
Force points, its saves, its natural armour and its challenge rating.
It keeps its own name, look, faction, scripts and equipment. `TEMPLATE`
may be a creature of any level of the game. The message says the level
and the hit points the creature now has.

## A door's lock

A selected door shows its lock and its toughness in the details panel,
and `edit number LABEL VALUE` sets them. They are the door's own, kept in
its entry of the area's object list.

- `Locked` and `Lockable`: whether the door is locked, and whether it can
  be locked again.
- `KeyRequired`: Security cannot open it. Only its key or a script does.
- `OpenLockDC`: how hard the lock is to open with Security, from 0 to 255.
- `Plot`: the door takes no damage, so it cannot be bashed open.
- `Min1HP`: it is hurt down to 1 hit point and no further.
- `Hardness`, `HP` and `CurrentHP`: its toughness.

A door that only a puzzle opens has `Locked`, `KeyRequired` and `Plot`
all set to 1. A script then opens it, or the logic block that unlocks a
door.

## A project's own items and quests

**Items.** A project makes an item of its own as a copy of one of the
game's. `edit item new NAME BASE` copies the game's item `BASE` under the
name `NAME`. The copy keeps that item's kind, look and properties.

- `edit item name NAME TEXT` sets the name the games show.
- `edit item text NAME TEXT` sets its description. A datapad's text is
  its description.
- `edit item tag NAME TAG` sets the tag scripts find it by.
- `edit item number NAME LABEL VALUE` sets its worth (`Cost`), how many
  stack (`StackSize`), whether it can be sold or dropped (`Plot`), and
  whether it is known when found (`Identified`).
- `edit item drop NAME` takes it out of the project.

A placeable, a creature or a store takes the item by its name, as it
takes any item (`edit item add NAME`). The item is saved in the project's
folder as `NAME.uti`.

**What an item does.** An item's properties say what it does when it is
worn, wielded or used. A new item starts with its base item's properties.
Each property is a row of the game's table `itempropdef.2da`, such as an
attribute bonus or a limit to one alignment. Most properties have a
subtype, which says which attribute or which alignment. Most have a
value, which says how much. A few take a parameter as well.

- `edit item properties NAME` lists the item's properties among the
  messages, each after its place in the list, from 0.
- `edit item property NAME add ROW SUBTYPE VALUE [PARAM]` adds a
  property. `ROW` is the property's row of `itempropdef.2da`. `SUBTYPE`,
  `VALUE` and `PARAM` are rows of the tables that property names. A
  property without subtypes or without values takes 0 there. An item
  holds a property of one row and subtype once, so adding it again
  changes its value.
- `edit item property NAME remove N` takes out the property at place `N`.
- `edit item property NAME clear` takes out every property.

The editor checks each row against the game's tables and refuses one the
game does not have. Over the agent link, `properties` writes every
property with the rows of its tables (`docs/LEVEL-EDIT-AGENT.md`).

For example, a robe that only a dark side character wears, and that adds
3 to Wisdom:

    edit item new kje_shroud g_a_mstrrobe07
    edit item property kje_shroud clear
    edit item property kje_shroud add 43 3 0
    edit item property kje_shroud add 0 4 3

**The window.** The **Items and quests** button of the top bar opens a
window that lists the project's own items and its own quests. An item's
name, description, worth and properties are changed there, and a
property is added with pickers that list the game's tables by name. A
quest's entries are changed and added there too. Every change in the
window is one of the commands of this section.

**Quests.** The games keep every quest in one journal file. A project
that adds a quest keeps a copy of that file, `global.jrl`, with its quest
added.

- `edit quest new TAG NAME` adds a quest. The journal shows `NAME`.
- `edit quest entry TAG N TEXT` sets the quest's entry number `N`.
- `edit quest end TAG N TEXT` sets an entry that closes the quest.
- `edit quest show TAG` lists a quest's entries among the messages.
- `edit quest drop TAG` takes a quest out.

A conversation's line, a logic block or a script then adds an entry by
the quest's tag and the entry's number.

The journal is one file for the whole game. Two mods that each brought a
copy of it would take each other's place. So an exported mod does not
bring `global.jrl`. It brings only the project's own quests, in
`kjk-journal-<project>.jrl`, and kjk adds the quests of every mod to the
game's journal when it loads. The original game does not read that file,
so it does not show a mod's quests ("Sharing a level").

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
in the window. Below it the trial lists what the conversation's own
scripts called, under "what the conversation's scripts called". A door
that a line's script unlocks shows there as `SetLocked`, since the lines
themselves do not say it.

**A conversation in few commands.** Three commands build a conversation
without counting nodes by hand. Each answers the numbers of what it made.

- `edit conversation NAME say start TEXT` adds a line the conversation
  starts with. `say reply N TEXT` adds the line that answers reply N.
  `say line N TEXT` adds a reply to line N.
- `edit conversation NAME replies line N TEXT TEXT ...` adds several
  replies to line N at once, each text in double quotes: the choices of a
  question.
- `edit conversation NAME lead line TO from N N ...` leads each of those
  replies to line TO, which is shown elsewhere: the wrong answers of a
  quiz, which all get the same line.

A quiz of three questions with four choices each takes 14 commands this
way (`tools/level-editor/tomb-replay/commands.txt`).

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
(`%APPDATA%\kjk\editor-link` on Windows). An editor started with
`KJK_EDITOR_LINK=NAME` uses `editor-link-NAME`, so two editors open at
once each have their own:

- It writes `live.json` there four times a second, when anything in it
  changed. The file holds the project and the level, whether there are
  changes not saved, the selected object with its details and the command
  that selects it, everything else selected with it, the camera, the editor's last messages, the answers to
  the agent's last 20 commands, each with its number, the scripts that
  did not compile, what the last script tried in the editor printed, and
  whether an audit or a bake runs.
- It runs each line an agent writes to `inbox.txt` there as a command, in
  order, and then deletes the file. A line is one of the editor's own
  commands (the table below), with or without its `edit`, or one of
  the link's own. `camera X Y Z YAW PITCH` moves the view. `frame` looks
  at the selection. `list` writes everything the level holds to
  `objects.json`, each thing with the command that selects it. `shot`
  writes a picture of the window to `view.png`. `ground X Y` answers how
  high the level's surfaces are at that place. `catalog` writes every
  name a command can add to `catalog.json`. `level` writes what a dump
  says of the editor to `level.json`. `begin` and `end` make the edits
  between them one undo step. A value between double quotes can have a
  space in it. Each command's answer is
  `ok` or the reason it failed, and the editor's messages show it too.
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
| Ctrl+A | select every object, prop, collision box and lamp of the level, and every room of a new map |
| Ctrl+D | copy everything selected, in the same place |
| Delete | delete everything selected |
| End | put everything selected on the walkable floor under it |
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
undo takes all of it back. The editor keeps the last 200 edits for undo
and forgets the older ones. Every kind can be moved, copied and deleted,
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
| `edit select KIND N`, `edit select more KIND N` | selects, or adds to the selection, the Nth object of a kind (`creature`, `placeable`, `door`, `waypoint`, `trigger`, `sound`, `store`, `encounter`), from 0 |
| `edit select sun` | selects the area's sun |
| `edit sun bearing DEGREES`, `edit sun height DEGREES`, `edit sun colour R G B`, `edit sun multiplier M`, `edit sun radius METRES` | sets the area's sun, which becomes the project's |
| `edit sun add`, `edit sun game` | gives a level with no sun one; gives the game's sun back |
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
| `edit undo`, `edit redo` | takes back the last edit, or makes it again |
| `edit add KIND TEMPLATE [X Y Z]` | adds a `placeable`, `creature`, `door`, `waypoint`, `trigger`, `sound`, `store` or `encounter` made from the template, at the point or where the view's centre meets the level |
| `edit link MODULE [TAG]`, `edit link none` | leads the selected door or trigger to the module, arriving at the door or waypoint of that tag (the module's entry without one), or nowhere |
| `edit entrance MODULE door\|trigger N [TAG]` | leads door or trigger N of the module's entry area here, arriving at the door or waypoint of that tag (this level's entry without one) |
| `edit entrance drop AREA` | gives that area's object list back to the install |
| `edit entrance add MODULE TEMPLATE X Y Z [TAG [ARRIVAL]]` | adds a new trigger to the module's entry area, made from the install's trigger template, at that point of that area. It leads here, to the door or waypoint tagged `ARRIVAL`, or to this level's entry |
| `edit item new NAME BASE`, `edit item name\|text NAME TEXT`, `edit item tag NAME TAG`, `edit item number NAME LABEL VALUE`, `edit item drop NAME` | makes an item of the project as a copy of the game's item `BASE`; sets its name, its description, its tag or a number; takes it out |
| `edit item properties NAME`, `edit item property NAME add ROW SUBTYPE VALUE [PARAM]`, `edit item property NAME remove N`, `edit item property NAME clear` | lists the properties of one of the project's items; adds one by its rows of the game's tables; takes out the one at place `N`; takes out all of them |
| `edit fights like TEMPLATE` | makes the selected creature fight as the creature of that template does, with its classes, feats, attributes and hit points |
| `edit ui adventure`, `edit ui adventure off` | opens or closes the window of the project's items and quests |
| `edit ui elsewhere on`, `edit ui elsewhere off` | whether the asset picker also lists the templates only other levels have |
| `edit quest new TAG NAME`, `edit quest entry\|end TAG N TEXT`, `edit quest show TAG`, `edit quest drop TAG` | adds a quest to the project's journal; sets one of its entries, or one that closes it; lists it; takes it out |
| `edit conversation NAME screen talk\|computer\|rakatan` | shows the conversation as speech between characters, on a computer's screen, or on a Rakatan computer's |
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
| `edit script text NAME FILE` | gives the script the text of a file, making the script when the project has none of that name, and opens it in the viewer |
| `edit script write`, `edit script compile` | writes the scripts changed in the viewer; compile also compiles the project's scripts |
| `edit script debug NAME`, `edit script debug NAME TAG` | tries the script as `edit script try` does and records it line by line; the stops are under the viewer's Debug tab |
| `edit talk NAME`, `edit talk -` | gives the selected object the conversation, or none |
| `edit conversation new NAME`, `edit conversation copy NAME`, `edit conversation drop NAME` | makes a conversation of the project, copies the game's, or takes one out of the project |
| `edit conversation NAME add`, `edit conversation NAME add line\|reply N` | adds a starting line, or a node under line or reply N (a reply under a line, a line under a reply) |
| `edit conversation NAME text line\|reply N TEXT ...` | sets a node's own text (the words to the end of the command) |
| `edit conversation NAME say start TEXT`, `say line\|reply N TEXT` | adds a starting line, or a node under line or reply N, with its text in one step |
| `edit conversation NAME replies line N TEXT TEXT ...` | adds several replies to line N, each text in double quotes |
| `edit conversation NAME lead line TO from N N ...` | leads each of those replies to line TO, shown elsewhere |
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
| `edit number LABEL VALUE` | sets a number of the selected creature, store or encounter by its template field (`Str`, `Appearance_Type`, `MarkUp`, `MaxCreatures`), or of the selected door (`Locked`, `Plot`, `OpenLockDC`); a check box is 0 or 1 |
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
| `edit bake keep`, `edit bake drop` | keeps the bake the Baked view shows in the project, where play with the project is lit with it; takes the project's bake out |
| `edit bake renew` | saves, bakes, and keeps the new bake in the project when it is done |
| `edit snap door` | puts the selected door in the nearest doorway of the level's layout, facing as the doorway does |
| `edit align` | lines the selected thing up with the nearest thing within 1 metre along X and along Y |
| `edit measure A to B` | says how far apart two things or places are; each is a thing as `select` names it, or X Y Z |
| `edit save` | saves the project |
| `edit play` | saves, then starts `kjk play` on the project |
| `edit play here`, `edit walk here` | the same, the player starting where the view looks at the floor |
| `edit audit` | saves, then runs `kjk collision audit` on the project. The audit runs beside the editor, which says its result among the messages when it ends |
| `edit ui tool select`, `move`, `rotate`, `scale`, `measure` | picks the tool |
| `edit ui tab placeables`, `creatures`, `props`, `shapes`, `waypoints`, `triggers`, `sounds`, `stores`, `encounters`, `lights` | shows a tab of the asset picker |
| `edit ui search TEXT`, `edit ui find TEXT` | sets the asset picker's or the outliner's search |
| `edit ui snap on`, `edit ui snap off` | turns snapping on or off |
| `edit ui view SCRIPT`, `edit ui view` | opens the script viewer on the script, or closes it |
| `edit ui blocks EVENT`, `edit ui blocks` | shows the selected object's blocks on the event, or none |
| `edit ui link on`, `edit ui link off` | turns the agent link on or off |
| `edit ui debug N`, `edit ui break SCRIPT LINE` | the Debug tab's stop N (from 1); sets or clears a breakpoint |
| `edit ui tab doors`, `edit ui way_in MODULE [TAG]` | the asset picker's Doors tab; the level and arrival Ways in shows |
| `edit ui view window on`, `edit ui view window off` | the viewer in a window of its own, or under the details |
| `edit ui view tab source`, `summary`, `listing`, `try`, `debug` | picks the viewer's tab |
| `edit ui file open [PROJECT]` | opens the picker, or the project, in a new editor (a headless run names it instead) |
| `edit ui file level MODULE` | opens the project on its level MODULE in a new editor, adding the level first when the project does not have it |
| `edit ui file new NAME` | adds the new map NAME to the project, its light, fog and sound from this level, and opens it in a new editor |
| `edit mod version [TEXT]` | says the version an exported mod has, or sets it and saves the project |
| `edit ui quit` | closes the editor |
| `edit ui confirm save`, `discard`, `cancel` | answers the question asked before leaving with changes not saved |
| `edit add room MODEL [X Y Z]` | adds the install's room to a new map, its floor's middle at the point or where the view's centre meets the level |
| `edit add room MODEL home` | adds the room at the place its own level gives it, so rooms of one level fit together as they do there |
| `edit select room N`, `edit select entry` | selects a new map's room (from 0) or its entry |
| `edit seam` | moves the selected room to meet the nearest seam |
| `edit drop` | puts everything selected on the walkable floor under it, as one edit |
| `edit markers KIND on`, `edit markers KIND off`, `edit markers all on` | shows or hides the markers and labels of a kind of object (`creature`, `placeable`, `door`, `waypoint`, `trigger`, `sound`, `store`, `encounter`), or of every kind |
| `edit publish` | writes the project's own lights, lamps, kinds of fitting and room levels to kjk's light data, then saves the project without them |
| `edit export [FOLDER]` | saves, then writes the project as a mod of the game to the folder, or to `exports/<project>` in kjk's own folder |
| `edit recover`, `edit recover discard` | takes the work an editor that did not close left, as one edit, or removes it |
| `edit recover write` | writes the recovery copy now, not at its next turn |
| `edit entry [X Y Z]` | moves a new map's entry to the point or where the view's centre meets the level |
| `edit ui tab rooms` | shows the Rooms tab of a new map |

The picker takes `edit open GAME MODULE [PROJECT]`, `edit new GAME BASE
NAME` (a new map), `edit search TEXT` and `edit tab k1`, `tsl` or `ja`.
`edit open ja t1_sour` opens a Jedi Academy level in the rooms tool.

A dump (`dump NAME`) has an `editor` fact: the project, the counts of each
kind of object, every object with its template, tag, position and facing,
the selection (a placeable's or a creature's with its behaviour), the props
(with their bounds) and boxes, the sun, the depths of undo and redo, the last
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

## Work that is not saved

The editor keeps a recovery copy of work that is not saved, so a crash or
a power cut does not lose it. While the project has changes that are not
saved, the editor writes the copy once a minute to `recovery/<project>`
in kjk's own folder. The copy holds the project as a save would write it,
the level's lights and lamps placed by hand, and each script changed in
the script window. The project's own folder is not touched.

A save removes the copy. So does leaving the level by choice, with or
without saving. A copy is left only when the editor did not close.

When the editor opens a project that has a copy, a window says so, with
the level the copy is of and how long ago it was written.

- **Recover** takes the copy into the level as one edit. The level then
  has changes that are not saved, and nothing is written to the project
  until a save. Undo gives the saved level back.
- **Discard** removes the copy.

Until one of the two is chosen, the editor writes no new copy, so the
old one is not lost. Two editors open on one project at once share one
copy, and the second one offers the first one's work as if it had been
left behind.

## Placing things

**Drop to floor** (the End key, the details panel's button, or `edit
drop`) puts everything selected on the walkable floor under it. Each
thing goes straight down to the highest floor at or under it. A thing
with no floor under it goes up to the lowest floor above it. A collision
box rests its underside on the floor. The floor is the rooms' walkable
walkmesh and the tops of the project's boxes that are walked on, so
furniture, walls and roofs do not catch a dropped thing. The sun, the
game's own lights and a new map's rooms are not dropped.

The toolbar's **Markers** menu hides the markers and labels of a kind of
object, such as every waypoint, so a crowded level can be read. A hidden
kind still shows what is selected of it, and its models stay drawn.

With several things selected, the details panel says how far their
places span along X, along Y and in height. With two selected it also
says how far apart they are.

**Doors and doorways.** A level's layout says where each of its doors
hangs and which way it faces. A door added within 1.5 metres of such a
doorway stands in it, at the doorway's place and facing. The message says
so. With **Snap** ticked, a door let go within 1.5 metres of a doorway
does the same. The details panel's **Snap to doorway** button, or
`edit snap door`, puts the selected door in the nearest doorway however
far away it is. A new map's rooms bring the doorways of the areas they
come from, moved and turned with the rooms. A room joined to another
where its own level had a wall has no doorway there, and a door at such a
place is turned by hand.

**Lining things up.** With **Snap** ticked and the Move tool, a thing let
go lines up with what stands near it. It takes the X of the nearest thing
whose X is within 0.25 metres of its own, and the Y likewise. `edit align`
does it for the selected thing at any time, within 1 metre. Only things
on the same floor and within 30 metres count.

**Measuring.** The toolbar's **Measure** tool measures between two
places. Click one place of the level, then another. The view draws a line
between them and says how far apart they are, how far along the floor,
and how far up. A third click starts again. `edit measure A to B` says
the same among the messages. `A` and `B` are each a thing as `select`
names it, or a place as three numbers.

An edit or a command that does nothing says why in a red notice at the
top of the view for five seconds, as well as among the messages.

## Sharing a level

A project is shared as a mod of the game. **Export as a mod** (in the File
menu, `edit export`) saves the project and writes it to a folder, as
files to copy into the game's own folder. A player needs the game to use
it, as for any mod of KOTOR.

The folder is `exports/<project>` in kjk's own folder, or the one `edit
export FOLDER` names. An export replaces the one made there before. A
folder that holds other files is refused. The export has two parts.

- **Files the games read.** `override/` holds each level's object list
  (`<area>.git`), the templates, conversations and scripts the project
  brings, and an area file the project changed. A new map's module is in
  `modules/`. The original game and kjk both read these, as they read any
  mod.
- **Files only kjk reads.** The original game ignores them. They are
  named so that two mods in one `override/` do not take each other's
  place.
  - `kjk-area-<area>.toml` for each level: its collision boxes, Jedi
    Academy props, lights and lamps placed by hand, links between floors
    and sun, with the level's module and area named at its top.
  - `kjk-lighting-<project>.toml`: the kinds of fitting and room levels
    the project changed.
  - `kjk-bake/<module>/`: the bake the project keeps of a level.
  - `kjk-journal-<project>.jrl`: the project's own quests. kjk adds them
    to the game's journal when it loads.
  - `kjk-mod-<project>.toml`: the mod's name, its version, its levels, its
    quests, and every file it brings with its size and its digest. It
    also lists the files of other mods the project was built on (see
    "A project built on another mod" below).
  - `kjk-ja/<map>/` for each new map that places rooms cut from Jedi
    Academy levels. `rooms.toml` says which rooms the map places and
    where. Beside it is a copy of each level's cuts, such as
    `t1_sour.toml`, which names the level's file by its digest. See
    "A map with Jedi Academy rooms" below.

`kjk-mod-readme.txt` beside the two folders says how to install the mod,
and how many of its files the original game reads.

**A mod's version.** `edit mod version TEXT` sets the version the mod says
it is, and saves the project. It is 1 until it is set. `edit mod version`
says it. The version is kept in `project.toml`.

**A project built on another mod.** The editor reads the game as kjk
does, with the mods installed in it. A mod's loose file in the game's
override folder wins over the game's own file. A module's mod file
(`modules/<module>.mod`) wins over the module's own archives. A community
patch and a restored content mod install both kinds of file. So the
levels, templates, conversations and scripts the editor shows are the
installed mods' versions.

The asset picker tags what another mod brings. A tile with the tag
**override** is read from the override folder. A tile with the tag **mod**
is read from the level's mod file. The game's own files and the project's
own files have no tag. In `catalog.json` the same templates have the field
`from`, which is `override` or `mod`. A template of another level (the
**Other levels** check box) has no tag.

When a project is exported, the editor looks through every level of the
project. A level names its templates, and a template names its items, its
conversations and its scripts. Each of these files that the game reads
from the override folder or from a mod file, and that the project does not
bring itself, is listed in `kjk-mod-<project>.toml` under `[[needs]]`,
with its size and its digest. A file larger than 1 MB is listed with its
size alone. The answer to `export` says how many files are listed.

The listed files are not copied into the mod. A player installs the other
mod first. kjk reads the list at its start. It says when a listed file is
missing, and when a listed file differs from the one the project was
built on, which means the other mod is installed in another version. The
original game gives no such warning.

Two things are not looked for. A mod's models and textures are not listed,
because a level names them through the game's tables. A line of text that
only a mod's `dialog.tlk` has is not listed either.

**A map with Jedi Academy rooms.** The mod holds none of Jedi Academy's
files. It holds only where the map places each room, and the shapes the
rooms were cut with. When kjk starts without `--project`, it reads these
files and builds each room from the player's own Jedi Academy install,
as the editor does. Building t1_sour's hangar took 0.6 seconds.

- A player needs Jedi Academy installed to play such a map in kjk. The
  original game cannot play it at all, because its rooms are not in the
  module.
- The cuts name the level's file by its digest. A player whose file of
  the level is different gets no rooms from it. kjk then says so at its
  start: "the map kjk_jka_test has rooms of Jedi Academy's
  maps/t1_sour.bsp, which kjk could not build. The installed
  maps/t1_sour.bsp is not the file its rooms were cut from."
- The readme beside the mod says that it needs Jedi Academy.

**Removing a mod.** A mod is removed by deleting the files that its
`kjk-mod-<project>.toml` lists, and then that file.

**Two mods together.** kjk reads the list of every mod at its start, and
says:

- which mods it found, each with its version and how many files and
  quests it brings;
- when two mods list a file of the same name. The game reads the copy
  that was installed last, so one of the two mods is not whole;
- when a file of a mod is missing, or is not the one the mod brought;
- when a file of another mod that a mod was built on is missing, or is
  not the one it was built on;
- when two mods bring a quest of the same tag. The first one read is
  kept.

**Co-op.** Two machines that play together compare their game data. The
comparison now covers what the mods change beyond the game's own files:
every `kjk-area-*.toml` and every `kjk-journal-*.jrl`. A guest whose mods
differ from the host's there is refused, and the refusal names the group
`kjk mods`.

**Saves.** A save made in a level that a mod added does not load once the
mod is removed. kjk then says that the save's level is not one of the
game's own, and that the mod must be installed again.

**kjk reads a mod's own files from `override/` when it runs without
`--project`.** It takes every `kjk-area-*.toml` and `kjk-lighting-*.toml`
at the top of each game's `override/`, and every bake under `kjk-bake/`.
The levels they name then get their boxes, props, lights, lamps and bake,
as a project's levels do. kjk says at its start which levels it found.
With `--project`, kjk plays that project and reads no mod's own files.

What to know:

- **A mod holds changed copies of the game's files**, as mods of this
  game do: the object lists, the templates and conversations the project
  changed, a new map's rooms, and a kept bake.
- **Two mods that change the same area take each other's place.** Their
  `<area>.git` files have the same name, and so do their kjk files. Two
  adventures whose ways in are in the same level of the game are such a
  pair. kjk says so at its start.
- **A mod's quests show in kjk only.** The original game reads its own
  journal and nothing else.
- **In the original game, a level's object list from `override/` shows
  on a first visit.** Once a save holds the area, the game takes the
  list from the save.
- **What only kjk reads does not exist in the original game.** A level
  that needs its collision boxes or props to be passable is a level for
  kjk.
- **The project's folder is not the mod.** It also holds the project's
  own notes, which the export leaves out.

## Jedi Academy levels

A KOTOR area is made of rooms. A Jedi Academy level is one piece. Before a
Jedi Academy level can be used as rooms of a map, it has to be cut into
rooms. `kjk-level-editor ja LEVEL` opens a level for that, and so does
picking the level in the Jedi Academy tab of the editor's start screen.

```bash
kjk-level-editor                               # pick a level in the Jedi Academy tab
kjk-level-editor ja t1_sour                    # the project is named after the level
kjk-level-editor ja mp/ffa3 --project town     # the rooms are kept in the project "town"
```

The level opens as one room, called the rest of the level. It is shown lit
by its own stored light. The rest of the level is grey, and every room cut
from it has a colour of its own. The panel on the left has the tool and
lists the rooms, each with the triangles drawn in it.

**Cutting a room.**

1. Click the view. Each click places a point where it meets the level. A
   shape takes up to 32 points.
2. The points make a shape, drawn in the view in yellow as they are
   placed. The shape is the smallest solid without a dent that holds every
   point.
3. **Cut a new room** gives everything inside the shape to a new room.
   **Add to the selected room** gives it to the room selected in the list.

A shape has no dent, so a room with a bend is made of two shapes: cut the
first, then add the second to it.

**Adjusting a shape.** Until a shape is cut, its points can be moved and
added to. The point or the line under the pointer is drawn stronger.

- **Drag a point** to move it. The point stays at its own height and
  follows the pointer over that height.
- **With Shift held**, the drag moves the point up and down. A point can
  so be put in the air, where no click lands.
- **With Alt held**, the dragged point sticks to the level: it goes where
  the pointer meets a surface, as a click does.
- **Click a line of the shape** to place a new point on it. A drag that
  starts on a line places the point and moves it at once. The new point
  lies between the two points the line joins, so the shape stays as it is
  until the point is moved. A line of a shape that reaches up or down
  joins the raised or lowered copies of two points, and the new point is
  placed between those two points themselves. A line that stands on one
  point, from its lowered to its raised copy, takes no new point.

Points placed at different heights give a shape whose top and bottom are
not flat. They are made of triangles, and the lines between those
triangles are drawn faintly. Such a line is one where the two faces it
joins lie within 25 degrees of one plane. It is part of the shape like any
other line, but it takes no new point.

**How far a shape reaches.** A click lands on a surface. Points clicked on
a floor lie in one plane and have no inside, and a hall under an open sky
has no ceiling to click. So the shape also reaches from each point.

- **Reach up** is how far the shape reaches above each point. It starts
  at 8 metres.
- **Reach down** is how far it reaches below each point. It starts at 1
  metre.
- **Margin** moves every face of the shape out. It starts at 10
  centimetres, so that a wall the points were clicked on is inside.

With all three at 0 the shape is exactly the points, in any direction.

**What a cut takes.** A triangle that a shape's face crosses is cut there,
and each part goes to its own side. A hangar's floor may be one triangle
50 metres long, so without this a room's edge would follow the level's
triangles and not the shape. A static model is cut the same way.

**Later cuts take from earlier ones.** The cuts are kept in the order they
were made. A place belongs to the room of the last cut that holds it, and
to the rest of the level when no cut holds it. A small room can therefore
be cut out of the middle of a large one.

**The other buttons.**

- **Remove the last point** and **Clear the points** change the shape
  being placed.
- **Delete the room** takes the selected room out. Its space goes back to
  what it was cut from.
- The **Name** field names the selected room. Enter sets the name.
- **Undo** takes the last change back.
- A click on a row of the list selects a room, and the other rooms are
  dimmed. Ctrl and a click on the view selects the room there.

**Adjusting a room after it is cut.** The selected room lists its shapes,
in the order they were made, each with two buttons.

- **Adjust** places the shape's points again, with its reach, to be
  dragged and added to as before it was cut. The room keeps its old shape
  until **Apply to the shape** gives it the new one. **Cancel** leaves the
  shape as it was. While a shape is adjusted, no new room can be cut.
- **Take out** removes the shape. What it held goes back to what it was
  cut from. The room stays, with its other shapes.

**Seeing the level's textures.** The **Show** switch under the list picks
how the level is drawn. **Rooms in colour** colours everything by its room,
as above. **Textures** draws the level as the game draws it, with its
textures, its own light, its sky and its doors. Every room's shapes are
then drawn as outlines in the room's colour, the selected room's stronger.

**Where the rooms are kept.** Every change is saved at once, as
`ja-rooms/<level>.toml` in the project's folder. The file holds the rooms'
names, each cut's points and reach, the level's path, and the digest of
the level's file. It holds nothing of the level itself, which stays in
Jedi Academy's install. A point is a place in the file it was placed in,
so rooms made for another file of the same name are refused.

**Commands.** `edit point X Y Z` (metres), `edit move POINT X Y Z`,
`edit edge POINT POINT [SHARE]` (a new point between two, half way
unless a share from 0 to 1 is given), `edit points undo`,
`edit points clear`, `edit up METRES`, `edit down METRES`,
`edit margin METRES`, `edit cut [NAME]`, `edit add ROOM`,
`edit delete ROOM`, `edit rename ROOM NAME`, `edit select [ROOM]`,
`edit adjust ROOM N` (the room's Nth shape), `edit apply`, `edit cancel`,
`edit unshape ROOM N`, `edit view textures` or `edit view rooms`, and
`edit undo`. A room is named by its number in the list or by its name. A
point is named by its number, from 1, in the order the points were
placed. `dump` writes the rooms as the fact `ja_rooms`.

### Jedi Academy rooms in a new map

A room cut from a Jedi Academy level can be placed in any new map. When
a map is opened, the Rooms tab lists every room of every level that any
project keeps rooms of, the rest of each level among them.

- A level the map's own project keeps rooms of is listed from that
  project alone.
- A level that only other projects keep is listed from the project whose
  rooms of it changed last. Its rooms show that project's name, such as
  "t1_sour hangar (Jedi Academy, project t1_sour)".

Saving a map that places a room cut in another project copies that
level's cuts file (`ja-rooms/<level>.toml`) into the map's project. The
file holds only the shapes and the rooms' names. The map's project then
keeps everything it needs, and plays without the other project. Later
changes to the other project's cuts do not reach the map. To change the
map's rooms, open the level with the map's project named:
`kjk-level-editor ja t1_sour --project my_map`.

**What the map keeps.** The map keeps a Jedi Academy room as the level's
path and the room's name (`ja_level` and `ja_room` in `kjk-map.toml`). It
keeps none of the room's geometry, textures or light. The module that
saving writes (`modules/<name>.mod`) leaves the room out too. So a map
holds nothing of Jedi Academy's files, and a player needs their own copy
of Jedi Academy to play it.

**How kjk builds the room.** Each time the editor or the game opens the
project, kjk reads the level from the Jedi Academy install and builds
each room the map needs as a KOTOR room, in memory. No file of it is
written. The room is built as follows.

- Its model holds the room's part of the level, as the rooms tool's
  **Textures** view draws it, with the level's textures and its own stored
  light. A surface the level draws from both sides is drawn from both
  sides. An animated texture shows its first picture.
- Its collision is every drawn triangle of the room, as for a KOTOR room. A
  surface that a body passes through in Jedi Academy, such as water or a
  hanging cloth, is left out of the collision.
- Its walkmesh is the level's floor inside the room. The level's floor is
  every surface that faces up and stops a body. All of it is walkable, and
  the game plays its footsteps as stone.
- Its navigation mesh joins its floors as the level's own navigation mesh
  does. Floors whose edges meet within a step's height are joined, and so
  are floors that a gap of 30 centimetres or less parts. An area holding
  any Jedi Academy room joins all of its floors this way, its KOTOR rooms'
  too.

Building t1_sour's two rooms takes under a second. The rooms' names,
such as `ja8c722675cf2396`, are made from the level's path and the
room's name. Renaming a room in the rooms tool therefore takes it out of
every map that placed it.

**Seams.** A Jedi Academy room meets another room as a KOTOR room does.
Two rooms cut from one level and both placed at their home places
(`edit add room MODEL home`) meet along their cut, and a character walks
from one into the other. t1_sour's hangar and the rest of the level so
placed give the same navigation mesh as the whole level: 482 pieces, and
the hangar's piece is the largest. A Jedi Academy room also meets a
KOTOR room where an edge of one's floor lies on an edge of the other's,
which "Meet a seam" finds. A Jedi Academy room's floor has many short
edges, so "Meet a seam" can find an edge nearer than the one meant. In a
test, the hangar moved 67 centimetres off its home place met a seam 44
centimetres from home. Rooms of one level are therefore best placed at
their home places.

**Sharing the map.** **Export as a mod** takes the map's Jedi Academy
rooms with it, as their places and their cuts, and none of Jedi
Academy's files ("Sharing a level", "A map with Jedi Academy rooms").

**What a Jedi Academy room does not have yet.** Its doors and lifts are
drawn shut and do not move, and they block the way. Its sky is not drawn.
Its walkmesh's outline gets KOTOR's walls, so a character cannot jump
onto a ledge higher than a step, except onto furniture under a roof
(`collision_odyssey::edges`). Its light is Jedi Academy's own and does not
match the KOTOR rooms' light yet.

## Limits of the prototype

- **A new map's rooms are the games' own.** There is no new geometry
  yet, and a room is not resized. Rooms from different
  areas carry their own baked light, so a mixed map shows seams in its
  lighting.
- **A new map is played in kjk only.** Its module has no AI paths (PTH),
  no map picture, and walkmeshes whose edges name no neighbouring room,
  which the original game needs. Every room sees every room.
- **KOTOR 2's rooms are not listed yet.** Its layouts are in its modules'
  archives, which the Rooms tab does not read.
- **Making a bake needs kjk built from source.** A project keeps its
  own lights, lamps, kinds of fitting, room levels and a bake, and play
  with the project uses them. But a bake is made by the `kjk` that has
  `detail-bake`, in the kjk checkout, so an editor from a release package
  cannot make one.
- **Lights without a bake do not show in play by themselves.** A level
  whose project has its own lights or kinds but keeps no bake is lit as
  the game stores it, unless `KJK_FITTED_LIGHTS` is set. The live fitted
  lights are still a trial.
- **Props are not in the navigation mesh.** A solid prop is solid for
  every body, but the game's characters do not path onto it or around it.
  A walkable box or a nav blocker placed with it does that.
- **Props are lit by the floor under them, not by their lamps.** A KOTOR
  room is lit by its lightmaps, which a Jedi Academy prop does not have
  (below, "How props are lit"). A prop takes the room's light where it
  stands as a whole, so a prop beside a lamp is not brighter on the side
  that faces it, except from the live fitted lights.
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
  out of a project yet.
- **An exported new map is for kjk.** Export writes a new map's `.mod`
  beside the files for `override/`, but the module lacks what the
  original game needs to play it, as the limit on new maps above says.
- **The view is drawn under the panels.** The scene's camera fills the
  window, and the panels cover its edges.

## Future work

These come after the prototype: navigation that includes the added
props, export of a new map for the original game (its walkmeshes'
neighbours, AI paths and a map picture),
rooms made in Blender, markers for vehicle
routes and spawn points for vehicles and jetpacks, and editing an area
together in co-op.

## Checks

- The approved test `level_edit_round_trip` (`docs/TESTS.md`) edits a GIT
  made by the test, saves the project to a temporary folder and reads it
  back. It needs no install.
- `level_edit_project_scripts` compiles a project's scripts in a folder the
  test makes, and `script_vm` runs scripts compiled from source.
- `level_edit_sun` turns and raises the area's sun round the level's
  middle, keeps it in the area file, and gives the game's back with undo.
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
- `level_edit_light_rules` changes a kind of fitting and a room's level
  in rule files the test makes, and undoes both in the editor. `level_edit_fitted_lights` places,
  changes and saves lights placed by hand.
- `level_edit_nav_edits` checks what walkable boxes, nav blockers and nav
  floors do to the navigation mesh. `level_edit_new_map` makes a new map,
  adds and moves its rooms, and saves its module.
- `tools/level-editor/tomb-replay/replay.py` is a benchmark, not an
  approved test. It needs KOTOR 1 installed. It rebuilds a whole level, the
  Tomb of Vessk Tarn, through the agent link from about 220 commands: 17
  rooms, a quest, two items, a sealed door, a terminal's quiz, a guardian
  and seven lights. It passes when every command is answered ok, the
  level's facts are the expected ones, a trial of the quiz opens the seal,
  and the level exports as a mod. A run takes about 35 seconds.
- `tools/level-editor/big-level/measure.py` measures how the editor
  holds a big level. It fills a new map with 1,000 objects, 200 lights and
  50 conversations through the agent link, then reads the editor's own
  timings. It passes when the editor draws 60 frames a second while idle,
  a save takes under 2 seconds, and the scene is drawn again after an
  edit in under 1 second. It needs a window.
- `tools/level-editor/tomb-speedrun/run.py` plays the tomb with no
  person at the controls. `kjk playthrough` walks `tomb.route`: into the
  tomb from the Valley of the Dark Lords, the datapad, the carvings, the
  terminal's quiz, the guardian and the reward. It passes when every goal
  is met, the tomb's quest is closed, and the party carries the reward.
- `tools/level-editor/three-maps/build.py` is a benchmark too. It builds
  the Three Halls: three new maps and a level of the game in one project.
  With `--play` it installs the exported mod into a stand-in for the
  game's folder, and `kjk playthrough` walks from Dreshdae into the hub,
  to each wing and back, and out again.
- `tools/level-editor/two-mods/check.py` installs the tomb and the Three
  Halls side by side in a stand-in for the game's folder. It passes when
  the journal has both mods' quests, both mods play, kjk names a replaced
  file, and the Three Halls still plays after the tomb is removed by its
  own list of files. A run takes about two minutes.
- The demo (`LEVEL-EDIT-DEMO.md`) shows every behaviour on the Ebon Hawk,
  by hand in a window and as headless runs whose dumps say what changed.
