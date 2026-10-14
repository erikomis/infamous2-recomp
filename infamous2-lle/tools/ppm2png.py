#!/usr/bin/env python3
"""ppm2png.py in.ppm out.png -- binary P6 to PNG, no dependencies."""
import struct, sys, zlib
d = open(sys.argv[1], 'rb').read()
assert d[:2] == b'P6'
parts, i = [], 2
while len(parts) < 3:
    while d[i:i+1].isspace(): i += 1
    j = i
    while not d[j:j+1].isspace(): j += 1
    parts.append(int(d[i:j])); i = j
w, h, _ = parts
px = d[i+1:i+1+w*h*3]
raw = b''.join(b'\0' + px[y*w*3:(y+1)*w*3] for y in range(h))
def chunk(t, b): return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t + b))
open(sys.argv[2], 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
                              + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))
nonblack = sum(1 for k in range(0, len(px), 3 * 97) if px[k] | px[k+1] | px[k+2])
print(f'{w}x{h}, sampled non-black pixels: {nonblack}/{len(px)//(3*97)}')
