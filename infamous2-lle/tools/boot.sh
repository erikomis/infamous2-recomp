#!/bin/sh
# Headless bring-up run: tools/boot.sh <log> [seconds]. Kills the port after
# the time limit (wall clock, so a starved machine cannot stretch it) and
# reports whether it was still running.
here=$(cd "$(dirname "$0")/.." && pwd)
log=${1:?log file}; secs=${2:-40}
# caffeinate: an idle sleep in the middle of a run stretches every timing in
# the log and makes a healthy boot look stalled.
awake=""; command -v caffeinate >/dev/null 2>&1 && awake="caffeinate -i"
HEADLESS=1 SDL_AUDIODRIVER=dummy $awake "$here/run.sh" > "$log" 2>&1 &
pid=$!; start=$(date +%s)
while kill -0 $pid 2>/dev/null && [ $(( $(date +%s) - start )) -lt "$secs" ]; do /bin/sleep 1; done
if kill -0 $pid 2>/dev/null; then echo "still running after ${secs}s -> killed"; kill -9 $pid
else wait $pid; echo "exited by itself after $(( $(date +%s) - start ))s, status=$?"; fi
