#!/usr/bin/env python3
"""Small static-analysis helpers for a Sucker Punch PS3 ELF (inFAMOUS 1/2).

    elftool.py <EBOOT.elf> str NAME [NAME...]     where a string is, and the data words pointing at it
    elftool.py <EBOOT.elf> dump ADDR [N]          N words from ADDR, strings resolved (property tables)
    elftool.py <EBOOT.elf> dis ADDR[:N] ...       disassemble (stops at blr; ADDR:N! does not stop)
    elftool.py <EBOOT.elf> opd ADDR ...           a function descriptor: entry, TOC, and its code
    elftool.py <EBOOT.elf> pool TOC TOCOFF OFF... the address a function's constant pool holds:
                                                  pool = [TOC+TOCOFF], then [pool+OFF] for each OFF
    elftool.py <EBOOT.elf> pair OP DISP_A DISP_B  code that uses both displacements within 12
                                                  instructions with primary opcode OP (48 = lfs)
    elftool.py <EBOOT.elf> disp OP DISP...        every instruction with opcode OP and one of DISP
    elftool.py <EBOOT.elf> refs ADDR              data words equal to ADDR (who points at a global)

Addresses and offsets are hex (a leading - for negative offsets). `dis`/`opd` need capstone
(the repo's .venv has it); the rest is plain Python.
"""
import struct
import sys


class Elf:
    def __init__(self, path):
        self.d = d = open(path, 'rb').read()
        phoff = struct.unpack('>Q', d[0x20:0x28])[0]
        phnum = struct.unpack('>H', d[0x38:0x3A])[0]
        self.segs = []
        for i in range(phnum):
            t, fl, off, va, pa, fsz, msz, al = struct.unpack('>IIQQQQQQ', d[phoff + 56 * i:phoff + 56 * i + 56])
            if t == 1 and fsz:
                self.segs.append((va, off, fsz))

    def v2o(self, v):
        for va, off, fsz in self.segs:
            if va <= v < va + fsz:
                return off + v - va
        return None

    def o2v(self, o):
        for va, off, fsz in self.segs:
            if off <= o < off + fsz:
                return va + o - off
        return None

    def u32(self, v):
        o = self.v2o(v)
        return struct.unpack('>I', self.d[o:o + 4])[0] if o is not None else None

    def cstr(self, v, n=96):
        o = self.v2o(v)
        if o is None:
            return None
        e = self.d.find(b'\0', o, o + n)
        if e <= o:
            return None
        s = self.d[o:e]
        return s.decode('ascii') if all(32 <= c < 127 for c in s) else None

    def findstr(self, s):
        out, b, i = [], s.encode(), 0
        while True:
            i = self.d.find(b, i)
            if i < 0:
                return out
            if (i == 0 or self.d[i - 1] == 0) and self.d[i + len(b)] == 0 and self.o2v(i) is not None:
                out.append(self.o2v(i))
            i += 1

    def refs(self, v):
        out, b, i = [], struct.pack('>I', v), 0
        while True:
            i = self.d.find(b, i)
            if i < 0:
                return out
            if i % 4 == 0 and self.o2v(i) is not None:
                out.append(self.o2v(i))
            i += 1

    def code_words(self):
        va, off, fsz = self.segs[0]
        return va, struct.unpack('>%dI' % (fsz // 4), self.d[off:off + fsz // 4 * 4])

    def dis(self, a, n=80, stop=True):
        from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN
        md = Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
        for _ in range(n):
            o = self.v2o(a)
            w = self.d[o:o + 4]
            r = list(md.disasm(w, a))
            print('%08x  %s' % (a, '%-8s %s' % (r[0].mnemonic, r[0].op_str) if r else '.word %08x' % struct.unpack('>I', w)[0]))
            a += 4
            if stop and w == b'\x4e\x80\x00\x20':
                break


def hx(s):
    return -int(s[1:], 16) if s.startswith('-') else int(s, 16)


def main():
    if len(sys.argv) < 4:
        sys.exit(__doc__)
    e, cmd, args = Elf(sys.argv[1]), sys.argv[2], sys.argv[3:]
    if cmd == 'str':
        for s in args:
            for v in e.findstr(s):
                print('%s @%08X  refs: %s' % (s, v, ' '.join('%08X' % r for r in e.refs(v)) or '-'))
    elif cmd == 'dump':
        a, n = hx(args[0]), int(args[1]) if len(args) > 1 else 32
        for i in range(n):
            v = e.u32(a + 4 * i)
            s = e.cstr(v) if v and e.v2o(v) is not None else None
            print('%08X: %08X %s' % (a + 4 * i, v or 0, repr(s) if s and len(s) > 2 else ''))
    elif cmd == 'dis':
        for x in args:
            stop = not x.endswith('!')
            x = x.rstrip('!')
            a, n = (x.split(':') + ['80'])[:2]
            e.dis(hx(a), int(n), stop)
            print('---')
    elif cmd == 'opd':
        for x in args:
            o = hx(x)
            print('OPD %08X -> entry %08X, TOC %08X' % (o, e.u32(o), e.u32(o + 4)))
            e.dis(e.u32(o))
            print('---')
    elif cmd == 'pool':
        toc, tocoff = hx(args[0]), hx(args[1])
        pool = e.u32(toc + tocoff)
        print('pool = [%08X] = %08X' % (toc + tocoff, pool))
        for x in args[2:]:
            off = hx(x)
            print('  [pool%+X] = [%08X] = %08X' % (off, pool + off, e.u32(pool + off) or 0))
    elif cmd == 'pair':
        op, a, b = int(args[0]), hx(args[1]), hx(args[2])
        va, W = e.code_words()
        for i, w in enumerate(W):
            if (w >> 26) == op and (w & 0xFFFF) == a:
                win = W[max(0, i - 12):i + 12]
                if any((x >> 26) == op and (x & 0xFFFF) == b for x in win):
                    div = any((x >> 26) == 59 and ((x >> 1) & 0x1F) == 18 for x in win)
                    print('%08X%s' % (va + 4 * i, '  (fdivs nearby)' if div else ''))
    elif cmd == 'disp':
        op, ds = int(args[0]), [hx(x) & 0xFFFF for x in args[1:]]
        va, W = e.code_words()
        for i, w in enumerate(W):
            if (w >> 26) == op and (w & 0xFFFF) in ds:
                print('%08X  disp %X  base r%d' % (va + 4 * i, w & 0xFFFF, (w >> 16) & 31))
    elif cmd == 'refs':
        for x in args:
            print('%08X: %s' % (hx(x), ' '.join('%08X' % r for r in e.refs(hx(x))) or '-'))
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main()
