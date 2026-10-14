#!/usr/bin/env python3
"""passlog.py LOG [FRAME] -- summarise PS3RECOMP_METAL_PASS_LOG output.

Lists the logged frames, then prints one frame's passes (the last one by
default): record range, draw count, colour targets, depth, the textures the
first draw samples and its pipeline. Pipeline numbers are what
PS3RECOMP_METAL_SKIP_PIPE / PS3RECOMP_METAL_DBG_SHOWTEX take and what
PS3RECOMP_ENG_SHADER_DUMP's p<N>_*.hlsl are named by.
"""
import re, sys

log = open(sys.argv[1], errors='replace').read().split('\n')
frames, cur = [], None
for l in log:
    m = re.match(r'\[pass\] ---- frame (\d+): (\d+) records', l)
    if m:
        cur = {'frame': int(m[1]), 'records': int(m[2]), 'passes': []}
        frames.append(cur); continue
    m = re.match(r'\[pass\]\s+(\d+) x(\d+)\s+kind=(\d+) rt=(\S*) depth=(\d+)(.*)', l)
    if m and cur is not None:
        p = {'at': int(m[1]), 'n': int(m[2]), 'kind': int(m[3]), 'rt': m[4],
             'depth': int(m[5]), 'tex': '', 'pipe': None, 'desc': ''}
        rest = m[6]
        t = re.search(r'tex=(.*?)\s*\[p=(\d+) (.*?)\]', rest)
        if t:
            p['tex'], p['pipe'], p['desc'] = t[1].strip(), int(t[2]), t[3]
        cur['passes'].append(p)

print('frames logged:', [(f['frame'], f['records']) for f in frames])
if not frames:
    sys.exit(0)
want = int(sys.argv[2]) if len(sys.argv) > 2 else frames[-1]['frame']
f = next(x for x in frames if x['frame'] == want)
print(f"\n==== frame {f['frame']}: {f['records']} records, {len(f['passes'])} passes")
for p in f['passes']:
    kind = {0: 'clear', 1: 'draw', 2: 'blit'}.get(p['kind'], str(p['kind']))
    line = f"{p['at']:5d} x{p['n']:<4d} {kind:5s} rt={p['rt']:<12s} z={p['depth']:<4d}"
    if p['pipe'] is not None:
        line += f" p={p['pipe']:<3d} tex={p['tex']:<28s} {p['desc']}"
    print(line)
