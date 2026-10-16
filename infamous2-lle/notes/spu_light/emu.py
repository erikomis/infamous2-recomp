import struct, subprocess, os, numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
JOB = '/Users/erik-martins/Desktop/recomp-infamous-2/infamous2-lle/spu/jobmods/jobmod_c6e7383dc013_e5400.bin'
class LS:
    def __init__(self):
        self.b = bytearray(256*1024)
        j = open(JOB,'rb').read()
        self.b[0x4000:0x4000+len(j)] = j
        for a in range(0x9C80, 0x9F80): self.b[a] = 0
        self.regs = {}
    def w32(self,a,v): struct.pack_into('>I', self.b, a, v & 0xffffffff)
    def wf(self,a,f): struct.pack_into('>f', self.b, a, f)
    def w16(self,a,v): struct.pack_into('>H', self.b, a, v & 0xffff)
    def wh(self,a,f): struct.pack_into('>e', self.b, a, f)
    def r32(self,a): return struct.unpack_from('>I', self.b, a)[0]
    def rf(self,a): return struct.unpack_from('>f', self.b, a)[0]
    def rfs(self,a,n): return struct.unpack_from('>%df'%n, self.b, a)
    def setreg(self, r, *v):
        v = list(v) + [0]*(4-len(v))
        self.regs[r] = [x if isinstance(x,int) else struct.unpack('>I',struct.pack('>f',x))[0] for x in v]
    def run(self, addr, log=None, tag='t'):
        p = os.path.join(HERE, 'run_'+tag)
        open(p+'.ls','wb').write(self.b)
        with open(p+'.regs','w') as f:
            for r,v in self.regs.items(): f.write('r%d %x %x %x %x\n'%(r,*[x&0xffffffff for x in v]))
        args = [os.path.join(HERE,'harness'), p+'.ls', '%X'%addr, p+'.regs', p+'.out', p+'.rout']
        if log is not None: args.append(str(log))
        r = subprocess.run(args, capture_output=True, text=True, cwd=HERE, timeout=90)
        if r.returncode: print('ERR', r.returncode, r.stderr[-3000:])
        out = LS.__new__(LS); out.b = bytearray(open(p+'.out','rb').read()); out.regs={}
        out.rregs = {}
        for line in open(p+'.rout'):
            n, *v = line.split(); out.rregs[int(n[1:])] = [int(x,16) for x in v]
        out.stderr = r.stderr
        return out
    def regf(self, r): return struct.unpack('>4f', struct.pack('>4I', *self.rregs[r]))
def diff(a, b):
    """list of (addr) 16-byte lines that differ"""
    A = np.frombuffer(bytes(a.b), dtype=np.uint8).reshape(-1,16); B = np.frombuffer(bytes(b.b), dtype=np.uint8).reshape(-1,16)
    d = np.nonzero((A!=B).any(1))[0]
    return [int(x)*16 for x in d]
def ranges(lines):
    out=[]; 
    for a in lines:
        if out and out[-1][1]==a: out[-1][1]=a+16
        else: out.append([a,a+16])
    return [(hex(x),hex(y)) for x,y in out]
