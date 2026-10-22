#!/bin/sh
# Profile a headless run: tools/profile_run.sh <name> [prologue|menu|newmarais] [seconds] [sample points...]
#
# Runs the game headless (tools/play_prologue.sh for "prologue": menu, docks
# cutscene, pier gameplay; tools/capture.sh with the menu presses for "menu"),
# "newmarais" plays a copy of Erik's save (SAVE=<dir>, default the newest
# save-backup-*; capture.sh copies it, the real one is never written):
# the bayou by ~150 s, the city streets by ~350 s (sample at 250 and 400, run 480).
# with SPU_INTERP_HIST=1, and takes a 5 s `sample` of the process at each
# sample point (seconds of run time; default 120 300 620: main menu, docks
# cutscene, pier gameplay). Then prints tools/profile_report.py: fps per 10 s,
# busy cores per thread kind, hottest code, and where the SPU interpreter runs.
#
# Measurements share the machine: it waits until no other run (tools/boot.sh)
# and no windowed game is up, and it only ever stops its own process.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}; mode=${2:-prologue}; secs=${3:-700}
shift 3 2>/dev/null; points=${*:-"120 300 620"}

# By process name (-x), not command line: a shell or monitor whose command
# merely mentions the path used to count as a windowed game for ever.
windowed() { for p in $(pgrep -x Infamous2Recomp); do
                 ps eww -o command= -p "$p" | grep -q "HEADLESS=1" || return 0; done; return 1; }
while windowed || pgrep -f "tools/boot.sh" >/dev/null; do
    echo "[profile] machine busy (another run or a windowed game); waiting"; sleep 60; done

# The game process descended from this run's boot.sh (boot.sh -> caffeinate -> run.sh -> game).
own_game() { for p in $(pgrep -x Infamous2Recomp); do q=$p
                 while [ -n "$q" ] && [ "$q" -gt 1 ]; do
                     [ "$q" = "$boot" ] && { echo "$p"; return; }
                     q=$(ps -o ppid= -p "$q" | tr -d ' '); done; done; }

rm -f "$here/out/$name".sample.*.txt
export SPU_INTERP_HIST=1
case "$mode" in
    prologue) "$here/tools/play_prologue.sh" "$name" "$secs" > "$here/out/$name.capture.txt" 2>&1 & ;;
    menu)     PAD_SCRIPT="95:0x4000,110:0x4000" "$here/tools/capture.sh" "$name" "$secs" > "$here/out/$name.capture.txt" 2>&1 & ;;
    newmarais)
        sq=""; save="${SAVE:-$(ls -d "$here"/save-backup-* 2>/dev/null | tail -1)}"
        [ -n "$save" ] || { echo "newmarais needs SAVE=<save dir>"; exit 1; }; t=200; while [ $t -le 330 ]; do sq="$sq,$t:0x8000"; t=$((t + 5)); done
        PAD_SCRIPT="95:0x4000,110:0x4000,125:0x4000,140:0x4000$sq" PAD_STICK="128,0,128,128,335,420" SAVE="$save" \
            "$here/tools/capture.sh" "$name" "$secs" > "$here/out/$name.capture.txt" 2>&1 & ;;
    *) echo "mode: prologue, menu or newmarais"; exit 1 ;;
esac
runner=$!; start=$(date +%s)
sleep 5
boot=$(pgrep -f "tools/boot.sh $here/out/$name.log" | head -1)
for t in $points; do
    while [ $(( $(date +%s) - start )) -lt "$t" ]; do kill -0 $runner 2>/dev/null || break 2; sleep 5; done
    pid=$(own_game)
    [ -n "$pid" ] || break
    echo "[profile] t=$t s: sampling pid $pid"
    sample "$pid" 5 -file "$here/out/$name.sample.$(printf %04d $t).txt" >/dev/null 2>&1
done
wait $runner
"$here/../.venv/bin/python" "$here/tools/profile_report.py" "$name"
