#!/bin/sh
# Shoots the poses of ml-poses.txt in one module, for the measures of
# handover-light-probes.md.
#   model-shoot.sh <module> <area> <label> [VAR=value ...]
# KJK_ROOT is the worktree whose target/light-kinds holds the bakes, and
# KJK_EXE the game to run. A label that starts with "off" shoots the game
# as it is today. Any other label shoots it under the bake set
# bake/$SET/<area>/b (round r2 when SET is unset). VAR=value pairs are set
# for the launch, for example KJK_MODEL_LIGHT=mask or SET=ml-probe. The
# pictures go to shots/<label> in the module's folder.
[ $# -ge 3 ] || { sed -n '2,11p' "$0"; exit 2; }
T=$(cd "$(dirname "$0")" && pwd)
module=$1 area=$2 label=$3; shift 3
for kv in "$@"; do export "$kv"; done
H=$KJK_ROOT/target/light-kinds/game/bake-$module
case $label in
  off*) timeout 300 sh "$T/bake-shoot.sh" "bake-$module" "$module" off "" "$H/ml-poses.txt" "$label" ;;
  *) timeout 300 sh "$T/bake-shoot.sh" "bake-$module" "$module" baked "$H/bake/${SET:-r2}/$area/b" "$H/ml-poses.txt" "$label" ;;
esac
