# Findings on characters and placeables under the bake (22 November 2026)

This file was written from memory after the Mac crashed for the second time. The session stopped in the middle of its measurements. Only unk_m44ac was measured. Neither 411dxn nor danm13 was shot with the debug view, and no fix was made the normal behaviour.

What is committed: `1eb9184fc` "Fitted lights' buffer for the fragment stage only on Metal" holds the `material.rs` change of the first agent and nothing else. Everything else below is uncommitted in the worktree.

Where the files are: the pictures, poses and bakes of this session are under `target/light-kinds/game/bake-unk_m44ac/` in this worktree, and they should have survived the reboot. The scratchpad with the scripts is gone, so section 6 gives the scripts again.

## 1. The sun bake of unk_m44ac

### How the sun was found

The bake of unk_m44ac has one room, `m44ac_01a`. Its levels file is `levels-mac1.txt`, which holds the lit level 0.6012. Its lights dump is `lights-mac1.txt`. Neither file was changed.

I ran `detail-bake unk_m44ac --game k1 --lights lights-mac1.txt --gpu --sun-fit --out bake/sunfit` from the area's folder. It took 87 seconds, of which 73 seconds were the sun fit. A first try was killed at 60 seconds before it printed anything. The sun fit read 1,061,437 texels under the open sky. The best way to the sun was 0.837, -0.224, 0.500, with a fit of 0.826. The stored colour in the sun was 0.904, 0.925, 0.905, and out of the sun 0.311, 0.370, 0.432. The next best ways fit at 0.743 and lower. The room has 66% of its upward texels under the sky.

The colours come from the recipe in `handover.md` ("The Sand People Enclave"), with the factor 1.0, because the module's name does not start with `tat_`. Each stored level is taken to the power 2.2. The sun's colour is the level of the difference between the sunlit and the shaded value, divided by the sun's height of 0.5, taken back to the power 1 / 2.2. The sky's colour is the level of 1.6 times the shaded value. This gives a sun of 1.183, 1.188, 1.123 and a sky of 0.385, 0.458, 0.535. The sun's red, 1.18, is the number that `handover.md` gives for the batch's sun of unk_m44ac. So this is very likely the same sun the Windows batch used. The batch rule also gives `width = 0.01` and no even light.

### The bake

The command was run from `target/light-kinds/game/bake-unk_m44ac`:

```
kjk detail-bake unk_m44ac --game k1 --lights lights-mac1.txt --gpu \
  --sun "0.837 -0.224 0.500 1.183 1.188 1.123 0.385 0.458 0.535 0.01" \
  --no-even --weigh --levels levels-mac1.txt --out bake/sun1
```

It took 20 seconds. The room's set `b` came out at 0.6592 against its target of 0.6012, with the lamps at the least scale of 0.5. The log is `bake-sun1.log`, and `sun1-args.txt` holds the arguments. `KJK_DETAIL_BAKE` is `bake/sun1/m44ac/b`. No `bake.toml` entry was added.

### How close it is

The picture at the report camera of `200333` is `shots/sun1/rep2-shade.png`. The framing is the same as the owner's screenshot, and the cylinder is the same bright cyan-white. The gravel to the left is dark and the stone floor to the right is lit, as in the screenshot. The sharp shadow of the pillar on the floor, which runs from the pillar's foot to the bottom left of the owner's screenshot, is weaker and less sharp in my picture. The lit gravel patch in the bottom middle of the screenshot is grey in mine. I judge it close enough to judge the owner's report on, but not the same. The Mac bake without a sun (`bake/mac1`) is flat and grey, as the coordinator said.

## 2. Measurements under the bake

All of them are in unk_m44ac under `bake/sun1`, at the poses of `m-poses.txt` (copied to `all-poses.txt`). The pose `gwing` found no object in the mask picture, so it has no numbers. Pictures are 960 by 540.

The measure: the object is the magenta region of the `mask` picture nearest the picture's middle. The floor is a band of rows 6 to 40 pixels below the object's lowest row, over the object's columns, without magenta. Each number is a mean luma (0.2126 R + 0.7152 G + 0.0722 B, 0 to 255). The ratio is object over floor.

In a term picture the model shows only that term, with no texture. The room shows its whole light with no texture, so the floor number of a term picture is the floor's light, not its textured colour. In the textured pictures (today and baked) both are textured.

### Today and the bake before any fix (measured)

