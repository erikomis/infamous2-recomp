import sys, struct, numpy as np
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from emu import *
# A capture from RSX_SNAP_AT/_DIR with RSX_SNAP_LABEL_B=870 and ranges
# 47500B80:384000,47884B80:384000,00a93800:800,47e70000:10000 (see ROADMAP).
SN = os.environ.get('SNAP', '.').rstrip('/') + '/'
def load(p):
    fb = open(SN+p+'_00a93800.bin','rb').read()
    N = np.fromfile(SN+p+'_47500b80.bin', np.uint8).reshape(720,1280,4)
    D = np.fromfile(SN+p+'_47884b80.bin', np.uint8).reshape(720,1280,4)
    L = open(SN+p+'_47e70000.bin','rb').read()
    return fb, N, D, L
FB, aN, aD, LT = load('a')
_, bN, bD, _ = load('b')
FRAME = FB[0x480:0x580]
NL, LEA, _ = struct.unpack('>IIf', FRAME[0x90:0x9c])
LIGHTS = LT[LEA-0x47e70000: LEA-0x47e70000 + 48*NL]
# LS layout chosen for the harness
A_FRAME = 0xA000; A_LIGHTS = 0x1000; A_DESC = 0x2000; A_PARAM = 0x2100
A_INN = 0x1A000; A_IND = 0x1B800; A_OUTA = 0x1D000; A_OUTB = 0x1E800
def make(tx, ty, lights=LIGHTS, nl=None, frame=FRAME, same_buffers=True):
    L = LS()
    L.b[A_FRAME:A_FRAME+0x100] = frame
    nl = NL if nl is None else nl
    L.w32(A_FRAME+0x90, nl)
    L.b[A_LIGHTS:A_LIGHTS+len(lights)] = lights
    L.w32(0x9C80, A_FRAME); L.w32(0x9C90, 0); L.w32(0x9CB0, A_LIGHTS)
    L.b[A_DESC] = 40; L.b[A_DESC+1] = 18
    L.w16(A_DESC+0x48, 32); L.w16(A_DESC+0x4A, 40)
    n = aN[ty*40:ty*40+40, tx*32:tx*32+32].tobytes(); d = aD[ty*40:ty*40+40, tx*32:tx*32+32].tobytes()
    L.b[A_INN:A_INN+len(n)] = n; L.b[A_IND:A_IND+len(d)] = d
    oa, ob = (A_INN, A_IND) if same_buffers else (A_OUTA, A_OUTB)
    L.w32(A_PARAM+0, A_DESC); L.w32(A_PARAM+4, tx); L.w32(A_PARAM+8, ty)
    L.w32(A_PARAM+0x0C, oa); L.w32(A_PARAM+0x10, A_INN); L.w32(A_PARAM+0x14, A_IND); L.w32(A_PARAM+0x18, ob)
    L.setreg(1, 0x3FE00); L.setreg(3, A_PARAM)
    return L
def tile_out(o, tx, ty, same_buffers=True):
    oa, ob = (A_INN, A_IND) if same_buffers else (A_OUTA, A_OUTB)
    A = np.frombuffer(bytes(o.b[oa:oa+5120]), np.uint8).reshape(40,32,4)
    B = np.frombuffer(bytes(o.b[ob:ob+5120]), np.uint8).reshape(40,32,4)
    return A, B
def ref(tx, ty):
    return bN[ty*40:ty*40+40, tx*32:tx*32+32], bD[ty*40:ty*40+40, tx*32:tx*32+32]
