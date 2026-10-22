import struct, os, sys
D = sys.argv[1] if len(sys.argv) > 1 else None
class Mem:
    def __init__(s, d, t):
        s.r = []
        for f in os.listdir(d):
            if f.startswith('t%s_' % t):
                base = int(f.split('_')[1].split('.')[0], 16); s.r.append((base, open(os.path.join(d, f), 'rb').read()))
    def has(s, a, n=4): return any(b <= a and a + n <= b + len(x) for b, x in s.r)
    def rd(s, a, n):
        for b, x in s.r:
            if b <= a and a + n <= b + len(x): return x[a - b:a - b + n]
        return None
    def u32(s, a): v = s.rd(a, 4); return struct.unpack('>I', v)[0] if v else None
    def f32(s, a): v = s.rd(a, 4); return struct.unpack('>f', v)[0] if v else None
    def words(s, a, n):
        return ' '.join('%08X' % (s.u32(a + 4 * i) if s.u32(a + 4 * i) is not None else 0xDEADDEAD) for i in range(n))
