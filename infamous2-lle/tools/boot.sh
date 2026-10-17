#!/bin/sh
# Headless bring-up run: tools/boot.sh <log> [seconds]. Kills the port after
# the time limit (wall clock, so a starved machine cannot stretch it) and
# reports whether it was still running.
here=$(cd "$(dirname "$0")/.." && pwd)
log=${1:?log file}; secs=${2:-40}
# caffeinate: an idle sleep in the middle of a run stretches every timing in
# the log and makes a healthy boot look stalled.
awake=""; command -v caffeinate >/dev/null 2>&1 && awake="caffeinate -i"
# PS3_PAD_SDL=0: a headless run must not read the physical pad (it would take
# the START of a game being played in a window beside it).
HEADLESS=1 SDL_AUDIODRIVER=dummy PS3_PAD_SDL="${PS3_PAD_SDL:-0}" $awake "$here/run.sh" > "$log" 2>&1 &
pid=$!; start=$(date +%s)
while kill -0 $pid 2>/dev/null && [ $(( $(date +%s) - start )) -lt "$secs" ]; do /bin/sleep 1; done
# $pid is caffeinate when it is available, and run.sh execs the game as its
# CHILD: killing caffeinate alone left the game running as an orphan, so a
# hung run kept going underneath the next one (2026-10-17: v8 lived 519 s of a
# 290 s run and the following runs' menus dropped to 4-8 fps).
if kill -0 $pid 2>/dev/null; then echo "still running after ${secs}s -> killed"; pkill -9 -P $pid; kill -9 $pid
else wait $pid; echo "exited by itself after $(( $(date +%s) - start ))s, status=$?"; fi
