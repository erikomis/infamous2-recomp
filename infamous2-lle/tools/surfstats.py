#!/usr/bin/env python3
"""surfstats.py DIR [--png] [--green MIN%] -- one line per PS3RECOMP_METAL_SURF_DUMP
target (f<frame>_h<handle>.ppm): size, non-black share, saturated-green share,
mean brightness. --png converts those whose green share is at least MIN% (default 0.2)
to PNG beside the .ppm, so they can be looked at."""
import glob, os, re, subprocess, sys

d = sys.argv[1]
png = '--png' in sys.argv
mn = float(sys.argv[sys.argv.index('--green') + 1]) if '--green' in sys.argv else 0.2
here = os.path.dirname(os.path.abspath(__file__))

def load(p):
    b = open(p, 'rb').read(); parts = []; i = 2
    while len(parts) < 3:
        while b[i:i+1].isspace(): i += 1
        j = i
        while not b[j:j+1].isspace(): j += 1
        parts.append(int(b[i:j])); i = j
    return parts[0], parts[1], b[i+1:]

files = sorted(glob.glob(os.path.join(d, 'f*_h*.ppm')),
               key=lambda s: tuple(int(x) for x in re.search(r'f(\d+)_h(\d+)', s).groups()))
for p in files:
    w, h, px = load(p); n = w * h; step = max(1, n // 6000)
    g = t = nz = 0; lum = 0
    for k in range(0, n, step):
        r, gg, b = px[3*k], px[3*k+1], px[3*k+2]; t += 1; lum += r + gg + b
        nz += (r | gg | b) != 0
        if gg > 200 and r < 90 and b < 90: g += 1
    gp = 100 * g / t
    print(f"{os.path.basename(p)[:-4]:14s} {w}x{h:<5d} nonblack {100*nz/t:4.0f}%  green {gp:5.1f}%  mean {lum/t/3:4.0f}")
    if png and gp >= mn:
        subprocess.run([sys.executable, os.path.join(here, 'ppm2png.py'), p, p[:-4] + '.png'],
                       stdout=subprocess.DEVNULL)
