#!/usr/bin/env python3
"""psfeatures.py DIR [PIPE...] -- one line per PS3RECOMP_ENG_SHADER_DUMP pixel
shader: the texture units it samples, cube units, how many colour targets it
writes, and operations that can blow a channel up (divisions, exp2, rsqrt,
normalize). With pipeline numbers, prints only those."""
import glob, os, re, sys

d = sys.argv[1]
want = {int(x) for x in sys.argv[2:]}
rows = []
for p in sorted(glob.glob(os.path.join(d, 'p*_ps.hlsl')),
                key=lambda s: int(re.search(r'p(\d+)_', s)[1])):
    n = int(re.search(r'p(\d+)_', p)[1])
    if want and n not in want:
        continue
    s = open(p, errors='replace').read()
    units = sorted({int(u) for u in re.findall(r'rsx_tex(?:\[(\d+)\]|(\d+))\.(?:Sample|Load|Gather)', s)
                    for u in u if u})
    cubes = sorted({int(u) for u in re.findall(r'TextureCube\s+rsx_tex(\d+)', s)})
    cubes = [u for u in cubes if u in units]
    targets = len(set(re.findall(r'SV_T(?:arget|ARGET)(\d*)', s)))
    ops = {k: len(re.findall(v, s)) for k, v in {
        'div': r'[^/]/[^/*]', 'exp2': r'\bexp2\(', 'rsq': r'\brsqrt\(',
        'norm': r'\bnormalize\(', 'pow': r'\bpow\(', 'cmp': r'SampleCmp|_cmp\(|shadow'}.items()}
    body = s[s.find('main('):]
    lines = body.count('\n')
    rows.append(f"p{n:<4d} tex={','.join(map(str, units)):<18s} cube={','.join(map(str, cubes)) or '-':<5s} "
                f"rt={targets} lines={lines:<4d} " + ' '.join(f'{k}={v}' for k, v in ops.items() if v))
print('\n'.join(rows))
