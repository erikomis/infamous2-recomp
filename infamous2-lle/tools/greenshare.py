#!/usr/bin/env python3
"""greenshare.py DIR... -- for every t*.png a capture left, the share of
saturated-green pixels (G > 200, R and B < 90) and the mean brightness.
No dependencies: reads the PNGs ppm2png.py writes (8-bit RGB, filter 0)."""
import glob, os, struct, sys, zlib

def pixels(p):
    d = open(p, 'rb').read(); i = 8; idat = b''; w = h = 0
    while i < len(d):
        n = struct.unpack('>I', d[i:i+4])[0]; t = d[i+4:i+8]; c = d[i+8:i+8+n]
        if t == b'IHDR': w, h = struct.unpack('>II', c[:8])
        if t == b'IDAT': idat += c
        i += 12 + n
    return w, h, zlib.decompress(idat)

for d in sys.argv[1:]:
    for p in sorted(glob.glob(os.path.join(d, 't*.png'))):
        w, h, raw = pixels(p); st = w * 3 + 1; g = t = 0; lum = 0
        for y in range(0, h, 4):
            row = raw[y*st+1:(y+1)*st]
            for x in range(0, w*3, 12):
                r, gg, b = row[x], row[x+1], row[x+2]; t += 1; lum += r + gg + b
                if gg > 200 and r < 90 and b < 90: g += 1
        print(f"{p}: green {100*g/t:5.2f}%  mean {lum/t/3:4.0f}")
