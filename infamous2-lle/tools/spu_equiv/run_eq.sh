#!/bin/sh
# Equivalence harness for lifter codegen changes: the same SPU images lifted two ways must
# behave the same, function by function.
#
#   tools/spu_equiv/run_eq.sh <liftA root> <liftB root> <out dir> [seeds] [jobs] [extra -include for B]
#
# A root is a tools/relift_spu.sh destination (<root>/spu/*/spu_recomp.c, <root>/lle_spu/*/...),
# e.g. A = SPU_LIFT_LEAN_SHUFB=0 SPU_LIFT_LEAN_LS=0 (the old form), B = the defaults or
# SPU_LIFT_MERGE_CHUNKS=1. Every image is compiled twice against the toolkit's real runtime
# headers (TK=<toolkit>, default ../ps3recomp-fork) with spu_equiv_pre.h force-included, and
# spu_equiv_main.c runs every function of its table on seeded random register files and local
# stores, with random stack hints and pending interrupts so the trampoline side of every
# SPU_DIRECT / chunk gate is taken too. Each run prints a hash of every runtime call (with the
# register file then), every cross-chunk transfer, the drained trampoline chain and the final
# registers, LS, pc and status; runs that hit the 200 ms timer (loops on random data) are
# skipped on both sides. Prints per image: runs, skipped, differing (must be 0).
# Planted-bug check: run_eq.sh A B out 3 2 $PWD/tools/spu_equiv/plant_gate.h (a gate that
# forgets the stack-reset hint) must report differences.
A=$1; B=$2; O=$3; seeds=${4:-3}; jobs=${5:-4}; extra=${6:-}
here=$(cd "$(dirname "$0")" && pwd)
TK=${TK:-$here/../../../ps3recomp-fork}
mkdir -p "$O"
one() {
    rel=$1; n=$(echo "$rel" | tr / _)
    for side in A B; do
        root=$A; inc=""; [ $side = B ] && { root=$B; [ -n "$extra" ] && inc="-include $extra"; }
        cc -O1 -w -fno-strict-aliasing -fwrapv -I$TK/runtime/spu -I$TK/include -include "$here/spu_equiv_pre.h" $inc \
           -DLIFTED="\"$root/$rel/spu_recomp.c\"" "$here/spu_equiv_main.c" -o "$O/$n.$side.x" 2> "$O/$n.$side.cc" || { echo "$rel $side: COMPILE FAILED"; return; }
        "$O/$n.$side.x" "$seeds" > "$O/$n.$side.txt" 2>/dev/null
        rm -f "$O/$n.$side.x"
    done
    echo "$rel $(python3 "$here/cmp.py" "$O/$n.A.txt" "$O/$n.B.txt")"
}
cd "$A" || exit 1
for d in spu/*/ lle_spu/*/; do [ -f "$A/$d/spu_recomp.c" ] && echo "${d%/}"; done | \
{ while read rel; do
    while [ "$(jobs -r | wc -l)" -ge "$jobs" ]; do sleep 0.2; done
    one "$rel" &
  done; wait; }
