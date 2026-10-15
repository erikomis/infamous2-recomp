#!/bin/sh
# Headless run that keeps a frame every 10 s: tools/capture.sh <name> [seconds]
# Writes out/<name>.log and out/<name>/t<sec>.png. Environment (PAD_SCRIPT,
# diagnostics) passes through to the port.
#
# The game runs in its own working directory, out/<name>.cwd/. Saves live in
# gamedata/ relative to the working directory (cellFs maps /dev_hdd0 there), and
# infamous2-lle/gamedata is Erik's real save: a run from there continued it and
# autosaved over it. Without SAVE the run starts with no save (a new game);
# SAVE=<dir> copies a save in first -- a gamedata directory (containing
# dev_hdd0/) or a savedata backup (containing savedata/) -- so the run plays from
# it and writes only to its copy.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}; secs=${2:-120}
PY="${PY:-$here/../.venv/bin/python}"
dir="$here/out/$name"; rm -rf "$dir"; mkdir -p "$dir"

cwd="$here/out/$name.cwd"; rm -rf "$cwd"; mkdir -p "$cwd"
if [ -n "$SAVE" ]; then
    if [ -d "$SAVE/dev_hdd0" ]; then cp -R "$SAVE" "$cwd/gamedata"
    elif [ -d "$SAVE/savedata" ]; then
        mkdir -p "$cwd/gamedata/dev_hdd0/home/00000001"
        cp -R "$SAVE/savedata" "$cwd/gamedata/dev_hdd0/home/00000001/"
    else echo "SAVE=$SAVE: no dev_hdd0/ or savedata/ inside"; exit 1; fi
fi
cd "$cwd" || exit 1

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
