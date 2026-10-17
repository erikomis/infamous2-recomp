#!/usr/bin/env python3
"""decrypt_self.py -- a retail PS3 SELF (EBOOT.BIN, a firmware .sprx) to a plain ELF,
without RPCS3.

    python3 decrypt_self.py IN.self -o OUT.elf --keys KEYFILE [--rap FILE.rap]
    python3 decrypt_self.py IN.self --info          # describe, write nothing
    python3 decrypt_self.py --selftest              # synthetic round trip, no keys

The chain is the standard one:

    appldr erk/riv (by key revision and program type)  --AES-256-CBC-->  metadata info
    [NPDRM: klicensee, unwrapped with NP_klic_key       --AES-128-CBC-->  first]
    metadata key/iv  --AES-128-CTR-->  metadata header, section table, data keys
    section key/iv   --AES-128-CTR-->  each segment, zlib-inflated when compressed

The klicensee of an NPDRM SELF comes from the player's .rap (licence types 1
and 2: rap_to_rif, as in rap2rifkey.py) or is NP_klic_free (type 3).

The ELF is written the way RPCS3's `--decrypt` writes it (ELF header, program
headers after it, every program segment at its p_offset, the section header
table at e_shoff; nothing else), so the output is byte-identical to the
RPCS3-decrypted files the port is tested with.

No key is in this repository. --keys (default $PS3_KEYS) names a key file in
scetool's format -- `[section]` blocks with type=SELF (or category=SELF),
revision=, self_type= (or program_type=), erk=, riv=, and `[NP_klic_key]`
/ `[NP_klic_free]` blocks with key= -- and may also hold plain `name=hex`
lines (NP_klic_key=, NP_klic_free=, RAP_KEY=). The RAP key can also come from
$PS3_RAP_KEY, as for rap2rifkey.py; it is checked against its SHA-256.

The SELF layout and the decryption chain follow sp00nznet/twistedmetal's
tools/decrypt_self.py (MIT, Copyright (c) 2026 Ned Heller / sp00nznet) and
RPCS3's Crypto/unself.cpp (read, not copied).
"""
from __future__ import annotations

import argparse
import hashlib
import os
import struct
import sys
import zlib

try:                            # pycryptodome, in the toolkit's requirements; only
    from Crypto.Cipher import AES   # decrypting needs it (rebuild.py parses key files without)
except ImportError:
    AES = None

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from rap2rifkey import RAP_KEY_SHA256, rap_to_rif   # noqa: E402

PROGRAM_TYPES = {1: 'LV0', 2: 'LV1', 3: 'LV2', 4: 'APP', 5: 'ISO', 6: 'LDR', 7: 'UNK7', 8: 'NPDRM'}
KEY_REV_DEBUG = 0x8000          # fSELF: metadata info is not encrypted
LICENCES = {1: 'network', 2: 'local', 3: 'free'}


class SelfError(Exception):
    pass


# --------------------------------------------------------------------------
# keys
# --------------------------------------------------------------------------
def _hex(v):
    v = v.strip().replace(' ', '').replace(',', '')
    if v.lower().startswith('0x'):
        v = v[2:]
    return bytes.fromhex(v)


