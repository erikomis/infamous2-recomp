"""CPU recompute of inFAMOUS 2's sun mask (FP 55295a0f6cc1985b), cascade by
cascade, from a PS3RECOMP_ENG_OCCDUMP_SPAN dump that also lists the deferred
composite (f384f111ad5956ba) with PS3RECOMP_ENG_OCCDUMP_UNITS=0x7:

  PS3RECOMP_ENG_OCCDUMP=0:<dir>:55295a0f6cc1985b,f384f111ad5956ba
  PS3RECOMP_ENG_OCCDUMP_SPAN=1 PS3RECOMP_ENG_OCCDUMP_UNITS=0x7
  PS3RECOMP_ENG_OCCDUMP_TRIGGER=<file>     (touch it to dump a frame)

Prints the GPU-vs-CPU difference of the summed mask (the composite's unit 2
R) and, for world receivers (stencil 0) turned away from the sun (the
composite's normals . its sun direction k5), the 2x2 PCF coverage per
cascade: ~0.5 with most pixels partial is the self-compare lattice of the
second-depth map (see PS3RECOMP_ENG_SHADE_BACKFACE).
Usage: sunmask_check.py <dump>/dNN
"""
import os, re, sys
import numpy as np

d = sys.argv[1]; W, H = 1280, 720
draws = []; cur = None
for line in open(os.path.join(d, 'draws.txt')):
    t = line.split()
    if t[0] == 'draw':
        cur = {'fp': t[5], 'k': {}, 'c': {}}; draws.append(cur)
    elif t[0] == 'k': cur['k'][int(t[1])] = np.array([float(x) for x in t[2:6]])
    elif t[0] == 'c': cur['c'][int(t[1])] = np.array([float(x) for x in t[2:6]])
    elif t[0] == 'xf': cur['xf'] = [float(x) for x in t[1:9]]
def rgba(name): return np.fromfile(os.path.join(d, name), np.uint8).reshape(H, W, 4).astype(np.float64)
masks = [i for i, dr in enumerate(draws) if dr['fp'].startswith('55295a0f')]
comp = [i for i, dr in enumerate(draws) if dr['fp'].startswith('f384f111')][0]
k5 = draws[comp]['k'][5][:3]                                   # -sun direction
n = (np.floor(rgba('unit1_d%03u.rgba' % comp)[..., :3] + 0.5) - 128) / 127
n /= np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
NL = -(n @ k5)
gpu = rgba('unit2_d%03u.rgba' % comp)[..., 0] / 255             # display buffer C.R
z = rgba('unit0_d%03u.rgba' % masks[0])                          # R=Zmid G=Zlo B=stencil A=Zhi
stencil = z[..., 2]
depth = (z[..., 3] * 65536 + z[..., 0] * 256 + z[..., 1]) * 5.96046519e-08
py, px = np.mgrid[0:H, 0:W].astype(np.float64)
total = np.zeros((H, W)); per = []
for i in masks:
    dr = draws[i]; k, c, xf = dr['k'], dr['c'], dr['xf']
    u = ((2 * (px + 0.5) / W - 1) - xf[4]) / xf[0] * c[467][0] + c[467][2]
    v = ((1 - 2 * (py + 0.5) / H) - xf[5]) / xf[1] * c[467][1] + c[467][3]
    r0 = depth[..., None] * k[4] + u[..., None] * c[256] + v[..., None] * c[257] + c[259]
    flag = (np.mod(stencil * k[3][3], 1.0) >= k[5][0]).astype(float)   # stencil bit 7
    r0[..., 2] -= r0[..., 3] * k[6][2] * flag                           # its receiver bias
    co = r0[..., :3] / r0[..., 3:4]; ref = np.clip(co[..., 2], 0, 1)
    sm = np.fromfile(os.path.join(d, 'unit1_d%03u.rgba' % i), np.float32).reshape(1024, 1024)
    pos = co[..., :2] * 1024 - 0.5; base = np.floor(pos); f = pos - base
    taps = []; lit = []
    for t in range(4):
        tx = np.clip(base[..., 0] + (t & 1), 0, 1023).astype(int); ty = np.clip(base[..., 1] + (t >> 1), 0, 1023).astype(int)
        taps.append(sm[ty, tx]); lit.append((ref < sm[ty, tx]).astype(float))   # RSX: stored GREATER ref = lit
    cov = (lit[0] * (1 - f[..., 0]) + lit[1] * f[..., 0]) * (1 - f[..., 1]) + (lit[2] * (1 - f[..., 0]) + lit[3] * f[..., 0]) * f[..., 1]
    fade = np.clip(depth * k[7][0] + k[7][1], 0, 1)
    win = np.clip((depth - k[8][0]) * k[9][2], 0, 1) * np.clip((depth - k[8][1]) * k[9][3], 0, 1)
    total += (cov + (1 - cov) * fade) * win
    per.append((cov, np.stack(taps, -1), ref, win, fade))
valid = depth < 0.9999
err = np.abs(np.clip(total, 0, 1) - gpu)[valid]
print('mask: CPU vs GPU |diff| mean %.4f, p99 %.3f; bias for stencil bit 7: %.4g (flag k3.w %.4g >= %.2f)'
      % (err.mean(), np.percentile(err, 99), draws[masks[0]]['k'][6][2], draws[masks[0]]['k'][3][3], draws[masks[0]]['k'][5][0]))
back = valid & (NL < -0.05) & (stencil == 0)
for ci, (cov, taps, ref, win, fade) in enumerate(per):
    s = back & (win > 0.99) & (fade < 0.01)
    if not s.any(): continue
    dif = (taps - ref[..., None])[s] * 16777215
    print('cascade %d: %6d world back-facing px, coverage mean %.2f, partial %3.0f%%, tap-ref median %7.0f D24, tap spread median %6.0f'
          % (ci, s.sum(), cov[s].mean(), 100 * ((cov[s] > .02) & (cov[s] < .98)).mean(), np.median(dif),
             np.median((taps.max(-1) - taps.min(-1))[s] * 16777215)))
for name, m in (('world back-facing', back), ('front-facing', valid & (NL > 0.05))):
    print('%-17s px %6d: GPU mask mean %.3f, partial %.0f%%' % (name, m.sum(), gpu[m].mean(), 100 * ((gpu[m] > .02) & (gpu[m] < .98)).mean()))
