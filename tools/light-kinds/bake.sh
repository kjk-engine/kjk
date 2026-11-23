#!/bin/sh
# Bakes one area's lightmaps from the game's fitted lights, at the levels
# that data/lights/bake.toml gives its module.
#   bake.sh <module> <game> [<label> [<folder>]]
# <module> is the module as detail-bake takes it, for example danm13 or
# end_m01aa. <game> is k1 or tsl.
# <label> names the round and is the month, day and time when left out.
# <folder> is the area's scratch folder under target/light-kinds/game and is
# bake-<module> when left out.
# A module with a share bakes twice (rebake.sh): once to measure each room's
# level, and once held to the share of it. A module with a levels file
# bakes once, held to that file (rebake-held.sh), and its `of` says which
# level of the room the file holds. Its `scale` multiplies every level of
# the file, so an approved file can be made darker and stay as it was. The result is bake/<label>
# in the area's folder, and levels-<label>.txt there holds the levels used.
# A module with a sun in data/lights/bake.toml is baked with it.
# Its `even`, `fill`, `least`, `most` and `floor` there become detail-bake's flags.
# The bake has the placeables block rays and lights them at their vertices,
# as the shipped bakes are made. canon/canon.sh bakes every module of a game
# for the lighting download, with the same flags.
[ $# -ge 2 ] && [ $# -le 4 ] || { sed -n '2,21p' "$0"; exit 2; }
module=$1 game=$2 label=${3:-$(date +%m%d-%H%M)} folder=${4:-bake-$1}
. "$(dirname "$0")/bake-env.sh" "$folder"
set -- $(python -I - "$ROOT/data/lights/bake.toml" "$module" <<'PY'
import sys, tomllib
path, module = sys.argv[1], sys.argv[2]
data = tomllib.load(open(path, 'rb'))
entry = data.get('modules', {}).get(module, data['default'])
if 'levels' in entry:
    print('levels', entry['levels'], entry.get('of', 'original'), entry.get('scale', 1.0))
else:
    print('share', entry['share'], entry.get('ceiling', 'none'), entry.get('of', 'original'))
PY
)
KJK_BAKE_SUN=$(python -I - "$ROOT/data/lights/bake.toml" "$module" <<'PY'
import sys, tomllib
data = tomllib.load(open(sys.argv[1], 'rb'))
sun = data.get('modules', {}).get(sys.argv[2], {}).get('sun')
if sun:
    print(' '.join(str(number) for key in ('toward', 'colour', 'sky', 'toward2', 'colour2') for number in sun.get(key, [])), sun.get('width', ''))
PY
)
KJK_BAKE_SKYLIGHTS=$(python -I - "$ROOT/data/lights/bake.toml" "$module" <<'PY'
import sys, tomllib
data = tomllib.load(open(sys.argv[1], 'rb'))
print(' '.join(data.get('modules', {}).get(sys.argv[2], {}).get('sun', {}).get('skylights', [])))
PY
)
KJK_BAKE_EVEN=$(python -I - "$ROOT/data/lights/bake.toml" "$module" <<'PY'
import sys, tomllib
data = tomllib.load(open(sys.argv[1], 'rb'))
entry = data.get('modules', {}).get(sys.argv[2], {})
flags = [] if entry.get('even', True) else ['--no-even']
if entry.get('fill') == 'stored':
    flags.append('--fill-stored')
if entry.get('every_room'):
    flags.append('--every-room')
if entry.get('backdrop') == 'stored':
    flags.append('--stored-backdrop')
for key in ('least', 'most', 'floor'):
    if key in entry:
        flags += ['--' + key, str(entry[key])]
print(' '.join(flags))
PY
)
export KJK_BAKE_SUN KJK_BAKE_SKYLIGHTS KJK_BAKE_EVEN
[ -n "$KJK_BAKE_SUN" ] && echo "$module has a sun: $KJK_BAKE_SUN"
case "$1" in
  levels)
    echo "$module is held to data/lights/$2"
    python -I - "$ROOT/data/lights/$2" "$4" > "$HERE/levels-$label.txt" <<'PY' || exit 1
import sys
for line in open(sys.argv[1]):
    parts = line.split()
    if len(parts) != 2 or line.startswith('#'):
        print(line, end='')
    else:
        print(parts[0], f'{float(parts[1]) * float(sys.argv[2]):.4f}')
PY
    "$BIN/rebake-held.sh" "$folder" "$module" "$game" "levels-$label.txt" "$label" "$3" ;;
  share)
    echo "$module aims at $2 of each room's $4 level, ceiling $3"
    "$BIN/rebake.sh" "$folder" "$module" "$game" "$label" "$2" "$3" "$4" ;;
  *) echo "data/lights/bake.toml gave no level for $module"; exit 1 ;;
esac
# A module whose entry has `dark_unlit = true` gets `unlit.txt` in its set,
# and the game then shows its rooms' surfaces with no lightmap dark.
status=$?
if python -I -c "import tomllib,sys;sys.exit(0 if tomllib.load(open(sys.argv[1],'rb')).get('modules',{}).get(sys.argv[2],{}).get('dark_unlit') else 1)" "$ROOT/data/lights/bake.toml" "$module"; then
  for set_ in "$HERE/bake/$label"/*/b; do [ -d "$set_" ] && echo "dark" > "$set_/unlit.txt"; done
fi
exit $status
