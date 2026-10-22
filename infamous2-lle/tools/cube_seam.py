import sys, itertools, numpy as np
a = np.fromfile(sys.argv[1], np.uint8).reshape(6, 8, 8, 4).astype(float)[..., :3]
N = 8
def face_dir(f, s, t):
    sc, tc = 2 * s - 1, 2 * t - 1
    return {0: (1, -tc, -sc), 1: (-1, -tc, sc), 2: (sc, 1, tc), 3: (sc, -1, -tc), 4: (sc, -tc, 1), 5: (-sc, -tc, -1)}[f]
def lookup(faces, d):
    x, y, z = d; ax = np.argmax(np.abs(d))
    if ax == 0: f, sc, tc, ma = (0, -z, -y, x) if x > 0 else (1, z, -y, -x)
    elif ax == 1: f, sc, tc, ma = (2, x, z, y) if y > 0 else (3, x, -z, -y)
    else: f, sc, tc, ma = (4, x, -y, z) if z > 0 else (5, -x, -y, -z)
    s = (sc / ma + 1) / 2; t = (tc / ma + 1) / 2
    i = min(N - 1, max(0, int(t * N))); j = min(N - 1, max(0, int(s * N)))
    return faces[f][i, j]
def seam_error(faces):
    err = []; 
    for f in range(6):
        for k in range(N):
            for (i, j, ds, dt) in ((0, k, 0, -1), (N - 1, k, 0, 1), (k, 0, -1, 0), (k, N - 1, 1, 0)):
                s = (j + 0.5 + ds) / N; t = (i + 0.5 + dt) / N   # one texel beyond the edge
                d = np.array(face_dir(f, s, t), float)
                err.append(np.abs(lookup(faces, d) - faces[f][i, j]).mean())
    return np.mean(err)
print('as stored (standard GL/D3D table): %.2f' % seam_error(a))
best = []
tr = {'id': lambda x: x, 'T': lambda x: x.transpose(1, 0, 2), 'flipV': lambda x: x[::-1], 'flipH': lambda x: x[:, ::-1],
      'rot180': lambda x: x[::-1, ::-1]}
for name, fn in tr.items():
    print('all faces %-6s: %.2f' % (name, seam_error([fn(a[f]) for f in range(6)])))
for perm in itertools.permutations(range(6)):
    best.append((seam_error([a[p] for p in perm]), perm))
best.sort(); print('best face orders:', [(round(e, 2), p) for e, p in best[:4]])