class Keys:
    """SELF key sets and named keys, from a scetool key file and/or name=hex lines."""

    def __init__(self):
        self.self_keys = []      # [(program type name, revision, erk, riv)]
        self.named = {}          # lower-case name -> bytes

    @classmethod
    def load(cls, path=None, env=os.environ):
        k = cls()
        if path:
            with open(path, encoding='utf-8', errors='replace') as f:
                k.parse(f.read())
        if env.get('PS3_RAP_KEY') and 'rap_key' not in k.named:
            k.named['rap_key'] = _hex(env['PS3_RAP_KEY'])
        return k

    def parse(self, text):
        blocks, cur = [], None
        for raw in text.splitlines():
            line = raw.split('#', 1)[0].strip()
            if not line or line.startswith(';'):
                continue
            if line.startswith('[') and line.endswith(']'):
                cur = {'name': line[1:-1].strip()}
                blocks.append(cur)
                continue
            if '=' not in line:
                continue
            name, _, val = line.partition('=')
            name, val = name.strip(), val.strip()
            if cur is None:                     # a bare name=hex line
                try:
                    self.named[name.lower()] = _hex(val)
                except ValueError:
                    raise SelfError('key file: %s is not hex' % name)
            else:
                cur[name.lower()] = val
        for b in blocks:
            kind = (b.get('type') or b.get('category') or '').upper()
            ptype = (b.get('self_type') or b.get('program_type') or '').upper()
            try:
                if kind == 'SELF' and b.get('erk') and b.get('riv') and b.get('revision'):
                    self.self_keys.append((ptype, int(b['revision'], 16), _hex(b['erk']), _hex(b['riv'])))
                elif b.get('key'):
                    self.named.setdefault(b['name'].lower(), _hex(b['key']))
            except ValueError:
                raise SelfError('key file: block [%s] has a value that is not hex' % b['name'])

    def self_key(self, ptype, revision):
        for t, rev, erk, riv in self.self_keys:
            if t == ptype and rev == revision and len(erk) == 32 and len(riv) == 16:
                return erk, riv
        raise SelfError('no SELF key for program type %s, key revision 0x%04X in the key file'
                        % (ptype, revision))

    def get(self, name, size=16):
        v = self.named.get(name.lower())
        if v is None:
            raise SelfError('no %s in the key file' % name)
        if len(v) != size:
            raise SelfError('%s is %d bytes; expected %d' % (name, len(v), size))
        return v

    def rap_key(self):
        k = self.named.get('rap_key')
        if k is None:
            raise SelfError('no RAP key: put RAP_KEY=<32 hex> in the key file or set PS3_RAP_KEY')
        if hashlib.sha256(k).hexdigest() != RAP_KEY_SHA256:
            raise SelfError('RAP key: wrong key (SHA-256 does not match)')
        return k


# --------------------------------------------------------------------------
# the SELF
# --------------------------------------------------------------------------
def _need_aes():
    if AES is None:
        raise SelfError('pycryptodome is not installed (pip install pycryptodome)')


def aes_ctr(key, iv, data):
    _need_aes()
    return AES.new(key, AES.MODE_CTR, nonce=b'', initial_value=iv).decrypt(data)