"Today" is `KJK_FITTED_LIGHTS=off`, shot with the same build (`shots/s0-off`). "Baked" is `shots/s0-tex`.

| Pose | Today object / floor | Today ratio | Baked object / floor | Baked ratio |
|---|---|---|---|---|
| cyl-console | 127.3 / 66.8 | 1.91 | 173.1 / 67.7 | 2.56 |
| cyl-corner | 177.5 / 23.4 | 7.57 | 183.9 / 12.7 | 14.50 |
| cyl-shade | 118.6 / 62.2 | 1.91 | 173.8 / 67.9 | 2.56 |
| plc-plc_rakatcom | 52.7 / 76.4 | 0.69 | 60.0 / 83.4 | 0.72 |
| rep1-cylinders | 178.7 / 24.6 | 7.26 | 186.3 / 14.0 | 13.26 |
| rep2-shade | 115.6 / 61.0 | 1.90 | 171.6 / 66.6 | 2.58 |

### Each term under the bake before any fix (measured)

Each cell is the object's luma, then the ratio to the floor's whole light in the same picture. The floor's whole light was 83.5 at cyl-console, 30.3 at cyl-corner, 91.8 at cyl-shade, 105.2 at plc_rakatcom, 33.1 at rep1-cylinders and 88.5 at rep2-shade.

"lamps, old" was shot with the first agent's build, whose lamps term left out the ambient-only lamps. "lamps, new" was shot after I added them to the term (section 4); only its ratio was noted.

| Pose | whole | probes | above | sides | lamps, old | lamps, new | fitted | glow | floor |
|---|---|---|---|---|---|---|---|---|---|
| cyl-console | 236.2, 2.83 | 72.9, 0.87 | 0.1, 0.00 | 72.9, 0.87 | 27.8, 0.33 | 2.33 | 0.1, 0.00 | 0.0, 0.00 | 37.6, 0.45 |
| cyl-corner | 246.4, 8.11 | 17.1, 0.57 | 0.0, 0.00 | 17.1, 0.57 | 125.7, 4.14 | 6.77 | 65.7, 2.16 | 0.0, 0.00 | 37.6, 1.24 |
| cyl-shade | 238.8, 2.60 | 87.1, 0.95 | 0.2, 0.00 | 87.1, 0.95 | 23.2, 0.25 | 2.07 | 0.2, 0.00 | 0.0, 0.00 | 37.6, 0.41 |
| plc-plc_rakatcom | 235.9, 2.24 | 57.4, 0.55 | 0.1, 0.00 | 57.4, 0.55 | 68.4, 0.65 | 2.03 | 0.1, 0.00 | 0.1, 0.00 | 33.7, 0.32 |
| rep1-cylinders | 246.4, 7.41 | 16.9, 0.51 | 0.1, 0.00 | 16.9, 0.51 | 125.4, 3.78 | 6.51 | 70.8, 2.14 | 0.0, 0.00 | 37.6, 1.13 |
| rep2-shade | 238.7, 2.70 | 89.6, 1.01 | 0.1, 0.00 | 89.6, 1.01 | 20.8, 0.24 | 2.13 | 0.1, 0.00 | 0.0, 0.00 | 37.6, 0.43 |

The light from above is zero on every model in the area. A dump (`dump d1` at cyl-shade, `dump d2` at cyl-corner) showed `"up": 0.0` for every model and `without_lamps` setting slot 2 to 0, 0, 0 while the raw probes held 44 to 50 there. The dump also showed a lamp of radius 5000 metres in the light list (light 7, shown in the shadow facts at about 773, 491 in Bevy space), and `light_types` showed one ambient-only lamp among 18. I infer that the ambient-only lamp is the radius 5000 one. `without_lamps` takes an ambient-only lamp out of the light from above at full strength, so it empties it everywhere. The shader then adds the same lamp to the ambient sum, which `BAKED_GAIN_PROBES` doubles.

### Each `KJK_MODEL_FIX` combination, textured ratio (measured)

All shot under `bake/sun1` with the build that has the switches of section 4 (without `ambout`, which came later).

