# The level editor's demo

This demo shows the level editor (`LEVEL-EDIT.md`) on KOTOR 1's Ebon Hawk
(`ebo_m12aa`). It moves a metal box, adds a Jedi Academy crate and a ledge
of invisible collision, saves the project, plays it, and jumps onto the
ledge. It needs KOTOR 1 and Jedi Academy installed. It writes only the
project's folder, never a game install.

The demo can be done by hand in a window, or run headless. The headless
runs write dumps that say what changed, and screenshots.

## By hand

1. Build both programs, then open the editor on the Ebon Hawk, in a project
   named `demo`.

   ```bash
   cargo build --release -p launcher --bin kjk -p level_editor --bin kjk-level-editor
   kjk-level-editor k1 ebo_m12aa --project demo
   ```

   The Ebon Hawk opens at its entry, the cockpit, with the toolbar, the
   outliner, the details panel and the asset picker around the view. Hold
   the right mouse button to look, and fly with W, A, S, D, Q and E while
   it is held.

2. In the outliner, open Placeables and double-click `MetalBox
   (metalbox001)`. The camera moves to the first metal box in the cargo
   hold, and the details panel names it.

3. Press W for the Move tool and drag the blue arrow up by half a metre.
   Press E for the Rotate tool and drag the ring a quarter of the way round.
   Press Ctrl+Z and Ctrl+Y to undo and redo the turn. The details panel's
   numbers follow the gizmo, and can be typed instead.

4. In the asset picker, open the Jedi Academy props tab and type `crate` in
   its search box. Drag `factory/s_crate_01` into the view, onto the
   floor of the hold. The crate stands where it was dropped.

5. Open the Collision shapes tab and drag the Ledge into the view, onto a
   clear patch of floor. A see-through cyan box appears. Press W and drag
   its blue arrow up until it hangs about 1.5 m over the floor. Press R and
   drag its handles to make it 2 m wide. It is now a ledge too high for a
   plain jump.

6. Tick Collision in the toolbar to see the collision. Walkable floor is
   green, other solid surfaces are orange, and the walls along the floor's
   open edges are blue. Untick it again.

7. Press Audit. The editor saves the project and runs the containment
   audit. The details panel lists, for walking, jumping and each Force jump
   level, how many regions a body reaches off the walkmesh, and how many of
   them are escapes from the level. The ledge and the crate are new perches,
   and there are no escapes.

8. Press Play. The editor saves and starts `kjk play` on the project, as a
   level 6 Jedi. Walk to the cargo hold. The crate stands where it was put,
   and the metal box floats where it was lifted. Run at the ledge, and jump
   just before it while holding the jump key: the Force jump carries the
   player up onto the invisible ledge.

## Headless

These are the runs the demo was checked with. `KJK_PROJECTS` keeps the
project in a folder of the reader's choice. On Windows, in PowerShell:

```powershell
$env:KJK_PROJECTS = "$env:TEMP\kjk-projects"
```

The edits. The camera stands in the cargo hold, looking at the place of
the ledge.

```bash
kjk-level-editor k1 ebo_m12aa --project demo --headless --cmds 'wait 5f; camera 31.5 29.5 3.6 85 -20; edit select MetalBox; edit nudge 0 0 0.5; edit turn 45; edit prop models/map_objects/factory/s_crate_01.md3 29.2 33.4 1.8; edit turn 90; edit box 31.6 34.8 3.4 2.0 2.0 0.3; wait 20f; screenshot demo-edit; edit audit; wait 2f; dump demo-edit; quit'
```

The run folder's `demo-edit.png` shows the crate, the ledge and the
markers. In `demo-edit.json`, the `editor` fact has the first metal box at
a height of 2.3 m, facing 45 degrees, one prop and one box. Its messages say
that 3 files were saved and give the audit's count. On 2026-11-19 the
audit found no escapes and 35 perch regions at the highest reach. The
same area without the project has 32.

The jump. The Jedi starts 3 m in front of the ledge, runs for 0.4 s, then
holds jump with no direction for 0.9 s.

```bash
kjk --project demo play --stage ebo_m12aa --class jedi --at 31.6,30.0,1.8,90 --moves '300ms; 400ms w; 900ms jump; 2s' --headless --cmds 'wait world; wait 4s; dump demo-play; quit'
```

In `demo-play.json`, the `project` fact gives the player's position as
about (31.82, 34.64, 4.16). The ledge's top is 3.55 m high, and a standing
body's position is 0.61 m above its feet, so the player stands on the
ledge. The same run with `--class soldier`, whose jump has no Force, ends
on the floor at a height of 2.41 m, in front of the ledge. The same run
without the box (a project with no boxes) ends on the floor beyond the
place where the ledge would be.

The crate is solid too. A Jedi that starts beside it and runs and jumps
forward (`--at 31.6,32.6,1.8,90 --moves '300ms; 800ms w jump; 700ms w; 2s'`,
with the crate at 31.6, 34.6) stands on the crate's footprint 1.1 m above
the floor. Without the project, the same run ends on the floor.

## A new map

Four rooms of the Ebon Hawk, put together again as a map of their own,
with one placed 0.6 m off and moved onto its seam.

```bash
kjk-level-editor k1 kjedemo --new-map ebo_m12aa --title "Hawk remix" --headless --cmds 'wait 5f; edit add room m12aa_01b 0 0 0; edit move 0 0 0; edit add room m12aa_01o 0 0 0; edit move -13.0 -4.1 0.2; edit seam; edit add room m12aa_01f 0 0 0; edit move -18 -18 0; edit add room m12aa_01p 0 0 0; edit move -11.7 10.8 0; edit entry -4.8 -3.8 1.8; edit save; quit'
kjk --project kjedemo collision nav k1 kjedemo
kjk --project kjedemo collision walk k1 kjedemo
kjk --project kjedemo play --stage kjedemo --class jedi
```

After `edit seam`, the second room stands at (-13.5, -4.5, 0), where the
Hawk's own layout has it beside the first. The navigation mesh of the
four rooms is one island of 110 faces, and 50 of 50 walks between drawn
points arrive. The play starts in the first room, at the entry.
