#!/usr/bin/env python3
"""Score a PS3_FRAMEGEN_TEST dump: tools/framegen_test.py <dir> [--png <n>] [--keep-raw]

The Metal backend's PS3_FRAMEGEN_TEST=<dir> (headless) withholds every other
frame from a second MetalFX interpolator and asks it for the frame between N
and N+2; each dumped triple t<k> holds

    c0, c1, c2   the 3D picture (no HUD) of frames N, N+1, N+2 (RGBA8 or BGRA8)
    interp       the interpolator's frame between c0 and c2, with our motion vectors
    interp0      the same with zero motion vectors
    mv           the motion vectors c2 -> c0 (RG16F, previous - current, UV units)
    depth        frame N+2's depth (R32F, 0 near .. 1 far)
    live, final  the live path at frame N+2: the picture it shows before the
                 frame (between N+1 and N+2, HUD put back) and the finished frame
    meta.txt     time, GPU ms, the reprojection matrix

c1 is the truth. Per triple this prints

  gate 1 (are the motion vectors right?)
    warp   PSNR of c0 pulled through the motion vectors against c2
    still  PSNR of c0 against c2 as it is (what zero motion would score)
  gate 4 (is the interpolated frame right?)
    interp / interp0 / blend / hold   PSNR and SSIM against c1 of: the
    interpolated frame, the one made without motion vectors, the 50/50 blend
    of c0 and c2, and c2 itself (the frame a player sees twice without
    frame generation)

and writes <dir>/t<k>.png for the first --png triples (default 6): truth,
interpolated, blend and 4x |interpolated - truth|, <dir>/t<k>_mv.png (the
motion field, hue = direction, brightness = length up to 32 px) and
<dir>/t<k>_live.png (finished frame | the live path's interpolated picture). Raw files are
deleted afterwards unless --keep-raw. numpy only.
"""
import glob
import os
import re
import struct
import sys
import zlib

import numpy as np


def write_png(path, a):
    a = np.ascontiguousarray(a.astype(np.uint8))
    h, w, _ = a.shape
    raw = b"".join(b"\x00" + a[y].tobytes() for y in range(h))

    def chunk(t, d):
        return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b""))


def load(d, k, name, dtype, ch):
    f = glob.glob(os.path.join(d, f"t{k:03d}_{name}_*.raw"))
    if not f:
        return None
    w, h = map(int, re.search(r"_(\d+)x(\d+)\.raw$", f[0]).groups())
    return np.fromfile(f[0], dtype=dtype).reshape(h, w, ch)


def psnr(a, b, mask=None):
    d = (a.astype(np.float64) - b.astype(np.float64)) ** 2
    if mask is not None:
        d = d[mask]
    m = d.mean()
    return 99.0 if m == 0 else 10.0 * np.log10(255.0 * 255.0 / m)


def luma(a):
    return a[..., 0] * 0.299 + a[..., 1] * 0.587 + a[..., 2] * 0.114


def box(a, n):
    c = np.cumsum(np.cumsum(np.pad(a, ((1, 0), (1, 0))), axis=0), axis=1)
    return (c[n:, n:] - c[:-n, n:] - c[n:, :-n] + c[:-n, :-n]) / (n * n)


def ssim(a, b, n=8):
    """Mean SSIM of the luma over n x n box windows (K1 0.01, K2 0.03)."""
    x, y = luma(a.astype(np.float64)), luma(b.astype(np.float64))
    mx, my = box(x, n), box(y, n)
    vx, vy, cxy = box(x * x, n) - mx * mx, box(y * y, n) - my * my, box(x * y, n) - mx * my
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    return float((((2 * mx * my + c1) * (2 * cxy + c2)) / ((mx * mx + my * my + c1) * (vx + vy + c2))).mean())


def warp(prev, mv_px):
    """prev sampled at p + mv (bilinear); also the mask of samples inside it."""
    h, w, _ = prev.shape
    ys, xs = np.mgrid[0:h, 0:w].astype(np.float64)
    sx, sy = xs + mv_px[..., 0], ys + mv_px[..., 1]
    ok = (sx >= 0) & (sx <= w - 1) & (sy >= 0) & (sy <= h - 1)
    sx, sy = np.clip(sx, 0, w - 1), np.clip(sy, 0, h - 1)
    x0, y0 = np.floor(sx).astype(int), np.floor(sy).astype(int)
    x1, y1 = np.minimum(x0 + 1, w - 1), np.minimum(y0 + 1, h - 1)
    fx, fy = (sx - x0)[..., None], (sy - y0)[..., None]
    p = prev.astype(np.float64)
    out = (p[y0, x0] * (1 - fx) + p[y0, x1] * fx) * (1 - fy) + (p[y1, x0] * (1 - fx) + p[y1, x1] * fx) * fy
    return out, ok


