#!/bin/sh
# Live A/B inside ONE headless run: tools/ab_live.sh <name> <KNOB> [B value] [A value]
#
# Two runs of the same binary differ by up to ~1 fps (where the walk script
# leaves Cole, what streamed in), more than many changes are worth. This flips
# one knob back and forth while the game runs -- PS3_TOGGLE_FILE, see the
# toolkit's include/ps3emu/ps3_toggles.h -- so A and B are the same scene at
# the same moment, interleaved every PERIOD seconds.
#
#   tools/ab_live.sh vc PS3RECOMP_ENG_VCACHE 0        # A = as configured, B = off
#   PERIOD=10 FROM=250 tools/ab_live.sh sh PS3_SPU_STACK_HINT 0 1
#
# A defaults to the environment's value (the knob left out of the file). The
# run is tools/profile_run.sh <name> $MODE $SECS with no `sample` (MODE
# newmarais, SECS 480 by default; it waits for a free machine and a copy of the
# save, as always). From FROM seconds after the game starts (default 200: past
# the bayou load) the knob alternates B, A, B, ... every PERIOD seconds
# (default 20), and PS3_FTRACE records frames and switches until 15 s before
# the end. Then tools/ftrace_report.py --phases prints each stretch and each
# value's frame times (SETTLE ms after a switch left out, default 1000).
# RUN_LOCK=<file>: hold that lock (lockf) for the game run only, not for the
# report, where several sessions share the machine one run at a time.
# Files: out/<name>.toggles, out/<name>.ftrace.txt, out/<name>.log.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}; knob=${2:?knob (an environment variable name)}
b=${3:-0}; a=${4-}
mode=${MODE:-newmarais}; secs=${SECS:-480}; from=${FROM:-200}; period=${PERIOD:-20}
PY="${PY:-$here/../.venv/bin/python}"
out="$here/out"; mkdir -p "$out"
tf="$out/$name.toggles"; trace="$out/$name.ftrace.txt"; log="$out/$name.log"
len=$((secs - from - 15))
[ "$len" -gt "$period" ] || { echo "SECS must leave more than PERIOD after FROM"; exit 1; }

# Written whole and renamed, so the game never reads half a file.
put() { printf '# tools/ab_live.sh %s: %s\n%s\n' "$name" "$1" "$2" > "$tf.tmp" && mv -f "$tf.tmp" "$tf"; }
put "A" "${a:+$knob=$a}"
rm -f "$trace" "$log"

lock=""; [ -n "$RUN_LOCK" ] && lock="/usr/bin/lockf $RUN_LOCK"
PS3_TOGGLE_FILE="$tf" PS3_FTRACE="$from:$len:$trace" PS3_FTRACE_KINDS="${PS3_FTRACE_KINDS:-4,5,6,7,9,10,15}" \
    $lock "$here/tools/profile_run.sh" "$name" "$mode" "$secs" 99999 > "$out/$name.ab.txt" 2>&1 &
runner=$!

# The clock starts when the game does (profile_run.sh may first wait for the machine).
while ! grep -aq "\[toggle\] watching" "$log" 2>/dev/null; do
    kill -0 $runner 2>/dev/null || { echo "the run ended before the game started"; cat "$out/$name.ab.txt"; exit 1; }
    sleep 1
done
start=$(date +%s)
echo "[ab] game up; $knob: A=${a:-(environment)} B=$b, switching every ${period}s from ${from}s"
next=$from; side=B
while kill -0 $runner 2>/dev/null; do
    t=$(( $(date +%s) - start ))
    if [ "$t" -ge "$next" ] && [ "$t" -lt $((from + len)) ]; then
        if [ $side = B ]; then put "B" "$knob=$b"; side=A
        else put "A" "${a:+$knob=$a}"; side=B; fi
        echo "[ab] t=${t}s: $( [ $side = A ] && echo B || echo A )"
        next=$((next + period))
    fi
    sleep 1
done
wait $runner
grep -a "^\[toggle\]" "$log" | head -40
"$PY" "$here/tools/ftrace_report.py" "$trace" --phases --settle "${SETTLE:-1000}"
