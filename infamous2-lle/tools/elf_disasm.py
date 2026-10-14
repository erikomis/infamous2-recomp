"""Disassemble a range of the game ELF: elf_disasm.py <lo> <hi> (hex vaddrs)."""
import sys, struct
from capstone import Cs, CS_ARCH_PPC, CS_MODE_64, CS_MODE_BIG_ENDIAN
d = open(sys.argv[3] if len(sys.argv) > 3 else 'game/EBOOT.elf', 'rb').read()
phoff = struct.unpack('>Q', d[0x20:0x28])[0]
_, _, off, va, _, _, _, _ = struct.unpack('>IIQQQQQQ', d[phoff:phoff + 56])
md = Cs(CS_ARCH_PPC, CS_MODE_64 | CS_MODE_BIG_ENDIAN)
a, hi = int(sys.argv[1], 16), int(sys.argv[2], 16)
while a < hi:
    w = d[off + a - va: off + a - va + 4]
    r = list(md.disasm(w, a))
    print(f'{a:08x}  ' + (f'{r[0].mnemonic:8s} {r[0].op_str}' if r else f'.word 0x{struct.unpack(">I", w)[0]:08x}'))
    a += 4
