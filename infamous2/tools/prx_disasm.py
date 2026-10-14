import sys,struct
from capstone import *
img=open('fw/out/libsre_image.bin','rb').read(); B=0x30000000
md=Cs(CS_ARCH_PPC,CS_MODE_64|CS_MODE_BIG_ENDIAN)
lo=int(sys.argv[1],16); hi=int(sys.argv[2],16)
a=lo
while a<hi:
    w=img[a-B:a-B+4]
    r=list(md.disasm(w,a))
    if r: print(f'{a:08x}  {r[0].mnemonic:8s} {r[0].op_str}')
    else: print(f'{a:08x}  .word 0x{struct.unpack(">I",w)[0]:08x}')
    a+=4
