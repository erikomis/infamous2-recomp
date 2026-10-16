"""Float64 reference of jobmod_c6e7383dc013's per-pixel light accumulation, from the
reverse-engineered pseudocode. Compares against a captured frame (snap2)."""
import sys, struct, numpy as np
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from setup import aN, aD, bN, bD, FRAME, LIGHTS, NL

def parse_lights(raw, n):
    out = []
    for i in range(n):
        r = raw[48*i:48*i+48]
        pos = np.array(struct.unpack('>3f', r[0:12]), np.float64)
        col = np.array(struct.unpack('>3e', r[12:18]), np.float64)
        typ = struct.unpack('>I', r[20:24])[0]
        h = struct.unpack('>8e', r[32:48])
        out.append(dict(pos=pos, col=col, type=typ, inner=h[0], outer=h[1], h2=h[2], h3=h[3],
                        dir=np.array(h[4:7], np.float64), near=h[7]))
    return out

def decode(Nbytes, Dbytes, M1):
    H, W = Nbytes.shape[:2]
    g = Nbytes[..., 0].astype(np.float64)
    n = Nbytes[..., 1:4].astype(np.float64) * (2/255) - 1
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    s = 2.0 ** (g * 8/255 - 0.88896)               # "spec power / 8"
    z = ((Dbytes[..., 0].astype(np.int64) << 16) | (Dbytes[..., 1].astype(np.int64) << 8) | Dbytes[..., 2]) / 2.0**24
    ys, xs = np.mgrid[0:H, 0:W]
    q = np.stack([xs/W, ys/H, z, np.ones_like(z)], -1)   # u = px/1280, v = py/720 (pixel corner, not centre)
    h = q @ M1
    P = h[..., :3] / h[..., 3:4]
    sky = Dbytes[..., 0:3].astype(np.int64).sum(-1) == 3*255
    return P, n, s, sky

def shade(P, N, s, lights):
    diff = np.zeros(P.shape[:-1]+(3,)); spec = np.zeros(P.shape[:-1])
    V = -P / np.linalg.norm(P, axis=-1, keepdims=True)    # eye at the origin
    for Lt in lights:
        Lv = Lt['pos'] - P; d = np.linalg.norm(Lv, axis=-1); l = Lv / d[..., None]
        ndl = np.clip((N*l).sum(-1), 0, None)
        att = np.clip((Lt['outer'] - d) / (Lt['outer'] - Lt['inner']), 0, 1) ** 2
        if Lt['type'] == 2:
            cos = (-l * Lt['dir']).sum(-1)
            att = att * np.clip((cos - Lt['h3']) * (-Lt['h2']), 0, 1) ** 2 * (d > Lt['near'])
        C = Lt['col'] * 1.6
        Hh = l + V; Hh /= np.linalg.norm(Hh, axis=-1, keepdims=True)
        ndh = np.clip((N*Hh).sum(-1), 0, None)
        diff += (att*ndl)[..., None] * C
        spec += att * ndl * np.linalg.norm(C) * (1 + 25/27*s) * (1 + s*(1 - ndh)) ** -8
    return diff, spec

def pack(diff, spec):
    K = 0.35355339 * 255/256 * 256
    sq = np.sqrt(diff); m = np.maximum(sq.max(-1), 1.0)
    a = np.minimum(np.floor(m*K), 255)
    rgb = np.minimum(np.floor(sq * K * 255 / a[..., None]), 255)
    sb = np.minimum(np.floor(np.sqrt(spec) * K), 255)
    A = np.concatenate([a[..., None], rgb], -1).astype(np.uint8)
    B = np.zeros(A.shape, np.uint8); B[..., 0] = sb
    return A, B

if __name__ == '__main__':
    M1 = np.array(struct.unpack('>16f', FRAME[0x10:0x50]), np.float64).reshape(4, 4)
    lights = parse_lights(LIGHTS, NL)
    P, N, s, sky = decode(aN, aD, M1)
    diff, spec = shade(P, N, s, lights)
    A, B = pack(diff, spec)
    lit = bN[..., 0] > 0              # pixels of tiles the job lit (others are memset to 0)
    for nm, x, y in (('diffuse A', A[..., 0], bN[..., 0]), ('diffuse R', A[..., 1], bN[..., 1]), ('diffuse G', A[..., 2], bN[..., 2]),
                     ('diffuse B', A[..., 3], bN[..., 3]), ('spec A', B[..., 0], bD[..., 0])):
        dd = np.abs(x.astype(int) - y.astype(int))[lit]
        print('%-10s on job-lit tiles: exact %.4f  |d|<=1 %.4f  |d|<=2 %.5f  max %d' % (nm, (dd == 0).mean(), (dd <= 1).mean(), (dd <= 2).mean(), dd.max()))
    # outside the lit tiles, the reference must be ~0 light
    out = ~lit
    print('outside job-lit tiles: model rgb>1 px', (A[..., 1:][out] > 1).any(-1).sum(), ' model spec>1 px', (B[..., 0][out] > 1).sum())
    big = np.argwhere(np.abs(A[..., 1].astype(int) - bN[..., 1].astype(int)) * lit > 2)
    print('worst px', big[:10].tolist())