| Pose | today | baked | nofitted | floor | nofitted,floor | nolamps | sides | lowfloor | all five |
|---|---|---|---|---|---|---|---|---|---|
| cyl-console | 1.91 | 2.56 | 2.55 | 2.55 | 2.55 | 0.97 | 2.56 | 2.55 | 0.97 |
| cyl-corner | 7.57 | 14.50 | 14.12 | 14.53 | 14.11 | 4.57 | 14.50 | 14.48 | 2.05 |
| cyl-shade | 1.91 | 2.56 | 2.56 | 2.56 | 2.56 | 1.13 | 2.56 | 2.56 | 1.13 |
| plc-plc_rakatcom | 0.69 | 0.72 | 0.72 | 0.72 | 0.72 | 0.27 | 0.72 | 0.72 | 0.27 |
| rep1-cylinders | 7.26 | 13.26 | 12.84 | 13.26 | 12.84 | 4.43 | 13.26 | 13.22 | 2.01 |
| rep2-shade | 1.90 | 2.58 | 2.57 | 2.57 | 2.57 | 1.14 | 2.58 | 2.56 | 1.14 |

"All five" is `nofitted,floor,nolamps,sides,lowfloor`. While the lamps are on, every other fix changes almost nothing, because the model's light is held at full (`min(1, …)`) by the lamps. With the lamps out, the other fixes matter: the corner cylinders go from 4.4 to 2.0.

I looked at the pictures of "all five" (`shots/s0-tex-nofitted+floor+nolamps+sides+lowfloor`). The cylinders no longer glow. But every model is now too dark. The Rakatan chair in the sun is dark brown (0.27 against 0.69 today). The cylinder in the shade has a dark top. The three corner cylinders are darker than the walls beside them. Taking all the lamps out is too much.

### Shot but not measured

These were shot just before the crash and were never measured. They may be incomplete: `shots/s0-tex-ambout`, `shots/s0-tex-nofitted+floor+ambout+sides+lowfloor`, and the term pictures `f1-probes`, `f1-above`, `f1-sides`, `f1-lamps` (all five fixes) and `f2-probes`, `f2-above`, `f2-sides`, `f2-lamps` (the five fixes with `ambout` in place of `nolamps`). Use `s0-mask` as the mask for all of them.

## 3. What makes each reported object too bright

- **The cylinders lit by corner light (report 200253, pose rep1-cylinders).** Measured: the lamps term is 6.5 times the floor's light, against 0.5 for the probes and 2.1 for the fitted lights. Inferred: most of the lamps term is the ambient-only lamp of radius 5000 metres, doubled by `BAKED_GAIN_PROBES`. The fitted lights are the second cause, since the corner's ceiling lamps are counted again on the model (measured: 2.1 times the floor). Taking the lamps out brings the ratio from 13.3 to 4.4, and taking the fitted lights out as well brings it to 2.0 (measured).
- **The cylinder in the shade (report 200333, pose rep2-shade).** Measured: the lamps term is 2.1 times the floor's light, against 1.0 for the probes. The fitted lights are zero there, because no fitting is near. Inferred: the cause is the same ambient-only lamp of radius 5000 metres, which lights the model the same whether it stands in sun or shade. The probes' light from above is zero there (measured), so the floor's shade never reaches the model.
- **The crates in the corners of 411dxn (report 222059).** Not measured. Inferred from the code and the corner cylinders of unk_m44ac: the fitted lights counted again on the model, and the game's lamps added on top of the bake. 411dxn's lamp list has not been looked at, so whether it has an ambient-only lamp is not known.

## 4. The trial switches in the uncommitted code

`KJK_MODEL_FIX` is a list of names with commas between them. `trial_fix(name)` in `crates/game/src/light_probes.rs` reads it. The shader gets the bits in `ProbeList::fixes`, which `light_objects` fills. All the switches act only under `KJK_FITTED_LIGHTS=baked` (`shadows.debug == 8` in the shader, `fitted_lights::baked()` in Rust). The shader's switches act only on a model that took the probes' light (`from_probes`).