class Self:
    def __init__(self, data: bytes):
        self.d = data
        if len(data) < 0x70 or data[:4] != b'SCE\0':
            raise SelfError('not a SELF (no SCE\\0 magic)')
        (_, self.version, self.key_revision, self.header_type, self.meta_off,
         self.header_len, self.data_len) = struct.unpack_from('>4sIHHIQQ', data, 0)
        if self.header_type != 1:
            raise SelfError('SCE header type %d is not a SELF' % self.header_type)
        (_ext, self.app_info_off, self.elf_off, self.phdr_off, self.shdr_off,
         _seg, _ver, self.ctrl_off, self.ctrl_size, _pad) = struct.unpack_from('>10Q', data, 0x20)
        self._need(self.app_info_off, 0x18, 'program info')
        (self.auth_id, self.vendor_id, self.program_type,
         self.app_version) = struct.unpack_from('>QIIQ', data, self.app_info_off)

        self._need(self.elf_off, 0x40, 'ELF header')
        self.eh = data[self.elf_off:self.elf_off + 0x40]
        if self.eh[:4] != b'\x7fELF':
            raise SelfError('the embedded ELF header is missing')
        if self.eh[4] != 2:
            raise SelfError('a 32-bit ELF (an SPU image?): only 64-bit PPU ELFs are handled')
        self.e_entry, self.e_phoff, self.e_shoff = struct.unpack_from('>QQQ', self.eh, 0x18)
        self.e_phentsize, self.e_phnum, self.e_shentsize, self.e_shnum = \
            struct.unpack_from('>HHHH', self.eh, 0x36)
        if self.e_phentsize != 0x38 or (self.e_shnum and self.e_shentsize != 0x40):
            raise SelfError('unexpected ELF header/entry sizes')
        self._need(self.phdr_off, self.e_phnum * 0x38, 'program headers')
        self.phdrs = [struct.unpack_from('>IIQQQQQQ', data, self.phdr_off + i * 0x38)
                      for i in range(self.e_phnum)]

    def _need(self, off, n, what):
        if off + n > len(self.d):
            raise SelfError('the SELF is truncated (%s)' % what)

    @property
    def type_name(self):
        return PROGRAM_TYPES.get(self.program_type, str(self.program_type))

    def npd(self):
        """(licence type, content id) from the NPDRM control info, or None."""
        ctrl = self.d[self.ctrl_off:self.ctrl_off + self.ctrl_size]
        i = ctrl.find(b'NPD\0')
        if i < 0:
            return None
        lic = struct.unpack_from('>I', ctrl, i + 8)[0]
        cid = ctrl[i + 0x10:i + 0x40].split(b'\0', 1)[0].decode('ascii', 'replace')
        return lic, cid

    def describe(self):
        fw = '%x.%04x' % (self.app_version >> 48, (self.app_version >> 32) & 0xffff)
        s = ('SELF v%d key_rev=0x%04X type=%s auth_id=0x%016X version=%s entry=0x%X phnum=%d shnum=%d'
             % (self.version, self.key_revision, self.type_name, self.auth_id, fw,
                self.e_entry, self.e_phnum, self.e_shnum))
        n = self.npd()
        if n:
            s += '\n  NPDRM licence %d (%s), content id %s' % (n[0], LICENCES.get(n[0], '?'), n[1])
        return s

    def klicensee(self, keys, rap=None):
        """The NPDRM content key, or None for a SELF without an NPDRM layer."""
        n = self.npd()
        if n is None:
            return None
        lic, cid = n
        if lic in (1, 2):
            if rap is None:
                raise SelfError('NPDRM licence %d: the player\'s .rap for %s is needed (--rap)' % (lic, cid))
            if len(rap) != 16:
                raise SelfError('a .rap is 16 bytes, this one is %d' % len(rap))
            return rap_to_rif(rap, keys.rap_key())
        if lic == 3:
            return keys.get('NP_klic_free')
        raise SelfError('NPDRM licence type %d is not known' % lic)

    def metadata(self, erk, riv, klic=None, klic_key=None):
        off = 0x20 + self.meta_off
        self._need(off, 0x40, 'metadata info')
        info = self.d[off:off + 0x40]
        if not self.key_revision & KEY_REV_DEBUG:
            if klic is not None:
                k = AES.new(klic_key, AES.MODE_ECB).decrypt(klic)
                info = AES.new(k, AES.MODE_CBC, bytes(16)).decrypt(info)
            info = AES.new(erk, AES.MODE_CBC, riv).decrypt(info)
        key, key_pad, iv, iv_pad = info[0:16], info[16:32], info[32:48], info[48:64]
        if key_pad != bytes(16) or iv_pad != bytes(16):
            raise SelfError('metadata info did not decrypt (wrong key, or the wrong .rap for this copy)')
        if self.header_len > len(self.d) or self.header_len < off + 0x40 + 0x20:
            raise SelfError('bad SELF header length')
        hdrs = aes_ctr(key, iv, self.d[off + 0x40:self.header_len])
        _sig, _u1, nsec, nkeys, _opt = struct.unpack_from('>QIIII', hdrs, 0)
        if not (0 < nsec < 256 and 0 < nkeys < 1024) or 0x20 + nsec * 0x30 + nkeys * 0x10 > len(hdrs):
            raise SelfError('implausible metadata (%d sections, %d keys)' % (nsec, nkeys))
        names = ('data_off', 'data_size', 'type', 'prog_idx', 'hashed', 'sha1_idx',
                 'encrypted', 'key_idx', 'iv_idx', 'compressed')
        sections = [dict(zip(names, struct.unpack_from('>QQIIIIIIII', hdrs, 0x20 + i * 0x30)))
                    for i in range(nsec)]
        ko = 0x20 + nsec * 0x30
        dkeys = [hdrs[ko + i * 0x10:ko + (i + 1) * 0x10] for i in range(nkeys)]
        return sections, dkeys

    def to_elf(self, keys, rap=None, log=None):
        _need_aes()
        erk = riv = None
        if not self.key_revision & KEY_REV_DEBUG:
            erk, riv = keys.self_key(self.type_name, self.key_revision)
        klic = self.klicensee(keys, rap)
        klic_key = keys.get('NP_klic_key') if klic is not None else None
        sections, dkeys = self.metadata(erk, riv, klic, klic_key)

        out = bytearray()

        def put(o, blob):
            if o + len(blob) > len(out):
                out.extend(bytes(o + len(blob) - len(out)))
            out[o:o + len(blob)] = blob

        # As RPCS3's SELFDecrypter::WriteElf: header, program headers right
        # after it, each program segment at its p_offset, section headers at
        # e_shoff. Non-program sections (type 3) are not written.
        put(0, self.eh)
        put(0x40, self.d[self.phdr_off:self.phdr_off + self.e_phnum * 0x38])
        for s in sections:
            if s['type'] != 2:
                continue
            if s['prog_idx'] >= len(self.phdrs):
                raise SelfError('section names program header %d of %d' % (s['prog_idx'], len(self.phdrs)))
            ph = self.phdrs[s['prog_idx']]
            p_offset, p_filesz = ph[2], ph[5]
            self._need(s['data_off'], s['data_size'], 'segment data')
            blob = self.d[s['data_off']:s['data_off'] + s['data_size']]
            if s['encrypted'] == 3:
                if s['key_idx'] >= len(dkeys) or s['iv_idx'] >= len(dkeys):
                    raise SelfError('section key index out of range')
                blob = aes_ctr(dkeys[s['key_idx']], dkeys[s['iv_idx']], blob)
            if s['compressed'] == 2:
                try:
                    blob = zlib.decompressobj().decompress(blob, p_filesz)
                except zlib.error as e:
                    raise SelfError('segment %d does not inflate (%s): wrong key?' % (s['prog_idx'], e))
            if log:
                log('  phdr[%d] -> 0x%X, 0x%X bytes%s' % (s['prog_idx'], p_offset, len(blob),
                                                         ' (inflated)' if s['compressed'] == 2 else ''))
            put(p_offset, blob)
        if self.shdr_off and self.e_shnum:
            self._need(self.shdr_off, self.e_shnum * 0x40, 'section headers')
            put(self.e_shoff, self.d[self.shdr_off:self.shdr_off + self.e_shnum * 0x40])
        return bytes(out)


