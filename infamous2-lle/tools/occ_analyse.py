"""Offline check of inFAMOUS 2's occlusion-volume passes from a
PS3RECOMP_ENG_OCCDUMP directory.

For every volume draw i: the GPU's alpha is rt_{i+1}.G / rt_i.G (blend
ZERO/SRC_ALPHA, colour mask G). For p113 (5f740845528b0e62) the program's
math is recomputed per pixel on the CPU from the dumped depth snapshot
(unit0), normals (unit1), the raw cone LUT and the draw's constants, and
compared with the GPU's alpha where the draw touched the pixel.
Usage: occ_analyse.py <dumpdir> [--png prefix]
"""
import os, re, sys
import numpy as np

d = sys.argv[1]
W, H = 1280, 720

def load_rgba(name, w=W, h=H):
    a = np.fromfile(os.path.join(d, name), dtype=np.uint8)
    return a.reshape(h, w, 4)

draws = []
cur = None
for line in open(os.path.join(d, 'draws.txt')):
    t = line.split()
    if t[0] == 'draw':
        cur = {'i': int(t[1]), 'fp': t[5], 'p': int(t[7]), 'n': int(t[9]), 'k': {}, 'c': {}, 'tex': {}, 'line': line.strip()}
        m = re.search(r'off 0x(\w+) (\d+)x(\d+)', line)
        cur['rt_off'] = int(m.group(1), 16); cur['w'] = int(m.group(2)); cur['h'] = int(m.group(3))
        draws.append(cur)
    elif t[0] == 'k':
        cur['k'][int(t[1])] = np.array([float(x) for x in t[2:6]])
    elif t[0] == 'c':
        cur['c'][int(t[1])] = np.array([float(x) for x in t[2:6]])
    elif t[0] == 'xf':
        cur['xf'] = [float(x) for x in t[1:9]]
    elif t[0] == 'tex':
        m = re.match(r'tex (\d) en (\d) h(\d+) loc (\d) off 0x(\w+) fmt 0x(\w+) (\d+)x(\d+) depth (\d+) dim (\d+) remap 0x(\w+) filter 0x(\w+) addr 0x(\w+)', line)
        cur['tex'][int(m.group(1))] = dict(en=int(m.group(2)), loc=int(m.group(4)), off=int(m.group(5), 16),
                                           fmt=int(m.group(6), 16), w=int(m.group(7)), h=int(m.group(8)),
                                           depth=int(m.group(9)), remap=m.group(11), filter=m.group(12), addr=m.group(13))

print('%d volume draws' % len(draws))
z = load_rgba('unit0.rgba').astype(np.float64)     # R=Zmid G=Zlo B=stencil A=Zhi
nrm = load_rgba('unit1.rgba').astype(np.float64) if os.path.exists(os.path.join(d, 'unit1.rgba')) else None
depth = z[..., 3] / 256.0 + z[..., 0] / 65536.0 + z[..., 1] / 16777216.0
stencil = z[..., 2].astype(np.int32)

py, px = np.mgrid[0:H, 0:W].astype(np.float64)

def lut_load(t):
    raw = np.fromfile(os.path.join(d, 'lut_%u_%08X.bin' % (t['loc'], t['off'])), dtype=np.uint8)
    w, h, dep = t['w'], t['h'], t['depth']
    pitch = w * 4
    a = raw[:dep * h * pitch].reshape(dep, h, w, 4).astype(np.float64)   # bytes A,R,G,B
    e = a / 255.0 * 2 - 1
    return e  # [z][y][x][A,R,G,B]

def sample3d(lut, u, v, s):
    dep, h, w, _ = lut.shape
    def axis(c, n):
        x = np.clip(c * n - 0.5, 0, n - 1)
        i0 = np.floor(x).astype(int); i1 = np.minimum(i0 + 1, n - 1); f = x - i0
        return i0, i1, f
    x0, x1, fx = axis(u, w); y0, y1, fy = axis(v, h); z0, z1, fz = axis(s, dep)
    out = 0
    for zi, wz in ((z0, 1 - fz), (z1, fz)):
        for yi, wy in ((y0, 1 - fy), (y1, fy)):
            for xi, wx in ((x0, 1 - fx), (x1, fx)):
                out = out + lut[zi, yi, xi] * (wz * wy * wx)[..., None]
    return out

def norm(v):
    l = np.sqrt((v * v).sum(-1, keepdims=True))
    return v / np.where(l == 0, np.nan, l)

def sat(x): return np.clip(np.nan_to_num(x, nan=0.0), 0, 1)

