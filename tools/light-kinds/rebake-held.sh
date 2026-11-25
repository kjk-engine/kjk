#!/bin/sh
# Rebakes one area from the game's fitted lights as they are now, held to
# the levels of a levels file made earlier.
#   rebake-held.sh <folder> <module> <game> <levels> <label> [<of>]
# <folder> is the area's scratch folder under target/light-kinds/game.
# <module> is the module as detail-bake takes it, <game> is k1 or tsl.
# <levels> is the levels file, in the area's folder. <label> names the round.
# <of> says which level of the room the file holds, as in rebake.sh. With
# lit, each texel counts by its stored level. It is original when left out.
# It writes, in the area's folder: lights-<label>.txt (the game's dump of its
# lights) and the bake under bake/<label>.
# The bake has the placeables block rays and lights them at their vertices
# (--placeables --placeable-light), as canon/canon.sh does.
[ $# -eq 5 ] || [ $# -eq 6 ] || { sed -n '2,14p' "$0"; exit 2; }
. "$(dirname "$0")/bake-env.sh" "$1"
module=$2 game=$3 levels=$4 label=$5
weigh=
[ "${6:-original}" = lit ] && weigh=--weigh
cd "$HERE" || exit 1
[ -f "$levels" ] || { echo "no levels file $levels in $HERE"; exit 1; }
KJK_FITTED_DUMP=$HERE/lights-$label.txt "$BIN/bake-play.sh" "$1" "$module" on "" "wait world; wait 200f; quit" > "dump-$label.log" 2>&1
[ -f "lights-$label.txt" ] || { echo "the game wrote no lights; see $HERE/dump-$label.log"; tail -5 "dump-$label.log"; exit 1; }
echo "fitted lights: $(grep -c '^fitted' "lights-$label.txt")"
echo "switched lamps: $(grep -c '^switched' "lights-$label.txt")"
"$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu ${KJK_BAKE_SUN:+--sun "$KJK_BAKE_SUN"} ${KJK_BAKE_SKYLIGHTS:+--skylights "$KJK_BAKE_SKYLIGHTS"} $KJK_BAKE_EVEN $weigh --levels "$levels" --placeables --placeable-light --out "bake/$label" > "bake-$label.log" 2>&1 || { tail -5 "bake-$label.log"; exit 1; }
echo "rooms on even light: $(grep -c 'even light' "bake-$label.log")"
grep "mean stored level" "bake-$label.log"
echo "show it with KJK_FITTED_LIGHTS=baked KJK_DETAIL_BAKE=bake/$label/$(ls "bake/$label")/b"