def check_elf(elf, prx):
    """A wrong key still yields a file: make sure this one looks like code."""
    if elf[:4] != b'\x7fELF':
        return 'no ELF magic'
    e_type = struct.unpack_from('>H', elf, 0x10)[0]
    e_entry, e_phoff = struct.unpack_from('>QQ', elf, 0x18)
    phnum = struct.unpack_from('>H', elf, 0x38)[0]
    loads = []
    for i in range(phnum):
        p = struct.unpack_from('>IIQQQQQQ', elf, e_phoff + i * 0x38)
        if p[0] == 1:
            loads.append(p)
    if not loads:
        return 'no PT_LOAD segment'
    if prx or e_type == 0xFFA4:
        # a PRX: its first segment starts with the module info's attributes,
        # and the TOC-relative module info must be inside the image
        return None
    end = max(p[3] + p[5] for p in loads)
    for p in loads:
        if p[3] <= e_entry < p[3] + p[5]:
            fo = p[2] + (e_entry - p[3])
            func, toc = struct.unpack_from('>II', elf, fo)
            if 0 < func < end and 0 < toc < end:
                return None
            return 'entry descriptor (func 0x%X, toc 0x%X) points outside the image' % (func, toc)
    return 'entry point 0x%X is outside every PT_LOAD segment' % e_entry


def decrypt_file(src, dst, keys, rap=None, log=print):
    with open(src, 'rb') as f:
        data = f.read()
    if data[:4] == b'\x7fELF':
        raise SelfError('%s is already a plain ELF' % src)
    s = Self(data)
    log(s.describe())
    elf = s.to_elf(keys, rap, log=log)
    bad = check_elf(elf, prx=src.lower().endswith(('.sprx', '.prx')))
    if bad:
        raise SelfError('the decrypted ELF does not look right (%s): wrong key?' % bad)
    tmp = dst + '.tmp'
    with open(tmp, 'wb') as f:
        f.write(elf)
    os.replace(tmp, dst)
    log('%s  %d bytes  sha256 %s' % (dst, len(elf), hashlib.sha256(elf).hexdigest()))
    return elf


