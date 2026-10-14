"""lldb helper: at a breakpoint on func_0054C730 (WWS job barrier test), print
the job list's sync rows. Usage inside lldb:
    command script import tools/lldb_barrier.py
    barrier
"""
import lldb, struct

def barrier(debugger, command, result, internal_dict):
    t = debugger.GetSelectedTarget(); p = t.GetProcess()
    f = p.GetSelectedThread().GetFrameAtIndex(0)
    err = lldb.SBError()
    ctx = f.FindRegister('x0').GetValueAsUnsigned()
    gpr = [p.ReadUnsignedFromMemory(ctx + 8 * i, 8, err) for i in range(8)]
    sym = t.FindSymbols('vm_base')[0].GetSymbol().GetStartAddress().GetLoadAddress(t)
    vb = p.ReadUnsignedFromMemory(sym, 8, err)
    def rd(ea, n): return p.ReadMemory(vb + (ea & 0xFFFFFFFF), n, err)
    jl, row, ticket = gpr[3] & 0xFFFFFFFF, gpr[4] & 0xFFFFFFFF, gpr[5] & 0xFFFF
    hdr = struct.unpack('>I', rd(jl, 4))[0]
    print(f'thread={p.GetSelectedThread().GetIndexID()} jobList=0x{jl:08X} row={row} ticket=0x{ticket:04X} header=0x{hdr:08X}')
    for r in range(4):
        w = struct.unpack('>8H', rd(hdr + 0x40 + 16 * r, 16))
        print(f'  row {r} @0x{hdr + 0x40 + 16 * r:08X}: ' + ' '.join(f'{x:04X}' for x in w))
    raw = rd(hdr, 0x40)
    for o in range(0, 0x40, 16):
        print(f'  hdr+{o:02X}: ' + ' '.join(f'{x:08X}' for x in struct.unpack('>4I', raw[o:o+16])))

def __lldb_init_module(debugger, internal_dict):
    debugger.HandleCommand('command script add -f lldb_barrier.barrier barrier')
