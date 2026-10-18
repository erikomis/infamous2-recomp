#!/usr/bin/env python3
"""Fail if a launcher bundle carries anything from the game.

    python3 launcher/scripts/check_bundle.py <path>...

Each <path> is a folder (the staging folder, a .app) or a finished bundle
(.dmg, .zip, .deb, .AppImage, .msi, NSIS .exe), which is unpacked into a
temporary folder first. Every file is checked by name and by its first
bytes; a hit on any of these fails the check:

  - licences and keys: *.rap, *.rif, act.dat, idps, *.edat/*.sdat
  - game code and data: EBOOT*, *.self/*.sprx/*.prx, PARAM.SFO/*.sfo, *.pkg,
    *.psarc, *.pup, NPUA80638*, folders dev_hdd0/dev_hdd1/dev_flash/
    PS3_GAME/USRDIR
  - anything derived from them: *.elf, the lifted output folders
    (recompiled/, lle/, lle_spu/, jobmods/)
  - by content: SCE (SELF/SPRX/PUP), PKG, PSF, PSAR, NPD headers and
    big-endian PowerPC or SPU ELF files
  - inside the bundled port/ folder: any file over 4 MB, except a text
    patch (*.patch starting with "diff --git"), allowed up to 16 MB: the
    toolkit patch carries the vendored Dear ImGui and is ~4 MB

It also checks that the port sources the launcher needs are there
(port/infamous2-lle/tools/rebuild.py), unless --no-expect-port.
"""
from __future__ import annotations

import argparse
import fnmatch
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile

BAD_NAMES = ('*.rap', '*.rif', '*.edat', '*.sdat', 'act.dat', 'idps*', 'eboot*', '*.self', '*.sprx', '*.prx',
             'param.sfo', '*.sfo', '*.pkg', '*.psarc', '*.pup', 'ps3updat*', '*.elf', 'npua80638*')
BAD_DIRS = ('dev_hdd0', 'dev_hdd1', 'dev_flash', 'ps3_game', 'usrdir', 'recompiled', 'lle', 'lle_spu',
            'jobmods')
MAGIC = ((b'SCE\0', 'SCE header (SELF/SPRX)'), (b'SCEUF', 'PUP firmware'), (b'\x7fPKG', 'PS3 PKG'),
         (b'\0PSF', 'PARAM.SFO'), (b'PSAR', 'psarc archive'), (b'NPD\0', 'NPD (EDAT/SDAT)'))
PORT_MAX_FILE = 4 * 1024 * 1024
PORT_MAX_PATCH = 16 * 1024 * 1024
EXPECT = ('port', 'infamous2-lle', 'tools', 'rebuild.py')


def ps3_elf(head):
    # big-endian ELF whose e_machine is PPC64 (21) or SPU (23)
    return (len(head) >= 20 and head[:4] == b'\x7fELF' and head[5] == 2
            and int.from_bytes(head[18:20], 'big') in (21, 23))


def scan(root, label):
    problems = []
    found_port = False
    for d, dirs, files in os.walk(root):
        rel_d = os.path.relpath(d, root)
        parts = [] if rel_d == '.' else rel_d.split(os.sep)
        for x in dirs:
            if x.lower() in BAD_DIRS:
                problems.append('%s: folder %s' % (label, os.path.join(rel_d, x)))
        in_port = 'port' in parts
        for f in files:
            p = os.path.join(d, f)
            rel = os.path.normpath(os.path.join(rel_d, f))
            low = f.lower()
            if any(fnmatch.fnmatch(low, pat) for pat in BAD_NAMES):
                problems.append('%s: %s (name)' % (label, rel))
                continue
            if os.path.islink(p):
                continue
            try:
                size = os.path.getsize(p)
                with open(p, 'rb') as fh:
                    head = fh.read(64)
            except OSError as e:
                problems.append('%s: %s unreadable (%s)' % (label, rel, e))
                continue
            for magic, what in MAGIC:
                if head.startswith(magic):
                    problems.append('%s: %s (%s)' % (label, rel, what))
            if ps3_elf(head):
                problems.append('%s: %s (PowerPC/SPU ELF)' % (label, rel))
            is_patch = f.endswith('.patch') and head.startswith(b'diff --git')
            if in_port and size > (PORT_MAX_PATCH if is_patch else PORT_MAX_FILE):
                problems.append('%s: %s is %d MB, too big for a port source' % (label, rel, size >> 20))
            if tuple(parts[-3:]) + (f,) == EXPECT:
                found_port = True
    return problems, found_port


def run(cmd, **kw):
    r = subprocess.run(cmd, capture_output=True, text=True, **kw)
    if r.returncode != 0:
        raise RuntimeError('%s: %s' % (' '.join(cmd), (r.stderr or r.stdout).strip()[-400:]))
    return r


def unpack(path, tmp):
    """Unpack a bundle into tmp; returns (root to scan, cleanup callable)."""
    low = path.lower()
    if low.endswith('.zip'):
        with zipfile.ZipFile(path) as z:
            z.extractall(tmp)
        return tmp, None
    if low.endswith('.dmg'):
        mnt = os.path.join(tmp, 'mnt')
        os.makedirs(mnt)
        run(['hdiutil', 'attach', '-nobrowse', '-readonly', '-noautoopen', '-mountpoint', mnt, path])
        return mnt, lambda: run(['hdiutil', 'detach', mnt, '-force'])
    if low.endswith('.deb'):
        run(['dpkg-deb', '-x', path, tmp])
        return tmp, None
    if low.endswith('.appimage'):
        exe = os.path.join(tmp, 'app.AppImage')
        shutil.copy2(path, exe)
        os.chmod(exe, 0o755)
        run([exe, '--appimage-extract'], cwd=tmp)
        return os.path.join(tmp, 'squashfs-root'), None
    if low.endswith('.msi'):
        # an administrative install lays the files out under their real names
        out = os.path.join(tmp, 'msi')
        run(['msiexec', '/a', os.path.abspath(path), '/qn', 'TARGETDIR=' + out])
        return out, None
    if low.endswith('.exe'):
        z = shutil.which('7z') or r'C:\Program Files\7-Zip\7z.exe'
        run([z, 'x', '-y', '-o' + tmp, path])         # 7-Zip reads NSIS installers
        return tmp, None
    raise RuntimeError('%s: unknown bundle type' % path)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('paths', nargs='+')
    ap.add_argument('--no-expect-port', dest='expect_port', action='store_false',
                    help='do not require port/infamous2-lle/tools/rebuild.py')
    a = ap.parse_args(argv)
    failed = False
    for path in a.paths:
        label = os.path.basename(os.path.normpath(path))
        tmp = tempfile.mkdtemp(prefix='check_bundle_')
        cleanup = None
        try:
            if os.path.isdir(path):
                root = path
                if os.path.basename(os.path.normpath(path)) == 'port':
                    # the staging folder itself: scan it as the "port" folder it becomes
                    root = os.path.dirname(os.path.normpath(path)) or '.'
            else:
                root, cleanup = unpack(path, tmp)
            problems, found = scan(root, label)
        except RuntimeError as e:
            problems, found = ['%s: %s' % (label, e)], True
        finally:
            if cleanup:
                try:
                    cleanup()
                except RuntimeError as e:
                    print('warning: %s' % e, file=sys.stderr)
            shutil.rmtree(tmp, ignore_errors=True)
        if a.expect_port and not found:
            problems.append('%s: port/infamous2-lle/tools/rebuild.py not in the bundle' % label)
        for p in problems:
            print('FAIL ' + p)
        if problems:
            failed = True
        else:
            print('ok   %s' % label)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
