#!/usr/bin/env python3
"""Extract the WWS SPU job code modules embedded in the EBOOT.

ps3recomp's extract_jobmods.py pulls these out of a title's SDATA archives;
inFAMOUS 2 links them into the executable instead. Same format either way:

    +0x00  4 x ila          (the 16 bytes lift_jobmods.py signs a module by)
    +0x10  entryOffset
    +0x14  imageSize        (may run a few bytes past the file image: bss)
    +0x20  0xC0DEC0DE
    +0x28  load base        (0x4000, the job code buffer)

Output: <out>/jobmod_<sha1[:12]>_e<entryOffset>.bin, the naming
lift_jobmods.py expects.

Usage: extract_eboot_jobmods.py game/EBOOT.elf --out spu/jobmods
"""
import argparse, hashlib, os, struct

MAGIC = b"\xC0\xDE\xC0\xDE"


def is_module(d, s):
    if s < 0 or s + 0x30 > len(d):
        return False
    ilas = struct.unpack_from(">4I", d, s)
    return all((w >> 24) in (0x42, 0x43) for w in ilas)


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("elf")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    d = open(args.elf, "rb").read()
    starts, i = [], 0
    while True:
        i = d.find(MAGIC, i + 1)
        if i < 0:
            break
        if i % 4 == 0 and is_module(d, i - 0x20):
            starts.append(i - 0x20)

    os.makedirs(args.out, exist_ok=True)
    for n, s in enumerate(starts):
        entry, size = struct.unpack_from(">II", d, s + 0x10)
        end = s + size
        if n + 1 < len(starts):
            end = min(end, starts[n + 1])
        raw = d[s:min(end, len(d))]
        name = f"jobmod_{hashlib.sha1(raw).hexdigest()[:12]}_e{entry:x}.bin"
        with open(os.path.join(args.out, name), "wb") as f:
            f.write(raw)
        print(f"  file+0x{s:06X}  entry=0x{entry:05X} size=0x{size:05X} "
              f"({len(raw)} bytes) -> {name}")
    print(f"Extracted {len(starts)} job module(s) into {args.out}")


if __name__ == "__main__":
    main()
