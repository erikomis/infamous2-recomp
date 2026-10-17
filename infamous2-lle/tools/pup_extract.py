#!/usr/bin/env python3
"""pup_extract.py -- the firmware modules the port translates, straight from
Sony's PS3 system software update (PS3UPDAT.PUP), without RPCS3.

    python3 -I pup_extract.py PS3UPDAT.PUP --info [--keys KEYFILE]
    python3 -I pup_extract.py PS3UPDAT.PUP -o DIR --keys KEYFILE [--all]
    python3 -I pup_extract.py --selftest            # synthetic round trip, no keys

-o DIR gets DIR/dev_flash/sys/external/{liblv2,libsysmodule,libsre}.sprx
(--all: the whole dev_flash), the layout `rebuild.py --firmware DIR` and
RPCS3 use. The .sprx are still SELFs: tools/decrypt_self.py (or rebuild.py)
turns them into the .prx the port lifts.

The chain:

    PS3UPDAT.PUP      "SCEUF" container: header, file table, hash table; one
                      entry (0x300) is update_files.tar (plain ustar)
    update_files.tar  holds dev_flash_NNN.tar.aa.<date>, each an SCE
                      container of type PKG
    SCE PKG           metadata info --AES-256-CBC (the PKG key set)-->
                      metadata key/iv --AES-128-CTR--> section table, data
                      keys --AES-128-CTR--> sections (zlib when compressed);
                      one section is a tar of dev_flash/...

The modules are in one or two of the ~25 packages; the extraction stops as
soon as it has all three. The port is tested with firmware 4.93: another
version only warns (the hashes rebuild.py checks will then differ too).

No key is in this repository. --keys (default $PS3_KEYS) is the player's key
file in scetool's format, read by decrypt_self.Keys: the PKG key set is a
`[section]` with type=PKG (newer key files: category=PKG), erk= and riv=
(revision= optional), or SCEPKG_ERK= / SCEPKG_RIV= lines (RPCS3's names).
Nothing is downloaded. The PUP's HMAC-SHA1 hashes are not checked (that needs
another key); every package's metadata is checked as it decrypts.

The PUP and firmware-package layout follow RPCS3's Loader/PUP.cpp and
Crypto/unself.cpp (SCEDecrypter) and the PS3 Developer Wiki (read, not copied).
"""
from __future__ import annotations

import argparse
import io
import os
import posixpath
import re
import struct
import sys
import tarfile
import zlib

sys.dont_write_bytecode = True      # leave no __pycache__ next to the tools
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import decrypt_self as ds           # noqa: E402

PUP_MAGIC = b'SCEUF\0\0\0'
PUP_ENTRIES = {0x100: 'version.txt', 0x101: 'license.xml', 0x102: 'promo_flags.txt',
               0x103: 'update_flags.txt', 0x104: 'patch_build.txt', 0x200: 'ps3swu.self',
               0x201: 'vsh.tar', 0x202: 'dots.txt', 0x203: 'patch_data.pkg',
               0x300: 'update_files.tar', 0x501: 'spkg_hdr.tar', 0x601: 'ps3swu2.self'}
ENTRY_VERSION, ENTRY_UPDATE_FILES = 0x100, 0x300
SCE_TYPE_PKG = 3
DEV_FLASH_PKG = re.compile(r'^dev_flash_\d+\.tar\.aa\.')     # not dev_flash3_*
EXTERNAL = 'sys/external/'           # inside dev_flash
MODULES = ('liblv2', 'libsysmodule', 'libsre')
TESTED_FIRMWARE = '4.93'
# The SELF key set the firmware modules of a version are signed with (read
# from the clear headers of the tested 4.93 modules), so rebuild.py can name
# it before anything is extracted. A version not listed is checked afterwards.
MODULE_SELF_KEY = {'4.93': ('APP', 0x1C)}
PKG_KEY_HELP = ('the PKG key set for the firmware packages: a [section] with type=PKG '
                '(newer key files: category=PKG), erk=<64 hex> and riv=<32 hex>, '
                'or SCEPKG_ERK= / SCEPKG_RIV= lines')


