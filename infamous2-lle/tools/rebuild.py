#!/usr/bin/env python3
"""rebuild.py -- the whole rebuild, from the player's own copy to a binary.

    python3 infamous2-lle/tools/rebuild.py --game <copy> --rap <file.rap> [--out DIR]

<copy> is the game as RPCS3 installed it (the NPUA80638 folder, or RPCS3's
data folder that holds dev_hdd0/game/NPUA80638) or one or more PKGs (base
game first, then updates: pass --game once per file). The .rap is the
player's licence. Every step of infamous2-lle/README.md "Rebuilding" runs in
order: toolkit, Python venv, game import, decryption, the firmware/PPU/SPU/job
lifts and the CMake build.

Decryption (the EBOOT and the three firmware modules the port translates) runs
either through RPCS3 (`rpcs3 --decrypt`, the licence installed in RPCS3's
exdata) or, with --keys KEYFILE (or $PS3_KEYS), through tools/decrypt_self.py
with no RPCS3 at all; --decrypt-with picks one (default: python when a key
file is given, else rpcs3). Both write the same bytes. The firmware .sprx are
read from --firmware (a dev_flash folder, its sys/external, or any folder
holding liblv2/libsysmodule/libsre.sprx; default: RPCS3's dev_flash). With
--firmware PS3UPDAT.PUP (Sony's system software update) and a key file
holding the PKG key set, an extra step ("pup") takes the three modules out
of it with tools/pup_extract.py into <out>/infamous2/dev_flash/sys/external.

--out (default: the app data folder, ~/Library/Application Support/
infamous2-recomp/install on macOS) mirrors this repository's layout, so the
port's own scripts work there unchanged:

    <out>/ps3recomp-fork/     the toolkit (tools/setup_toolkit.sh)
    <out>/.venv/              Python for the translators
    <out>/infamous2/{game,fw,vfs}/   decrypted EBOOT/firmware, game data clone
    <out>/infamous2/dev_flash/       the modules taken from a PUP (--firmware X.PUP)
    <out>/infamous2-lle/      port sources + lifted code + build/
    <out>/install.json        what the launcher reads (paths, version)

Nothing here embeds a key, a licence or a game file: they all come from the
player's machine at run time (the key file, and PS3_RAP_KEY for the DLC step
or the python decryption, are never printed).

Machine-readable output, one per line, for the launcher (everything else a
tool prints is passed through as "LOG <line>"):

    PLAN <i>/<n> <step> <state> <title>      state: run | up-to-date | skip
    CHECK <ok|warn|fail> <name> <message>
    PROGRESS <step> <i>/<n> <message>
    SUBPROGRESS <step> <done>/<total>         inside a step (the compile)
    STEP-DONE <step> <i>/<n> <seconds>
    STEP-SKIP <step> <i>/<n> <reason>
    WARN <step> <message>
    ERROR <step> <message>
    DONE <path of install.json, or - when it was not written>

Exit status: 0 done, 1 a step failed, 2 a prerequisite is missing.

--check-json checks only the machine (host, RPCS3 and its firmware, the build
tools, disk space; no game or licence needed) and prints one JSON object
{"supported", "ok", "homebrew", "checks": [{"level", "name", "message"}], ...}
instead of CHECK lines; the launcher's first-run screen reads it.
Each step remembers what it was built from (<out>/.rebuild/state.json); a
rerun skips the steps whose inputs did not change. --force reruns all,
--from STEP reruns from STEP on, --only STEP[,STEP] runs just those.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import struct
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
PORT_SRC = os.path.dirname(HERE)                 # infamous2-lle/
REPO = os.path.dirname(PORT_SRC)
SETUP_TOOLKIT = os.path.join(REPO, 'tools', 'setup_toolkit.sh')
PATCH = os.path.join(PORT_SRC, 'patches', 'ps3recomp-fork-770660a-with-upstream-prs.patch')

TITLE_ID = 'NPUA80638'
TESTED_APP_VER = '01.03'
# sha256 of the decrypted EBOOT.elf (NPUA80638, APP_VER 01.03) the port is
# built and tested against. A hash, not data: a different one only warns.
TESTED_EBOOT_ELF_SHA256 = '3bfe08df4e2ed1201a83be3d38f13a66e900153dd4071f25a35c3c95a1717298'
FIRMWARE = ('liblv2', 'libsysmodule', 'libsre')
# sha256 of the decrypted firmware modules (PS3 firmware 4.93) the port is
# tested with. Hashes, not data: a different one only warns.
TESTED_FW_PRX_SHA256 = {
    'liblv2': 'aac69540830b4a6673b93331bf52266f360551a2732adcbb3c7a808daddf09c5',
    'libsysmodule': '6e4c69756c84a0965206e64fee264c14d0e5ae09e35ec37f6713f519283c3be0',
    'libsre': '9e7ee6edc99b1d372686cf927b883b18f19ed4d32d1faf35e214cca4ae0737b6',
}
DECRYPT_SELF = os.path.join(HERE, 'decrypt_self.py')
PUP_EXTRACT = os.path.join(HERE, 'pup_extract.py')

# Port files copied into <out>/infamous2-lle (the build's sources and tools).
PORT_FILES = ('CMakeLists.txt', 'main.cpp', 'stubs.cpp', 'gpu_light.c', 'gpu_ssao.c',
              'ssao_gpu.c', 'ssao_gpu.h', 'edge_watch.c', 'run.sh', 'config.toml',
              'macos_host.m',      # App Nap off, thread QoS (macOS)
              'Info.plist.in',     # the macOS app bundle's Info.plist (Game Mode)
              'window_icon.png')   # the Dock icon run.sh points the game at
# Data files of the tools, copied with their *.py / *.sh into <out>/infamous2-lle/tools.
TOOL_DATA = ('log_triage_known.txt',)   # log_triage.py's known-harmless lines


def is_tool(name):
    return name.endswith(('.py', '.sh')) or name in TOOL_DATA


def game_executable(port):
    """The built game: on macOS the binary inside build/Infamous2Recomp.app
    (CMakeLists.txt; the bundle is what makes Game Mode turn on), elsewhere --
    or in a build from before the bundle -- build/Infamous2Recomp."""
    build = os.path.join(port, 'build')
    app = os.path.join(build, 'Infamous2Recomp.app', 'Contents', 'MacOS', 'Infamous2Recomp')
    if sys.platform == 'darwin' and os.path.isfile(app):
        return app
    return os.path.join(build, 'Infamous2Recomp' + ('.exe' if os.name == 'nt' else ''))

# Extra lifted entry points beyond the README recipe, found at run time and
# present in the build the port is tested with (an interrupted SPU job that
# resumes at an address with no lifted entry runs on the interpreter there).
SPU_IMAGE_EXTRA = ('0x9C84', '0x9DB8')                       # spu_0000_at_007FA100
JOBMOD_EXTRA = (('jobmod_ab654d30fd90_efe70', ('0x15280', '0x155EC')),
                ('jobmod_cbb4b5c85d75_e59f8', ('0x6C44', '0x6C64')))

GB = 1024 ** 3
WORK_BYTES = int(1.5 * GB)      # toolkit, venv, lifts, objects, executable
GAME_HEADROOM = 3 * GB          # below ~3 GB free the game refuses to boot

STEPS = [
    ('toolkit',  'Toolkit (ps3recomp fork + patches)'),
    ('venv',     'Python environment for the translators'),
    ('game',     'Import the game copy'),
    ('pup',      'Extract the firmware from PS3UPDAT.PUP'),    # only with --firmware X.PUP
    ('decrypt',  'Decrypt EBOOT and firmware'),
    ('port',     'Port sources'),
    ('firmware', 'Translate the firmware (liblv2, libsysmodule, libsre)'),
    ('ppu',      'Translate the PPU code'),
    ('spu',      'Translate the SPU programs'),
    ('jobs',     'Translate the SPU job modules'),
    ('dlc',      'DLC licence key (optional)'),
    ('compile',  'Compile'),
    ('finish',   'Write install.json'),
]
STEP_IDS = [s for s, _ in STEPS]


class StepError(Exception):
    pass


# --------------------------------------------------------------------------
# output
# --------------------------------------------------------------------------
def emit(*parts):
    sys.stdout.write(' '.join(str(p) for p in parts) + '\n')
    sys.stdout.flush()


def log(msg):
    for line in str(msg).splitlines() or ['']:
        emit('LOG', line)


# --------------------------------------------------------------------------
# small helpers
# --------------------------------------------------------------------------
def sha256_file(path, limit=None):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        left = limit
        while True:
            n = 1 << 20 if left is None else min(1 << 20, left)
            if n == 0:
                break
            b = f.read(n)
            if not b:
                break
            h.update(b)
            if left is not None:
                left -= len(b)
    return h.hexdigest()


def sha256_text(*parts):
    h = hashlib.sha256()
    for p in parts:
        h.update(str(p).encode('utf-8', 'replace'))
        h.update(b'\0')
    return h.hexdigest()


def file_sig(path):
    """Cheap identity of a file we do not want to hash in full."""
    st = os.stat(path)
    return '%s:%d:%d' % (os.path.abspath(path), st.st_size, int(st.st_mtime))


def read_sfo(path):
    """PARAM.SFO -> {key: str|int}."""
    with open(path, 'rb') as f:
        s = f.read()
    if s[:4] != b'\0PSF':
        raise ValueError('%s: not a PARAM.SFO' % path)
    _, kt, dt, n = struct.unpack_from('<IIII', s, 4)
    out = {}
    for k in range(n):
        ko, fmt, ln, _mx, do = struct.unpack_from('<HHIII', s, 20 + 16 * k)
        key = s[kt + ko:s.index(b'\0', kt + ko)].decode('ascii', 'replace')
        val = s[dt + do:dt + do + ln]
        if fmt == 0x404:
            out[key] = struct.unpack('<I', val[:4])[0]
        else:
            out[key] = val.rstrip(b'\0').decode('utf-8', 'replace')
    return out


def npd_content_id(path):
    """Content id from the NPD header of an NPDRM SELF (EBOOT.BIN) or EDAT."""
    with open(path, 'rb') as f:
        d = f.read(0x2000)
    i = d.find(b'NPD\0')
    if i < 0:
        return None
    return d[i + 0x10:i + 0x40].split(b'\0', 1)[0].decode('ascii', 'replace') or None


def pkg_content_id(path):
    with open(path, 'rb') as f:
        h = f.read(0x60)
    if h[:4] != b'\x7fPKG':
        return None
    return h[0x30:0x60].split(b'\0', 1)[0].decode('ascii', 'replace')


def existing_ancestor(path):
    p = os.path.abspath(path)
    while not os.path.exists(p):
        q = os.path.dirname(p)
        if q == p:
            break
        p = q
    return p


def free_bytes(path):
    return shutil.disk_usage(existing_ancestor(path)).free


def tree_size(path):
    total = 0
    for root, _dirs, files in os.walk(path):
        for f in files:
            try:
                total += os.lstat(os.path.join(root, f)).st_size
            except OSError:
                pass
    return total


def human(n):
    return '%.1f GB' % (n / GB) if n >= GB / 10 else '%d MB' % (n // (1024 * 1024))


def default_out():
    if sys.platform == 'darwin':
        base = os.path.expanduser('~/Library/Application Support/infamous2-recomp')
    elif os.name == 'nt':
        base = os.path.join(os.environ.get('LOCALAPPDATA', os.path.expanduser('~')), 'infamous2-recomp')
    else:
        base = os.path.join(os.environ.get('XDG_DATA_HOME', os.path.expanduser('~/.local/share')),
                            'infamous2-recomp')
    return os.path.join(base, 'install')


def default_rpcs3_dir():
    if sys.platform == 'darwin':
        return os.path.expanduser('~/Library/Application Support/rpcs3')
    if os.name == 'nt':
        return ''           # portable install: the folder rpcs3.exe sits in (pass --rpcs3-dir)
    return os.path.join(os.environ.get('XDG_CONFIG_HOME', os.path.expanduser('~/.config')), 'rpcs3')


def find_rpcs3(explicit):
    cands = [explicit, os.environ.get('RPCS3')]
    if sys.platform == 'darwin':
        cands += [os.path.expanduser('~/Applications/RPCS3.app/Contents/MacOS/rpcs3'),
                  '/Applications/RPCS3.app/Contents/MacOS/rpcs3']
    cands.append(shutil.which('rpcs3'))
    for c in cands:
        if c and os.path.isfile(c) and os.access(c, os.X_OK):
            return c
    return None


_CLT = []


def macos_clt():
    """macOS: the Command Line Tools folder, or '' when they are missing.
    Until they are installed /usr/bin/{clang,git,python3} are stubs that pop
    up Apple's installer, so nothing here may run them."""
    if sys.platform != 'darwin':
        return 'n/a'
    if not _CLT:
        try:
            r = subprocess.run(['xcode-select', '-p'], capture_output=True, text=True, timeout=20)
            _CLT.append(r.stdout.strip() if r.returncode == 0 and os.path.isdir(r.stdout.strip()) else '')
        except Exception:
            _CLT.append('')
    return _CLT[0]