def p113_alpha(dr):
    k, c, xf = dr['k'], dr['c'], dr['xf']
    ndc_x = ((2 * (px + 0.5) / W - 1) - xf[4]) / xf[0]
    ndc_y = ((1 - 2 * (py + 0.5) / H) - xf[5]) / xf[1]
    c467 = c[467]; c466 = c.get(466, np.zeros(4))
    u = ndc_x * c467[0] + c467[2]; v = ndc_y * c467[1] + c467[3]
    ray = u[..., None] * c[264] + v[..., None] * c[265] + c[267] + c466[0]
    ph = ray + depth[..., None] * k[2]
    P = ph[..., :3] / ph[..., 3:4]
    n = (np.floor(nrm[..., :3] + 0.5) - 128) / 127.0   # bytes R,G,B
    N = norm(n)
    P1 = np.concatenate([P + N * k[4][0], np.full(P.shape[:2] + (1,), k[3][0])], -1)
    r2 = np.stack([(P1 * k[7]).sum(-1), (P1 * k[6]).sum(-1), (P1 * k[5]).sum(-1)], -1)
    lut = lut_load(dr['tex'][2])
    h1 = sample3d(lut, r2[..., 0], r2[..., 1], r2[..., 2])   # A,R,G,B -> x,y,z,w
    h2xyz = h1[..., 1:2] * k[8][:3] + h1[..., 0:1] * k[10][:3] + h1[..., 2:3] * k[12][:3]
    h2w = h1[..., 3] * k[9][3] + k[9][3]
    ang = h2w * k[11][3]
    D = norm(h2xyz)
    nd = (N * D).sum(-1)
    s = np.sin(ang)
    with np.errstate(all='ignore'):
        cone = sat((nd + s) / (2 * s))
    strength = k[13][0] * k[14][0] * h2w
    return sat(-strength * cone + k[15][0]), dict(P=P, N=N, r2=r2, h2w=h2w, nd=nd, cone=cone, strength=strength)

rts = sorted(f for f in os.listdir(d) if f.startswith('rt_'))
G = [load_rgba(f)[..., 1].astype(np.float64) for f in rts]
total_before, total_after = G[0], G[-1]
print('AO G mean before %.1f after %.1f; pixels darkened >10%%: %d' %
      (total_before.mean(), total_after.mean(), ((total_after < total_before * 0.9) & (total_before > 20)).sum()))
np.save(os.path.join(d, 'ratio_total.npy'), np.where(total_before > 0, total_after / np.maximum(total_before, 1), 1))

for i, dr in enumerate(draws):
    if i + 1 >= len(G): break
    b, a = G[i], G[i + 1]
    touched = (a != b)
    gpu = np.where(b > 8, a / np.maximum(b, 1), np.nan)
    msg = 'draw %2d %s p%-3d n=%-3d touched %6d' % (i, dr['fp'][:6], dr['p'], dr['n'], touched.sum())
    if touched.any():
        msg += ' gpu alpha mean(touched) %.3f min %.3f' % (np.nanmean(gpu[touched]), np.nanmin(gpu[touched]) if np.isfinite(gpu[touched]).any() else -1)
    if dr['fp'].startswith('5f7408') and nrm is not None and 2 in dr['tex']:
        try:
            cpu, dbg = p113_alpha(dr)
        except Exception as ex:
            print(msg, 'cpu failed', ex); continue
        m = touched & (b > 40) & np.isfinite(gpu)
        if m.any():
            err = np.abs(cpu[m] - gpu[m])
            msg += ' | cpu mean %.3f  |cpu-gpu| mean %.3f p95 %.3f' % (cpu[m].mean(), err.mean(), np.percentile(err, 95))
        cen = dr['c'][259][:3]
        msg += ' | centre (%.0f,%.0f,%.0f) half %.0f' % (cen[0], cen[1], cen[2], np.linalg.norm(dr['c'][256][:3]))
    print(msg)

# ---- G-buffer normals vs geometry: reconstruct positions with the first p113
# draw's ray constants and compare each pixel's G-buffer normal with the
# normal of the reconstructed surface (cross product of neighbours).
def ppm(path, rgb):
    rgb = np.clip(rgb, 0, 255).astype(np.uint8)
    with open(path, 'wb') as f:
        f.write(b'P6 %d %d 255\n' % (rgb.shape[1], rgb.shape[0])); f.write(rgb.tobytes())

p113 = [dr for dr in draws if dr['fp'].startswith('5f7408')]
if p113 and nrm is not None:
    dr = p113[0]
    _, dbg = p113_alpha(dr)
    P = dbg['P']; N = dbg['N']
    dx = P[1:-1, 2:] - P[1:-1, :-2]; dy = P[2:, 1:-1] - P[:-2, 1:-1]
    Ng = norm(np.cross(dx, dy))
    Nc = N[1:-1, 1:-1]
    # orient the geometric normal like the G-buffer one (winding unknown)
    dot = (Ng * Nc).sum(-1)
    valid = np.isfinite(dot) & (depth[1:-1, 1:-1] < 0.99999)
    # smooth regions only: neighbouring depth differences small
    print('normals vs geometry: pixels %d  |dot| median %.3f  dot median %.3f  frac dot<0 %.3f' %
          (valid.sum(), np.median(np.abs(dot[valid])), np.median(dot[valid]), (dot[valid] < 0).mean()))
    img = np.zeros((H, W, 3)); d2 = np.nan_to_num(dot)
    img[1:-1, 1:-1, 0] = np.where(d2 < 0, -d2, 0) * 255; img[1:-1, 1:-1, 1] = np.where(d2 > 0, d2, 0) * 255
    ppm(os.path.join(d, 'normals_vs_geom.ppm'), img)
    ppm(os.path.join(d, 'gbuf_normals.ppm'), (N * 0.5 + 0.5) * 255)
    ppm(os.path.join(d, 'geom_normals.ppm'), np.nan_to_num(np.pad(Ng, ((1, 1), (1, 1), (0, 0)))) * 127.5 + 127.5)
    r = np.load(os.path.join(d, 'ratio_total.npy'))
    ppm(os.path.join(d, 'ao_ratio.ppm'), np.repeat((np.clip(r, 0, 1) * 255)[..., None], 3, -1))