# --------------------------------------------------------------------------
# self-test: a synthetic SELF, made-up keys (nothing here is a real key)
# --------------------------------------------------------------------------
def _fake_self(elf_hdr, phdrs, segs, shdrs, erk, riv, rev, ptype, npd=None, klic=None, klic_key=None):
    """Encrypt a synthetic ELF into a SELF the way a signer would (no signature)."""
    import random
    rnd = random.Random(1)
    rb = lambda n: bytes(rnd.getrandbits(8) for _ in range(n))
    mkey, miv = rb(16), rb(16)
    nsec = len(segs)
    dkeys = []
    secs = []
    payload = b''
    ctrl = b''
    if npd is not None:
        lic, cid = npd
        body = b'NPD\0' + struct.pack('>III', 3, lic, 1) + cid.encode().ljust(0x30, b'\0') + bytes(0x50)
        ctrl = struct.pack('>IIQ', 3, 0x10 + len(body), 0) + body
    app_off, elf_off = 0x70, 0x90
    phdr_off = elf_off + 0x40
    shdr_off = phdr_off + len(phdrs)
    ctrl_off = shdr_off + len(shdrs)
    meta_off = ctrl_off + len(ctrl)                      # file offset of metadata info
    hdr_len = meta_off + 0x40 + 0x20 + nsec * 0x30 + 2 * nsec * 0x10
    pos = hdr_len
    for i, (data, compress) in enumerate(segs):
        blob = zlib.compress(data) if compress else data
        k, v = rb(16), rb(16)
        dkeys += [k, v]
        enc = aes_ctr(k, v, blob)
        secs.append(struct.pack('>QQIIIIIIII', pos, len(enc), 2, i, 0, 0, 3, 2 * i, 2 * i + 1,
                                2 if compress else 1))
        payload += enc
        pos += len(enc)
    mhdr = struct.pack('>QIIIIII', 0, 0, nsec, 2 * nsec, 0, 0, 0)
    hdrs = aes_ctr(mkey, miv, mhdr + b''.join(secs) + b''.join(dkeys))
    info = AES.new(erk, AES.MODE_CBC, riv).encrypt(mkey + bytes(16) + miv + bytes(16))
    if klic is not None:
        k = AES.new(klic_key, AES.MODE_ECB).decrypt(klic)
        info = AES.new(k, AES.MODE_CBC, bytes(16)).encrypt(info)
    sce = struct.pack('>4sIHHIQQ', b'SCE\0', 2, rev, 1, meta_off - 0x20, hdr_len, 0)
    ext = struct.pack('>10Q', 3, app_off, elf_off, phdr_off, shdr_off, 0, 0, ctrl_off, len(ctrl), 0)
    app = struct.pack('>QIIQ', 0x1010000001000003, 0x01000002, ptype, 0x0001000000000000) + bytes(8)
    head = sce + ext + app + elf_hdr + phdrs + shdrs + ctrl
    assert len(head) == meta_off
    return head + info + hdrs + payload


