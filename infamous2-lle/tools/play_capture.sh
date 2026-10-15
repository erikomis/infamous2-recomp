#!/bin/sh
# Windowed play session that records what a rendering bug needs:
#   tools/play_capture.sh <name>
# Writes out/<name>.log (texture formats, fps, draw stats, SPU interpreter
# pages), out/<name>/shaders/ (every pipeline's shaders) and
# out/<name>/t<sec>.png (the game surface every 10 s). Play to the bad spot,
# note the time, then quit the game.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}
PY="${PY:-$here/../.venv/bin/python}"
dir="$here/out/$name"; rm -rf "$dir"; mkdir -p "$dir/shaders"

PS3RECOMP_ENG_TEXLOG=1 PS3RECOMP_ENG_FPSLOG=1 PS3RECOMP_ENG_DRAWSTATS=1 \
PS3RECOMP_ENG_SHADER_DUMP="$dir/shaders" PS3RECOMP_METAL_FRAME_DUMP="$dir/frame.ppm" \
SPU_INTERP_HIST=1 \
    "$here/run.sh" > "$here/out/$name.log" 2>&1 &
gp=$!; start=$(date +%s)
echo "running (pid $gp); log out/$name.log -- quit the game window to finish"
while kill -0 $gp 2>/dev/null; do
    /bin/sleep 10
    t=$(( $(date +%s) - start ))
    [ -f "$dir/frame.ppm" ] && cp "$dir/frame.ppm" "$dir/t$(printf %04d $t).ppm" 2>/dev/null
done
rm -f "$dir/frame.ppm"
for f in "$dir"/t*.ppm; do
    [ -f "$f" ] || continue
    "$PY" "$here/tools/ppm2png.py" "$f" "${f%.ppm}.png" >/dev/null && rm -f "$f"
done
echo "done: $(ls "$dir"/t*.png 2>/dev/null | wc -l) frames, $(ls "$dir/shaders" | wc -l) shaders"
