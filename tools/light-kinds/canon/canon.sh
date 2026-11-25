#!/bin/sh
# Bakes every module of one game for the lighting download (the canon
# round), with the placeables blocking rays and lit at their vertices.
#   sh canon.sh <k1|tsl> [<module> ...]
#
# It needs a release build of kjk and the game installed with its patch.
# These variables say where they are:
# - KJK_KOTOR1 and KJK_KOTOR2: the KOTOR 1 install with the K1 Community
#   Patch and the KOTOR 2 install with TSLRCM. The game being baked must be
#   set. The bakes are made for these patches only.
# - KJK_EXE: the game to run, target/release/kjk.exe of the repository when
#   it is not set. Build it first with `cargo build --release`.
# - KJK_CANON_OUT: the folder the bakes go to, target/canon-bakes of the
#   repository when it is not set.
# - KJK_CANON_LABEL: the round's name, c1 when it is not set.
# - KJK_ROOT: the repository, two folders over this file's when it is not
#   set.
#
# The modules are the lines of modules-<game>.txt beside this file. A line
# names a module and the module whose settings it takes, which is the
# module itself unless it shares another module's area. settings.py gives
# each module's settings (see there).
#
# Each module's folder is <out>/<game>/<module>. It holds the game's dump of
# its lights (lights-<label>.txt), for a module with a share the first bake
# (bake/<label>-first and first-<label>.log), the levels it was held to
# (levels-<label>.txt), the bake's log (bake-<label>.log), and the bake in
# bake/<label>/<area>/{a,b}. Set b is what `kjk lighting pack` ships.
# A module whose bake/<label>/*/b/fingerprint.txt exists is skipped, so a
# stopped run goes on where it stopped. A file named `stop` in <out> ends the
# run after the module it is on. Each module's result and time go to
# <out>/<game>.log. A failed bake is tried once more, except after a
# timeout.
#
# It does what bake.sh, rebake.sh and rebake-held.sh do for one module, for
# every module, with each module's settings from settings.py.
game=$1; shift
only="$*"
case "$game" in k1|tsl) ;; *) sed -n '2,36p' "$0"; exit 2 ;; esac
T=$(cd "$(dirname "$0")" && { pwd -W 2>/dev/null || pwd; })
ROOT=${KJK_ROOT:-$(cd "$T/../../.." && { pwd -W 2>/dev/null || pwd; })}
KJK=${KJK_EXE:-$ROOT/target/release/kjk}
[ -f "$KJK.exe" ] && KJK=$KJK.exe
C=${KJK_CANON_OUT:-$ROOT/target/canon-bakes}
label=${KJK_CANON_LABEL:-c1}
log=$C/$game.log
[ -f "$KJK" ] || { echo "no game at $KJK"; exit 1; }
if [ "$game" = k1 ]; then
  [ -n "$KJK_KOTOR1" ] || { echo "KJK_KOTOR1 must name the KOTOR 1 install with the K1 Community Patch"; exit 1; }
else
  [ -n "$KJK_KOTOR2" ] || { echo "KJK_KOTOR2 must name the KOTOR 2 install with TSLRCM"; exit 1; }
fi
unset KJK_WORKSHOP KJK_DETAIL_BAKE KJK_DETAIL_BAKES KJK_FITTED_LIGHTS KJK_FITTED_DUMP KJK_PLACEABLE_LIGHT KJK_MODEL_LAMPS KJK_BAKED_PROBES KJK_BAKE_DEBUG KJK_BAKE_TEXEL
# Every launch keeps its saves and settings in the run's folder, muted.
mkdir -p "$C/saves" || exit 1
[ -f "$C/settings.json" ] || cp "$T/../enhanced.json" "$C/settings.json" || exit 1
export KJK_SAVES=$C/saves KJK_SETTINGS=$C/settings.json KJK_MUTE=1

# One try at one module, run in its folder. Settings are in the shell's
# variables (settings.py).
bake_once() {
  rm -rf "bake/$label" "bake/$label-first" "lights-$label.txt"
  (cd run && KJK_FITTED_LIGHTS=on KJK_FITTED_DUMP="$H/lights-$label.txt" timeout 900 "$KJK" play --stage "$module" --walk --headless --size 960x540 --cmds "wait world; wait 200f; quit") > "dump-$label.log" 2>&1
  [ -f "lights-$label.txt" ] || { echo "the game wrote no lights (dump-$label.log)"; return 1; }
  echo "fitted lights: $(grep -c '^fitted' "lights-$label.txt")"
  weigh=
  [ "$OF" = lit ] && weigh=--weigh
  case $MODE in
    share)
      timeout 1500 "$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu ${SUN:+--sun "$SUN"} ${SKYLIGHTS:+--skylights "$SKYLIGHTS"} $(echo "$FLAGS" | grep -o -- --every-room) --out "bake/$label-first" > "first-$label.log" 2>&1
      code=$?
      [ $code -eq 0 ] || { echo "first bake exit $code: $(tail -2 "first-$label.log" | tr '\n' ' ')"; return $code; }
      python -I - "$module" "$SHARE" "first-$label.log" "levels-$label.txt" "$CAP" "$OF" <<'PY' || return 1