class PupError(Exception):
    pass


# --------------------------------------------------------------------------
# the PUP
# --------------------------------------------------------------------------
class Window:
    """A read-only, seekable view of [start, start+size) of an open file (for tarfile)."""

    def __init__(self, f, start, size):
        self.f, self.start, self.size, self.pos = f, start, size, 0

    def tell(self):
        return self.pos

    def seek(self, off, whence=0):
        base = (0, self.pos, self.size)[whence]
        self.pos = max(0, base + off)
        return self.pos

    def read(self, n=-1):
        left = self.size - self.pos
        n = left if n is None or n < 0 else min(n, left)
        if n <= 0:
            return b''
        self.f.seek(self.start + self.pos)
        b = self.f.read(n)
        self.pos += len(b)
        return b


def norm_version(text):
    """'4.93\\n' or '04.9300' -> '4.93'."""
    m = re.search(r'(\d+)\.(\d+)', text or '')
    return '%d.%s' % (int(m.group(1)), m.group(2)[:2].ljust(2, '0')) if m else None


class Pup:
    def __init__(self, path):
        self.path = path
        self.f = open(path, 'rb')
        try:
            self._parse()
        except Exception:
            self.f.close()
            raise

    def close(self):
        self.f.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    def _parse(self):
        size = os.fstat(self.f.fileno()).st_size
        h = self.f.read(0x30)
        if len(h) < 0x30 or h[:8] != PUP_MAGIC:
            raise PupError('%s: not a PS3 update (no SCEUF magic)' % self.path)
        (_, self.package_version, self.image_version, n,
         self.header_length, self.data_length) = struct.unpack('>8sQQQQQ', h)
        if not 0 < n < 256 or 0x30 + n * 0x40 > size:
            raise PupError('%s: implausible file table (%d entries)' % (self.path, n))
        tbl = self.f.read(n * 0x20)
        self.entries = {}
        for i in range(n):
            eid, off, ln = struct.unpack_from('>QQQ', tbl, i * 0x20)
            if off + ln > size:
                raise PupError('%s is truncated (entry 0x%X ends at 0x%X, the file at 0x%X)'
                               % (self.path, eid, off + ln, size))
            self.entries[eid] = (off, ln)
        self._tar = None

    def entry(self, eid):
        if eid not in self.entries:
            raise PupError('%s has no %s (entry 0x%X)' % (self.path, PUP_ENTRIES.get(eid, '?'), eid))
        off, ln = self.entries[eid]
        return Window(self.f, off, ln)

    def version(self):
        """The system software version ('4.93'), from version.txt."""
        if ENTRY_VERSION not in self.entries:
            return None
        return norm_version(self.entry(ENTRY_VERSION).read(64).decode('ascii', 'replace'))

    def update_files(self):
        if self._tar is None:
            try:
                self._tar = tarfile.open(fileobj=self.entry(ENTRY_UPDATE_FILES), mode='r:')
                self._members = self._tar.getmembers()
            except tarfile.TarError as e:
                raise PupError('update_files.tar does not read (%s)' % e)
        return self._tar

    def packages(self):
        """The dev_flash packages (TarInfo), in name order."""
        self.update_files()
        return sorted((m for m in self._members if m.isfile() and DEV_FLASH_PKG.match(m.name)),
                      key=lambda m: m.name)

    def read(self, member, n=None):
        f = self.update_files().extractfile(member)
        return f.read() if n is None else f.read(n)

    def package_headers(self):
        """[(TarInfo, key revision)] of the dev_flash packages, from their clear SCE headers."""
        out = []
        for m in self.packages():
            h = sce_header(self.read(m, 0x20), m.name)
            out.append((m, h[1]))
        return out


