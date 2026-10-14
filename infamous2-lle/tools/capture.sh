#!/bin/sh
# Headless run that keeps a frame every 10 s: tools/capture.sh <name> [seconds]
# Writes out/<name>.log and out/<name>/t<sec>.png. Environment (PAD_SCRIPT,
# diagnostics) passes through to the port.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}; secs=${2:-120}
PY="${PY:-$here/../.venv/bin/python}"
dir="$here/out/$name"; rm -rf "$dir"; mkdir -p "$dir"

PS3RECOMP_METAL_FRAME_DUMP="$dir/frame.ppm" "$here/tools/boot.sh" "$here/out/$name.log" "$secs" &
bp=$!; start=$(date +%s)
while kill -0 $bp 2>/dev/null; do
    /bin/sleep 10
    t=$(( $(date +%s) - start ))
    [ -f "$dir/frame.ppm" ] && cp "$dir/frame.ppm" "$dir/t$(printf %03d $t).ppm" 2>/dev/null
done
wait $bp
rm -f "$dir/frame.ppm"
for f in "$dir"/t*.ppm; do
    [ -f "$f" ] || continue
    printf '%s: ' "$(basename "$f" .ppm)"
    "$PY" "$here/tools/ppm2png.py" "$f" "${f%.ppm}.png" && rm -f "$f"
done
grep -a "captured frame" "$here/out/$name.log" | tail -1 | cut -c1-48
