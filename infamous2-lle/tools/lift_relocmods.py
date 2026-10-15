#!/usr/bin/env python3
"""lift_relocmods.py EBOOT.elf [--out spu] [--first-image-id 300]

The SPU job code inFAMOUS 2 runs in New Marais. The EBOOT carries ~30 position-
independent blobs (4 x ila, then `44012850 32000080 44012850 32xxxx80`) that the
SPURS kernel streams to a different local-store address on every run -- 0x4C00,
0x6400, 0x1A400, 0x1DC00, ... -- and enters there. A fixed-address lift can never
match them, so all of them ran on the SPU interpreter: ~11-14 fps in the open
world and audio rendered at a third of real time.

Each blob is wrapped as an SPU ELF at LS 0, lifted with the toolkit's SPU lifter
in --relocatable mode (every address derived from the code's own position adds
spu_context.code_base), and <out>/relocmods_register.c registers it with
spu_overlay_register_region_reloc at its guest address: the GET that loads it
makes the lift resident wherever it landed, and dispatch resolves pc - lsa.
"""
import argparse, os, re, struct, subprocess, sys

here = os.path.dirname(os.path.abspath(__file__))
toolkit = os.environ.get('PS3RECOMP', os.path.join(here, '..', '..', 'ps3recomp-fork'))
sys.path.insert(0, os.path.join(toolkit, 'tools'))
import lift_jobmods as lj          # noqa: E402  (manual_link_targets, switch_table_targets)
from wrap_spu_elf import wrap      # noqa: E402

SIG = bytes.fromhex('440128503200008044012850')
INC_RE = re.compile(r'#include "(?:\.\./)+runtime/spu/')


def find_blobs(d):
    """(file offset, span) of every blob: header at a 16-byte boundary, four
    ila/ilhu-class words, then the shared prologue. The span runs to the next
    blob (padding included: a GET of the shorter real length is still inside
    it), or to the first 256 zero bytes for the last one."""
    starts, i = [], d.find(SIG)
    while i != -1:
        s = i - 16
        if s >= 0 and s % 16 == 0 and all((struct.unpack_from('>I', d, s + 4 * k)[0] >> 24) in (0x42, 0x43)
                                          for k in range(4)):
            starts.append(s)
        i = d.find(SIG, i + 4)
    out = []
    for n, s in enumerate(starts):
        if n + 1 < len(starts):
            span = starts[n + 1] - s
        else:
            z = d.find(b'\x00' * 256, s)
            span = ((z if z > 0 else len(d)) - s + 15) & ~15
        out.append((s, min(span, 0x3C000)))
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('eboot')
    ap.add_argument('--out', default=os.path.join(here, '..', 'spu'))
    ap.add_argument('--first-image-id', type=int, default=300)
    ap.add_argument('--va-offset', type=lambda x: int(x, 0), default=0x10000,
                    help='guest address = file offset + this (the first PT_LOAD)')
    ap.add_argument('--extra', default='',
                    help='comma-separated EA+offset pairs "0xEA:0xOFF" to add as entries '
                         '(interpreter entries seen inside a resident blob)')
    a = ap.parse_args()

    d = open(a.eboot, 'rb').read()
    blobs = find_blobs(d)
    if not blobs:
        sys.exit('no relocatable job blobs found')
    extra = {}
    for tok in filter(None, (t.strip() for t in a.extra.split(','))):
        ea, off = (int(x, 0) for x in tok.split(':'))
        extra.setdefault(ea, set()).add(off)

    regs = []
    for n, (s, span) in enumerate(blobs):
        ea = s + a.va_offset
        raw = d[s:s + span]
        name = f'relocmod_{ea:08x}'
        prefix = name + '_'
        outsub = os.path.join(a.out, name)
        os.makedirs(outsub, exist_ok=True)
        tmp = os.path.join(a.out, f'_tmp_{name}.elf')
        with open(tmp, 'wb') as f:
            f.write(wrap(raw, base=0, entry=0))
        extras = {'0x0'}
        extras |= {hex(x) for x in lj.manual_link_targets(raw, base=0)}
        extras |= {hex(x) for x in lj.switch_table_targets(raw, base=0)}
        extras |= {hex(x) for x in extra.get(ea, ())}
        cmd = [sys.executable, os.path.join(toolkit, 'tools', 'spu_lifter.py'), '--auto-functions', tmp,
               '--base', '0', '--relocatable', '--symbol-prefix', prefix, '-o', outsub,
               '--source-name', 'spu_recomp.c', '--header-name', 'spu_recomp.h',
               '--code-end', hex(span), '--extra-funcs', ','.join(sorted(extras, key=lambda x: int(x, 16)))]
        r = subprocess.run(cmd, capture_output=True, text=True)
        os.remove(tmp)
        if r.returncode != 0:
            sys.exit(f'{name}: lift failed\n{r.stderr[-3000:]}')
        for fn in ('spu_recomp.c', 'spu_recomp.h'):
            p = os.path.join(outsub, fn)
            t = open(p).read()
            open(p, 'w').write(INC_RE.sub('#include "', t))
        funcs = next((l.strip() for l in r.stdout.splitlines() if l.strip().endswith('function(s) lifted')), '?')
        img = a.first_image_id + n
        print(f'  {name}: file+0x{s:X} span 0x{span:X} -> image {img}, {funcs}')
        regs.append((name, prefix, ea, span, img))

    reg = os.path.join(a.out, 'relocmods_register.c')
    L = ['/* relocmods_register.c -- GENERATED by tools/lift_relocmods.py.',
         ' * Position-independent SPU job code streamed from the EBOOT to a different',
         ' * local-store address on every run. Lifted at LS 0 in --relocatable mode and',
         ' * registered as relocatable code regions at their guest addresses: the GET',
         ' * that loads one makes its lift resident wherever it landed. */',
         '#include <stdint.h>', '',
         'extern void spu_begin_image(int image_id);',
         'extern void spu_overlay_register_region_reloc(uint32_t content_ea, uint32_t span, int image_id);']
    L += [f'extern void {p}spu_recomp_register(void);' for _, p, _, _, _ in regs]
    L += ['', '__attribute__((constructor)) static void infamous2_relocmods_register(void)', '{',
          '    /* PS3_RELOCMODS=0 leaves them to the interpreter (A/B comparisons). */',
          '    { extern char* getenv(const char*); const char* e = getenv("PS3_RELOCMODS");',
          '      if (e && e[0] == \'0\') return; }']
    for name, p, ea, span, img in regs:
        L.append(f'    spu_begin_image({img}); {p}spu_recomp_register();')
        L.append(f'    spu_overlay_register_region_reloc(0x{ea:08X}u, 0x{span:X}u, {img});   /* {name} */')
    L += ['    spu_begin_image(0);', '}', '']
    open(reg, 'w').write('\n'.join(L))
    print(f'wrote {reg} ({len(regs)} modules)')


if __name__ == '__main__':
    main()