def selftest():
    # a tiny ELF64: header, 2 program headers, 2 segments, 1 section header
    code = struct.pack('>II', 0x10208, 0x18000) + bytes(range(256)) * 8
    data = b'inFAMOUS-2-selftest' * 300
    ph = [(1, 5, 0x200, 0x10000, 0x10000, len(code), len(code), 0x10000),
          (1, 6, 0x200 + len(code), 0x20000, 0x20000, len(data), len(data) + 0x100, 0x10000)]
    shoff = 0x200 + len(code) + len(data)
    eh = (b'\x7fELF\x02\x02\x01\x66' + bytes(8) +
          struct.pack(">HHIQQQIHHHHHH", 2, 0x15, 1, 0x10000, 0x40, shoff, 0, 0x40, 0x38, 2, 0x40, 1, 0))
    phdrs = b''.join(struct.pack('>IIQQQQQQ', *p) for p in ph)
    shdrs = struct.pack('>IIQQQQIIQQ', 1, 1, 6, 0x10000, 0x200, len(code), 0, 0, 4, 0)
    code = struct.pack('>II', 0x10208, 0x18000) + code[8:]
    # entry 0x10000 is the start of segment 0 (file 0x200): an OPD (func, toc)
    want = bytearray(shoff + len(shdrs))
    want[0:0x40] = eh
    want[0x40:0x40 + len(phdrs)] = phdrs
    want[0x200:0x200 + len(code)] = code
    want[0x200 + len(code):shoff] = data
    want[shoff:] = shdrs
    want = bytes(want)

    erk, riv = bytes(range(32)), bytes(range(16, 32))           # made up
    klic_key, rap_key = bytes([0xA5] * 16), bytes([0x3C] * 16)  # made up
    keys = Keys()
    keys.parse('[appldr]\ncategory=SELF\nrevision=0010\nprogram_type=NPDRM\nerk=%s\nriv=%s\n'
               '[NP_klic_key]\nkey=%s\n' % (erk.hex(), riv.hex(), klic_key.hex()))
    keys.parse('[appldr]\ntype=SELF\nrevision=001C\nself_type=APP\nerk=%s\nriv=%s\n'
               % (erk[::-1].hex(), riv[::-1].hex()))

    # 1. application SELF, one compressed segment
    blob = _fake_self(eh, phdrs, [(code, True), (data, False)], shdrs,
                      erk[::-1], riv[::-1], 0x1C, 4)
    s = Self(blob)
    assert s.type_name == 'APP' and s.npd() is None
    got = s.to_elf(keys)
    assert got == want, 'APP round trip differs'
    assert check_elf(got, prx=False) is None

    # 2. NPDRM SELF, local licence, klicensee from a (made-up) .rap
    rap = bytes(range(0x40, 0x50))
    klic = rap_to_rif(rap, rap_key)
    blob = _fake_self(eh, phdrs, [(code, False), (data, True)], shdrs, erk, riv, 0x10, 8,
                      npd=(2, 'UP0000-NPXX00000_00-SELFTEST00000000'), klic=klic, klic_key=klic_key)
    s = Self(blob)
    assert s.npd() == (2, 'UP0000-NPXX00000_00-SELFTEST00000000')
    keys.named['rap_key'] = rap_key
    import rap2rifkey
    real = rap2rifkey.RAP_KEY_SHA256
    globals()['RAP_KEY_SHA256'] = hashlib.sha256(rap_key).hexdigest()   # accept the made-up key
    try:
        assert s.to_elf(keys, rap=rap) == want, 'NPDRM round trip differs'
        try:
            s.to_elf(keys, rap=bytes(16))
            raise AssertionError('a wrong .rap must fail')
        except SelfError:
            pass
        try:
            s.to_elf(keys)
            raise AssertionError('a missing .rap must fail')
        except SelfError:
            pass
    finally:
        globals()['RAP_KEY_SHA256'] = real
    try:
        s.to_elf(keys, rap=rap)
        raise AssertionError('a RAP key with the wrong hash must be refused')
    except SelfError:
        pass

    # 3. no key for the revision
    k2 = Keys()
    k2.parse('NP_klic_key=%s\n' % klic_key.hex())
    try:
        Self(blob).to_elf(k2, rap=rap)
        raise AssertionError('expected a missing-key error')
    except SelfError:
        pass
    print('selftest ok')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('self_file', nargs='*', help='EBOOT.BIN or .sprx (several: each to <name>.elf/.prx)')
    ap.add_argument('-o', '--output', help='output ELF (one input only; default: .BIN -> .elf, .sprx -> .prx)')
    ap.add_argument('--keys', default=os.environ.get('PS3_KEYS'), help='key file (default: $PS3_KEYS)')
    ap.add_argument('--rap', help="the player's .rap, for an NPDRM SELF with licence type 1 or 2")
    ap.add_argument('--info', action='store_true', help='describe the SELF, write nothing')
    ap.add_argument('--selftest', action='store_true', help='synthetic round trip with made-up keys')
    a = ap.parse_args(argv)
    if a.selftest:
        try:
            selftest()
        except SelfError as e:
            print('error: %s' % e, file=sys.stderr)
            return 1
        return 0
    if not a.self_file:
        ap.error('a SELF file is required')
    if a.output and len(a.self_file) > 1:
        ap.error('-o takes one input')
    try:
        if a.info:
            for p in a.self_file:
                with open(p, 'rb') as f:
                    print('%s: %s' % (p, Self(f.read()).describe()))
            return 0
        if not a.keys:
            ap.error('no key file: pass --keys or set PS3_KEYS')
        keys = Keys.load(a.keys)
        rap = None
        if a.rap:
            with open(a.rap, 'rb') as f:
                rap = f.read()
        for p in a.self_file:
            base, ext = os.path.splitext(p)
            out = a.output or base + ('.prx' if ext.lower() == '.sprx' else '.elf')
            decrypt_file(p, out, keys, rap)
    except (SelfError, OSError) as e:
        print('error: %s' % e, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
