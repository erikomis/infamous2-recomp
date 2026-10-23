#!/bin/sh
# Windowed session for the occlusion-volume darkening (dark facades / dark Cole
# at dusk): tools/play_occdump.sh <name>
# The occlusion volumes are drawn (run.sh's default skip is lifted). Play to a
# spot that looks too dark, then press Enter in this terminal: the next frame's
# volume draws are dumped to out/<name>/dNN/ (PS3RECOMP_ENG_OCCDUMP: the AO
# target before and after every volume draw, the depth snapshot, the normals,
# the cone LUTs and each draw's constants) together with a screenshot
# out/<name>/shotNN.png. Press Enter again for another frame; quit the game to
# finish. tools/occ_analyse.py out/<name>/dNN then recomputes every draw.
# OCC_FP=<hex>,<hex>.. dumps those fragment programs' draws instead (up to 4),
# e.g. the sun shadows: OCC_FP=55295a0f6cc1985b,20e579e1b50ec50d,0a1fd0e8c0ad368c
# with PS3RECOMP_ENG_OCCDUMP_SPAN=1 (draws spread over the frame: the mask
# after each shadow cascade; every draw's target and inputs are kept).
here=$(cd "$(dirname "$0")/.." && pwd)
name=${1:?run name}
dir="$here/out/$name"; rm -rf "$dir"; mkdir -p "$dir"
trig="$dir/.dump-now"; rm -f "$trig"

PS3RECOMP_ENG_SKIP_FP= PS3RECOMP_ENG_OCCDUMP="0:$dir${OCC_FP:+:$OCC_FP}" PS3RECOMP_ENG_OCCDUMP_TRIGGER="$trig" \
PS3RECOMP_METAL_FRAME_DUMP="$dir/frame.ppm" \
    "$here/run.sh" > "$here/out/$name.log" 2>&1 &
gp=$!
echo "running (pid $gp). Go to a spot that is too dark, press Enter here to dump that frame; quit the game to finish."
n=0
while kill -0 $gp 2>/dev/null; do
    if read -t 2 _line; then
        n=$((n + 1)); touch "$trig"
        cp "$dir/frame.ppm" "$dir/shot$(printf %02d $n).ppm" 2>/dev/null
        echo "dump $n requested"
    fi
done
for f in "$dir"/shot*.ppm; do
    [ -f "$f" ] || continue
    "$here/../.venv/bin/python" "$here/tools/ppm2png.py" "$f" "${f%.ppm}.png" >/dev/null && rm -f "$f"
done
rm -f "$dir/frame.ppm"
echo "done: $(ls -d "$dir"/d?? 2>/dev/null | wc -l) volume dumps in $dir"
