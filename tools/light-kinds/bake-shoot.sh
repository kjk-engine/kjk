#!/bin/sh
# Shoots every pose of a poses file in one launch.
#   bake-shoot.sh <folder> <module> <fitted> <bake> <poses> <label>
# The first four are bake-play.sh's. <poses> is a file in the area's folder
# with one pose a line: name x y z yaw pitch fov. <label> names the round.
# The pictures are copied to shots/<label>/<pose>.png in the area's folder,
# and the game's output is shots/<label>.log.
[ $# -eq 6 ] || { sed -n '2,8p' "$0"; exit 2; }
. "$(dirname "$0")/bake-env.sh" "$1"
cd "$HERE" || exit 1
CMDS="wait world; wait 200f; anim 5; wait 20f"
while read -r name x y z yaw pitch fov; do
  [ -z "$name" ] && continue
  CMDS="$CMDS; camera $x $y $z $yaw $pitch $fov; wait 14f; screenshot $name"
done < "$5"
CMDS="$CMDS; quit"
"$BIN/bake-play.sh" "$1" "$2" "$3" "$4" "$CMDS" > "shots/$6.log" 2>&1
code=$?
run=$(ls -t run/kjk-artifacts/runs | head -1)
mkdir -p "shots/$6"
cp "run/kjk-artifacts/runs/$run"/*.png "shots/$6/"
echo "exit $code run $run"