| Name | Bit | What it does | Where |
|---|---|---|---|
| `nofitted` | 1 | A probed model takes no fitted light. The bake holds the fittings' light already. | `fragment` in `odyssey.wgsl` (`skip_fitted`) |
| `nolamps` | 2 | A probed model takes none of the area's lamps, neither the directed nor the ambient-only ones. A fight's light still lights it. The probes keep their light, since no lamp is taken out of them (`without_lamps` is not called). | `fragment` in `odyssey.wgsl` (`lamps_out`, in the lamp loop) and `light_objects` in `light_probes.rs` |
| `lowfloor` | 4 | The least probe light is `PROBE_FLOOR / BAKED_GAIN_PROBES`, so that after the doubling it is 0.35 of the flat ambient, not 0.7. | `fragment` in `odyssey.wgsl` (where `area_ambient` is set, and in `term_floor`) |
| `ambout` | 8 | A probed model takes none of the ambient-only lamps, and those lamps are not taken out of the probes. The directed lamps stay at half and are taken out of the light from above as before. | `fragment` in `odyssey.wgsl` (lamp loop) and `light_objects` in `light_probes.rs` (the list given to `without_lamps`) |
| `floor` | none | The light from above is the mean of the floor's lightmap within 0.5 metres of the model's origin (`AreaProbes::floor_under`, `FloorLight`, made by `floor_rooms` at load under the bake). The first agent wrote it. | `light_objects` in `light_probes.rs` |
| `sides` | none | The probes see each room surface at the light the screen shows it with. Every surface's glow is halved. A surface with no lightmap takes a flat lightmap of 0.25 (half of full light, at the bake's storage of one half), or 0.06 in an area whose bake marks its unlit rooms dark. | `shown_as_baked`, called from `bake` in `light_probes.rs` |

The debug view `KJK_MODEL_LIGHT` was changed in one way: its `lamps` term now holds the ambient-only lamps too, times `BAKED_GAIN_PROBES`, kept in `lamp_ambient` in the lamp loop. Without that, the term left out the largest part of the light in unk_m44ac.

My judgement of which should become the normal behaviour under the bake:

- `nofitted`: yes. The bake holds the fittings, so adding them on the model counts them twice (measured 2.1 times the floor in the corner).
- `floor`: yes, because it follows the bake's shade under the model. It was not measured on its own with the lamps out, so its effect is not known yet.
- `sides`: yes in principle, since it makes the probes see what the screen shows. Not measured with the lamps out.
- `lowfloor`: probably yes. Not measured with the lamps out.
- `ambout`: probably yes, in place of `nolamps`. It removes the inferred main cause and keeps the directed lamps, which give the model some direction. It was shot but not measured.
- `nolamps`: no. It makes every model too dark (measured: the chair at 0.27 against 0.69 today), because the probes' sides see the walls' drawn colour and so miss most of the sun's direct light on a model's sides.

## 5. The uncommitted change in `crates/game/src/viewer.rs`

`module_scene` now calls `light_probes::bake(&models, &rooms, &textures, dark_unlit(module))`. The new fourth argument tells the probes' bake whether the area's bake asks for its unlit room surfaces to be dark (`unlit.txt` in the set, as for 305nar). The `sides` switch needs it to give such surfaces 0.06 in place of 0.25. Nothing else reads it.

## 6. The scripts

`shoot.sh` and `poses.py` were named in my brief, but they were not in the scratchpad when this session started. I wrote my own scripts in their place. Pillow was only in `/opt/homebrew/bin/python3.14`. `/opt/homebrew/bin/python3` is 3.15 and has no Pillow. In the text below, `$S` is the scratchpad folder and `<root>` is the worktree's root.

### run-capped.sh (written by the earlier agent)

```sh
#!/bin/sh
# run-capped.sh <seconds> <log> <command...>: runs the command, kills it by PID after <seconds>.
limit=$1 log=$2; shift 2
start=$(date +%s)
"$@" > "$log" 2>&1 &
pid=$!
while kill -0 $pid 2>/dev/null; do
  if [ $(( $(date +%s) - start )) -ge "$limit" ]; then
    kill -9 $pid; echo "KILLED after $limit s"; break
  fi
  sleep 1
done
wait $pid 2>/dev/null
echo "exit $? after $(( $(date +%s) - start )) s"
```

Note: it kills the shell of `bake-shoot.sh`, not the game it started. A game that hangs past the limit keeps running.

### watchdog.sh (written by the earlier agent, ran all session as `watchdog.sh 120`)