def find_python(explicit):
    cands = [explicit] if explicit else ['python3.12', 'python3.13', 'python3.11', 'python3.10', 'python3']
    for c in cands:
        p = c if os.path.sep in c else shutil.which(c)
        if not p or (not explicit and p.startswith('/usr/bin/') and not macos_clt()):
            continue
        try:
            v = subprocess.run([p, '-c', 'import sys; print("%d.%d" % sys.version_info[:2])'],
                               capture_output=True, text=True, timeout=20).stdout.strip()
            major, minor = (int(x) for x in v.split('.'))
        except Exception:
            continue
        if (major, minor) >= (3, 10):
            return p, v
    return None, None


def tool_version(cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=20)
        return (r.stdout or r.stderr).strip().splitlines()[0]
    except Exception:
        return None


def port_build_info():
    """BUILD_INFO.json, written next to the sources when the launcher bundles
    them (launcher/scripts/stage_port.py): there is no git checkout there."""
    try:
        with open(os.path.join(REPO, 'BUILD_INFO.json')) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def resolve_fw_dir(path):
    """The folder holding the firmware .sprx, from a dev_flash, its parent or sys/external."""
    if not path:
        return None
    for c in (path,
              os.path.join(path, 'sys', 'external'),
              os.path.join(path, 'dev_flash', 'sys', 'external')):
        if all(os.path.isfile(os.path.join(c, m + '.sprx')) for m in FIRMWARE):
            return os.path.abspath(c)
    return None


def self_header(path):
    """(program type, key revision) of a SELF, read from its clear header; None if not a SELF."""
    with open(path, 'rb') as f:
        h = f.read(0x200)
    if len(h) < 0x70 or h[:4] != b'SCE\0':
        return None
    rev = struct.unpack_from('>H', h, 8)[0]
    app = struct.unpack_from('>Q', h, 0x28)[0]
    if app + 0x10 > len(h):
        return None
    ptype = struct.unpack_from('>I', h, app + 0xC)[0]
    return {4: 'APP', 8: 'NPDRM'}.get(ptype, str(ptype)), rev


def load_tool(name):
    """One of the tools next to this script, imported (decrypt_self, pup_extract: parsing
    needs no pycryptodome; decrypting runs later in the venv, which has it)."""
    sys.path.insert(0, HERE)
    sys.dont_write_bytecode = True      # leave no __pycache__ in the checkout
    try:
        return __import__(name)
    finally:
        sys.path.pop(0)


def load_keys(path):
    """The key file through decrypt_self.Keys."""
    ds = load_tool('decrypt_self')
    return ds, ds.Keys.load(path)


def resolve_game_dir(path):
    """The NPUA80638 folder from whatever the player pointed at."""
    for c in (path,
              os.path.join(path, TITLE_ID),
              os.path.join(path, 'game', TITLE_ID),
              os.path.join(path, 'dev_hdd0', 'game', TITLE_ID)):
        if os.path.isfile(os.path.join(c, 'PARAM.SFO')):
            return os.path.abspath(c)
    return None


