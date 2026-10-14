#!/usr/bin/env python3
"""rap2rifkey.py EDAT [--exdata DIR] [--rap FILE]

Derive the RIF key of a licence-type-1/2 EDAT from the player's own .rap and
write it beside the EDAT as <EDAT>.rifkey, the sidecar ps3recomp's EDAT reader
takes (see libs/filesystem/edat.c: "PS3_RIF_KEY or <file>.rifkey").

The content id is read from the EDAT's NPD header (0x30 bytes at 0x10) and the
.rap is looked up as <content id>.rap in --exdata (default: RPCS3's
dev_hdd0/home/00000001/exdata), unless --rap names it. The conversion is the
public rap_to_rif (RPCS3 Crypto/key_vault.cpp): AES-128 decrypt under the RAP
key, then five rounds of the P-box / E1 / E2 mixing.

This runs on the player's own licence file for their own copy; the runtime
deliberately does not do it on its own.
"""
import argparse, os, sys
from Crypto.Cipher import AES   # pycryptodome, in the toolkit's requirements

RAP_KEY = bytes.fromhex(os.environ['PS3_RAP_KEY'])   # removed from history; RAP_KEY in RPCS3 Crypto/key_vault.h
PBOX = [0x0C, 0x03, 0x06, 0x04, 0x01, 0x0B, 0x0F, 0x08, 0x02, 0x07, 0x00, 0x05, 0x0A, 0x0E, 0x0D, 0x09]
E1 = [0xA9, 0x3E, 0x1F, 0xD6, 0x7C, 0x55, 0xA3, 0x29, 0xB7, 0x5F, 0xDD, 0xA6, 0x2A, 0x95, 0xC7, 0xA5]
E2 = [0x67, 0xD4, 0x5D, 0xA3, 0x29, 0x6D, 0x00, 0x6A, 0x4E, 0x7C, 0x53, 0x7B, 0xF5, 0x53, 0x8C, 0x74]


def rap_to_rif(rap):
    key = bytearray(AES.new(RAP_KEY, AES.MODE_ECB).decrypt(rap))   # CBC with a zero IV on one block
    for _ in range(5):
        for i in range(16):
            p = PBOX[i]; key[p] ^= E1[p]
        for i in range(15, 0, -1):
            p, pp = PBOX[i], PBOX[i - 1]; key[p] ^= key[pp]
        o = 0
        for i in range(16):
            p = PBOX[i]; kc = (key[p] - o) & 0xFF; ec2 = E2[p]
            if o != 1 or kc != 0xFF:
                o = 1 if kc < ec2 else 0
                key[p] = (kc - ec2) & 0xFF
            elif kc == 0xFF:
                key[p] = (kc - ec2) & 0xFF
            else:
                key[p] = kc
    return bytes(key)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('edat')
    ap.add_argument('--exdata', default=os.path.expanduser('~/Library/Application Support/rpcs3/dev_hdd0/home/00000001/exdata'))
    ap.add_argument('--rap', help='the .rap to use instead of <content id>.rap in --exdata')
    a = ap.parse_args()

    hdr = open(a.edat, 'rb').read(0x100)
    if hdr[:4] != b'NPD\0':
        sys.exit(f'{a.edat}: not an NPD/EDAT file')
    content_id = hdr[0x10:0x40].split(b'\0', 1)[0].decode('ascii', 'replace')
    license_type = int.from_bytes(hdr[0x08:0x0C], 'big')
    print(f'{a.edat}: content id {content_id}, licence type {license_type}')
    if license_type not in (1, 2):
        print('  licence type needs no RIF key; nothing written'); return

    rap_path = a.rap or os.path.join(a.exdata, content_id + '.rap')
    try:
        rap = open(rap_path, 'rb').read()
    except OSError as e:
        sys.exit(f'  no .rap for this content id: {e}')
    if len(rap) != 16:
        sys.exit(f'  {rap_path}: a .rap is 16 bytes, this one is {len(rap)}')
    rif = rap_to_rif(rap)
    out = a.edat + '.rifkey'
    with open(out, 'wb') as f:
        f.write(rif)
    print(f'  {rap_path} -> {out} ({rif.hex()})')


if __name__ == '__main__':
    main()