```sh
#!/bin/sh
# Kills any `kjk detail-bake` process older than LIMIT seconds, and prints a
# line when memory is low or when macOS writes a new WindowServer hang report.
LIMIT=${1:-120}
SEEN=$(mktemp -d)
reports() { ls /Library/Logs/DiagnosticReports 2>/dev/null | grep -c WindowServer; }
known=$(reports)
while true; do
  now=$(date +%s)
  for pid in $(pgrep -f "kjk detail-bake"); do
    first="$SEEN/$pid"
    [ -f "$first" ] || echo "$now" > "$first"
    age=$((now - $(cat "$first")))
    if [ "$age" -gt "$LIMIT" ]; then
      kill -9 "$pid" 2>/dev/null && echo "KILLED detail-bake pid $pid after ${age}s"
      rm -f "$first"
    fi
  done
  free=$(memory_pressure 2>/dev/null | awk '/free percentage/ {gsub("%","",$5); print $5}')
  if [ -n "$free" ] && [ "$free" -lt 12 ]; then echo "LOW MEMORY: ${free}% free"; fi
  count=$(reports)
  if [ "$count" -gt "$known" ]; then echo "NEW WindowServer hang report ($count in total)"; known=$count; fi
  sleep 5
done
```

### shoot1.sh

```sh
#!/bin/sh
# shoot1.sh <folder> <module> <fitted> <bake> <poses> <label>: one capped launch of bake-shoot.sh (240 s).
# Extra variables (KJK_MODEL_LIGHT, KJK_MODEL_FIX) come from the caller's environment.
S=$(dirname "$0")
sh "$S/run-capped.sh" 240 "$S/last-shoot.log" sh <root>/tools/light-kinds/bake-shoot.sh "$@"
cat "$S/last-shoot.log"
```

### shootall.sh

```sh
#!/bin/sh
# shootall.sh <folder> <module> <bake> <poses> <prefix> [terms...]
# One launch after the other: the textured picture under the bake as
# <prefix>-tex, and one launch for each term of KJK_MODEL_LIGHT as
# <prefix>-<term>. KJK_MODEL_FIX passes through from the caller.
S=$(dirname "$0")
folder=$1 module=$2 bake=$3 poses=$4 prefix=$5; shift 5
terms=${*:-tex mask whole probes above sides lamps fitted glow floor}
for term in $terms; do
  if [ "$term" = tex ]; then
    env -u KJK_MODEL_LIGHT sh "$S/shoot1.sh" "$folder" "$module" baked "$bake" "$poses" "$prefix-tex"
  else
    KJK_MODEL_LIGHT=$term sh "$S/shoot1.sh" "$folder" "$module" baked "$bake" "$poses" "$prefix-$term"
  fi
done
```

### fixes.sh

```sh
#!/bin/sh
# fixes.sh <folder> <module> <bake> <poses> <prefix> <fix-set>...
# One textured launch under the bake for each set of KJK_MODEL_FIX, as
# <prefix>-tex-<set with + for ,>.
S=$(dirname "$0")
folder=$1 module=$2 bake=$3 poses=$4 prefix=$5; shift 5
for set in "$@"; do
  name=$(echo "$set" | tr ',' '+')
  KJK_MODEL_FIX=$set sh "$S/shoot1.sh" "$folder" "$module" baked "$bake" "$poses" "$prefix-tex-$name"
done
```

Today's pictures were shot with `sh shoot1.sh bake-unk_m44ac unk_m44ac off "" all-poses.txt s0-off`.

### measure.py

```python
"""measure.py <shots folder> <mask label> <label> [<label> ...]

For each pose of the mask label's pictures: the object is the magenta
region nearest the picture's middle (a 4-connected component of pixels
with r > 200, g < 60, b > 200, at least 200 pixels). The floor is a band
of the rows 6 to 40 pixels below the object's lowest row, over the
object's columns, without magenta. Prints, per label and pose, the mean
luma of the object, of the floor, and the ratio object / floor.
"""
import sys, os
from PIL import Image

def luma(p):
    return 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]

def region(mask):
    w, h = mask.size
    px = mask.load()
    magenta = lambda x, y: (lambda p: p[0] > 200 and p[1] < 60 and p[2] > 200)(px[x, y])
    seen, best = set(), None
    cx, cy = w / 2, h / 2
    for y in range(0, h, 2):
        for x in range(0, w, 2):
            if (x, y) in seen or not magenta(x, y):
                continue
            stack, comp = [(x, y)], []
            seen.add((x, y))
            while stack:
                a, b = stack.pop()
                comp.append((a, b))
                for na, nb in ((a + 1, b), (a - 1, b), (a, b + 1), (a, b - 1)):
                    if 0 <= na < w and 0 <= nb < h and (na, nb) not in seen and magenta(na, nb):
                        seen.add((na, nb))
                        stack.append((na, nb))
            if len(comp) < 200:
                continue
            mx = sum(p[0] for p in comp) / len(comp)
            my = sum(p[1] for p in comp) / len(comp)
            d = (mx - cx) ** 2 + (my - cy) ** 2
            if best is None or d < best[0]:
                best = (d, comp)
    if best is None:
        return None, None
    comp = set(best[1])
    xs = [p[0] for p in comp]
    low = max(p[1] for p in comp)
    floor = [(x, y) for y in range(low + 6, min(h, low + 41)) for x in range(min(xs), max(xs) + 1)
             if not magenta(x, y)]
    return comp, floor

def main():
    folder, mask_label, labels = sys.argv[1], sys.argv[2], sys.argv[3:]
    poses = sorted(f[:-4] for f in os.listdir(os.path.join(folder, mask_label)) if f.endswith('.png'))
    print('label pose object floor ratio')
    for pose in poses:
        comp, floor = region(Image.open(os.path.join(folder, mask_label, pose + '.png')).convert('RGB'))
        if not comp or not floor:
            print('-', pose, 'no object')
            continue
        for label in labels:
            path = os.path.join(folder, label, pose + '.png')
            if not os.path.exists(path):
                continue
            px = Image.open(path).convert('RGB').load()
            o = sum(luma(px[p]) for p in comp) / len(comp)
            f = sum(luma(px[p]) for p in floor) / len(floor)
            print(label, pose, '%.1f %.1f %.2f' % (o, f, o / max(f, 0.5)))

main()
```