# --------------------------------------------------------------------------
# the run
# --------------------------------------------------------------------------
class Rebuild:
    def __init__(self, a):
        self.a = a
        self.out = os.path.abspath(os.path.expanduser(a.out))
        self.toolkit = os.path.abspath(a.toolkit) if a.toolkit else os.path.join(self.out, 'ps3recomp-fork')
        self.venv = os.path.abspath(a.venv) if a.venv else os.path.join(self.out, '.venv')
        self.py = os.path.join(self.venv, 'Scripts' if os.name == 'nt' else 'bin',
                               'python.exe' if os.name == 'nt' else 'python')
        self.legacy = os.path.join(self.out, 'infamous2')         # game/, fw/, vfs/
        self.game_dir = os.path.join(self.legacy, 'game')
        self.fw_dir = os.path.join(self.legacy, 'fw')
        self.vfs = os.path.join(self.legacy, 'vfs')
        self.vfs_title = os.path.join(self.vfs, 'dev_hdd0', 'game', TITLE_ID)
        self.port = os.path.join(self.out, 'infamous2-lle')
        self.state_path = os.path.join(self.out, '.rebuild', 'state.json')
        self.elf = os.path.join(self.game_dir, 'EBOOT.elf')
        self.rpcs3_dir = os.path.abspath(os.path.expanduser(a.rpcs3_dir)) if a.rpcs3_dir else default_rpcs3_dir()
        self.rpcs3 = find_rpcs3(a.rpcs3)
        self.keys_file = os.path.abspath(os.path.expanduser(a.keys)) if a.keys else None
        self.decrypt_with = a.decrypt_with
        if self.decrypt_with == 'auto':
            self.decrypt_with = 'python' if self.keys_file else 'rpcs3'
        fw = a.firmware or (os.path.join(self.rpcs3_dir, 'dev_flash') if self.rpcs3_dir else None)
        self.fw_given = os.path.abspath(os.path.expanduser(fw)) if fw else None
        # --firmware PS3UPDAT.PUP: the "pup" step extracts the modules into
        # <out>/infamous2/dev_flash, and the rest reads them from there
        self.fw_pup = self.fw_given if self.fw_given and os.path.isfile(self.fw_given) else None
        self.pup_root = os.path.join(self.legacy, 'dev_flash')
        self.pup_version = None
        self.fw_src = resolve_fw_dir(self.pup_root if self.fw_pup else self.fw_given)
        self.steps = [s for s in STEPS if s[0] != 'pup' or self.fw_pup]
        self.jobs = a.jobs or os.cpu_count() or 4
        self.sources = []          # [('dir', path)] or [('pkg', path), ...]
        self.sfo = {}
        self.content_id = None
        self.state = {}
        self.fails = 0
        self.checks = []           # (level, name, message), for --check-json
        self.quiet = False
        self.keys = {}

    # ---- checks ---------------------------------------------------------
    def check(self, level, name, msg):
        self.checks.append((level, name, msg))
        if not self.quiet:
            emit('CHECK', level, name, msg)
        if level == 'fail':
            self.fails += 1

    def check_host(self):
        mach = platform.machine()
        if sys.platform == 'darwin' and mach == 'arm64':
            self.check('ok', 'host', 'macOS %s arm64' % platform.mac_ver()[0])
        elif sys.platform == 'darwin':
            self.check('warn', 'host', 'macOS %s %s: only macOS on Apple Silicon is tested' % (platform.mac_ver()[0], mach))
        else:
            # The game's renderer and window are Metal/Cocoa only so far.
            # INFAMOUS2_ANY_HOST=1 lets someone working on another port try anyway.
            self.check('warn' if os.environ.get('INFAMOUS2_ANY_HOST') else 'fail', 'host',
                       '%s %s: the port builds and runs only on macOS for now' % (sys.platform, mach))

    def check_all(self):
        a = self.a
        self.check_host()

        # the game copy
        if not a.game:
            self.check('fail', 'game', 'no game copy given (--game)')
        for g in a.game or []:
            g = os.path.abspath(os.path.expanduser(g))
            if os.path.isfile(g) and g.lower().endswith('.pkg'):
                cid = pkg_content_id(g)
                if not cid:
                    self.check('fail', 'game', '%s: not a PS3 PKG' % g)
                elif TITLE_ID not in cid:
                    self.check('fail', 'game', '%s: PKG for %s, not %s' % (g, cid, TITLE_ID))
                else:
                    self.sources.append(('pkg', g))
                    self.check('ok', 'game', 'PKG %s (%s, %s)' % (os.path.basename(g), cid, human(os.path.getsize(g))))
            elif os.path.isdir(g):
                d = resolve_game_dir(g)
                if not d:
                    self.check('fail', 'game', '%s: no PARAM.SFO (expected RPCS3\'s dev_hdd0/game/%s)' % (g, TITLE_ID))
                    continue
                try:
                    sfo = read_sfo(os.path.join(d, 'PARAM.SFO'))
                except Exception as e:
                    self.check('fail', 'game', str(e)); continue
                if sfo.get('TITLE_ID') != TITLE_ID:
                    self.check('fail', 'game', '%s is %s, not %s' % (d, sfo.get('TITLE_ID'), TITLE_ID)); continue
                if not os.path.isfile(os.path.join(d, 'USRDIR', 'EBOOT.BIN')):
                    self.check('fail', 'game', '%s: no USRDIR/EBOOT.BIN' % d); continue
                self.sfo = sfo
                self.sources.append(('dir', d))
                ver = sfo.get('APP_VER', '?')
                lvl = 'ok' if ver == TESTED_APP_VER else 'warn'
                self.check(lvl, 'game', '%s "%s" APP_VER %s%s at %s' % (
                    TITLE_ID, sfo.get('TITLE', ''), ver,
                    '' if lvl == 'ok' else ' (tested: %s)' % TESTED_APP_VER, d))
                eb = os.path.join(d, 'USRDIR', 'EBOOT.BIN')
                self.content_id = npd_content_id(eb)
            else:
                self.check('fail', 'game', '%s: neither a game folder nor a .pkg' % g)
        kinds = {k for k, _ in self.sources}
        if len(kinds) > 1 or (kinds == {'dir'} and len(self.sources) > 1):
            self.check('fail', 'game', 'give one game folder, or one or more PKGs, not a mix')

        # the licence
        rap = os.path.abspath(os.path.expanduser(a.rap)) if a.rap else None
        self.rap = rap
        if not rap:
            self.check('fail', 'rap', 'no licence given (--rap file.rap)')
        elif not os.path.isfile(rap):
            self.check('fail', 'rap', '%s: not found' % rap)
        elif os.path.getsize(rap) != 16:
            self.check('fail', 'rap', '%s: a .rap is 16 bytes, this one is %d' % (rap, os.path.getsize(rap)))
        else:
            name = os.path.basename(rap)
            if TITLE_ID not in name:
                self.check('warn', 'rap', '%s: the name does not mention %s' % (name, TITLE_ID))
            else:
                self.check('ok', 'rap', name)
            if self.content_id is None and TITLE_ID in name and name.lower().endswith('.rap'):
                self.content_id = name[:-4]

        self.check_tools()
        self.check_space()
        if os.environ.get('PS3_RAP_KEY'):
            self.check('ok', 'dlc', 'PS3_RAP_KEY set: the DLC licence key will be derived')
        else:
            self.check('warn', 'dlc', 'PS3_RAP_KEY not set: the optional DLC step will be skipped')

    def check_tools(self):
        """RPCS3, its firmware and the build tools: everything that does not
        depend on the game copy or the licence (all --check-json runs)."""
        a = self.a
        # decryption: RPCS3, or decrypt_self.py with the player's key file
        if self.decrypt_with == 'rpcs3':
            if self.rpcs3:
                self.check('ok', 'rpcs3', self.rpcs3)
            else:
                self.check('fail', 'rpcs3', 'RPCS3 not found (it decrypts the EBOOT and the firmware); '
                           'install it or pass --rpcs3, or decrypt without it: --keys KEYFILE')
            if self.fw_pup:
                self.check_keys(pkg_only=True)      # the PUP's packages still need the key file
        else:
            self.check_keys()
        if not self.fw_given:
            self.check('fail', 'firmware', 'pass --firmware (a dev_flash folder or PS3UPDAT.PUP) or --rpcs3-dir')
        elif self.fw_pup:
            self.check_pup()
        elif not self.fw_src:
            self.check('fail', 'firmware', 'PS3 firmware not found: no %s in %s (or its sys/external)'
                       % ('/'.join(m + '.sprx' for m in FIRMWARE), self.fw_given))
        else:
            self.check('ok', 'firmware', self.fw_src)

        # build tools
        py, pv = find_python(a.python)
        self.base_python = py
        if self.a.venv and os.path.isfile(self.py):
            self.check('ok', 'python', 'existing venv %s' % self.venv)
        elif py:
            self.check('ok', 'python', '%s (%s)' % (py, pv))
        else:
            self.check('fail', 'python', 'Python 3.10+ not found (macOS: brew install python@3.12)')
        cm = shutil.which('cmake')
        v = tool_version([cm, '--version']) if cm else None
        m = re.search(r'(\d+)\.(\d+)', v or '')
        if not cm:
            self.check('fail', 'cmake', 'CMake not found (brew install cmake)')
        elif m and (int(m.group(1)), int(m.group(2))) < (3, 20):
            self.check('fail', 'cmake', '%s: 3.20 or newer needed' % v)
        else:
            self.check('ok', 'cmake', v)
        nj = shutil.which('ninja')
        self.check('ok' if nj else 'fail', 'ninja',
                   tool_version([nj, '--version']) if nj else 'Ninja not found (brew install ninja)')
        clt = macos_clt()
        if sys.platform == 'darwin':
            self.check('ok' if clt else 'fail', 'xcode-clt',
                       clt or 'Xcode Command Line Tools not installed (xcode-select --install)')
        cc = shutil.which('clang') or shutil.which('cc')
        cxx = shutil.which('clang++') or shutil.which('c++')
        if sys.platform == 'darwin' and not clt:
            # /usr/bin/clang is a stub that would pop up Apple's installer
            self.check('fail', 'compiler', 'no C/C++ compiler (xcode-select --install)')
        elif cc and cxx:
            self.check('ok', 'compiler', tool_version([cc, '--version']))
        else:
            self.check('fail', 'compiler', 'no C/C++ compiler (macOS: xcode-select --install)')
        if not a.toolkit or not os.path.isdir(self.toolkit):
            git = shutil.which('git')
            if git and git.startswith('/usr/bin/') and not clt:
                git = None          # the CLT stub
            self.check('ok' if git else 'fail', 'git',
                       'git found' if git else 'git not found (needed to fetch the toolkit)')
        if sys.platform == 'darwin':
            prefix = '/opt/homebrew' if os.path.isdir('/opt/homebrew') else '/usr/local'
            libs = (('SDL2', os.path.join(prefix, 'lib', 'cmake', 'SDL2'), 'fail', 'sdl2'),
                    ('glslang', os.path.join(prefix, 'lib', 'cmake', 'glslang'), 'fail', 'glslang'),
                    ('spirv-cross', os.path.join(prefix, 'include', 'spirv_cross', 'spirv_cross_c.h'), 'fail', 'spirv-cross'))
            for name, p, lvl, pkg in libs:
                if os.path.exists(p):
                    self.check('ok', name.lower(), p)
                else:
                    # The toolkit's CMake finds these QUIETly: without them it
                    # still builds, but the game's shaders are never translated.
                    self.check(lvl, name.lower(), '%s not found (brew install %s)' % (name, pkg))

        # the toolkit patch this checkout of the port expects
        if not os.path.isfile(PATCH):
            self.check('fail', 'patch', '%s missing' % PATCH)
        if a.toolkit:
            if not os.path.isdir(os.path.join(self.toolkit, 'tools')):
                self.check('fail', 'toolkit', '%s is not a ps3recomp checkout' % self.toolkit)
            else:
                r = subprocess.run(['git', '-C', self.toolkit, 'apply', '--reverse', '--check', PATCH],
                                   capture_output=True, text=True)
                self.check('ok' if r.returncode == 0 else 'warn', 'toolkit',
                           '%s (%s)' % (self.toolkit, 'patched' if r.returncode == 0 else
                                        'patch not detected: it may be ahead of this port'))

    def check_space(self):
        a = self.a
        # the output folder
        if os.path.abspath(self.out) in (os.path.abspath(PORT_SRC), os.path.abspath(REPO)):
            self.check('fail', 'out', 'the output folder must not be the repository itself')
        for k, s in self.sources:
            if os.path.abspath(self.out).startswith(s + os.sep) or s.startswith(os.path.abspath(self.out) + os.sep):
                self.check('fail', 'out', 'the output folder and the game copy must not contain each other')

        # disk space
        need = WORK_BYTES
        clone = False
        if self.sources and self.sources[0][0] == 'dir' and not os.path.isdir(self.vfs_title):
            src = self.sources[0][1]
            same_dev = os.stat(src).st_dev == os.stat(existing_ancestor(self.out)).st_dev
            clone = sys.platform == 'darwin' and same_dev
            if not clone:
                need += tree_size(src)
        elif self.sources and self.sources[0][0] == 'pkg' and not os.path.isdir(self.vfs_title):
            need += sum(os.path.getsize(p) for _, p in self.sources)
        if self.sources and not a.toolkit and os.path.isdir(self.toolkit):
            need -= 150 * 1024 * 1024
        free = free_bytes(self.out)
        what = 'need ~%s%s, %s free on %s' % (human(need), ' (game data cloned, no copy)' if clone else '',
                                               human(free), existing_ancestor(self.out))
        if free < need:
            self.check('fail', 'disk', what)
        elif free - need < GAME_HEADROOM:
            self.check('warn', 'disk', what + '; the game needs ~3 GB free to boot')
        else:
            self.check('ok', 'disk', what)

    def check_json(self):
        """--check-json: the machine's prerequisites as one JSON object, for
        the launcher's first-run screen. Needs no game copy and no licence."""
        self.quiet = True
        self.check_host()
        self.check_tools()
        self.check_space()
        brew = next((b for b in ('/opt/homebrew/bin/brew', '/usr/local/bin/brew') if os.path.isfile(b)),
                    shutil.which('brew'))
        out = {
            'schema': 1,
            'platform': sys.platform,
            'machine': platform.machine(),
            'supported': sys.platform == 'darwin',
            'ok': self.fails == 0,
            'python': platform.python_version(),
            'homebrew': brew,
            'rpcs3': self.rpcs3,
            'rpcs3_dir': self.rpcs3_dir,
            'out': self.out,
            'checks': [{'level': l, 'name': n, 'message': m} for l, n, m in self.checks],
        }
        sys.stdout.write(json.dumps(out, indent=1) + '\n')
        return 0 if self.fails == 0 else 2

    def check_pup(self):
        """--firmware PS3UPDAT.PUP: a PUP with dev_flash packages, of the tested version."""
        try:
            pe = load_tool('pup_extract')
            with pe.Pup(self.fw_pup) as pup:
                self.pup_version = pup.version()
                n = len(pup.packages())
        except Exception as e:
            msg = str(e)
            self.check('fail', 'firmware', msg if self.fw_pup in msg else '%s: %s' % (self.fw_pup, msg))
            return
        if not n:
            self.check('fail', 'firmware', '%s: no dev_flash packages in it' % self.fw_pup)
        elif self.pup_version != pe.TESTED_FIRMWARE:
            self.check('warn', 'firmware', '%s: firmware %s (tested: %s; the modules will differ)'
                       % (self.fw_pup, self.pup_version or '?', pe.TESTED_FIRMWARE))
        else:
            self.check('ok', 'firmware', '%s: firmware %s, extracted in the "pup" step' % (self.fw_pup, self.pup_version))

    def check_keys(self, pkg_only=False):
        """The key file holds what this copy's EBOOT and firmware need (read from their
        clear headers; with a PUP, also the PKG key set its firmware packages need)."""
        if not self.keys_file:
            self.check('fail', 'keys', ('a PS3UPDAT.PUP' if pkg_only else '--decrypt-with python') +
                       ' needs a key file (--keys or PS3_KEYS)')
            return
        if not os.path.isfile(self.keys_file):
            self.check('fail', 'keys', '%s: not found' % self.keys_file)
            return
        try:
            ds, keys = load_keys(self.keys_file)
        except Exception as e:
            self.check('fail', 'keys', '%s: %s' % (self.keys_file, e))
            return
        missing = []
        if self.fw_pup:
            try:
                pe = load_tool('pup_extract')
                with pe.Pup(self.fw_pup) as pup:
                    # the modules' own SELF key: read from them once extracted
                    lack, notes = pe.missing_keys(pup, keys, need_self=not pkg_only and not self.fw_src)
                missing += lack
                for n in notes:
                    self.check('warn', 'keys', n)
            except Exception as e:
                missing.append('(the PUP does not read: %s)' % e)
            if pkg_only:
                if missing:
                    self.check('fail', 'keys', '%s lacks: %s' % (self.keys_file, '; '.join(missing)))
                else:
                    self.check('ok', 'keys', '%s (the PUP\'s PKG key set)' % self.keys_file)
                return
        need = []
        for kind, p in self.sources:
            if kind == 'dir':
                h = self_header(os.path.join(p, 'USRDIR', 'EBOOT.BIN'))
                if h:
                    need.append(('EBOOT.BIN',) + h)
        if self.fw_src:
            for m in FIRMWARE:
                h = self_header(os.path.join(self.fw_src, m + '.sprx'))
                if h:
                    need.append((m + '.sprx',) + h)
        for what, ptype, rev in need:
            try:
                keys.self_key(ptype, rev)
            except ds.SelfError:
                missing.append('%s (%s, key revision 0x%04X)' % (what, ptype, rev))
        try:
            keys.get('NP_klic_key')
        except ds.SelfError as e:
            missing.append(str(e))
        try:
            keys.rap_key()
        except ds.SelfError as e:
            missing.append(str(e))
        if missing:
            self.check('fail', 'keys', '%s lacks: %s' % (self.keys_file, '; '.join(missing)))
        else:
            self.check('ok', 'keys', '%s (decryption without RPCS3)' % self.keys_file)

    # ---- state ----------------------------------------------------------
    def load_state(self):
        try:
            with open(self.state_path) as f:
                self.state = json.load(f)
        except Exception:
            self.state = {}

    def save_state(self):
        os.makedirs(os.path.dirname(self.state_path), exist_ok=True)
        tmp = self.state_path + '.tmp'
        with open(tmp, 'w') as f:
            json.dump(self.state, f, indent=1, sort_keys=True)
        os.replace(tmp, self.state_path)

    def compute_keys(self):
        """What each step is built from; a step reruns when its key changes.

        The lifts are keyed on the content of what they read (the decrypted
        EBOOT and firmware), so re-importing an identical copy does not
        re-translate anything. Recomputed before every step, since the
        inputs of a step only exist once the steps before it ran."""
        k = self.keys
        def h(path):
            return sha256_file(path) if os.path.isfile(path) else 'missing:' + path
        patch = h(PATCH)
        k['toolkit'] = sha256_text('toolkit', self.toolkit,
                                   'external' if self.a.toolkit else patch + h(SETUP_TOOLKIT))
        k['venv'] = sha256_text('venv', self.venv, self.base_python or '',
                                h(os.path.join(self.toolkit, 'requirements.txt')))
        src = []
        for kind, p in self.sources:
            if kind == 'pkg':
                src.append(file_sig(p))
            else:
                src.append(p)
                for f in ('PARAM.SFO', os.path.join('USRDIR', 'EBOOT.BIN')):
                    q = os.path.join(p, f)
                    if os.path.exists(q):
                        src.append(file_sig(q))
        k['game'] = sha256_text('game', *src)
        rap = h(self.rap) if self.rap else ''
        k['pup'] = sha256_text('pup', file_sig(self.fw_pup), self.pup_root) if self.fw_pup else 'n/a'
        if self.fw_pup:
            fw = [k['pup']]     # re-extracting rewrites the .sprx: key on the PUP, not on them
        else:
            fw = [file_sig(os.path.join(self.fw_src, m + '.sprx')) for m in FIRMWARE] if self.fw_src else []
        k['decrypt'] = sha256_text('decrypt', k['game'], rap, self.decrypt_with, *fw)
        port = [f + h(os.path.join(PORT_SRC, f)) for f in PORT_FILES]
        for f in sorted(os.listdir(HERE)):
            if is_tool(f):
                port.append(f + h(os.path.join(HERE, f)))
        k['port'] = sha256_text('port', *port)
        elf = h(self.elf)
        fw = [h(os.path.join(self.fw_dir, m + '.prx')) for m in FIRMWARE]
        k['firmware'] = sha256_text('firmware', k['toolkit'], k['venv'], *fw)
        k['ppu'] = sha256_text('ppu', k['toolkit'], k['venv'], elf)
        k['spu'] = sha256_text('spu', k['toolkit'], k['venv'], elf)
        k['jobs'] = sha256_text('jobs', k['spu'], k['port'])
        k['dlc'] = sha256_text('dlc', k['game'], rap, bool(os.environ.get('PS3_RAP_KEY')))
        k['compile'] = sha256_text('compile', k['toolkit'], k['port'], k['firmware'], k['ppu'],
                                   k['spu'], k['jobs'])
        k['finish'] = 'always'

    def outputs_present(self, step):
        exe = game_executable(self.port)
        probe = {
            'toolkit': os.path.join(self.toolkit, 'CMakeLists.txt'),
            'venv': self.py,
            'game': os.path.join(self.vfs_title, 'PARAM.SFO'),
            'pup': os.path.join(self.pup_root, 'sys', 'external', FIRMWARE[-1] + '.sprx'),
            'decrypt': self.elf,
            'port': os.path.join(self.port, 'CMakeLists.txt'),
            'firmware': os.path.join(self.port, 'lle', 'libsre'),
            'ppu': os.path.join(self.port, 'recompiled', 'ppu_recomp.h'),
            'spu': os.path.join(self.port, 'spu', 'spu_workloads.c'),
            'jobs': os.path.join(self.port, 'spu', 'jobmods_register.c'),
            'compile': exe,
        }.get(step)
        return probe is None or os.path.exists(probe)

    def forced(self, sid):
        a = self.a
        if a.only:
            return sid in a.only.split(',')
        if a.force:
            return True
        return bool(a.from_step) and STEP_IDS.index(sid) >= STEP_IDS.index(a.from_step)

    def wanted(self, sid):
        """Whether a step has to run, judged from what is on disk right now."""
        if self.a.only and sid not in self.a.only.split(','):
            return False
        if self.forced(sid) or sid == 'finish':
            return True
        return self.state.get(sid) != self.keys.get(sid) or not self.outputs_present(sid)

    # ---- running tools --------------------------------------------------
    def run(self, step, cmd, cwd=None, env=None, secret=False, ninja=False, keep=None):
        shown = ' '.join(('"%s"' % c) if ' ' in str(c) else str(c) for c in cmd)
        log('$ ' + shown)
        e = dict(os.environ)
        e['PS3RECOMP'] = self.toolkit
        e['PYTHONUNBUFFERED'] = '1'
        e['PYTHONDONTWRITEBYTECODE'] = '1'   # leave the toolkit checkout as it was
        if env:
            e.update(env)
        if not secret:
            e.pop('PS3_RAP_KEY', None)      # only the DLC step sees it
        p = subprocess.Popen([str(c) for c in cmd], cwd=cwd, env=e,
                             stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        last = -1
        buf = b''
        while True:
            chunk = p.stdout.read1(65536) if hasattr(p.stdout, 'read1') else p.stdout.read(4096)
            if not chunk:
                break
            buf += chunk
            parts = re.split(rb'[\r\n]', buf)
            buf = parts.pop()
            for raw in parts:
                line = raw.decode('utf-8', 'replace').rstrip()
                if not line:
                    continue
                if keep is not None and not keep.search(line):
                    continue
                if secret:      # never echo key material
                    line = re.sub(r'\b[0-9a-fA-F]{32}\b', '<hidden>', line)
                if ninja:
                    m = re.match(r'\[(\d+)/(\d+)\]', line)
                    if m:
                        done, total = int(m.group(1)), int(m.group(2))
                        pct = done * 200 // max(total, 1)
                        if pct != last:
                            last = pct
                            emit('SUBPROGRESS', step, '%d/%d' % (done, total))
                emit('LOG', line)
        if buf.strip():
            emit('LOG', buf.decode('utf-8', 'replace').rstrip())
        rc = p.wait()
        if rc != 0:
            what = cmd[1] if len(cmd) > 1 and str(cmd[0]) == self.py else cmd[0]
            raise StepError('%s exited with status %d' % (os.path.basename(str(what)), rc))

    def T(self, name):
        return os.path.join(self.toolkit, 'tools', name)

    # ---- steps ----------------------------------------------------------
    def step_toolkit(self):
        if self.a.toolkit:
            log('using the toolkit at %s as is' % self.toolkit)
            return
        self.run('toolkit', ['sh', SETUP_TOOLKIT, self.toolkit])

    def step_venv(self):
        if self.a.venv:
            log('using the venv at %s as is' % self.venv)
            return
        if not os.path.isfile(self.py):
            self.run('venv', [self.base_python, '-m', 'venv', self.venv])
        self.run('venv', [self.py, '-m', 'pip', 'install', '--disable-pip-version-check', '-q',
                          '-r', os.path.join(self.toolkit, 'requirements.txt')])

    def step_game(self):
        parent = os.path.dirname(self.vfs_title)
        os.makedirs(parent, exist_ok=True)
        if os.path.lexists(self.vfs_title):
            log('replacing %s' % self.vfs_title)
            shutil.rmtree(self.vfs_title)
        kind = self.sources[0][0]
        if kind == 'dir':
            src = self.sources[0][1]
            emit('PROGRESS', 'game', self.idx('game'), 'cloning %s' % src)
            ok = False
            if sys.platform == 'darwin':
                # APFS clone: no extra space, and the port never writes into RPCS3's copy
                r = subprocess.run(['cp', '-Rc', src, self.vfs_title], capture_output=True, text=True)
                ok = r.returncode == 0
                if not ok:
                    log('clone failed (%s), copying instead' % r.stderr.strip())
                    if os.path.exists(self.vfs_title):
                        shutil.rmtree(self.vfs_title)
            if not ok:
                shutil.copytree(src, self.vfs_title, symlinks=True)
        else:
            for _, pkg in self.sources:
                emit('PROGRESS', 'game', self.idx('game'), 'extracting %s' % os.path.basename(pkg))
                self.run('game', [self.py, self.T('pkg_extract.py'), pkg, self.vfs_title])
        sfo_path = os.path.join(self.vfs_title, 'PARAM.SFO')
        if not os.path.isfile(sfo_path):
            raise StepError('no PARAM.SFO after the import')
        self.sfo = read_sfo(sfo_path)
        if self.sfo.get('TITLE_ID') != TITLE_ID:
            raise StepError('imported copy is %s, not %s' % (self.sfo.get('TITLE_ID'), TITLE_ID))
        if self.sfo.get('APP_VER') != TESTED_APP_VER:
            emit('WARN', 'game', 'APP_VER %s; the port is tested with %s (install the game update)'
                 % (self.sfo.get('APP_VER'), TESTED_APP_VER))
        for d in (os.path.join(self.vfs, 'dev_hdd0', 'home', '00000001', 'savedata'),
                  os.path.join(self.vfs, 'dev_hdd1')):
            os.makedirs(d, exist_ok=True)
        link = os.path.join(self.vfs, 'PS3_GAME')
        if not os.path.lexists(link):
            os.symlink(os.path.join('dev_hdd0', 'game', TITLE_ID), link)

    def step_pup(self):
        if os.path.isdir(self.pup_root):
            shutil.rmtree(self.pup_root)
        # -I -B: isolated, and no __pycache__ next to the tools; [i/n] lines are SUBPROGRESS
        self.run('pup', [self.py, '-I', '-B', PUP_EXTRACT, self.fw_pup, '-o', self.legacy,
                         '--keys', self.keys_file], secret=True, ninja=True)
        self.fw_src = resolve_fw_dir(self.pup_root)
        if not self.fw_src:
            raise StepError('pup_extract.py did not write %s' % os.path.join(self.pup_root, 'sys', 'external'))

    def python_decrypt(self, path, want, rap=False):
        if os.path.exists(want):
            os.remove(want)
        cmd = [self.py, DECRYPT_SELF, path, '-o', want, '--keys', self.keys_file]
        if rap:
            cmd += ['--rap', self.rap]
        # secret: decrypt_self.py may take the RAP key from PS3_RAP_KEY
        self.run('decrypt', cmd, secret=True)
        if not os.path.isfile(want):
            raise StepError('decrypt_self.py did not write %s' % want)

    def rpcs3_decrypt(self, path, want):
        if os.path.exists(want):
            os.remove(want)
        # RPCS3 prints its whole Vulkan/LLVM start-up banner; keep what matters
        self.run('decrypt', [self.rpcs3, '--decrypt', path],
                 keep=re.compile(r'decrypt|error|fail|fatal|rap|licen|key', re.I))
        if not os.path.isfile(want):
            raise StepError('RPCS3 did not write %s (is the licence installed and the firmware present?)' % want)

    def step_decrypt(self):
        os.makedirs(self.game_dir, exist_ok=True)
        os.makedirs(self.fw_dir, exist_ok=True)
        usr = os.path.join(self.vfs_title, 'USRDIR')
        bin_src = os.path.join(usr, 'EBOOT.BIN')
        bin_dst = os.path.join(self.game_dir, 'EBOOT.BIN')
        if os.path.isfile(bin_src):
            shutil.copy2(bin_src, bin_dst)
        if not os.path.isfile(bin_dst):
            raise StepError('no EBOOT.BIN in the imported copy')

        cid = npd_content_id(bin_dst) or self.content_id
        if not cid:
            raise StepError('EBOOT.BIN has no NPD header (not the PSN release?)')
        if self.decrypt_with == 'rpcs3':
            # RPCS3 looks the licence up as exdata/<content id>.rap
            exdata = os.path.join(self.rpcs3_dir, 'dev_hdd0', 'home', '00000001', 'exdata')
            dst = os.path.join(exdata, cid + '.rap')
            if os.path.isfile(dst):
                if sha256_file(dst) != sha256_file(self.rap):
                    emit('WARN', 'decrypt', '%s already in RPCS3 and differs from %s; using RPCS3\'s'
                         % (os.path.basename(dst), self.rap))
                else:
                    log('licence already installed in RPCS3: %s' % dst)
            else:
                os.makedirs(exdata, exist_ok=True)
                shutil.copyfile(self.rap, dst)
                log('licence installed in RPCS3: %s' % dst)
            decrypt = self.rpcs3_decrypt
        else:
            log('decrypting with %s (no RPCS3)' % os.path.basename(DECRYPT_SELF))
            decrypt = self.python_decrypt

        emit('PROGRESS', 'decrypt', self.idx('decrypt'), 'EBOOT.BIN')
        if self.decrypt_with == 'rpcs3':
            decrypt(bin_dst, self.elf)
        else:
            decrypt(bin_dst, self.elf, rap=True)
        with open(self.elf, 'rb') as f:
            if f.read(4) != b'\x7fELF':
                raise StepError('EBOOT.elf is not an ELF: decryption failed')
        if sha256_file(self.elf) != TESTED_EBOOT_ELF_SHA256:
            emit('WARN', 'decrypt', 'EBOOT.elf differs from the tested 01.03 executable: '
                 'the lift may not match (expect problems)')
        for m in FIRMWARE:
            emit('PROGRESS', 'decrypt', self.idx('decrypt'), m + '.sprx')
            s = os.path.join(self.fw_dir, m + '.sprx')
            prx = os.path.join(self.fw_dir, m + '.prx')
            shutil.copy2(os.path.join(self.fw_src, m + '.sprx'), s)
            decrypt(s, prx)
            if sha256_file(prx) != TESTED_FW_PRX_SHA256[m]:
                emit('WARN', 'decrypt', '%s.prx differs from the tested firmware 4.93 module: '
                     'the lift may not match (expect problems)' % m)
        # the game runs the decrypted executable from its own USRDIR
        if os.path.isfile(bin_src):
            os.remove(bin_src)
        shutil.copy2(self.elf, os.path.join(usr, 'EBOOT.elf'))

    def step_port(self):
        os.makedirs(os.path.join(self.port, 'tools'), exist_ok=True)
        pairs = [(os.path.join(PORT_SRC, f), os.path.join(self.port, f)) for f in PORT_FILES]
        pairs += [(os.path.join(HERE, f), os.path.join(self.port, 'tools', f))
                  for f in sorted(os.listdir(HERE)) if is_tool(f)]
        n = 0
        for s, d in pairs:
            if not os.path.isfile(s):
                continue
            if os.path.isfile(d):
                with open(s, 'rb') as fs, open(d, 'rb') as fd:
                    if fs.read() == fd.read():
                        continue
            shutil.copy2(s, d)       # unchanged files keep their mtime: no needless recompile
            n += 1
        log('%d port file(s) updated in %s' % (n, self.port))

    def fresh(self, *dirs):
        for d in dirs:
            p = os.path.join(self.port, d)
            if os.path.isdir(p):
                shutil.rmtree(p)

    def step_firmware(self):
        self.fresh('lle', 'lle_spu')
        bases = {'liblv2': '0x30000000', 'libsysmodule': '0x30180000', 'libsre': '0x30100000'}
        for m in FIRMWARE:
            emit('PROGRESS', 'firmware', self.idx('firmware'), m)
            self.run('firmware', [self.py, self.T('lift_firmware_module.py'),
                                  os.path.join(self.fw_dir, m + '.prx'),
                                  '--base', bases[m], '--out', os.path.join('lle', m)], cwd=self.port)
        emit('PROGRESS', 'firmware', self.idx('firmware'), 'libsre SPU kernel')
        self.run('firmware', [self.py, self.T('lift_firmware_spu.py'), '--module-dir', 'lle/libsre',
                              '--base', '0x30100000', '--name', 'libsre', '--first-image-id', '100',
                              '--body', '0x21480:0x2200:0xA00:0xA00:0x2110',
                              '--body', '0x23680:0x1E40:0xA00:0xA00:0x1C80',
                              '--body', '0x2A180:0x3DA0:0xA00:0xA00:0x3D00',
                              '--out', 'lle_spu'], cwd=self.port)

    def step_ppu(self):
        self.fresh('recompiled')
        os.makedirs(os.path.join(self.port, 'out'), exist_ok=True)
        emit('PROGRESS', 'ppu', self.idx('ppu'), 'loading EBOOT.elf')
        self.run('ppu', [self.py, self.T('ppu_loader.py'), self.elf, '-o', 'out/'], cwd=self.port)
        emit('PROGRESS', 'ppu', self.idx('ppu'), 'translating (a few minutes)')
        self.run('ppu', [self.py, self.T('ppu_lifter.py'), self.elf,
                         '--functions', 'out/EBOOT.functions.json', '--hle-stubs', 'out/EBOOT.imports.json',
                         '--code-end', '0x742e18', '--max-chunk-lines', '150000',
                         '--jobs', str(self.jobs), '--output', 'recompiled/'], cwd=self.port)

    def step_spu(self):
        self.fresh('spu')
        os.makedirs(os.path.join(self.port, 'out'), exist_ok=True)
        emit('PROGRESS', 'spu', self.idx('spu'), 'SPU images')
        self.run('spu', [self.py, self.T('extract_spu_images.py'), self.elf, '-o', 'spu/images'], cwd=self.port)
        # the Wws_Job policy module: a raw blob the title hands to
        # cellSpursAddWorkload (guest 0x8E5B00, 11904 bytes, file offset 0x8D5B00)
        with open(self.elf, 'rb') as f:
            f.seek(0x8D5B00)
            blob = f.read(11904)
        pm = os.path.join(self.port, 'out', 'wwsjob_pm.bin')
        with open(pm, 'wb') as f:
            f.write(blob)
        self.run('spu', [self.py, self.T('wrap_spu_elf.py'), 'out/wwsjob_pm.bin', '--base', '0xA00',
                         '--entry', '0xA00', '--out', 'spu/images/wwsjob_pm_at_008E5B00.elf'], cwd=self.port)
        emit('PROGRESS', 'spu', self.idx('spu'), 'translating the SPU images')
        self.run('spu', [self.py, self.T('build_spu_workloads.py'), '--images', 'spu/images', '--lifted', 'spu',
                         '--out', 'spu/spu_workloads.c', '--register-fn', 'infamous2_spu_register_all',
                         '--constructor', '--title', 'infamous2', '--eboot', self.elf,
                         '--extra-funcs', 'wwsjob_pm_at_008E5B00=0xA2C',
                         '--extra-funcs', 'spu_0000_at_007FA100=' + ','.join(SPU_IMAGE_EXTRA)],
                 cwd=self.port)

    def step_jobs(self):
        spu = os.path.join(self.port, 'spu')
        for n in os.listdir(spu) if os.path.isdir(spu) else []:
            p = os.path.join(spu, n)
            if n.startswith(('jobmod_', 'datamod_', 'relocmod_')) or n == 'jobmods':
                shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
            elif n in ('jobmods_register.c', 'datamods_register.c', 'relocmods_register.c', 'jobmods_manifest.json'):
                os.remove(p)
        tools = os.path.join(self.port, 'tools')
        emit('PROGRESS', 'jobs', self.idx('jobs'), 'job modules (C0DEC0DE)')
        self.run('jobs', [self.py, os.path.join(tools, 'extract_eboot_jobmods.py'), self.elf,
                          '--out', 'spu/jobmods'], cwd=self.port)
        self.run('jobs', [self.py, self.T('lift_jobmods.py'), '--jobmods', 'spu/jobmods', '--out', 'spu',
                          '--register', 'spu/jobmods_register.c', '--symbol-prefix', 'infamous2'], cwd=self.port)
        # entries the interpreter was seen resuming at (SPU_REJOIN_DBG /
        # "branched into unlifted LS"), added by hand to the dev build
        for mod, extra in JOBMOD_EXTRA:
            self.run('jobs', [self.py, os.path.join(tools, 'relift_jobmod.py'), mod, '--extra', ','.join(extra),
                              '--jobmods', 'spu/jobmods', '--out', 'spu'], cwd=self.port)
        emit('PROGRESS', 'jobs', self.idx('jobs'), 'data modules (DA7ABA5E)')
        self.run('jobs', [self.py, os.path.join(tools, 'lift_datamods.py'), self.elf,
                          '--out', 'spu', '--extra', '0x6C44,0x6C64'], cwd=self.port)
        emit('PROGRESS', 'jobs', self.idx('jobs'), 'relocatable jobs')
        self.run('jobs', [self.py, os.path.join(tools, 'lift_relocmods.py'), self.elf, '--out', 'spu'],
                 cwd=self.port)

    def step_dlc(self):
        if not os.environ.get('PS3_RAP_KEY'):
            emit('WARN', 'dlc', 'PS3_RAP_KEY not set: DLC licence key not derived (the base game is unaffected)')
            return
        exdata = os.path.join(self.rpcs3_dir, 'dev_hdd0', 'home', '00000001', 'exdata')
        found = 0
        for root, _d, files in os.walk(os.path.join(self.vfs_title, 'USRDIR', 'dlc')):
            for f in sorted(files):
                if not f.lower().endswith('.edat'):
                    continue
                found += 1
                edat = os.path.join(root, f)
                try:
                    self.run('dlc', [self.py, os.path.join(self.port, 'tools', 'rap2rifkey.py'), edat,
                                     '--exdata', exdata], secret=True)
                except StepError as e:
                    emit('WARN', 'dlc', '%s: %s' % (f, e))
        if not found:
            log('no DLC EDATs in this copy')

    def step_compile(self):
        build = os.path.join(self.port, 'build')
        emit('PROGRESS', 'compile', self.idx('compile'), 'configuring')
        self.run('compile', ['cmake', '-S', self.port, '-B', build, '-G', 'Ninja',
                             '-DCMAKE_BUILD_TYPE=Release', '-DPS3RECOMP_DIR=' + self.toolkit,
                             '-DPython3_EXECUTABLE=' + self.py])
        emit('PROGRESS', 'compile', self.idx('compile'), 'compiling (a few minutes)')
        self.run('compile', ['cmake', '--build', build, '-j', str(self.jobs)], ninja=True)

    def step_finish(self):
        exe = game_executable(self.port)
        if not os.path.isfile(exe):
            raise StepError('no executable at %s' % exe)
        sfo = self.sfo
        if not sfo and os.path.isfile(os.path.join(self.vfs_title, 'PARAM.SFO')):
            sfo = read_sfo(os.path.join(self.vfs_title, 'PARAM.SFO'))
        tk = subprocess.run(['git', '-C', self.toolkit, 'rev-parse', '--short', 'HEAD'],
                            capture_output=True, text=True).stdout.strip()
        port_commit = port_build_info().get('port_commit') or subprocess.run(
            ['git', '-C', REPO, 'rev-parse', '--short', 'HEAD'], capture_output=True, text=True).stdout.strip()
        title = os.path.join(self.vfs, 'PS3_GAME')
        info = {
            'schema': 1,
            'title_id': TITLE_ID,
            'title': sfo.get('TITLE', ''),
            'app_ver': sfo.get('APP_VER', ''),
            'root': self.out,
            'port_dir': self.port,
            'run_sh': os.path.join(self.port, 'run.sh'),
            'executable': exe,
            'eboot': os.path.join(title, 'USRDIR', 'EBOOT.elf'),
            'vfs': self.vfs,
            'dev_flash': self.dev_flash_root(),
            'icon': os.path.join(title, 'ICON0.PNG'),
            'background': os.path.join(title, 'PIC1.PNG'),
            'toolkit_commit': tk,
            'port_commit': port_commit,
            'built_at': time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        }
        path = os.path.join(self.out, 'install.json')
        with open(path + '.tmp', 'w') as f:
            json.dump(info, f, indent=2)
        os.replace(path + '.tmp', path)
        log('wrote %s' % path)

    def dev_flash_root(self):
        """The dev_flash tree the game's /dev_flash maps to at run time."""
        if self.fw_src and os.path.basename(self.fw_src) == 'external':
            root = os.path.dirname(os.path.dirname(self.fw_src))
            if os.path.basename(os.path.dirname(self.fw_src)) == 'sys':
                return root
        rp = os.path.join(self.rpcs3_dir, 'dev_flash') if self.rpcs3_dir else ''
        return rp if rp and os.path.isdir(rp) else ''

    # ---- driver ---------------------------------------------------------
    def idx(self, step):
        ids = [s for s, _ in self.steps]
        return '%d/%d' % (ids.index(step) + 1, len(ids))

    def main(self):
        self.check_all()
        self.load_state()
        self.compute_keys()
        # The plan is an estimate: a step after one that runs is listed as
        # "run", and is skipped at its turn if its inputs came out unchanged.
        upstream = {'venv': ['toolkit'], 'pup': ['venv'], 'decrypt': ['game', 'pup'],
                    'firmware': ['toolkit', 'venv', 'decrypt'], 'ppu': ['toolkit', 'venv', 'decrypt'],
                    'spu': ['toolkit', 'venv', 'decrypt'], 'jobs': ['spu', 'port'], 'dlc': ['game'],
                    'compile': ['toolkit', 'port', 'firmware', 'ppu', 'spu', 'jobs']}
        planned = {}
        for i, (sid, title) in enumerate(self.steps):
            r = self.wanted(sid)
            if not r and not self.a.only:
                r = any(planned.get(u) for u in upstream.get(sid, []))
            planned[sid] = r
            st = 'run' if r else ('skip' if self.a.only else 'up-to-date')
            emit('PLAN', '%d/%d' % (i + 1, len(self.steps)), sid, st, title)
        if self.fails:
            emit('ERROR', 'checks', '%d prerequisite(s) missing' % self.fails)
            return 2
        if self.a.dry_run:
            emit('PROGRESS', 'checks', '0/%d' % len(self.steps), 'dry run: prerequisites ok, nothing done')
            return 0
        if self.a.nice:
            try:
                os.nice(self.a.nice)
            except (AttributeError, OSError):
                pass
        os.makedirs(self.out, exist_ok=True)
        for sid, title in self.steps:
            i = self.idx(sid)
            self.compute_keys()
            if not self.wanted(sid):
                emit('STEP-SKIP', sid, i, 'not requested' if self.a.only else 'up to date')
                continue
            emit('PROGRESS', sid, i, title)
            t0 = time.time()
            try:
                getattr(self, 'step_' + sid)()
            except StepError as e:
                emit('ERROR', sid, str(e))
                return 1
            except Exception as e:      # an unexpected failure still ends with a parsable line
                emit('ERROR', sid, '%s: %s' % (type(e).__name__, e))
                return 1
            if sid != 'finish':
                self.compute_keys()     # e.g. the venv key reads the toolkit's requirements
                self.state[sid] = self.keys[sid]
                self.save_state()
            emit('STEP-DONE', sid, i, '%.0f' % (time.time() - t0))
        info = os.path.join(self.out, 'install.json')
        emit('DONE', info if os.path.isfile(info) else '-')
        return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--game', action='append', help='RPCS3 game folder (NPUA80638) or a PKG; repeat for update PKGs')
    ap.add_argument('--rap', help="the player's licence (.rap)")
    ap.add_argument('--out', default=default_out(), help='install folder (default: %(default)s)')
    ap.add_argument('--rpcs3', help='RPCS3 executable (default: found in the usual places)')
    ap.add_argument('--rpcs3-dir', help="RPCS3's data folder, holding dev_flash/ and dev_hdd0/ "
                                        '(default: %s)' % (default_rpcs3_dir() or 'none'))
    ap.add_argument('--decrypt-with', choices=('auto', 'python', 'rpcs3'), default='auto',
                    help='python: tools/decrypt_self.py with --keys, no RPCS3; rpcs3: rpcs3 --decrypt '
                         '(default auto: python when a key file is given)')
    ap.add_argument('--keys', default=os.environ.get('PS3_KEYS'),
                    help='key file for --decrypt-with python, scetool format (default: $PS3_KEYS)')
    ap.add_argument('--firmware', help='PS3 firmware: a dev_flash folder, its sys/external, a folder '
                                       'with liblv2/libsysmodule/libsre.sprx, or PS3UPDAT.PUP (needs '
                                       '--keys with the PKG key set) (default: RPCS3\'s dev_flash)')
    ap.add_argument('--toolkit', help='use this patched ps3recomp checkout instead of cloning one (left untouched)')
    ap.add_argument('--venv', help='use this Python venv instead of creating one')
    ap.add_argument('--python', help='Python 3.10+ used to create the venv')
    ap.add_argument('--jobs', '-j', type=int, help='parallel jobs for the lifter and the compiler')
    ap.add_argument('--nice', type=int, default=0, help='lower the priority of the whole rebuild by N')
    ap.add_argument('--dry-run', action='store_true', help='check prerequisites and print the plan; change nothing')
    ap.add_argument('--force', action='store_true', help='rerun every step')
    ap.add_argument('--from', dest='from_step', choices=STEP_IDS, help='rerun from this step on')
    ap.add_argument('--only', help='run only these steps (comma-separated)')
    ap.add_argument('--list-steps', action='store_true', help='print the step ids and exit')
    ap.add_argument('--check-json', action='store_true',
                    help='check only the machine (RPCS3, firmware, tools, disk; no game or licence needed) '
                         'and print the result as JSON')
    a = ap.parse_args(argv)
    if a.list_steps:
        for i, (sid, title) in enumerate(STEPS):
            emit('STEP', '%d/%d' % (i + 1, len(STEPS)), sid, title)
        return 0
    if a.check_json:
        return Rebuild(a).check_json()
    if a.only:
        bad = [s for s in a.only.split(',') if s not in STEP_IDS]
        if bad:
            ap.error('unknown step(s): %s' % ', '.join(bad))
    return Rebuild(a).main()


if __name__ == '__main__':
    sys.exit(main())