import re, sys
module, share, log, out, of = sys.argv[1], float(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[6]
cap = float('inf') if sys.argv[5] == 'none' else float(sys.argv[5])
patterns = {'original': r'\s+(\S+): original ([0-9.]+)', 'lit': r'\s+(\S+): original .* lit ([0-9.]+)',
            'live': r'\s+(\S+): \d+, live ([0-9.]+);'}
lines = ['# %s of the %s level of each room of module %s, and at most %s.' % (share, of, module, sys.argv[5])]
for line in open(log):
    m = re.match(patterns[of], line)
    if m:
        lines.append('%s %.4f' % (m[1], min(cap, share * float(m[2]))))
if len(lines) == 1:
    sys.exit('no %s room levels in %s' % (of, log))
open(out, 'w', newline='\n').write('\n'.join(lines) + '\n')
print('rooms:', len(lines) - 1)
PY
      ;;
    levels)
      python -I - "$LEVELS" "$SCALE" > "levels-$label.txt" <<'PY' || return 1
import sys
sys.stdout.reconfigure(newline='\n')
for line in open(sys.argv[1]):
    parts = line.split()
    if len(parts) != 2 or line.startswith('#'):
        print(line, end='')
    else:
        print(parts[0], f'{float(parts[1]) * float(sys.argv[2]):.4f}')
PY
      ;;
    walked)
      cp "$LEVELS" "levels-$label.txt" || return 1
      weigh=--weigh ;;
    *) echo "no mode"; return 1 ;;
  esac
  echo "bake: --gpu ${SUN:+--sun \"$SUN\"} ${SKYLIGHTS:+--skylights \"$SKYLIGHTS\"} $FLAGS $weigh --levels levels-$label.txt --placeables --placeable-light"
  timeout 1500 "$KJK" detail-bake "$module" --game "$game" --lights "lights-$label.txt" --gpu ${SUN:+--sun "$SUN"} ${SKYLIGHTS:+--skylights "$SKYLIGHTS"} $FLAGS $weigh --levels "levels-$label.txt" --placeables --placeable-light --out "bake/$label" > "bake-$label.log" 2>&1
  code=$?
  [ $code -eq 0 ] || { echo "bake exit $code: $(tail -2 "bake-$label.log" | tr '\n' ' ')"; return $code; }
  if [ -n "$DARK_UNLIT" ]; then
    for set_ in "bake/$label"/*/b; do [ -d "$set_" ] && echo "dark" > "$set_/unlit.txt"; done
  fi
  echo "rooms on even light: $(grep -c 'even light' "bake-$label.log")"
  grep "mean stored level" "bake-$label.log"
  return 0
}

echo "run $game started $(date '+%m-%d %H:%M:%S')" >> "$log"
while read -r module source; do
  [ -f "$C/stop" ] && { echo "stopped" >> "$log"; break; }
  [ -n "$only" ] && ! echo " $only " | grep -q " $module " && continue
  H=$C/$game/$module
  ls "$H/bake/$label"/*/b/fingerprint.txt > /dev/null 2>&1 && continue
  mkdir -p "$H/run" "$H/appdata" "$H/bake"
  export APPDATA=$H/appdata
  MODE= SHARE= CAP= OF= LEVELS= SCALE= SUN= SKYLIGHTS= FLAGS= DARK_UNLIT= ORIGIN=
  eval "$(python -I "$T/settings.py" "$T" "$ROOT" "$game" "$module" "$source")"
  start=$(date +%s)
  note=
  cd "$H" || continue
  {
    echo "$module ($game) settings from $source: $ORIGIN, mode $MODE share $SHARE cap $CAP of $OF levels $LEVELS scale $SCALE sun [$SUN] skylights [$SKYLIGHTS] flags [$FLAGS] dark_unlit [$DARK_UNLIT]"
  } > "$label.out"
  bake_once >> "$label.out" 2>&1 < /dev/null
  code=$?
  if [ $code -ne 0 ] && [ $code -ne 124 ]; then
    note="try 1 failed (exit $code); "
    bake_once >> "$label.out" 2>&1 < /dev/null
    code=$?
  fi
  [ $code -eq 124 ] && note="${note}timeout; "
  rm -rf "$H/run/kjk-artifacts/runs"
  cd "$C"
  set_=$(ls -d "$H/bake/$label"/*/b 2>/dev/null | head -1)
  if [ $code -ne 0 ] || [ -z "$set_" ] || [ -z "$(ls "$set_"/*.png 2>/dev/null | head -1)" ] || [ ! -f "$set_/fingerprint.txt" ]; then
    echo "$module $source $ORIGIN FAILED $(( $(date +%s) - start ))s exit $code $note$(grep -v '^ ' "$H/$label.out" | tail -1)" >> "$log"
    continue
  fi
  echo "$module $source $ORIGIN ok $(( $(date +%s) - start ))s pngs $(ls "$set_"/*.png | wc -l) $note" >> "$log"
done < "$T/modules-$game.txt"
echo "run $game ended $(date '+%m-%d %H:%M:%S')" >> "$log"
