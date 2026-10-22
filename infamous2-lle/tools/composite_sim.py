"""CPU recompute of inFAMOUS 2's deferred composite (FP f384f111ad5956ba) from an
OCCDUMP of its inputs (units 0-5), to see which term leaves a pixel dark."""
import sys, numpy as np
sys.path.insert(0, sys.argv[0].rsplit('/', 1)[0])
d, cube_path = sys.argv[1], sys.argv[2]
W, H = 1280, 720
U = {u: np.fromfile('%s/unit%d.rgba' % (d, u), np.uint8).reshape(H, W, 4).astype(float) / 255 for u in range(6)}
out = np.fromfile('%s/rt_001.rgba' % d, np.uint8).reshape(H, W, 4).astype(float) / 255
k = {0: (0.734354, -0.0642477, -0.675719, 0), 1: (0.5, 0, 0, 0), 2: (4, 0, 0, 0), 3: (0, 255, 0.5, 0), 4: (0.00390625, 0, 0, 0),
     5: (0.734354, -0.0642477, -0.675719, 0), 6: (0.5, 0, 0, 0), 7: (0.015625, 0, 0, 0), 8: (0.734354, -0.0642477, -0.675719, 0),
     9: (1, 0, 0, 0), 10: (0, 0.5, 0, 0), 11: (0.5, 0, 0, 1), 12: (1, 0, 0, 1), 13: (0, 0.0010004, 0, 0), 14: (0.5, 0, 0, 0),
     15: (1.24277, 0.866108, 0.393123, 0.912037), 16: (0, 0, 0, 129), 17: (1, 0, 0, 0), 18: (1.24277, 0.866108, 0.393123, 0.912037),
     19: (1.24277, 0.866108, 0.393123, 0.912037), 20: (0.1, 0.3, 2, 1), 21: (0.1, 0.3, 2, 1), 22: (1, 1, 1, 1)}
k = {i: np.array(v, float) for i, v in k.items()}
cube = np.fromfile(cube_path, np.uint8).reshape(6, 8, 8, 4).astype(float) / 255
def srgb2lin(v): return np.where(v > 0.04045, ((v + 0.055) / 1.055) ** 2.4, v / 12.92)
def cube_lookup(n):
    x, y, z = n[..., 0], n[..., 1], n[..., 2]; ax = np.argmax(np.abs(n), -1)
    res = np.zeros(n.shape[:-1] + (3,))
    for f in range(6):
        a = f // 2; pos = f % 2 == 0
        m = (ax == a) & ((n[..., a] > 0) == pos)
        if not m.any(): continue
        X, Y, Z = x[m], y[m], z[m]
        if f == 0: sc, tc, ma = -Z, -Y, X
        elif f == 1: sc, tc, ma = Z, -Y, -X
        elif f == 2: sc, tc, ma = X, Z, Y
        elif f == 3: sc, tc, ma = X, -Z, -Y
        elif f == 4: sc, tc, ma = X, -Y, Z
        else: sc, tc, ma = -X, -Y, -Z
        s = (sc / ma + 1) / 2 * 8 - 0.5; t = (tc / ma + 1) / 2 * 8 - 0.5
        i0 = np.clip(np.floor(t), 0, 7).astype(int); j0 = np.clip(np.floor(s), 0, 7).astype(int)
        i1 = np.clip(i0 + 1, 0, 7); j1 = np.clip(j0 + 1, 0, 7)
        ft = np.clip(t - i0, 0, 1)[:, None]; fs = np.clip(s - j0, 0, 1)[:, None]
        c = cube[f][..., :3]
        v = (c[i0, j0] * (1 - fs) + c[i0, j1] * fs) * (1 - ft) + (c[i1, j0] * (1 - fs) + c[i1, j1] * fs) * ft
        res[m] = v
    return res
alb = srgb2lin(U[0][..., :3])
nrm = (np.floor(U[1] * 255 + 0.5) - 128) / 127
N = nrm[..., :3] / np.maximum(np.linalg.norm(nrm[..., :3], axis=-1, keepdims=True), 1e-6)
C = U[2]; Cb = srgb2lin(C[..., 2])
z3 = U[3]; stencil = np.floor(z3[..., 2] * 255 + 0.5)
L4 = U[4]; L5 = U[5]
diffuse = (L4[..., :3] * L4[..., 3:4]) ** 2 * 8                  # line 82/84
amb = srgb2lin(cube_lookup(N))                                    # line 109
amb_scale = np.where(stencil >= 128, k[9][0], k[11][0])           # lines 100/112/115
ndl = -(N @ k[5][:3])                                             # line 97: N . -sun
sun_shadow = C[..., 0]                                            # h4.x (C.r), line 127
sun = sun_shadow[..., None] * k[15][:3] * np.clip(ndl, 0, 1)[..., None]   # approximate
ao = C[..., 1]
for name, y0, y1, x0, x1 in (('Cole back', 300, 460, 560, 760), ('upper left', 0, 150, 0, 300), ('right', 200, 400, 1000, 1250)):
    sl = (slice(y0, y1), slice(x0, x1))
    print('%-11s albedo %.3f  N %s  AO(C.g) %.2f  C.r(sun sh) %.2f  N.L %.2f  ambient %s  amb*scale %.3f  diffuse %.3f  stencil %d  composite out %.3f'
          % (name, alb[sl].mean(), N[sl].mean((0, 1)).round(2), ao[sl].mean(), sun_shadow[sl].mean(), ndl[sl].mean(),
             amb[sl].mean((0, 1)).round(3), (amb[sl].mean(-1) * amb_scale[sl]).mean(), diffuse[sl].mean(), np.median(stencil[sl]),
             out[sl][..., :3].mean()))
