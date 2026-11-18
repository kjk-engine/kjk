#!/bin/sh
# Runs one headless, muted walk-mode launch of a module with scratch saves,
# settings and APPDATA.
#   bake-play.sh <folder> <module> <fitted> <bake> <commands>
# <folder> is the area's scratch folder under target/light-kinds/game.
# <module> is the stage, for example 103per or end_m01aa.
# <fitted> is KJK_FITTED_LIGHTS: off, on or baked.
# <bake> is KJK_DETAIL_BAKE and may be empty. A path that does not start at a
# drive or at / is read from the area's folder, for example bake/c5/103per/b.
# Extra variables such as KJK_FITTED_DUMP may be set by the caller.
[ $# -eq 5 ] || { sed -n '2,10p' "$0"; exit 2; }
. "$(dirname "$0")/bake-env.sh" "$1"
export KJK_FITTED_LIGHTS="$3"
case "$4" in
  "") unset KJK_DETAIL_BAKE ;;
  /*|?:*) export KJK_DETAIL_BAKE="$4" ;;
  *) export KJK_DETAIL_BAKE="$HERE/$4" ;;
esac
cd "$HERE/run" || exit 1
"$KJK" play --stage "$2" --walk --headless --size 960x540 --cmds "$5"
