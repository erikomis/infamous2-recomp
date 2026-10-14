#!/bin/sh
# Catch the cityfalls cutscene stall live: tools/watch_stall.sh <run name> [seconds]
#
# Runs tools/capture.sh and watches its log. The docks cutscene opens
# subtitle/h_jbi_cityfalls.bsub for the shot that sometimes never ends, and
# h_jbi_bus.bsub when it does end. If the second open has not shown up
# <grace> seconds after the first, the game is stalled: sample every host
# thread of the process (out/<name>.sample.txt) and dump the guest threads,
# so the stall can be read instead of guessed at.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}; secs=${2:-720}; grace=${STALL_GRACE:-120}
log="$here/out/$name.log"

PS3_THREAD_DUMP=${PS3_THREAD_DUMP:-100} "$here/tools/capture.sh" "$name" "$secs" &
cp=$!
seen=0; sampled=0
while kill -0 $cp 2>/dev/null; do
    /bin/sleep 15
    [ -f "$log" ] || continue
    if [ $seen -eq 0 ] && grep -q "h_jbi_cityfalls" "$log" 2>/dev/null; then
        seen=$(date +%s); echo "[watch] cityfalls shot at $(date +%T)"
    fi
    if [ $seen -ne 0 ] && [ $sampled -eq 0 ] && [ $(( $(date +%s) - seen )) -ge "$grace" ]; then
        if grep -q "h_jbi_bus" "$log" 2>/dev/null; then
            echo "[watch] shot ended normally"; sampled=1
        else
            pid=$(pgrep -f "build/Infamous2Recomp" | head -1)
            echo "[watch] STALLED: sampling pid $pid at $(date +%T)"
            sample "$pid" 5 -file "$here/out/$name.sample.txt" >/dev/null 2>&1
            sampled=1
        fi
    fi
done
wait $cp
