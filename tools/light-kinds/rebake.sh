#!/bin/sh
# Rebakes one area from the game's fitted lights, at a share of the live
# level of each room.
#   rebake.sh <folder> <module> <game> <label> <share> <cap>
# <folder> is the area's scratch folder under target/light-kinds/game.
# <module> is the module as detail-bake takes it, <game> is k1 or tsl.
# <label> names the round. <share> is the share of a room's live level the
# bake aims at, and <cap> is the most it aims at.
# It writes, in the area's folder: lights-<label>.txt (the game's dump of its
# lights), a first bake under bake/<label>-first whose log first-<label>.log
# holds each room's live level, levels-<label>.txt (min(cap, share x live)
# for each room), and the bake under bake/<label>.
[ $# -eq 6 ] || { sed -n '2,13p' "$0"; exit 2; }
. "$(dirname "$0")/bake-env.sh" "$1"
module=$2 game=$3 label=$4
cd "$HERE" || exit 1
KJK_FITTED_DUMP=$HERE/lights-$label.txt "$BIN/bake-play.sh" "$1" "$module" on "" "wait world; wait 200f; quit" > "dump-$label.log" 2>&1
[ -f "lights-$label.txt" ] || { echo "the game wrote no lights; see $HERE/dump-$label.log"; tail -5 "dump-$label.log"; exit 1; }
echo "fitted lights: $(grep -c '^fitted' "lights-$label.txt")"
"$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu --out "bake/$label-first" > "first-$label.log" 2>&1 || { tail -5 "first-$label.log"; exit 1; }
python -I - "$module" "$5" "first-$label.log" "levels-$label.txt" "$6" <<'PY' || exit 1
import re, sys
module, share, log, out, cap = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], float(sys.argv[5])
lines = ['# %s of the live look\'s mean stored level in each room of module %s, and at most %s.' % (share, module, cap)]
for line in open(log):
    m = re.match(r'\s+(\S+): \d+, live ([0-9.]+);', line)
    if m:
        lines.append('%s %.4f' % (m[1], min(cap, share * float(m[2]))))
if len(lines) == 1:
    sys.exit('no room levels in ' + log)
open(out, 'w').write('\n'.join(lines) + '\n')
print('rooms:', len(lines) - 1)
PY
"$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu --levels "levels-$label.txt" --out "bake/$label" > "bake-$label.log" 2>&1 || { tail -5 "bake-$label.log"; exit 1; }
echo "rooms on even light: $(grep -c 'even light' "bake-$label.log")"
grep "mean stored level" "bake-$label.log"
echo "show it with KJK_FITTED_LIGHTS=baked KJK_DETAIL_BAKE=bake/$label/$(ls "bake/$label")/b"