# --------------------------------------------------------------------------
# the SCE PKG
# --------------------------------------------------------------------------
def sce_header(d, what):
    if len(d) < 0x20 or d[:4] != b'SCE\0':
        raise PupError('%s: not an SCE container' % what)
    _, version, key_rev, htype, meta_off, header_len, data_len = struct.unpack_from('>4sIHHIQQ', d, 0)
    if htype != SCE_TYPE_PKG:
        raise PupError('%s: SCE header type %d is not a PKG' % (what, htype))
    return version, key_rev, meta_off, header_len, data_len


def decrypt_pkg(d, keys, what='package'):
    """The decrypted sections of an SCE PKG (a list of bytes)."""
    _v, key_rev, meta_off, header_len, _dl = sce_header(d, what)
    off = 0x20 + meta_off
    if off + 0x40 > len(d):
        raise PupError('%s is truncated (metadata info)' % what)
    raw = d[off:off + 0x40]
    if key_rev & ds.KEY_REV_DEBUG:
        tries = [None]                  # a debug package: the metadata info is clear
    else:
        tries = keys.pkg_key_sets(key_rev)
        if not tries:
            raise PupError('the key file has no %s' % PKG_KEY_HELP)
    ds._need_aes()
    err = None
    for k in tries:
        info = raw if k is None else ds.AES.new(k[0], ds.AES.MODE_CBC, k[1]).decrypt(raw)
        try:
            sections, dkeys = ds.sce_metadata(d, off, header_len, info)
            break
        except ds.SelfError as e:
            err = e
    else:
        raise PupError('%s: the metadata did not decrypt with any PKG key set in the key file '
                       '(wrong key?): %s' % (what, err))
    out = []
    for i, s in enumerate(sections):
        if s['data_off'] + s['data_size'] > len(d):
            raise PupError('%s is truncated (section %d)' % (what, i))
        blob = d[s['data_off']:s['data_off'] + s['data_size']]
        if s['encrypted'] == 3:
            if max(s['key_idx'], s['iv_idx']) >= len(dkeys):
                raise PupError('%s: section key index out of range' % what)
            blob = ds.aes_ctr(dkeys[s['key_idx']], dkeys[s['iv_idx']], blob)
        if s['compressed'] == 2:
            try:
                z = zlib.decompressobj()
                blob = z.decompress(blob) + z.flush()
            except zlib.error as e:
                raise PupError('%s: section %d does not inflate (%s)' % (what, i, e))
        out.append(blob)
    return out


def pkg_tar(sections, what):
    """The section that is the dev_flash tar."""
    for s in sections:
        if len(s) >= 512 and s[257:262] == b'ustar':
            return s
    if len(sections) > 2:               # RPCS3 takes the third
        return sections[2]
    raise PupError('%s: no tar section' % what)


def safe_path(name):
    """A tar member name ('dev_flash/sys/...', './dev_flash/...', '/dev_flash/...' or
    'sys/...') -> its path inside dev_flash ('sys/...'), or None if it escapes."""
    parts = name.replace('\\', '/').split('/')
    if '..' in parts:
        return None                     # nothing in a firmware tar climbs; refuse what does
    p = posixpath.normpath('/' + '/'.join(parts)).lstrip('/')
    if p == 'dev_flash' or p.startswith('dev_flash/'):
        p = p[len('dev_flash/'):]
    return p or None


