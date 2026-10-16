"""numpy reference of the whole SSAO pipeline (passes E, A, D, C, B) as reverse-engineered."""
import numpy as np
W, H = 1280, 720
w, h = 640, 360
def d500(z24, p):
    return (1.0 / (np.float64(p[0]) * 2.0**-24 * (1 + 2.0**-23) * z24[0::2, 0::2] + p[1])).astype(np.float32)
def pad(a, ay, ax):
    return np.pad(a, ((ay, ay), (ax, ax)), mode='edge')
def f880(hd, nrm, p):
    p = np.asarray(p, np.float64)
    X, Y, Z = p[0:12], p[12:24], p[24:36]
    M = np.stack([p[36:39], p[40:43], p[44:47]]); D0, E0, F0 = p[52:56], p[56:60], p[60:64]
    d = hd.astype(np.float64)
    dc = np.clip(d, E0[2], E0[3]); q = d / dc
    nb = nrm[0::2, 0::2, 1:4].astype(np.float64) / 127.5 - 1.0
    n2 = nb[..., 0:1] * M[0] + nb[..., 1:2] * M[1] + nb[..., 2:3] * M[2]
    dref = d + n2[..., 2] * q
    P = pad(d, 9, 12)
    yy, xx = np.mgrid[0:h, 0:w]
    acc = np.zeros((h, w))
    for k in range(12):
        tx = np.floor(xx + 0.5 + (X[k] * D0[1] + n2[..., 0]) / dc).astype(int)
        ty = np.floor(yy + 0.5 + (Y[k] * D0[2] + n2[..., 1]) / dc).astype(int)
        ds = P[np.clip(ty, -9, h + 8) + 9, np.clip(tx, -12, w + 11) + 12]
        hk = Z[k] * D0[3] * q
        x2 = (ds - dref + hk) * 0.5
        acc += np.minimum(np.maximum(x2, 0), hk) + hk * np.clip(x2 * E0[0] / q + E0[1] * q, 0, 1)
    acc /= (D0[0] * q)
    v = np.minimum(np.clip(acc * F0[0] + F0[1], 0, 1), 1 - 2.0**-24)
    return np.floor(v * 255.0).astype(np.uint8)
def blur_weights(dc, dn, p):
    return np.maximum(0, 1 - np.abs(dn - dc) * p[0] / dc)
def d880(ao, hd, p):   # horizontal 5-tap, edge clamp
    A = pad(ao.astype(np.float64), 0, 2); D = pad(hd.astype(np.float64), 0, 2)
    c = slice(2, 2 + w); num = A[:, c].copy(); den = np.ones_like(num)
    for k, pk in ((-2, p[1]), (-1, p[2]), (1, p[2]), (2, p[1])):
        sl = slice(2 + k, 2 + k + w); wt = blur_weights(D[:, c], D[:, sl], p) * pk
        num += wt * A[:, sl]; den += wt
    return np.floor(num / den).astype(np.uint8)
def e480_inplace(ao, hd, p, tw=128, th=45):  # vertical 5-tap, tiles processed in order, in place
    ao = ao.astype(np.float64).copy(); D = hd.astype(np.float64)
    for ty in range(h // th):
        for tx in range(w // tw):
            xs = slice(tx * tw, (tx + 1) * tw)
            rows = np.clip(np.arange(ty * th - 2, (ty + 1) * th + 2), 0, h - 1)
            A = ao[rows][:, xs]; Dd = D[rows][:, xs]
            c = slice(2, 2 + th); num = A[c].copy(); den = np.ones_like(num)
            for k, pk in ((-2, p[1]), (-1, p[2]), (1, p[2]), (2, p[1])):
                sl = slice(2 + k, 2 + k + th); wt = blur_weights(Dd[c], Dd[sl], p) * pk
                num += wt * A[sl]; den += wt
            ao[ty * th:(ty + 1) * th, xs] = np.floor(num / den)
    return ao.astype(np.uint8)
def ec80(ao, hd, z24, p):
    Z = 1.0 / (np.float64(p[0]) * 2.0**-24 * (1 + 2.0**-23) * z24 + p[1])
    A = pad(ao.astype(np.float64), 1, 1); D = pad(hd.astype(np.float64), 1, 1)
    Y, X = np.mgrid[0:H, 0:W]; y, x = Y >> 1, X >> 1
    dy = np.where(Y & 1, 1, -1); dx = np.where(X & 1, 1, -1)
    num = np.zeros((H, W)); den = np.zeros((H, W))
    for oy, ox, bw in ((0, 0, 9), (0, 1, 3), (1, 0, 3), (1, 1, 1)):
        yi = y + oy * dy + 1; xi = x + ox * dx + 1
        zi = D[yi, xi]; ai = A[yi, xi]
        wt = bw * (np.maximum(0, 1 - np.abs(zi - Z) * p[2] / Z) + 1e-4)
        num += wt * ai; den += wt
    return np.floor(num / den).astype(np.uint8)
