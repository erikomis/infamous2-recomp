#!/bin/sh
# Relift every SPU image, job/data/relocatable module and the libsre SPU kernel with the
# current toolkit lifter, the way rebuild.py's spu/jobs/firmware steps do:
#
#   tools/relift_spu.sh [toolkit] [dest]     # default: ../ps3recomp-fork, this port dir
#
# rebuild.py relifts by itself whenever the toolkit patch changes (the spu, jobs and
# firmware steps are keyed on it); this is the same for a development tree, after
# folding a lifter change into ps3recomp-fork/. It reuses the extracted spu/images/ and
# spu/jobmods/ and rewrites <dest>/spu/*/ and <dest>/lle_spu/ (lle/libsre is only read),
# so a different <dest> leaves this tree alone. Lifter switches reach every lift through
# the environment, e.g. SPU_LIFT_REG_LOCALS=0 SPU_LIFT_LEAN_FLOAT=0 for the form before
# 2026-11-05. Then rebuild (cmake --build build): only the SPU sources recompile.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
tk=$(cd "${1:-$here/../ps3recomp-fork}" && pwd)
dest=${2:-$here}
P="${PY:-$here/../.venv/bin/python}"
elf="$here/../infamous2/game/EBOOT.elf"
[ -f "$here/spu/images/spu_0000_at_007FA100.elf" ] && [ -d "$here/spu/jobmods" ] ||
    { echo "no spu/images or spu/jobmods here: run the full recipe (README) first"; exit 1; }
mkdir -p "$dest"; dest=$(cd "$dest" && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/relift.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/spu" "$tmp/lle_spu" "$tmp/out"
cp -R "$here/spu/images" "$here/spu/jobmods" "$tmp/spu/"
ln -s "$here/lle" "$tmp/lle"
export PS3RECOMP="$tk"
T="$tk/tools"
cd "$tmp"
echo "relift_spu: libsre SPU kernel"
"$P" "$T/lift_firmware_spu.py" --module-dir lle/libsre --base 0x30100000 --name libsre --first-image-id 100 \
    --body 0x21480:0x2200:0xA00:0xA00:0x2110 --body 0x23680:0x1E40:0xA00:0xA00:0x1C80 \
    --body 0x2A180:0x3DA0:0xA00:0xA00:0x3D00 --out lle_spu > out/log.txt 2>&1
echo "relift_spu: SPU images"
"$P" "$T/build_spu_workloads.py" --images spu/images --lifted spu --out spu/spu_workloads.c \
    --register-fn infamous2_spu_register_all --constructor --title infamous2 --eboot "$elf" \
    --extra-funcs wwsjob_pm_at_008E5B00=0xA2C --extra-funcs spu_0000_at_007FA100=0x9C84,0x9DB8 >> out/log.txt 2>&1
echo "relift_spu: job, data and relocatable modules"
"$P" "$T/lift_jobmods.py" --jobmods spu/jobmods --out spu --register spu/jobmods_register.c \
    --symbol-prefix infamous2 >> out/log.txt 2>&1
"$P" "$here/tools/relift_jobmod.py" jobmod_ab654d30fd90_efe70 --extra 0x15280,0x155EC \
    --jobmods spu/jobmods --out spu >> out/log.txt 2>&1
"$P" "$here/tools/relift_jobmod.py" jobmod_cbb4b5c85d75_e59f8 --extra 0x6C44,0x6C64 \
    --jobmods spu/jobmods --out spu >> out/log.txt 2>&1
"$P" "$here/tools/lift_datamods.py" "$elf" --out spu --extra 0x6C44,0x6C64 >> out/log.txt 2>&1
"$P" "$here/tools/lift_relocmods.py" "$elf" --out spu >> out/log.txt 2>&1
rm -f lle
# swap in: every lifted directory and generated registration file, images/jobmods kept
mkdir -p "$dest/spu"
for d in spu/*/; do
    n=$(basename "$d"); [ "$n" = images ] || [ "$n" = jobmods ] && continue
    rm -rf "$dest/spu/$n"; cp -R "$d" "$dest/spu/$n"
done
cp spu/*.c spu/*.json "$dest/spu/" 2>/dev/null || true
[ -L "$dest/lle_spu" ] && rm "$dest/lle_spu"
rm -rf "$dest/lle_spu"; cp -R lle_spu "$dest/lle_spu"
echo "relift_spu: done ($dest/spu, $dest/lle_spu)"
