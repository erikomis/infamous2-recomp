#!/usr/bin/env python3
"""relift_jobmod.py MODULE [--extra 0xADDR,...] [--jobmods DIR] [--out DIR]

Re-lift one WWS job module (spu/jobmods/<MODULE>.bin -> spu/<MODULE>/) the way
tools/lift_jobmods.py does, with extra function entries. The registration TU
is left alone: the lift keeps its symbol prefix and its register function
picks up the new chunks.

Use it when a job keeps landing on an address the lift has no entry for:
`[rejoin] ... (from 0xADDR)` with SPU_REJOIN_DBG, or `branched into unlifted
LS 0xADDR`. The address is an LS address (the module loads at 0x4000).
"""
import argparse, os, struct, subprocess, sys

here = os.path.dirname(os.path.abspath(__file__))
toolkit = os.environ.get('PS3RECOMP', os.path.join(here, '..', '..', 'ps3recomp-fork'))
sys.path.insert(0, os.path.join(toolkit, 'tools'))
import lift_jobmods as lj   # noqa: E402  (wrap, manual_link_targets, switch_table_targets, BASE)

ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
ap.add_argument('module', help='jobmod_<sha>_e<entry>, with or without .bin')
ap.add_argument('--extra', default='', help='comma-separated extra LS entry addresses')
ap.add_argument('--jobmods', default=os.path.join(here, '..', 'spu', 'jobmods'))
ap.add_argument('--out', default=os.path.join(here, '..', 'spu'))
a = ap.parse_args()

name = a.module[:-4] if a.module.endswith('.bin') else a.module
raw = open(os.path.join(a.jobmods, name + '.bin'), 'rb').read()
entry_off = struct.unpack_from('>I', raw, 0x10)[0]
base = lj.BASE
entry = base + entry_off
outsub = os.path.join(a.out, name)
tmp_elf = os.path.join(a.out, '_tmp_' + name + '.elf')
with open(tmp_elf, 'wb') as f:
    f.write(lj.wrap(raw, base=base, entry=entry))

extras = [hex(base + 0x30)]
extras += [hex(x) for x in lj.manual_link_targets(raw)]
extras += [hex(x) for x in lj.switch_table_targets(raw)]
for x in a.extra.split(','):
    x = x.strip()
    if not x:
        continue
    v = int(x, 0)
    if not (base <= v < base + len(raw)):
        sys.exit(f'{x} is outside this module ({hex(base)}..{hex(base + len(raw))})')
    extras.append(hex(v))

cmd = [sys.executable, os.path.join(toolkit, 'tools', 'spu_lifter.py'), '--auto-functions', tmp_elf,
       '--base', hex(base), '--symbol-prefix', name + '_', '-o', outsub,
       '--source-name', 'spu_recomp.c', '--header-name', 'spu_recomp.h',
       '--extra-funcs', ','.join(sorted(set(extras)))]
if name in lj.CODE_END:
    cmd += ['--code-end', hex(lj.CODE_END[name])]
print(' '.join(os.path.basename(c) if c.endswith('.py') else c for c in cmd[1:]))
r = subprocess.run(cmd, capture_output=True, text=True)
os.remove(tmp_elf)
print(r.stdout.strip().splitlines()[-4:] and '\n'.join(r.stdout.strip().splitlines()[-4:]))
if r.returncode != 0:
    sys.exit(r.stderr)
