#!/bin/sh
# Rebakes one area from the game's fitted lights, at a share of a level of
# each room.
#   rebake.sh <folder> <module> <game> <label> <share> <cap> [<of>]
# <folder> is the area's scratch folder under target/light-kinds/game.
# <module> is the module as detail-bake takes it, <game> is k1 or tsl.
# <label> names the round. <share> is the share of a room's level the bake
# aims at, and <cap> is the most it aims at, or none.
# <of> says which level of the room: original (the mean of its stored
# lightmaps, what the game shows today), lit (the same where the game lights
# the room: each texel counts by its stored level) or live (what it shows
# under the live fitted lights). It is original when left out.
# It writes, in the area's folder: lights-<label>.txt (the game's dump of its
# lights), a first bake under bake/<label>-first whose log first-<label>.log
# holds each room's levels, levels-<label>.txt (min(cap, share x level)
# for each room), and the bake under bake/<label>.
# KJK_BAKE_SUN, where it is set, is the area's sun for detail-bake's --sun,
# and KJK_BAKE_SKYLIGHTS its skylights for --skylights. KJK_BAKE_EVEN is
# the module's other flags: --no-even, --fill-stored, --least and --most.
# bake.sh sets it from data/lights/bake.toml.
[ $# -eq 6 ] || [ $# -eq 7 ] || { sed -n '2,20p' "$0"; exit 2; }
. "$(dirname "$0")/bake-env.sh" "$1"
module=$2 game=$3 label=$4
cd "$HERE" || exit 1
weigh=
[ "${7:-original}" = lit ] && weigh=--weigh
KJK_FITTED_DUMP=$HERE/lights-$label.txt "$BIN/bake-play.sh" "$1" "$module" on "" "wait world; wait 200f; quit" > "dump-$label.log" 2>&1
[ -f "lights-$label.txt" ] || { echo "the game wrote no lights; see $HERE/dump-$label.log"; tail -5 "dump-$label.log"; exit 1; }
echo "fitted lights: $(grep -c '^fitted' "lights-$label.txt")"
"$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu ${KJK_BAKE_SUN:+--sun "$KJK_BAKE_SUN"} ${KJK_BAKE_SKYLIGHTS:+--skylights "$KJK_BAKE_SKYLIGHTS"} $(echo "$KJK_BAKE_EVEN" | grep -o -- --every-room) --out "bake/$label-first" > "first-$label.log" 2>&1 || { tail -5 "first-$label.log"; exit 1; }
python -I - "$module" "$5" "first-$label.log" "levels-$label.txt" "$6" "${7:-original}" <<'PY' || exit 1
import re, sys
module, share, log, out, of = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[6]
cap = float('inf') if sys.argv[5] == 'none' else float(sys.argv[5])
patterns = {'original': r'\s+(\S+): original ([0-9.]+)', 'lit': r'\s+(\S+): original .* lit ([0-9.]+)',
            'live': r'\s+(\S+): \d+, live ([0-9.]+);'}
if of not in patterns:
    sys.exit('the level is original, lit or live, not ' + of)
lines = ['# %s of the %s level of each room of module %s, and at most %s.' % (share, of, module, sys.argv[5])]
for line in open(log):
    m = re.match(patterns[of], line)
    if m:
        lines.append('%s %.4f' % (m[1], min(cap, share * float(m[2]))))
if len(lines) == 1:
    sys.exit('no %s room levels in %s' % (of, log))
open(out, 'w').write('\n'.join(lines) + '\n')
print('rooms:', len(lines) - 1)
PY
"$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu ${KJK_BAKE_SUN:+--sun "$KJK_BAKE_SUN"} ${KJK_BAKE_SKYLIGHTS:+--skylights "$KJK_BAKE_SKYLIGHTS"} $KJK_BAKE_EVEN $weigh --levels "levels-$label.txt" --out "bake/$label" > "bake-$label.log" 2>&1 || { tail -5 "bake-$label.log"; exit 1; }
echo "rooms on even light: $(grep -c 'even light' "bake-$label.log")"
grep "mean stored level" "bake-$label.log"
echo "show it with KJK_FITTED_LIGHTS=baked KJK_DETAIL_BAKE=bake/$label/$(ls "bake/$label")/b"