It was run from the module's `shots` folder as `python3.14 -I measure.py . s0-mask s0-off s0-tex s0-whole …`. `measure_lib.py` was the same file without `main`, for `sheet.py` to import.

### sheet.py (written, never run)

It made one row per pose with each label's picture at 480 by 270, a caption with the pose, the label's title and the ratio, and a table of the ratios at the bottom. It was called as `sheet.py <shots folder> <mask label> <out.png> <title> <label:caption>...`. Rewrite it from that description; it is simple.

## 7. What I believe made the WindowServer hang

This is inferred. Nothing in the session showed the hang itself.

- Another session's game ran headless at 1280 by 720 in the worktree `atris-walk` the whole time (a long `--cmds` film with clips). My launches shared the graphics card with it. My brief's rule of one launch at a time held for my own processes only.
- My shooting loops started about twelve game launches one after the other, each about 45 seconds, with no pause between them. The last loop was running when the Mac crashed.
- `detail-bake --sun-fit` ran 192 sun passes on the graphics card for 73 seconds. It ended normally, but it was the longest single graphics job of the session.
- `run-capped.sh` kills only the shell of `bake-shoot.sh`. A game that hung would have kept running past the limit, and nothing but the watchdog's detail-bake rule would have killed it, which does not match the game.

My best guess is the combined load of two games on the graphics card at once, more than any one job of mine.

## 8. What is left, in order

1. Measure the pictures that were shot before the crash: `s0-tex-ambout`, `s0-tex-nofitted+floor+ambout+sides+lowfloor`, and the `f1-*` and `f2-*` term pictures, all against `s0-mask`.
2. Decide the set by the numbers. My expectation is `nofitted`, `floor`, `sides`, `lowfloor` and `ambout` together. If the models are still too dark with the lamps out, keep the directed lamps at half as `ambout` does and do not take them out of the light from above.
3. Shoot the same terms and trials in 411dxn (`bake/mac-r1`, then `bake/mac1`) and danm13 (`bake/mac1`), at the poses of each `m-poses.txt`. Check what lamps 411dxn's list holds, and whether it has an ambient-only lamp.
4. Turn the kept switches into the normal behaviour under the bake. Delete `trial_fix`, `ProbeList::fixes` and the switches that are not kept. Keep `KJK_MODEL_LIGHT`.
5. Check that nothing changes with the bake off: one pose with `KJK_FITTED_LIGHTS=off` before and after, the same picture expected.
6. Make one sheet per module: today, baked before, baked after, with the ratio table. Look at the glowing tables (`plc_reptab2`) and the door glow, since `GLOW_BY_ROOM` reads the probes' light, which these fixes change.
7. Commit in small commits: the debug view first, then each fix. Do not commit the file-mode changes of `tools/light-kinds/*.sh`.
8. If the models stay wrong, the follow-up is probes written by the baker: the baker computes each probe's six sides from the same lamps, suns and bounce as the lightmap. The probes' sides now take the drawn colour of the walls, which misses the sun's direct light on a model's sides.