def write_file(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + '.tmp'
    with open(tmp, 'wb') as f:
        f.write(data)
    os.replace(tmp, path)


# --------------------------------------------------------------------------
# what a run needs, and the extraction
# --------------------------------------------------------------------------
def missing_keys(pup, keys, need_self=True):
    """(missing, notes): the key sets the key file lacks to extract this PUP
    (and, with need_self, to decrypt its modules), each named exactly."""
    missing, notes = [], []
    revs = sorted({rev for _m, rev in pup.package_headers() if not rev & ds.KEY_REV_DEBUG})
    if revs and not keys.pkg_key_sets(revs[0]):
        missing.append('%s (package key revision %s)' % (PKG_KEY_HELP, ', '.join('0x%04X' % r for r in revs)))
    if need_self:
        ver = pup.version()
        want = MODULE_SELF_KEY.get(ver)
        if want is None:
            notes.append('firmware %s: the SELF key set of its modules is known only once they '
                         'are extracted' % ver)
        else:
            try:
                keys.self_key(*want)
            except ds.SelfError:
                missing.append('%s (%s, key revision 0x%04X): the SELF key set of the firmware %s '
                               'modules' % ('/'.join(m + '.sprx' for m in MODULES), want[0], want[1], ver))
    return missing, notes


def extract(pup_path, out_dir, keys, everything=False, log=print):
    """Write <out_dir>/dev_flash/sys/external/<module>.sprx (everything: all of dev_flash).
    Returns {module: path}."""
    written = {}
    with Pup(pup_path) as pup:
        ver = pup.version()
        if ver != TESTED_FIRMWARE:
            log('warning: firmware %s; the port is tested with %s (the modules will differ)'
                % (ver or 'of unknown version', TESTED_FIRMWARE))
        else:
            log('firmware %s' % ver)
        pkgs = pup.packages()
        if not pkgs:
            raise PupError('%s: no dev_flash packages in update_files.tar' % pup_path)
        want = {m + '.sprx': m for m in MODULES}
        for i, m in enumerate(pkgs, 1):
            log('[%d/%d] %s' % (i, len(pkgs), m.name))
            tar = pkg_tar(decrypt_pkg(pup.read(m), keys, m.name), m.name)
            try:
                with tarfile.open(fileobj=io.BytesIO(tar), mode='r:') as t:
                    for ti in t.getmembers():
                        p = safe_path(ti.name)
                        if p is None:
                            if not ti.isdir():
                                log('  skipped %r (outside dev_flash)' % ti.name)
                            continue
                        dst = os.path.join(out_dir, 'dev_flash', *p.split('/'))
                        if everything:
                            if ti.isdir():
                                os.makedirs(dst, exist_ok=True)
                            elif ti.isfile():
                                write_file(dst, t.extractfile(ti).read())
                            else:
                                log('  skipped %s (not a regular file)' % p)
                                continue
                        if (ti.isfile() and p.startswith(EXTERNAL)
                                and p[len(EXTERNAL):] in want):
                            if not everything:
                                write_file(dst, t.extractfile(ti).read())
                            written[want[p[len(EXTERNAL):]]] = dst
                            log('  dev_flash/%s' % p)
            except tarfile.TarError as e:
                raise PupError('%s: the decrypted tar does not read (%s): wrong key?' % (m.name, e))
            if not everything and len(written) == len(MODULES):
                break
    lost = [m for m in MODULES if m not in written]
    if lost:
        raise PupError('%s: no %s in its dev_flash' % (pup_path, ', '.join(m + '.sprx' for m in lost)))
    for m, p in written.items():
        with open(p, 'rb') as f:
            h = f.read(0x10)
        if h[:4] != b'SCE\0' or struct.unpack_from('>H', h, 0xA)[0] != 1:
            raise PupError('%s is not a SELF' % p)
    return written


def info(pup_path, keys=None, out=print):
    with Pup(pup_path) as pup:
        ver = pup.version()
        out('%s: PS3 system software %s (image version %d), %d entries'
            % (pup_path, ver or '?', pup.image_version, len(pup.entries)))
        for eid, (off, ln) in sorted(pup.entries.items()):
            out('  0x%03X %-18s %12d bytes at 0x%X' % (eid, PUP_ENTRIES.get(eid, '?'), ln, off))
        hdrs = pup.package_headers()
        out('update_files.tar: %d dev_flash packages' % len(hdrs))
        for m, rev in hdrs:
            out('  %-44s %10d bytes  key revision 0x%04X' % (m.name, m.size, rev))
        if ver != TESTED_FIRMWARE:
            out('warning: the port is tested with firmware %s' % TESTED_FIRMWARE)
        if keys is not None:
            missing, notes = missing_keys(pup, keys)
            for n in notes:
                out('note: ' + n)
            out('keys: ' + ('missing ' + '; '.join(missing) if missing else 'all present'))
            return 1 if missing else 0
    return 0


# --------------------------------------------------------------------------
# self-test: a synthetic PUP -> PKG -> SELF chain, made-up keys
# --------------------------------------------------------------------------
def _fake_pkg(sections, erk, riv, rev, seed):
    """An SCE PKG the way the firmware packages are laid out (no signature)."""
    import random
    rnd = random.Random(seed)
    rb = lambda n: bytes(rnd.getrandbits(8) for _ in range(n))
    mkey, miv = rb(16), rb(16)
    n = len(sections)
    meta_off = 0x40                                     # some filler before the metadata
    off = 0x20 + meta_off
    hdr_len = off + 0x40 + 0x20 + n * 0x30 + 2 * n * 0x10
    pos, secs, dkeys, payload = hdr_len, [], [], b''
    for i, (data, compress) in enumerate(sections):
        blob = zlib.compress(data) if compress else data
        k, v = rb(16), rb(16)
        dkeys += [k, v]
        enc = ds.aes_ctr(k, v, blob)
        secs.append(struct.pack('>QQIIIIIIII', pos, len(enc), 3, i, 0, 0, 3, 2 * i, 2 * i + 1,
                                2 if compress else 1))
        payload += enc
        pos += len(enc)
    mhdr = struct.pack('>QIIIIII', 0, 0, n, 2 * n, 0, 0, 0)
    hdrs = ds.aes_ctr(mkey, miv, mhdr + b''.join(secs) + b''.join(dkeys))
    meta_info = ds.AES.new(erk, ds.AES.MODE_CBC, riv).encrypt(mkey + bytes(16) + miv + bytes(16))
    sce = struct.pack('>4sIHHIQQ', b'SCE\0', 2, rev, SCE_TYPE_PKG, meta_off, hdr_len, len(payload))
    return sce + rb(meta_off) + meta_info + hdrs + payload


def _tar(files, links=()):
    """{path: bytes} (+ symlinks [(path, target)]) -> ustar bytes."""
    b = io.BytesIO()
    with tarfile.open(fileobj=b, mode='w', format=tarfile.USTAR_FORMAT) as t:
        for p, data in files.items():
            ti = tarfile.TarInfo(p)
            ti.size = len(data)
            t.addfile(ti, io.BytesIO(data))
        for p, target in links:
            ti = tarfile.TarInfo(p)
            ti.type, ti.linkname = tarfile.SYMTYPE, target
            t.addfile(ti)
    return b.getvalue()


def _fake_pup(entries):
    """{entry id: bytes} -> a PUP (hashes left zero)."""
    n = len(entries)
    hdr_len = 0x30 + n * 0x40 + 0x20
    pos, tbl, hashes, data = hdr_len, b'', b'', b''
    for eid, blob in sorted(entries.items()):
        tbl += struct.pack('>QQQQ', eid, pos, len(blob), 0)
        hashes += struct.pack('>Q', eid) + bytes(24)
        data += blob
        pos += len(blob)
    return struct.pack('>8sQQQQQ', PUP_MAGIC, 1, 68500, n, hdr_len, len(data)) + tbl + hashes + bytes(0x20) + data


def selftest():
    import tempfile
    eh, phdrs, code, data, shdrs, want_elf = ds.selftest_elf()
    self_erk, self_riv = bytes(range(0x80, 0xA0)), bytes(range(0xA0, 0xB0))     # made up
    pkg_erk, pkg_riv = bytes(range(0x20, 0x40)), bytes(range(0x50, 0x60))       # made up
    sprx = {m: ds._fake_self(eh, phdrs, [(code, i % 2 == 0), (data, True)], shdrs,
                             self_erk, self_riv, 0x1C, 4)
            for i, m in enumerate(MODULES)}
    self_block = '[appldr_493]\ntype=SELF\nrevision=001C\nself_type=APP\nerk=%s\nriv=%s\n' % (
        self_erk.hex(), self_riv.hex())
    key_files = {
        'scetool': self_block + '[pkg_493]\ntype=PKG\nrevision=0000\nversion=0004009300000000\n'
                                'erk=%s\nriv=%s\n' % (pkg_erk.hex(), pkg_riv.hex()),
        'new': self_block.replace('type=SELF', 'category=SELF').replace('self_type', 'program_type') +
               '[PKG]\ncategory=PKG\nerk=%s\nriv=%s\n' % (pkg_erk.hex(), pkg_riv.hex()),
        'rpcs3': 'SCEPKG_ERK=%s\nSCEPKG_RIV=%s\n' % (pkg_erk.hex(), pkg_riv.hex()) + self_block,
    }
    # an unrelated PKG key set first, so the lookup has to pick the right one
    wrong = '[pkg_other]\ntype=PKG\nrevision=0000\nerk=%s\nriv=%s\n' % ((b'\x11' * 32).hex(), (b'\x22' * 16).hex())

    tar0 = _tar({'dev_flash/vsh/etc/version.txt': b'release:04.9300:\n',
                 'dev_flash/sys/external/liblv2.sprx': sprx['liblv2'],
                 'dev_flash/sys/external/libfoo.sprx': b'not wanted'})
    tar1 = _tar({'dev_flash/sys/external/libsysmodule.sprx': sprx['libsysmodule'],
                 'dev_flash/sys/external/libsre.sprx': sprx['libsre'],
                 '../escape.txt': b'must not be written',
                 'dev_flash/../../escape2.txt': b'must not be written',
                 './dev_flash/vsh/module/x.sprx': b'dotted name'},
                links=[('dev_flash/sys/link.sprx', '/etc/passwd')])
    pkg0 = _fake_pkg([(b'info0' * 8, False), (b'info1' * 8, False), (tar0, False)], pkg_erk, pkg_riv, 0, 1)
    pkg1 = _fake_pkg([(b'info0' * 8, False), (b'info1' * 8, True), (tar1, True)], pkg_erk, pkg_riv, 0, 2)
    upd = _tar({'UPL.xml.pkg': b'junk',
                'dev_flash_000.tar.aa.2026_01_08_000000': pkg0,
                'dev_flash_001.tar.aa.2026_01_08_000000': pkg1,
                'dev_flash3_000.tar.aa.2026_01_08_000000': b'junk, never opened'})

    def pup_bytes(version):
        return _fake_pup({ENTRY_VERSION: version, 0x200: b'ps3swu junk', ENTRY_UPDATE_FILES: upd})

    quiet = lambda *_: None
    with tempfile.TemporaryDirectory(prefix='pup_selftest_') as tmp:
        pup = os.path.join(tmp, 'PS3UPDAT.PUP')
        with open(pup, 'wb') as f:
            f.write(pup_bytes(b'4.93\n'))

        # the clear structure, and what a key file lacks
        with Pup(pup) as p:
            assert p.version() == '4.93' and p.image_version == 68500
            assert [m.name[:13] for m in p.packages()] == ['dev_flash_000', 'dev_flash_001']
            assert [r for _m, r in p.package_headers()] == [0, 0]
            missing, _ = missing_keys(p, ds.Keys())
            assert len(missing) == 2 and 'type=PKG' in missing[0] and 'APP, key revision 0x001C' in missing[1], missing
            k = ds.Keys()
            k.parse(key_files['scetool'])
            assert missing_keys(p, k) == ([], [])
        assert norm_version('04.9300') == '4.93' and norm_version('4.9') == '4.90'

        # each key-file flavour: extract, then decrypt the modules to the expected ELF
        for flavour, text in key_files.items():
            keys = ds.Keys()
            keys.parse(wrong + text if flavour != 'rpcs3' else text)
            out = os.path.join(tmp, 'out_' + flavour)
            got = extract(pup, out, keys, log=quiet)
            assert sorted(got) == sorted(MODULES)
            for m in MODULES:
                p = os.path.join(out, 'dev_flash', 'sys', 'external', m + '.sprx')
                with open(p, 'rb') as f:
                    assert f.read() == sprx[m], '%s: %s differs' % (flavour, m)
                elf = ds.Self(sprx[m]).to_elf(keys)
                assert elf == want_elf and ds.check_elf(elf, prx=True) is None, '%s: %s.prx' % (flavour, m)
            assert not os.path.exists(os.path.join(out, 'dev_flash', 'sys', 'external', 'libfoo.sprx'))

        # --all: the whole dev_flash, nothing outside it, no links
        keys = ds.Keys()
        keys.parse(key_files['scetool'])
        out = os.path.join(tmp, 'out_all')
        extract(pup, out, keys, everything=True, log=quiet)
        found = sorted(os.path.relpath(os.path.join(r, f), out) for r, _d, fs in os.walk(out) for f in fs)
        assert found == sorted(['dev_flash/vsh/etc/version.txt', 'dev_flash/sys/external/libfoo.sprx',
                                'dev_flash/vsh/module/x.sprx'] +
                               ['dev_flash/sys/external/%s.sprx' % m for m in MODULES]), found
        assert not any(os.path.exists(os.path.join(tmp, f)) for f in ('escape.txt', 'escape2.txt'))
        for name, want in (('dev_flash/sys/a', 'sys/a'), ('/dev_flash/sys/a', 'sys/a'), ('./sys/a', 'sys/a'),
                           ('../a', None), ('dev_flash/../../a', None), ('dev_flash', None), ('./', None)):
            assert safe_path(name) == want, (name, safe_path(name))

        # a wrong PKG key, and none at all
        for text, msg in ((wrong + self_block, 'did not decrypt'), (self_block, 'has no the PKG key set')):
            k = ds.Keys()
            k.parse(text)
            try:
                extract(pup, os.path.join(tmp, 'out_bad'), k, log=quiet)
                raise AssertionError('expected a key error')
            except PupError as e:
                assert msg in str(e), e

        # another firmware version only warns
        with open(pup, 'wb') as f:
            f.write(pup_bytes(b'4.92\n'))
        lines = []
        extract(pup, os.path.join(tmp, 'out_492'), keys, log=lines.append)
        assert any('warning: firmware 4.92' in l for l in lines), lines
        with Pup(pup) as p:
            assert missing_keys(p, keys)[1], 'an unknown version must leave a note'

        # not a PUP, and a truncated one
        for blob in (b'\x7fPKG' + bytes(60), pup_bytes(b'4.93\n')[:-4096]):
            with open(pup, 'wb') as f:
                f.write(blob)
            try:
                Pup(pup).close()
                raise AssertionError('expected a PupError')
            except PupError:
                pass
    print('selftest ok')


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('pup', nargs='?', help='PS3UPDAT.PUP')
    ap.add_argument('-o', '--output', help='folder to write dev_flash/sys/external/*.sprx into')
    ap.add_argument('--keys', default=os.environ.get('PS3_KEYS'), help='key file (default: $PS3_KEYS)')
    ap.add_argument('--all', action='store_true', help='extract all of dev_flash, not just the three modules')
    ap.add_argument('--info', action='store_true', help='describe the PUP (with --keys: which keys are missing)')
    ap.add_argument('--selftest', action='store_true', help='synthetic round trip with made-up keys')
    a = ap.parse_args(argv)
    if a.selftest:
        try:
            selftest()
        except (PupError, ds.SelfError) as e:
            print('error: %s' % e, file=sys.stderr)
            return 1
        return 0
    if not a.pup:
        ap.error('a PS3UPDAT.PUP is required')
    log = lambda s: print(s, flush=True)
    try:
        if a.info:
            return info(a.pup, ds.Keys.load(a.keys) if a.keys else None, out=log)
        if not a.output:
            ap.error('-o DIR is required (or --info)')
        if not a.keys:
            ap.error('no key file: pass --keys or set PS3_KEYS')
        got = extract(a.pup, a.output, ds.Keys.load(a.keys), everything=a.all, log=log)
        for m in MODULES:
            log('%s  %d bytes' % (got[m], os.path.getsize(got[m])))
    except (PupError, ds.SelfError, OSError) as e:
        print('error: %s' % e, file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
