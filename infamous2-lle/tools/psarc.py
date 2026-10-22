"""Minimal PSARC reader: psarc.py <archive> list | grep <bytes> [name-filter] | extract <name> <out>"""
import struct, sys, zlib

def open_psarc(path):
    f = open(path, 'rb')
    h = f.read(32)
    magic, ver, comp, toc_len, ent_size, n, bsize, flags = struct.unpack('>4sI4sIIIII', h)
    toc = f.read(toc_len - 32)
    entries = []
    for i in range(n):
        e = toc[i * ent_size:(i + 1) * ent_size]
        md5 = e[:16]; blk = struct.unpack('>I', e[16:20])[0]
        size = int.from_bytes(e[20:25], 'big'); off = int.from_bytes(e[25:30], 'big')
        entries.append((blk, size, off))
    zbytes = 2 if bsize <= 65536 else (3 if bsize <= 16777216 else 4)
    zt = toc[n * ent_size:]
    zsizes = [int.from_bytes(zt[i:i + zbytes], 'big') for i in range(0, len(zt) - zbytes + 1, zbytes)]
    return f, entries, zsizes, bsize

def read_entry(f, entry, zsizes, bsize):
    blk, size, off = entry
    out = bytearray(); f.seek(off)
    while len(out) < size:
        z = zsizes[blk]; blk += 1
        chunk = f.read(z if z else bsize)
        if z and chunk[:1] == b'\x78':
            try: chunk = zlib.decompress(chunk)
            except zlib.error: pass
        out += chunk
    return bytes(out[:size])

def names(path):
    f, entries, zsizes, bsize = open_psarc(path)
    man = read_entry(f, entries[0], zsizes, bsize).decode('latin1').split('\n')
    return f, entries, zsizes, bsize, man

if __name__ == '__main__':
    path, cmd = sys.argv[1], sys.argv[2]
    f, entries, zsizes, bsize, man = names(path)
    if cmd == 'list':
        for i, n in enumerate(man):
            print(i + 1, entries[i + 1][1] if i + 1 < len(entries) else '?', entries[i + 1][2] if i + 1 < len(entries) else '?', n)
    elif cmd == 'grep':
        pat = sys.argv[3].encode(); flt = sys.argv[4] if len(sys.argv) > 4 else ''
        for i, n in enumerate(man):
            if flt and flt not in n: continue
            if i + 1 >= len(entries): break
            try: data = read_entry(f, entries[i + 1], zsizes, bsize)
            except Exception as ex: print('ERR', n, ex); continue
            if pat in data: print(i + 1, n, entries[i + 1][1])
    elif cmd == 'extract':
        name = sys.argv[3]
        for i, n in enumerate(man):
            if n == name or n.endswith(name):
                open(sys.argv[4], 'wb').write(read_entry(f, entries[i + 1], zsizes, bsize)); print('wrote', n); break
