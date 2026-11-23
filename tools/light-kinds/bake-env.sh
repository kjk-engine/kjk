# Sourced by the bake scripts. $1 is the area's scratch folder, by name.
# Sets BIN (the scripts' own folder), ROOT (the worktree's root, taken from this file's place or from
# KJK_ROOT), KJK (the game) and HERE (target/light-kinds/game/<folder>), and
# points every launch at scratch saves, settings and APPDATA, muted.
# The scripts work from tools/light-kinds and from a copy in target/light-kinds.
BIN=$(cd "$(dirname "$0")" && pwd)
if [ -z "$KJK_ROOT" ]; then
  KJK_ROOT=$(cd "$BIN/../.." && { pwd -W 2>/dev/null || pwd; })
fi
ROOT=$KJK_ROOT
KJK=$ROOT/target/release/kjk
[ -f "$KJK.exe" ] && KJK=$KJK.exe
# KJK_EXE names another build of the game, for when the owner's own game runs
# from target/release/kjk.exe and a build cannot replace it.
[ -n "$KJK_EXE" ] && KJK=$KJK_EXE
HERE=$ROOT/target/light-kinds/game/$1
mkdir -p "$HERE/run" "$HERE/appdata" "$HERE/shots" "$HERE/bake" || exit 1
export KJK_SAVES=$ROOT/target/detail-bake/saves
export KJK_SETTINGS=$ROOT/target/detail-bake/shots/enhanced.json
export KJK_MUTE=1
export APPDATA=$HERE/appdata
# The settings are Enhanced lighting with the other options as a player
# leaves them (enhanced.json beside this file), copied on the first run.
mkdir -p "$KJK_SAVES" "$(dirname "$KJK_SETTINGS")" || exit 1
[ -f "$KJK_SETTINGS" ] || cp "$BIN/enhanced.json" "$KJK_SETTINGS" || exit 1