def mv_picture(mv_px):
    ang = np.arctan2(mv_px[..., 1], mv_px[..., 0])
    mag = np.clip(np.hypot(mv_px[..., 0], mv_px[..., 1]) / 32.0, 0, 1)
    rgb = np.stack([np.cos(ang), np.cos(ang - 2.094), np.cos(ang + 2.094)], axis=-1) * 0.5 + 0.5
    return rgb * mag[..., None] * 255.0


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    d = args[0]
    n_png = int(args[args.index("--png") + 1]) if "--png" in args else 6
    keep = "--keep-raw" in args
    metas = sorted(glob.glob(os.path.join(d, "t*_meta.txt")))
    if not metas:
        sys.exit(f"no t*_meta.txt in {d}")
    rows = []
    print("triple  time   mv px mean/p95/max   | gate1 warp  still | interp  interp0  blend   hold  (PSNR dB) |"
          " interp interp0 blend  hold (SSIM) | GPU ms")
    t0 = None
    for m in metas:
        k = int(re.search(r"t(\d+)_meta", m).group(1))
        meta = dict(l.split(" ", 1) for l in open(m).read().splitlines() if " " in l)
        t = int(meta["time"]); t0 = t if t0 is None else t0
        c = [load(d, k, n, np.uint8, 4) for n in ("c0", "c1", "c2", "interp", "interp0")]
        mv = load(d, k, "mv", np.float16, 2)
        if any(x is None for x in c) or mv is None:
            continue
        c = [x[..., :3][..., ::-1] if meta.get("bgra", "0").strip() == "1" else x[..., :3] for x in c]
        c0, c1, c2, it, it0 = c
        h, w, _ = c0.shape
        mv_px = mv.astype(np.float64) * np.array([mv.shape[1], mv.shape[0]], dtype=np.float64)
        if mv.shape[:2] != (h, w):          # depth and motion at another size than colour
            yi = (np.arange(h) * mv.shape[0] // h)[:, None]; xi = (np.arange(w) * mv.shape[1] // w)[None, :]
            mv_px = mv_px[yi, xi] * np.array([w / mv.shape[1], h / mv.shape[0]])
        mag = np.hypot(mv_px[..., 0], mv_px[..., 1])
        wp, ok = warp(c0, mv_px)
        g1w, g1s = psnr(wp, c2, ok), psnr(c0, c2, ok)
        blend = (c0.astype(np.float64) + c2.astype(np.float64)) * 0.5
        ps = [psnr(x, c1) for x in (it, it0, blend, c2)]
        ss = [ssim(x, c1) for x in (it, it0, blend, c2)]
        gpu = float(meta.get("gpu_ms", "0"))
        rows.append((mag.mean(), g1w, g1s, *ps, *ss, gpu))
        print(f"t{k:03d}  {t - t0:4d}s  {mag.mean():6.2f} {np.percentile(mag, 95):6.2f} {mag.max():7.2f}  |"
              f"  {g1w:6.2f} {g1s:6.2f} |  {ps[0]:6.2f}  {ps[1]:6.2f} {ps[2]:6.2f} {ps[3]:6.2f}            |"
              f" {ss[0]:.4f} {ss[1]:.4f} {ss[2]:.4f} {ss[3]:.4f}      | {gpu:5.2f}")
        if k < n_png:
            diff = np.clip(np.abs(it.astype(np.float64) - c1) * 4.0, 0, 255)
            write_png(os.path.join(d, f"t{k:03d}.png"),
                      np.vstack([np.hstack([c1, it]), np.hstack([blend, diff])]))
            write_png(os.path.join(d, f"t{k:03d}_mv.png"), mv_picture(mv_px))
            live, fin = load(d, k, "live", np.uint8, 4), load(d, k, "final", np.uint8, 4)
            if live is not None and fin is not None:
                bgra = meta.get("bgra", "0").strip() == "1"
                live, fin = [x[..., :3][..., ::-1] if bgra else x[..., :3] for x in (live, fin)]
                write_png(os.path.join(d, f"t{k:03d}_live.png"), np.hstack([fin, live]))
        if not keep:
            for f in glob.glob(os.path.join(d, f"t{k:03d}_*.raw")):
                os.remove(f)
    if not rows:
        sys.exit("no complete triple")
    r = np.array(rows)
    mov = r[:, 0] > 1.0
    for name, sel in (("all triples", np.ones(len(r), bool)), ("camera moving (mean |mv| > 1 px)", mov),
                      ("camera still", ~mov)):
        if not sel.any():
            continue
        a = r[sel].mean(axis=0)
        print(f"\n{name}: {int(sel.sum())}")
        print(f"  gate 1  warp {a[1]:.2f} dB   still {a[2]:.2f} dB   (gain {a[1] - a[2]:+.2f})")
        print(f"  PSNR    interp {a[3]:.2f}   interp0 {a[4]:.2f}   blend {a[5]:.2f}   hold {a[6]:.2f}")
        print(f"  SSIM    interp {a[7]:.4f}  interp0 {a[8]:.4f}  blend {a[9]:.4f}  hold {a[10]:.4f}")
        print(f"  GPU     {a[11]:.2f} ms (motion pass + interpolation)")


if __name__ == "__main__":
    main()
