#!/bin/sh
# Windowed play session with an on-demand dump: tools/play_dump.sh <name>
# Play to the spot that looks wrong, then press Enter in this terminal: the
# next frame's render targets go to out/<name>/surf/, its pass log and the
# texture creations to out/<name>.log, every pipeline's shaders (with
# manifest.txt) to out/<name>/shaders/. Press Enter again for another frame;
# quit the game window to finish.
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}
dir="$here/out/$name"; rm -rf "$dir"; mkdir -p "$dir/shaders" "$dir/surf"
trig="$dir/.dump-now"; rm -f "$trig"

PS3RECOMP_ENG_TEXLOG=1 PS3RECOMP_ENG_SHADER_DUMP="$dir/shaders" \
PS3RECOMP_METAL_SURF_DUMP="$dir/surf" PS3RECOMP_METAL_SURF_DUMP_EVERY=100000000 \
PS3RECOMP_METAL_PASS_LOG=2 PS3RECOMP_METAL_PASS_LOG_MIN=1 PS3RECOMP_METAL_PASS_LOG_EVERY=100000000 \
PS3RECOMP_METAL_PASS_LOG_FLUSH=1 PS3RECOMP_METAL_DUMP_TRIGGER="$trig" \
PS3RECOMP_METAL_FRAME_DUMP="$dir/frame.ppm" \
    "$here/run.sh" > "$here/out/$name.log" 2>&1 &
gp=$!
echo "running (pid $gp). Go to the bad spot, then press Enter here to dump that frame; quit the game to finish."
n=0
while kill -0 $gp 2>/dev/null; do
    if read -t 2 _line; then
        n=$((n + 1)); touch "$trig"
        cp "$dir/frame.ppm" "$dir/shot$n.ppm" 2>/dev/null
        echo "dump $n requested"
    fi
done
for f in "$dir"/shot*.ppm; do
    [ -f "$f" ] || continue
    "$here/../.venv/bin/python" "$here/tools/ppm2png.py" "$f" "${f%.ppm}.png" >/dev/null && rm -f "$f"
done
echo "done: $(ls "$dir/surf" | wc -l) surfaces, $(ls "$dir/shaders" | wc -l) shaders, $(ls "$dir"/shot*.png 2>/dev/null | wc -l) shots"
