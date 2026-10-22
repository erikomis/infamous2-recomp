#!/usr/bin/env python3
"""Bring already-lifted SPU sources to the SPU_DIRECT form without relifting.

  tools/spu_direct_rewrite.py <spu dir> [...]

The toolkit's spu_lifter.py now emits a known-target cross-function branch as
`{ SPU_DIRECT(ctx, <pc>, <fn>); }` (spu_context.h: a tail call when the drain
has nothing to do on that step, the trampoline otherwise) instead of
`{ ctx->pc = <pc>; g_spu_trampoline_fn = <fn>; return; }`. A fresh rebuild
relifts and gets it; this rewrites a tree lifted before, in place: every
<dir>/*/spu_recomp.c. Idempotent (an already rewritten file has no match).
Indirect branches (spu_indirect_branch) are left alone.
"""
import glob
import os
import re
import sys

PAT = re.compile(r'\{ ctx->pc = ([^;{}]*); g_spu_trampoline_fn = '
                 r'([A-Za-z0-9_]+_spu_func_[0-9A-F]{8}); return; \}')


def main(dirs):
    total = files = 0
    for d in dirs:
        for f in sorted(glob.glob(os.path.join(d, '*', 'spu_recomp.c'))):
            with open(f) as fh:
                s = fh.read()
            s2, n = PAT.subn(r'{ SPU_DIRECT(ctx, \1, \2); }', s)
            if n:
                with open(f, 'w') as fh:
                    fh.write(s2)
                total += n
                files += 1
    print(f"spu_direct_rewrite: {total} branch(es) in {files} file(s)")


if __name__ == '__main__':
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    main(sys.argv[1:])
