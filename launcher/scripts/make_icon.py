#!/usr/bin/env python3
# Regenerate the placeholder icon: python3 scripts/make_icon.py src-tauri/icons/app-icon.png
# then: npx tauri icon src-tauri/icons/app-icon.png
# An original placeholder icon: a lightning bolt on a dark rounded square.
import struct, zlib, sys
N = 1024
bolt = [(600, 90), (300, 560), (500, 560), (400, 934), (740, 420), (530, 420), (650, 90)]
def inside(px, py, poly):
    c = False
    j = len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]; xj, yj = poly[j]
        if (yi > py) != (yj > py) and px < (xj - xi) * (py - yi) / (yj - yi) + xi:
            c = not c
        j = i
    return c
def rr(px, py, r=200, m=40):
    x0, y0, x1, y1 = m, m, N - m, N - m
    cx = min(max(px, x0 + r), x1 - r); cy = min(max(py, y0 + r), y1 - r)
    return (px - cx) ** 2 + (py - cy) ** 2 <= r * r and x0 <= px <= x1 and y0 <= py <= y1
rows = []
S = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)]
for y in range(N):
    row = bytearray([0])
    for x in range(N):
        a = b = 0
        for sx, sy in S:
            px, py = x + sx, y + sy
            if rr(px, py):
                a += 1
                if inside(px, py, bolt):
                    b += 1
        if a == 0:
            row += b'\0\0\0\0'; continue
        t = y / N
        bg = (int(18 + 20 * t), int(24 + 30 * t), int(48 + 60 * t))
        fg = (120, 200, 255)
        f = b / a
        col = [int(bg[i] * (1 - f) + fg[i] * f) for i in range(3)]
        row += bytes(col + [int(255 * a / 4)])
    rows.append(bytes(row))
def chunk(t, d):
    return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', N, N, 8, 6, 0, 0, 0)) \
    + chunk(b'IDAT', zlib.compress(b''.join(rows), 9)) + chunk(b'IEND', b'')
open(sys.argv[1], 'wb').write(png)
