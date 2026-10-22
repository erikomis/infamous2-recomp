#!/usr/bin/env python3
"""Triage a game log: what went wrong, how often, and when.

    python3 log_triage.py <session folder | game.log> [--out DIR] [--known FILE] [--quiet]
    python3 log_triage.py --selftest

A session folder is what the launcher's debug mode writes
(`~/Library/Application Support/infamous2-recomp/reports/<UTC time>/`):
game.log, events.tsv (start, a tick every 2 s with the log's size, captures,
exit), captures.txt, system.txt, shaders/, surf/, shots/, crash.ips. A bare
log works too (infamous2-lle/out/<name>.log; out/<name>/shaders/ is used
when it is there).

Writes triage.md (for a person or an agent to read) and triage.json (the same
data, for the launcher and scripts) into the session folder; for a bare log
named other than game.log, <name>.triage.md / .json next to it. --out picks
another folder.

What it looks for, grouped by message with numbers and addresses masked
(count, first and last time):

  crash          FATAL, signals, the exit status and a macOS crash report
  stall          FIFO/GCM stalls ("ref stuck"), semaphore acquires given up,
                 FIFO jumps to unmapped IO, resyncs, stall dumps
  shader         MSL compile, decompile and pipeline failures (with the FP/VP
                 hashes from shaders/manifest.txt), unsupported FP fetches,
                 decompiler TODOs in the dumped shaders
  texture        texture binds that came out black, failed texture creations
                 (by format), render-target reads with a size mismatch
  spu            Edge-watch hits, SPU faults, branches to 0, unimplemented
                 SPU ops, interpreter fallbacks
  unimplemented  unresolved HLE NIDs, stub syscalls, "not implemented"
  syscall        syscalls that failed (LV2_ERRDBG=1), by number and error
  fs             files the game could not open
  audio          the mixer behind real time or dropping frames (AUDIO_RATE=1)
  draw           draws the engine dropped (PS3RECOMP_ENG_DROPLOG=1)
  other          any other line that says error / failed / invalid / ...
  perf           fps and frame-time statistics ([RSX FPS], [rsx fps],
                 the frame-dump timestamps), hitches

Lines that match log_triage_known.txt (next to this file) are counted as
expected and kept out of the problem lists. Times are seconds since the
session started: from events.tsv when it is there, else from the log's own
monotonic timestamps (the frame dumps' t=), else not shown.

Standard library only, Python 3.9+.
"""
from __future__ import annotations

import argparse
import bisect
import json
import os
import re
import statistics
import sys
import time
from collections import Counter, OrderedDict

HERE = os.path.dirname(os.path.abspath(__file__))
KNOWN_FILE = os.path.join(HERE, 'log_triage_known.txt')
SCHEMA = 1

CATEGORIES = OrderedDict([
    ('crash', 'Crashes and exit'),
    ('stall', 'Stalls (GPU / FIFO)'),
    ('shader', 'Shaders and pipelines'),
    ('texture', 'Textures'),
    ('spu', 'SPU'),
    ('unimplemented', 'Unimplemented functions'),
    ('syscall', 'Syscall errors'),
    ('fs', 'File system'),
    ('audio', 'Audio'),
    ('draw', 'Draws dropped'),
    ('perf', 'Frame rate'),
    ('other', 'Other warnings and errors'),
])
# How bad a group is, for the top issues: category weight x severity.
SEV_RANK = {'error': 3, 'warn': 2, 'info': 1}
CAT_RANK = {'crash': 10, 'stall': 8, 'shader': 7, 'texture': 7, 'spu': 6, 'draw': 5, 'unimplemented': 5,
            'audio': 4, 'perf': 4, 'fs': 3, 'syscall': 2, 'other': 2}

# Trace tags the default (verbose) log is full of; they are counted, not read.
NOISE_TAGS = frozenset((
    'WAIT', 'evt', 'ch-block', 'WAKE', 'pass', 'rsx capture', 'mfc-list', 'spu-int', 'spu-ovl',
    'sys_memory', 'sem', 'SYS', 'cellPad', 'cellGcmSys', 'cellVideoOut', 'cellGame', 'cellSaveData',
    'lv2_prx', 'sceNpTrophy', 'cellSysutil', 'ppu', 'boot', 'HLE', 'cellAvconfExt', 'sc-caller',
    'regdump', 'texstate', 'fpk', 'surf-dump', 'interp-entry', 'rejoin', 'spu-hist', 'interp-where',
    'lwc', 'FSDBG', 'tdump', 'ftrace', 'RSX', 'spu-spin', 'SPU-TRACE', 'spu-trace', 'oracle',
    'FIFODUMP', 'FIFOSTEP',
))
# Tags whose lines are always read even though their tag is listed above.
READ_ANYWAY = re.compile(
    r'FAIL|FATAL|fault|FAULT|error|ERROR|unresolved|UNRESOLVED|stuck|unmapped|gave up|BRANCH-TO-0|'
    r'unimplemented|ContentErrorDialog|STARVATION|\(stub\)|SIZE MISMATCH')

TAG_RE = re.compile(r'\[([^\]\[]{1,40})\]')
# A line two threads wrote into each other: "...Init()[WAIT] event_queue..."
SPLIT_GLUED = re.compile(r'(?<=[^\s\[])(?=\[[A-Za-z][\w .:/+-]{0,30}\] )')
SPLIT_SPACED = re.compile(r'(?<=\s)(?=\[(?:WAIT|evt|ch-block|mfc-list|spu-int|spu-ovl|WAKE|sem|SYS|SPU|spu)\] )')
TIME_ANCHOR = re.compile(r' t=(\d{3,}\.\d{3})\b')
CAPTURED_FRAME = re.compile(r'^\[rsx engine/metal\] captured frame (\d+) to .* t=(\d+\.\d+)')

HOME = os.path.expanduser('~')


def redact(s):
    if HOME and len(HOME) > 1:
        s = s.replace(HOME, '~')
    return s


_MASK = re.compile(r'0x[0-9A-Fa-f]+|\b(?=[0-9A-Fa-f]*\d)[0-9A-Fa-f]{6,}\b|\d+(?:\.\d+)?')


def _mask(m):
    return '0x#' if m.group(0).startswith('0x') else '#'


def normalize(s):
    """The message with addresses and numbers masked, for grouping."""
    s = _MASK.sub(_mask, redact(s))
    return s[:220]


# ---- syscall and error names (toolkit runtime/syscalls/lv2_syscall_table.h,
# include/ps3emu/error_codes.h) -------------------------------------------------
SYSCALLS = {
    41: 'ppu_thread_exit', 44: 'ppu_thread_join', 52: 'ppu_thread_create', 82: 'event_flag_create',
    85: 'event_flag_wait', 86: 'event_flag_trywait', 87: 'event_flag_set', 90: 'semaphore_create',
    92: 'semaphore_wait', 93: 'semaphore_trywait', 94: 'semaphore_post', 95: 'lwmutex_create',
    97: 'lwmutex_lock', 98: 'lwmutex_unlock', 99: 'lwmutex_trylock', 100: 'mutex_create',
    102: 'mutex_lock', 103: 'mutex_trylock', 104: 'mutex_unlock', 105: 'cond_create', 107: 'cond_wait',
    108: 'cond_signal', 109: 'cond_signal_all', 113: 'lwcond_wait', 115: 'lwcond_signal',
    116: 'lwcond_signal_all', 123: 'rwlock_tryrlock', 128: 'event_queue_create',
    129: 'event_queue_destroy', 130: 'event_queue_receive', 131: 'event_queue_tryreceive',
    134: 'event_port_create', 136: 'event_port_connect_local', 138: 'event_port_send',
    141: 'timer_usleep', 142: 'timer_sleep', 148: 'rwlock_trywlock', 156: 'spu_image_open',
    170: 'spu_thread_group_create', 173: 'spu_thread_group_start', 178: 'spu_thread_group_join',
    190: 'spu_thread_write_spu_mb', 330: 'mmapper_allocate_address', 348: 'memory_allocate',
    349: 'memory_free', 480: 'prx_load_module', 801: 'fs_open', 802: 'fs_read', 803: 'fs_write',
    804: 'fs_close', 805: 'fs_opendir', 808: 'fs_stat', 809: 'fs_fstat', 811: 'fs_mkdir',
    814: 'fs_unlink', 818: 'fs_lseek', 871: 'ss_access_control_engine',
}
ERRORS = {
    0x80010001: 'EAGAIN', 0x80010002: 'EINVAL', 0x80010003: 'ENOSYS', 0x80010004: 'ENOMEM',
    0x80010005: 'ESRCH', 0x80010006: 'ENOENT', 0x80010007: 'ENOEXEC', 0x80010008: 'EDEADLK',
    0x80010009: 'EPERM', 0x8001000A: 'EBUSY', 0x8001000B: 'ETIMEDOUT', 0x8001000C: 'EABORT',
    0x8001000D: 'EFAULT', 0x8001000F: 'ESTAT', 0x80010010: 'EALIGN', 0x80010011: 'EKRESOURCE',
    0x80010012: 'EISDIR', 0x80010013: 'ECANCELED', 0x80010014: 'EEXIST', 0x80010016: 'ENOTCONN',
    0x8001001F: 'EINTR', 0x80010023: 'ENOSPC', 0x80010029: 'EACCES', 0x8001002A: 'EBADF',
    0x8001002B: 'EIO', 0x8001002C: 'EMFILE', 0x8001002E: 'ENOTDIR', 0x80010036: 'ENOTEMPTY',
    0x80010037: 'ENOTSUP', 0x80010039: 'EOVERFLOW', 0x8001003A: 'ENOTMOUNTED',
    0x80010120: 'EMUTEX_NOT_OWNED', 0x80010123: 'EMUTEX_DEADLK', 0x80010130: 'ECOND_NOT_OWNED',
}


def vfs_path(host):
    """A host path inside the port's VFS as the game named it."""
    m = re.search(r'/vfs/(dev_\w+/.*)$', host)
    if m:
        return '/' + m.group(1)
    m = re.search(r'/vfs/(PS3_GAME/.*)$', host)
    if m:
        return '/dev_bdvd/' + m.group(1)
    return redact(host)


# ---- the rules: (category, severity, regex, title) -------------------------------
# The first rule whose regex is found in a line takes it. A title is a format
# string over the match's groups ({0} is the whole match); without one the
# group is the normalized line.
RULES = [
    # crashes, fatal errors
    ('crash', 'error', r'\[ppu\] FATAL: (.*)', 'PPU fatal: {1}'),
    ('crash', 'error', r'^\[CRASH\] (.*)', 'crash: {1}'),
    ('crash', 'error', r'Segmentation fault|Bus error|Abort trap|Illegal instruction|libc\+\+abi|'
                       r'terminating due to|Assertion failed|assertion .* failed|\bSIG(?:SEGV|BUS|ABRT|ILL|FPE)\b', None),
    ('crash', 'warn', r'\[null-call\] (.*)', None),
    ('crash', 'error', r'\[cellGame\] \*\*\* ContentErrorDialog: (\w+)', 'the game showed its error dialog: {1}'),
    # stalls
    ('stall', 'warn', r'\[cellGcmSys\] ref stuck at 0x[0-9A-F]+ for 2 s: get=0x([0-9A-F]+) put=0x([0-9A-F]+)', None),
    ('stall', 'error', r'\[cellGcmSys\] FIFO (\w+) to unmapped IO', 'FIFO {1} to unmapped IO: the drain stopped'),
    ('stall', 'warn', r'\[cellGcmSys\] resync rescued', None),
    ('stall', 'warn', r'\[SEMA\] acquire off=0x([0-9A-F]+) .* gave up after (\d+) ms',
     'RSX semaphore acquire (offset 0x{1}) gave up after {2} ms: a label nobody wrote (lighting may be stale)'),
    ('stall', 'warn', r'\[stall-dump\] tid (\d+) polled at lr 0x([0-9A-F]+) for (\d+) s',
     'thread {1} polled one usleep site (lr 0x{2}) for {3} s: a stall (PS3_STALL_DUMP wrote the state)'),
    ('stall', 'warn', r'\[watch\] STALLED', None),
    ('stall', 'warn', r'\[lockstep\] STARVATION token held >5s by spu=(\w+)', 'SPU lockstep token held > 5 s by SPU {1}'),
    # shaders and pipelines
    ('shader', 'error', r'\[RSX metal\] (.*): MSL compile failed', 'MSL compile failed: {1}'),
    ('shader', 'error', r'\[RSX metal\] (.*): decompile failed \((-?\d+)\)', 'decompile failed: {1}'),
    ('shader', 'error', r'pipeline state failed', None),
    ('shader', 'error', r'\[rsx engine\] pipeline ([0-9a-f]{16}): (\S+) vp (-?\d+), fp (-?\d+), \d+ constants -> FAILED',
     'pipeline {1} ({2} vp {3}, fp {4}) FAILED: its draws are dropped'),
    ('shader', 'warn', r'\[rsx engine/metal\] pipeline_create: (.*)', None),
    ('shader', 'error', r'\[rsx engine/metal\] (.*?) (?:pipeline|shader compile) (?:\S+ )?failed', '{1} pipeline failed'),
    ('shader', 'error', r'(?:helper|built-in) shader compile failed', None),
    ('shader', 'warn', r'\[RSX fp\] (.*)', 'FP decompiler: {1}'),
    # textures
    ('texture', 'error', r'texture bind FAILED: loc (\d) off 0x[0-9A-F]+ fmt 0x([0-9A-F]{2}) (\d+)x(\d+)',
     'texture bind failed -> black: fmt 0x{2}'),
    ('texture', 'error', r'\[rsx texlog\] h0 <- loc (\d) off 0x[0-9A-F]+ fmt 0x([0-9A-F]{2})',
     'texture creation failed: fmt 0x{2}'),
    ('texture', 'warn', r'\[rsx texlog\] draw into .* unit (\d+) fmt 0x([0-9A-F]{2}) .* SIZE MISMATCH',
     'render-target read with a size mismatch (fmt 0x{2})'),
    # SPU
    ('spu', 'error', r'\[edge-watch\] spu=\w+ r(\d+) changed after (.*?): ',
     'Edge geometry job: r{1} changed after {2} (the rare Edge fault)'),
    ('spu', 'warn', r'\[edge-watch\] (.*inactive.*)', None),
    ('spu', 'error', r'\[SPU\] BRANCH-TO-0 unresolved pc=0x\w+ image=(\d+)', 'SPU branch to 0 (image {1})'),
    ('spu', 'error', r'\[spu_interp\] unimplemented op \'([^\']*)\'', 'SPU interpreter: unimplemented op {1}'),
    ('spu', 'error', r'\[SPU\] tid=0x\w+ stopped .* FAULT', 'SPU thread stopped with a fault'),
    ('spu', 'warn', r'\[POISON-DMA\]', None),
    ('spu', 'info', r'\[spu\] img=(\d+) (?:branched into|interpreting) unlifted LS 0x(\w+)',
     'SPU code not lifted, interpreted: image {1} LS 0x{2}'),
    ('spu', 'info', r'\[spu-smc\] ', None),
    # unimplemented
    ('unimplemented', 'warn', r'\[hle\] (?:UNRESOLVED|unresolved) NID 0x([0-9A-F]{8})', 'unresolved HLE import NID 0x{1}'),
    ('unimplemented', 'warn', r'lv2_syscall (\d+) \(stub\)', 'stub syscall {1}'),
    ('unimplemented', 'warn', r'(?i)\bnot implemented\b|\bunimplemented\b|\bunhandled\b|\(stub\)|\bstubbed\b', None),
    # syscall errors (LV2_ERRDBG)
    ('syscall', 'warn', r'\[lv2err\] syscall (\d+)\(.*\) -> 0x([0-9A-F]{8})', None),
    # file system
    ('fs', 'warn', r"\[fs\] open FAIL '([^']+)'", None),
    ('fs', 'warn', r'\[sys_fs\] open FAILED: (\S+) \(errno=(\d+)', None),
    ('fs', 'info', r"\[fs\] stat '([^']+)' -> ENOENT", None),
    # audio
    ('audio', 'info', r'\[audio-rate\] ', None),
    ('audio', 'warn', r'\[cellAudio\] port (\d+) HAS DATA but never started', 'audio port {1} has data but was never started'),
    ('audio', 'warn', r'\[cellAudio\] .*(?:fail|error|FAIL)', None),
    # draws
    ('draw', 'warn', r'\[rsx engine\] drop #\d+ \(([^)]*)\): prim=(\d+)', 'draw dropped: {1} (prim {2})'),
    ('draw', 'warn', r'\[rsx engine\] inline array dropped', None),
    # perf lines are read by the stats, not grouped
    ('perf', 'info', r'^\[RSX FPS\] |^\[rsx fps\] |^\[rsx present\] ', None),
    # anything else that sounds wrong
    ('other', 'warn', r'(?i)\b(?:error|errors|fail|failed|failure|invalid|unsupported|unknown|unexpected|cannot|'
                      r'could not|missing|timed out|abort(?:ed)?|overflow|corrupt\w*|panic)\b', None),
    ('other', 'info', r'(?i)\bwarn(?:ing)?\b', None),
]
RULES = [(c, s, re.compile(r), t) for c, s, r, t in RULES]

PERF_RSX_FPS = re.compile(r'^\[RSX FPS\] display=([\d.]+) guest=([\d.]+) avg_ms=([\d.]+) p95_ms=([\d.]+) '
                          r'max_ms=([\d.]+)')
PERF_ENG_FPS = re.compile(r'^\[rsx fps\] ([\d.]+) fps \((\d+) frames in (\d+)s: (\d+) world, (\d+) light\); '
                          r'(\d+) presents, (\d+) with no draws')
PERF_PRESENT = re.compile(r'^\[rsx present\] (\d+) presents: in-flight wait ([\d.]+) ms, nextDrawable ([\d.]+) ms, '
                          r'encode ([\d.]+) ms, commit ([\d.]+) ms')
AUDIO_RATE = re.compile(r'\[audio-rate\] ([\d.]+) blocks/s \(real time 187.5\), silent ([\d.]+)%, '
                        r'dropped (\d+) frames/s')
SURF_TRIGGER = re.compile(r'^\[surf-dump\] triggered \(present (\d+)\)')
PASS_FRAME = re.compile(r'^\[pass\] ---- frame (\d+): (\d+) records \((\w+)\)')
TEXLOG_NEW = re.compile(r'^\[rsx texlog\] h(\d+) <- loc \d off 0x[0-9A-F]+ fmt 0x([0-9A-F]{2}) (\d+)x(\d+)')
TDUMP = re.compile(r'^\[tdump\] (.*)')


# ---- known-harmless list ---------------------------------------------------------
def load_known(path):
    out = []
    if not path or not os.path.isfile(path):
        return out
    with open(path, encoding='utf-8') as f:
        for n, line in enumerate(f, 1):
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            parts = [p.strip() for p in line.split(' | ')]
            if len(parts) != 3:
                raise ValueError('%s:%d: want "<category> | <pattern> | <why>"' % (path, n))
            cat, pat, why = parts
            if cat != 'any' and cat not in CATEGORIES:
                raise ValueError('%s:%d: unknown category %r' % (path, n, cat))
            out.append((cat, re.compile(pat), why))
    return out


# ---- time: log offset -> seconds since the session started ---------------------
class TimeMap:
    """Seconds since the start for a byte offset of the log.

    From events.tsv (wall clock, a tick every couple of seconds with the
    log's size) when there is one, else from the monotonic timestamps the
    game prints itself (t= on the frame-dump and fs lines)."""

    def __init__(self):
        self.offs, self.secs = [], []
        self.source = 'none'
        self.start_unix = None

    @classmethod
    def from_ticks(cls, ticks, start_unix):
        m = cls()
        m.source = 'events.tsv'
        m.start_unix = start_unix
        last = -1
        for off, unix in sorted(ticks):
            if off > last:          # a size that did not grow says nothing new
                m.offs.append(off)
                m.secs.append(unix - start_unix)
                last = off
        return m

    @classmethod
    def from_anchors(cls, anchors):
        m = cls()
        if not anchors:
            return m
        m.source = 'log timestamps'
        t0 = anchors[0][1]
        best = -1e18
        for off, t in anchors:
            if t >= best:           # monotonic: drop a stray earlier value
                m.offs.append(off)
                m.secs.append(t - t0)
                best = t
        return m

    def at(self, off):
        if not self.offs:
            return None
        i = bisect.bisect_right(self.offs, off)
        if i == 0:
            return self.secs[0] if self.source == 'events.tsv' else None
        if i == len(self.offs):
            return self.secs[-1]
        o0, o1 = self.offs[i - 1], self.offs[i]
        s0, s1 = self.secs[i - 1], self.secs[i]
        # a tick's size counts what was written up to it: a line past it was
        # written between that tick and the next
        return s1 if self.source == 'events.tsv' else s0 + (s1 - s0) * (off - o0) / max(1, o1 - o0)

    def offset_at(self, secs):
        """The first log offset written at or after `secs` (the inverse)."""
        if not self.offs:
            return None
        i = bisect.bisect_left(self.secs, secs)
        return self.offs[min(i, len(self.offs) - 1)]

    def wall(self, secs):
        if self.start_unix is None or secs is None:
            return None
        return time.strftime('%Y-%m-%d %H:%M:%S', time.gmtime(self.start_unix + secs))


def fmt_t(secs):
    if secs is None:
        return '-'
    secs = int(round(secs))
    h, r = divmod(secs, 3600)
    m, s = divmod(r, 60)
    return '%d:%02d:%02d' % (h, m, s) if h else '%02d:%02d' % (m, s)


# ---- the session around the log --------------------------------------------------
def read_events(path):
    """events.tsv: utc, unix ms, log bytes, event, detail (tab-separated)."""
    ev = []
    if not os.path.isfile(path):
        return ev
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            if line.startswith('#') or not line.strip():
                continue
            p = line.rstrip('\n').split('\t')
            if len(p) < 4:
                continue
            try:
                ev.append({'utc': p[0], 'unix': int(p[1]) / 1000.0, 'log': int(p[2]), 'event': p[3],
                           'detail': p[4] if len(p) > 4 else ''})
            except ValueError:
                continue
    return ev


def read_system(path):
    info, env = OrderedDict(), OrderedDict()
    if not os.path.isfile(path):
        return info, env
    in_env = False
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            line = line.rstrip('\n')
            if line.startswith('environment given to run.sh'):
                in_env = True
                continue
            if in_env:
                k, _, v = line.strip().partition('=')
                if k:
                    env[k] = v.strip("'")
            elif ': ' in line:
                k, _, v = line.partition(': ')
                info[k] = v
    return info, env


def read_manifest(path):
    """shaders/manifest.txt: p=<pipeline> fp=<hash> vp=<hash> targets=<n> guest|built-in."""
    out = {}
    if not os.path.isfile(path):
        return out
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            kv = dict(x.split('=', 1) for x in line.split() if '=' in x)
            if 'p' in kv:
                out[kv['p']] = {'fp': kv.get('fp', ''), 'vp': kv.get('vp', ''), 'kind': line.split()[-1]}
    return out


SHADER_TODO = re.compile(r'TODO[: ]+([^*\n]*)')


def scan_shaders(d):
    """Pipelines whose dumped shaders carry a decompiler TODO."""
    out = []
    if not os.path.isdir(d):
        return out, 0
    man = read_manifest(os.path.join(d, 'manifest.txt'))
    for name in sorted(os.listdir(d)):
        m = re.match(r'p(\d+)_(vs|ps)\.hlsl$', name)
        if not m:
            continue
        try:
            with open(os.path.join(d, name), encoding='utf-8', errors='replace') as f:
                text = f.read()
        except OSError:
            continue
        todos = Counter(t.strip() for t in SHADER_TODO.findall(text))
        if todos:
            p = m.group(1)
            e = man.get(p, {})
            out.append({'pipeline': int(p), 'stage': 'fragment' if m.group(2) == 'ps' else 'vertex',
                        'fp': e.get('fp', ''), 'vp': e.get('vp', ''),
                        'todos': [{'what': k, 'count': v} for k, v in todos.most_common(8)]})
    out.sort(key=lambda x: x['pipeline'])
    return out, len(man)


def read_crash(path):
    """The macOS crash report the launcher copied (crash.ips): exception and top frames."""
    if not os.path.isfile(path):
        return None
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            text = f.read()
    except OSError:
        return None
    out = {'file': os.path.basename(path)}
    head, _, body = text.partition('\n')
    try:
        h = json.loads(head)
        out['app'] = h.get('app_name', '')
        out['time'] = h.get('timestamp', '')
    except ValueError:
        pass
    try:
        b = json.loads(body)
        exc = b.get('exception', {})
        out['exception'] = ' '.join(x for x in (exc.get('type', ''), exc.get('signal', ''), exc.get('subtype', '')) if x)
        out['termination'] = (b.get('termination') or {}).get('indicator', '')
        ft = b.get('faultingThread')
        threads = b.get('threads') or []
        if isinstance(ft, int) and ft < len(threads):
            frames = threads[ft].get('frames') or []
            imgs = b.get('usedImages') or []
            names = []
            for fr in frames[:12]:
                img = imgs[fr['imageIndex']]['name'] if fr.get('imageIndex', -1) < len(imgs) else '?'
                names.append('%s %s' % (img, fr.get('symbol', '+0x%x' % fr.get('imageOffset', 0))))
            out['thread'] = threads[ft].get('name', '') or ('thread %d' % ft)
            out['frames'] = names
    except (ValueError, KeyError, TypeError, IndexError):
        m = re.search(r'Exception Type:\s*(.*)', text)
        if m:
            out['exception'] = m.group(1).strip()
    return out


# ---- the pass over the log -------------------------------------------------------
class Group:
    __slots__ = ('cat', 'sev', 'title', 'count', 'first', 'last', 'first_line', 'last_line', 'example', 'known')

    def __init__(self, cat, sev, title, off, n, example, known):
        self.cat, self.sev, self.title = cat, sev, title
        self.count = 0
        self.first = self.last = off
        self.first_line = self.last_line = n
        self.example = example
        self.known = known


def title_for(rule_title, m, text):
    if not rule_title:
        return normalize(text)
    try:
        return redact(rule_title.format(m.group(0), *m.groups()))
    except (IndexError, KeyError):
        return normalize(text)


class Triage:
    MAX_PROBLEM_LINES = 20000

    def __init__(self, known):
        self.known = known
        self.groups = OrderedDict()
        self.tags = Counter()
        self.lines = 0
        self.bytes = 0
        self.anchors = []                 # (offset, monotonic seconds)
        self.frame_anchors = []           # (offset, present number, monotonic seconds)
        self.problems = []                # (offset, line no, category, severity, text) for the captures
        self.rsx_fps, self.eng_fps, self.present, self.audio = [], [], [], []
        self.triggers = []                # (offset, line no, present)
        self.pass_frames = []             # (offset, frame)
        self.tex_formats = Counter()
        self.tex_created = 0
        self.nids = Counter()
        self.stalls = []
        self.syscalls = OrderedDict()
        self.fs = OrderedDict()
        self.tdump = []                   # the last complete thread dump
        self._tdump_cur = []
        self._tdump_off = 0
        self.tdump_off = None

    def known_reason(self, cat, text):
        for c, rx, why in self.known:
            if (c == cat or c == 'any') and rx.search(text):
                return why
        return None

    def add(self, cat, sev, title, off, n, text):
        reason = self.known_reason(cat, text)
        key = (cat, title)
        g = self.groups.get(key)
        if g is None:
            g = self.groups[key] = Group(cat, sev, title, off, n, redact(text[:300]), reason)
        g.count += 1
        g.last, g.last_line = off, n
        if reason is None and sev != 'info' and len(self.problems) < self.MAX_PROBLEM_LINES:
            self.problems.append((off, n, cat, sev, redact(text[:240])))
        return reason

    def feed(self, path):
        with open(path, 'rb') as f:
            off = 0
            n = 0
            for raw in f:
                n += 1
                line = raw.decode('utf-8', 'replace').rstrip('\r\n')
                if '[' in line[1:]:
                    parts = SPLIT_SPACED.split(line)
                    if len(parts) == 1:
                        parts = SPLIT_GLUED.split(line)
                    else:
                        parts = [q for p in parts for q in SPLIT_GLUED.split(p)]
                    sub = off
                    for p in parts:
                        self.line(p.strip(), sub, n)
                        sub += len(p.encode('utf-8', 'replace'))
                else:
                    self.line(line, off, n)
                off += len(raw)
            self.lines, self.bytes = n, off
        if self._tdump_cur:
            self.tdump, self.tdump_off = self._tdump_cur, self._tdump_off

    def line(self, s, off, n):
        if not s:
            return
        tag = None
        if s[0] == '[':
            m = TAG_RE.match(s)
            if m:
                tag = m.group(1)
                if tag.startswith('THREAD '):
                    tag = 'THREAD n'
                elif tag.startswith('WAIT tid='):
                    tag = 'WAIT'
        self.tags[tag or '(untagged)'] += 1
        a = TIME_ANCHOR.search(s) if ' t=' in s else None
        if a:
            t = float(a.group(1))
            self.anchors.append((off, t))
            m = CAPTURED_FRAME.match(s)
            if m:
                self.frame_anchors.append((off, int(m.group(1)), t))
        if tag in NOISE_TAGS:
            if tag == 'tdump':
                return self.tdump_line(s, off)
            if tag == 'surf-dump':
                m = SURF_TRIGGER.match(s)
                if m:
                    self.triggers.append((off, n, int(m.group(1))))
                return
            if tag == 'pass':
                if s.startswith('[pass] ----'):
                    m = PASS_FRAME.match(s)
                    if m:
                        self.pass_frames.append((off, int(m.group(1))))
                return
            if not READ_ANYWAY.search(s):
                return
        if tag == 'rsx texlog':
            m = TEXLOG_NEW.match(s)
            if m:
                self.tex_created += 1
                self.tex_formats[m.group(2)] += 1
                if m.group(1) != '0':
                    return
            elif 'SIZE MISMATCH' not in s:
                return
        for cat, sev, rx, rtitle in RULES:
            m = rx.search(s)
            if not m:
                continue
            if cat == 'perf':
                return self.perf_line(s, off)
            if cat == 'audio' and s.startswith('[audio-rate]'):
                return self.audio_line(s, off, n)
            title = title_for(rtitle, m, s)
            if cat == 'unimplemented' and m.re.pattern.startswith(r'\[hle\]'):
                self.nids[m.group(1)] += 1
            if cat == 'syscall':
                title = self.syscall_title(m)
            if cat == 'fs':
                title = self.fs_title(m, s)
            if cat == 'stall' and 'ref stuck' in s:
                st = self.stall_line(s, off, n)
                if st:
                    title = ('GPU stalled 2 s: the FIFO is empty (get == put), the game stopped feeding it'
                             if st['drained'] else 'GPU stalled 2 s: the FIFO walker is stuck at get')
            self.add(cat, sev, title, off, n, s)
            return

    def tdump_line(self, s, off):
        if '---- guest threads ----' in s:
            if self._tdump_cur:
                self.tdump, self.tdump_off = self._tdump_cur, self._tdump_off
            self._tdump_cur, self._tdump_off = [], off
        else:
            m = TDUMP.match(s)
            if m and len(self._tdump_cur) < 80:
                self._tdump_cur.append(m.group(1))

    def perf_line(self, s, off):
        m = PERF_RSX_FPS.match(s)
        if m:
            self.rsx_fps.append((off,) + tuple(float(x) for x in m.groups()))
            return
        m = PERF_ENG_FPS.match(s)
        if m:
            self.eng_fps.append((off, float(m.group(1)), int(m.group(4)), int(m.group(5)), int(m.group(6))))
            return
        m = PERF_PRESENT.match(s)
        if m:
            self.present.append((off,) + tuple(float(x) for x in m.groups()))

    def audio_line(self, s, off, n):
        m = AUDIO_RATE.search(s)
        if not m:
            return
        rate, silent, dropped = float(m.group(1)), float(m.group(2)), int(m.group(3))
        self.audio.append((off, rate, silent, dropped))
        if dropped > 0:
            self.add('audio', 'warn', 'audio frames dropped (the output device had no room)', off, n, s)
        if rate < 175.0:
            self.add('audio', 'warn', 'audio mixer behind real time (< 175 of 187.5 blocks/s): crackle or slow sound',
                     off, n, s)

    def syscall_title(self, m):
        num, err = int(m.group(1)), int(m.group(2), 16)
        name = SYSCALLS.get(num, '')
        title = 'syscall %d%s -> 0x%08X %s' % (num, ' (%s)' % name if name else '', err, ERRORS.get(err, ''))
        return title.rstrip()

    def fs_title(self, m, s):
        p = m.group(1)
        if s.startswith('[sys_fs]'):
            p = vfs_path(p)
        return 'cannot open %s' % redact(p) if 'stat' not in s[:10] else 'stat: no %s' % redact(p)

    def stall_line(self, s, off, n):
        m = re.search(r'ref stuck at 0x([0-9A-F]+) for 2 s: get=0x([0-9A-F]+) put=0x([0-9A-F]+) word@get=0x([0-9A-F]+) '
                      r'.*labels 134/135/136/137 = (\S+)', s)
        if m:
            self.stalls.append({'offset': off, 'line': n, 'ref': '0x' + m.group(1), 'get': '0x' + m.group(2),
                                'put': '0x' + m.group(3), 'word': '0x' + m.group(4), 'labels': m.group(5),
                                'drained': m.group(2) == m.group(3)})
            return self.stalls[-1]
        return None


# ---- the report ------------------------------------------------------------------
def pct(xs, q):
    if not xs:
        return None
    xs = sorted(xs)
    i = min(len(xs) - 1, max(0, int(round(q * (len(xs) - 1)))))
    return xs[i]


def rnd(x, d=1):
    return None if x is None else round(x, d)


def perf_stats(tr, tm):
    out = OrderedDict()
    if tr.rsx_fps:
        guest = [r[2] for r in tr.rsx_fps if r[2] > 0]
        disp = [r[1] for r in tr.rsx_fps]
        # A hidden or covered window presents nothing (display 0) while the
        # game runs on: the "max frame" of that second and the next is the
        # gap until a drawable came back, not a stall of the game.
        shown = [r for i, r in enumerate(tr.rsx_fps)
                 if r[1] > 0 and (i == 0 or tr.rsx_fps[i - 1][1] > 0)]
        worst = [r[5] for r in shown] or [0.0]
        hitches = [{'t': rnd(tm.at(r[0])), 'max_ms': r[5], 'guest_fps': r[2]}
                   for r in shown if r[5] >= 250.0]
        out['window'] = OrderedDict([
            ('samples', len(tr.rsx_fps)),
            ('hidden_seconds', len(tr.rsx_fps) - len(shown)),
            ('guest_fps_median', rnd(statistics.median(guest)) if guest else None),
            ('guest_fps_p5', rnd(pct(guest, 0.05))),
            ('display_fps_median', rnd(statistics.median(disp))),
            ('p95_ms_median', rnd(statistics.median([r[4] for r in tr.rsx_fps]))),
            ('max_ms', rnd(max(worst))),
            ('hitches_250ms', len(hitches)),
            ('worst', sorted(hitches, key=lambda h: -h['max_ms'])[:10]),
        ])
    if tr.eng_fps:
        world = [r for r in tr.eng_fps if r[2] > r[3]]      # 10 s windows that were mostly 3D frames
        fps = [r[1] for r in (world or tr.eng_fps)]
        slow = [{'t': rnd(tm.at(r[0])), 'fps': r[1]} for r in tr.eng_fps if world and r in world and r[1] < 0.6 * statistics.median(fps)]
        out['engine'] = OrderedDict([
            ('windows', len(tr.eng_fps)),
            ('world_windows', len(world)),
            ('fps_median', rnd(statistics.median(fps))),
            ('fps_min', rnd(min(fps))),
            ('fps_max', rnd(max(fps))),
            ('slow_windows', slow[:10]),
        ])
    if tr.present:
        tot = [sum(r[i] for r in tr.present) for i in range(1, 6)]
        n = tot[0] or 1
        out['present_ms_per_frame'] = OrderedDict([
            ('in_flight_wait', rnd(tot[1] / n, 2)), ('next_drawable', rnd(tot[2] / n, 2)),
            ('encode', rnd(tot[3] / n, 2)), ('commit', rnd(tot[4] / n, 2))])
    fa = tr.frame_anchors
    if len(fa) >= 3:
        rates, gaps = [], []
        for (o0, f0, t0), (o1, f1, t1) in zip(fa, fa[1:]):
            dt = t1 - t0
            if dt > 0 and f1 > f0:
                rates.append((f1 - f0) / dt)
                if dt >= 2.0 * (f1 - f0) / 30.0 and dt >= 6.0:
                    gaps.append({'t': rnd(tm.at(o1)), 'presents': f1 - f0, 'seconds': rnd(dt)})
        if rates:
            out['presents'] = OrderedDict([
                ('frames', fa[-1][1]), ('seconds', rnd(fa[-1][2] - fa[0][2])),
                ('fps_median', rnd(statistics.median(rates))), ('fps_p5', rnd(pct(rates, 0.05))),
                ('fps_max', rnd(max(rates))),
                ('slow_spans', sorted(gaps, key=lambda g: -g['seconds'])[:10])])
    return out


def audio_stats(tr):
    if not tr.audio:
        return None
    rates = [a[1] for a in tr.audio]
    return OrderedDict([
        ('samples', len(tr.audio)), ('blocks_per_s_median', rnd(statistics.median(rates))),
        ('blocks_per_s_min', rnd(min(rates))), ('silent_pct_median', rnd(statistics.median(a[2] for a in tr.audio))),
        ('dropped_frames_total', sum(a[3] for a in tr.audio) * 5),     # per second over 5 s windows
        ('windows_behind', sum(1 for r in rates if r < 175.0)),
    ])


def group_json(g, tm):
    return OrderedDict([
        ('category', g.cat), ('severity', g.sev), ('title', g.title), ('count', g.count),
        ('first', rnd(tm.at(g.first))), ('last', rnd(tm.at(g.last))),
        ('first_line', g.first_line), ('last_line', g.last_line), ('example', g.example),
        ('known', g.known)])


def context_lines(path, off, before=8):
    """The last `before` lines of the log that are not trace noise, up to `off`."""
    start = max(0, off - 96 * 1024)
    try:
        with open(path, 'rb') as f:
            f.seek(start)
            data = f.read(off - start)
    except OSError:
        return []
    lines = data.decode('utf-8', 'replace').split('\n')
    if start:
        lines = lines[1:]
    out = []
    for ln in reversed(lines):
        ln = ln.strip()
        m = TAG_RE.match(ln) if ln.startswith('[') else None
        tag = m.group(1) if m else None
        if not ln or tag in NOISE_TAGS or tag == 'rsx texlog' or not tag:
            continue
        out.append(redact(ln[:240]))
        if len(out) >= before:
            break
    return list(reversed(out))


def build_captures(tr, tm, events, log_path):
    caps = []
    evcaps = [e for e in events if e['event'] == 'capture']
    trig = list(tr.triggers)
    if evcaps:
        for e in evcaps:
            d = dict(x.split('=', 1) for x in e['detail'].split() if '=' in x)
            c = OrderedDict([('n', int(d.get('n', len(caps) + 1))), ('utc', e['utc']), ('offset', e['log']),
                             ('t', rnd(e['unix'] - tm.start_unix) if tm.start_unix is not None else None),
                             ('frame_hint', d.get('frame')), ('shot', d.get('shot'))])
            nxt = [x for x in trig if x[0] >= e['log']]
            if nxt:
                c['taken_line'] = nxt[0][1]
                c['surf_present'] = nxt[0][2]
                c['taken_t'] = rnd(tm.at(nxt[0][0]))
                trig.remove(nxt[0])
            caps.append(c)
    # Dumps no launcher capture explains: the in-game capture key (F12), or a
    # session without events.tsv.
    for off, n, present in trig:
        caps.append(OrderedDict([('n', len(caps) + 1), ('utc', tm.wall(tm.at(off))), ('offset', off),
                                 ('t', rnd(tm.at(off))), ('taken_line', n), ('surf_present', present),
                                 ('source', 'in-game key' if evcaps else 'log')]))
    for c in caps:
        off = c['offset']
        pf = [f for o, f in tr.pass_frames if o >= off]
        if pf:
            c['pass_frame'] = pf[0]
        fa = [f for o, f, _ in tr.frame_anchors if o <= off]
        if fa:
            c['present_before'] = fa[-1]
        t = c.get('t')
        if t is not None and tm.offs:
            lo, hi = tm.offset_at(t - 10.0), tm.offset_at(t + 3.0)
            if hi is not None and hi <= off:
                hi = off + 256 * 1024
        else:
            lo, hi = off - 1024 * 1024, off + 256 * 1024
        near = [p for p in tr.problems if lo <= p[0] <= hi]
        c['problems_near'] = [OrderedDict([('t', rnd(tm.at(p[0]))), ('line', p[1]), ('category', p[2]),
                                           ('severity', p[3]), ('text', p[4])]) for p in near[:20]]
        c['problems_near_total'] = len(near)
        c['log_before'] = context_lines(log_path, off)
    return caps


def triage(target, out_dir=None, known_path=KNOWN_FILE):
    target = os.path.abspath(target)
    if os.path.isdir(target):
        session, log = target, os.path.join(target, 'game.log')
        stem = 'triage'
    else:
        session, log = os.path.dirname(target), target
        name = os.path.basename(target)
        stem = 'triage' if name == 'game.log' else os.path.splitext(name)[0] + '.triage'
    if not os.path.isfile(log):
        raise FileNotFoundError('%s: no game log' % log)
    out_dir = os.path.abspath(out_dir or session)
    side = os.path.splitext(log)[0]      # out/<name>/ next to out/<name>.log (capture.sh, play_dump.sh)
    is_session = stem == 'triage'
    events = read_events(os.path.join(session, 'events.tsv')) if is_session else []
    sysinfo, env = read_system(os.path.join(session, 'system.txt')) if is_session else ({}, {})
    shaders_dir = os.path.join(session, 'shaders') if is_session else os.path.join(side, 'shaders')
    crash = read_crash(os.path.join(session, 'crash.ips')) if is_session else None

    tr = Triage(load_known(known_path))
    t0 = time.time()
    tr.feed(log)
    took = time.time() - t0

    start = [e for e in events if e['event'] == 'start']
    ticks = [(e['log'], e['unix']) for e in events]
    if start and ticks:
        tm = TimeMap.from_ticks(ticks, start[0]['unix'])
    else:
        tm = TimeMap.from_anchors(tr.anchors)

    # how the session ended
    exit_ev = [e for e in events if e['event'] == 'exit']
    stop_ev = [e for e in events if e['event'] == 'stop']
    ending = OrderedDict()
    if exit_ev:
        d = dict(x.split('=', 1) for x in exit_ev[-1]['detail'].split() if '=' in x)
        ending['code'] = d.get('code')
        ending['signal'] = d.get('signal')
        ending['stopped_by_player'] = bool(stop_ev)
        ending['t'] = rnd(exit_ev[-1]['unix'] - tm.start_unix) if tm.start_unix is not None else None
        sig = d.get('signal')
        if sig and not stop_ev:
            tr.add('crash', 'error', 'the game was killed by signal %s' % sig, tr.bytes, tr.lines,
                   'exit: signal %s' % sig)
        elif d.get('code') not in (None, '0') and not stop_ev:
            tr.add('crash', 'error', 'the game exited with code %s' % d.get('code'), tr.bytes, tr.lines,
                   'exit: code %s' % d.get('code'))
    elif events:
        ending['note'] = 'no exit recorded (the launcher closed first, or the game is still running)'
    # a hang: the game kept running (and logging) but stopped presenting
    if tr.frame_anchors:
        last_off, last_frame, _ = tr.frame_anchors[-1]
        t_last = tm.at(last_off)
        if tm.source == 'events.tsv' and t_last is not None:
            t_end = (exit_ev[-1]['unix'] - tm.start_unix) if exit_ev else (tm.secs[-1] if tm.secs else None)
            if t_end is not None and t_end - t_last > 20:
                tr.add('crash', 'error', 'no frame presented in the last %d s of the session: a hang' % (t_end - t_last),
                       last_off, 0, 'last frame dump: present %d at %s' % (last_frame, fmt_t(t_last)))
        elif tr.bytes - last_off > max(1 << 20, tr.bytes // 5):
            tr.add('crash', 'warn', 'no frame presented in the last %d%% of the log: a hang?'
                   % (100 * (tr.bytes - last_off) // tr.bytes), last_off, 0,
                   'last frame dump: present %d' % last_frame)
    if crash:
        tr.add('crash', 'error', 'macOS crash report: %s' % (crash.get('exception') or 'crash'), tr.bytes, tr.lines,
               json.dumps(crash)[:300])

    shader_todos, n_pipelines = scan_shaders(shaders_dir)
    for s in shader_todos:
        for td in s['todos']:
            g = tr.groups.setdefault(('shader', 'decompiler TODO: %s' % td['what']),
                                     Group('shader', 'warn', 'decompiler TODO: %s' % normalize(td['what']), 0, 0,
                                           'p%d_%s.hlsl (fp %s vp %s)' % (s['pipeline'], 'ps' if s['stage'] == 'fragment' else 'vs',
                                                                          s['fp'], s['vp']), None))
            g.count += 1

    groups = list(tr.groups.values())
    problems = [g for g in groups if not g.known and g.sev != 'info']
    known = [g for g in groups if g.known]
    problems.sort(key=lambda g: (-CAT_RANK.get(g.cat, 1) * SEV_RANK[g.sev], -g.count))
    top = problems[:15]

    by_cat = OrderedDict()
    for c in CATEGORIES:
        gs = [g for g in groups if g.cat == c]
        by_cat[c] = OrderedDict([
            ('errors', sum(g.count for g in gs if not g.known and g.sev == 'error')),
            ('warnings', sum(g.count for g in gs if not g.known and g.sev == 'warn')),
            ('info', sum(g.count for g in gs if not g.known and g.sev == 'info')),
            ('expected', sum(g.count for g in gs if g.known)),
            ('groups', len(gs))])

    perf = perf_stats(tr, tm)
    audio = audio_stats(tr)
    caps = build_captures(tr, tm, events, log)
    duration = None
    if exit_ev and tm.start_unix is not None:
        duration = exit_ev[-1]['unix'] - tm.start_unix
    elif tm.secs:
        duration = tm.secs[-1]

    # stalls, with the time and whether they are the expected loading wait
    for s in tr.stalls:
        s['t'] = rnd(tm.at(s['offset']))
        s['expected'] = tr.known_reason('stall', 'ref stuck at %s for 2 s: get=%s put=%s labels 134/135/136/137 = %s'
                                        % (s['ref'], s['get'], s['put'], s['labels'])) is not None

    tex_fail = OrderedDict()
    for g in groups:
        if g.cat == 'texture':
            m = re.search(r'fmt 0x([0-9A-F]{2})', g.title)
            if m:
                tex_fail[m.group(1)] = tex_fail.get(m.group(1), 0) + g.count

    data = OrderedDict([
        ('schema', SCHEMA), ('tool', 'log_triage.py'),
        ('generated', time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())),
        ('log', redact(log)), ('log_bytes', tr.bytes), ('lines', tr.lines), ('parse_seconds', rnd(took, 2)),
        ('time_source', tm.source),
        ('session', OrderedDict([
            ('id', os.path.basename(session) if is_session else os.path.splitext(os.path.basename(log))[0]),
            ('debug', bool(events)), ('start_utc', start[0]['utc'] if start else sysinfo.get('report')),
            ('duration_s', rnd(duration)), ('end', ending), ('system', sysinfo), ('env', env),
            ('notes', _read(os.path.join(session, 'notes.txt')) if is_session else None)])),
        ('summary', OrderedDict([
            ('errors', sum(v['errors'] for v in by_cat.values())),
            ('warnings', sum(v['warnings'] for v in by_cat.values())),
            ('expected', sum(v['expected'] for v in by_cat.values())),
            ('captures', len(caps)), ('categories', by_cat)])),
        ('top', [group_json(g, tm) for g in top]),
        ('captures', caps),
        ('unimplemented_nids', [OrderedDict([('nid', '0x' + k), ('count', v)]) for k, v in tr.nids.most_common()]),
        ('stalls', tr.stalls),
        ('shaders', OrderedDict([('pipelines_dumped', n_pipelines), ('with_todos', shader_todos)])),
        ('textures', OrderedDict([('created', tr.tex_created), ('formats', OrderedDict(tr.tex_formats.most_common())),
                                  ('failed_by_format', tex_fail)])),
        ('perf', perf),
        ('audio', audio),
        ('crash_report', crash),
        ('last_thread_dump', OrderedDict([('t', rnd(tm.at(tr.tdump_off)) if tr.tdump_off is not None else None),
                                          ('threads', [redact(x) for x in tr.tdump])]) if tr.tdump else None),
        ('groups', [group_json(g, tm) for g in sorted(groups, key=lambda g: (list(CATEGORIES).index(g.cat), -g.count))]),
        ('tags', OrderedDict(tr.tags.most_common(25))),
    ])
    md = render_md(data, tm)
    os.makedirs(out_dir, exist_ok=True)
    md_path = os.path.join(out_dir, stem + '.md')
    js_path = os.path.join(out_dir, stem + '.json')
    with open(md_path, 'w', encoding='utf-8') as f:
        f.write(md)
    with open(js_path, 'w', encoding='utf-8') as f:
        json.dump(data, f, indent=1)
        f.write('\n')
    return data, md_path, js_path


def _read(path):
    try:
        with open(path, encoding='utf-8', errors='replace') as f:
            return f.read().strip()[:2000] or None
    except OSError:
        return None


def esc(s):
    return str(s).replace('|', '\\|').replace('\n', ' ')


def render_md(d, tm):
    s = d['session']
    L = []
    L.append('# Triage: %s' % s['id'])
    L.append('')
    bits = ['%s (%.1f MB, %s lines)' % (os.path.basename(d['log']), d['log_bytes'] / 1e6, format(d['lines'], ','))]
    if s.get('start_utc'):
        bits.append('started %s UTC' % s['start_utc'])
    if s.get('duration_s') is not None:
        bits.append('%s long' % fmt_t(s['duration_s']))
    end = s.get('end') or {}
    if end.get('signal'):
        bits.append('ended by signal %s%s' % (end['signal'], ' (Stop pressed)' if end.get('stopped_by_player') else ''))
    elif end.get('code') is not None:
        bits.append('exit code %s%s' % (end['code'], ' (Stop pressed)' if end.get('stopped_by_player') else ''))
    L.append(' · '.join(bits))
    L.append('')
    L.append('Times are min:sec since the session started (%s). Generated by `infamous2-lle/tools/log_triage.py`; '
             'lines in `log_triage_known.txt` count as expected.' %
             {'events.tsv': "the launcher's events.tsv",
              'log timestamps': "the game's own timestamps, from the first one; the boot before it shows '-'",
              'none': 'no timestamps in this log'}[d['time_source']])
    if s.get('notes'):
        L += ['', '**Player notes:** ' + esc(s['notes'])]
    L.append('')

    sm = d['summary']
    L.append('## Summary')
    L.append('')
    L.append('**%d error(s), %d warning(s)** in problem groups; %d expected line(s); %d capture(s).'
             % (sm['errors'], sm['warnings'], sm['expected'], sm['captures']))
    L.append('')
    L.append('| Category | Errors | Warnings | Info | Expected |')
    L.append('|---|---:|---:|---:|---:|')
    for c, v in sm['categories'].items():
        if v['groups']:
            L.append('| %s | %d | %d | %d | %d |' % (CATEGORIES[c], v['errors'], v['warnings'], v['info'], v['expected']))
    L.append('')

    L.append('## Top issues')
    L.append('')
    if not d['top']:
        L.append('Nothing beyond the expected lines.')
    for i, g in enumerate(d['top'], 1):
        L.append('%d. **[%s] %s** — %d×, first %s, last %s (line %d)' % (
            i, g['category'], esc(g['title']), g['count'], fmt_t(g['first']), fmt_t(g['last']), g['first_line']))
        if g['example'] and g['example'] != g['title']:
            L.append('   `%s`' % g['example'][:200].replace('`', "'"))
    L.append('')

    if d['captures']:
        L.append('## Captures')
        L.append('')
        for c in d['captures']:
            head = '### Capture %d at %s' % (c['n'], fmt_t(c.get('t')))
            if c.get('utc'):
                head += ' (%s UTC)' % c['utc']
            L.append(head)
            L.append('')
            facts = []
            if c.get('present_before') is not None:
                facts.append('present ≈ %d (last frame-dump stamp before the press)' % c['present_before'])
            if c.get('frame_hint'):
                facts.append('launcher saw frame %s' % c['frame_hint'])
            if c.get('surf_present') is not None:
                facts.append('surfaces `surf/f%d_h*.ppm`, taken at line %d (%s)' % (
                    c['surf_present'], c['taken_line'], fmt_t(c.get('taken_t', c.get('t')))))
            elif 'taken_line' not in c:
                facts.append('the game never took it (no present after the press)')
            if c.get('pass_frame') is not None:
                facts.append('pass log frame %d' % c['pass_frame'])
            if c.get('shot') and c['shot'] != '-':
                facts.append('last whole frame `%s`' % c['shot'])
            for f in facts:
                L.append('- ' + f)
            if c['problems_near']:
                L.append('- problems from 10 s before to 3 s after (%d):' % c['problems_near_total'])
                for p in c['problems_near'][:10]:
                    L.append('  - %s [%s] `%s`' % (fmt_t(p['t']), p['category'], p['text'][:180].replace('`', "'")))
            else:
                L.append('- no problem lines from 10 s before to 3 s after')
            if c['log_before']:
                L.append('- log just before:')
                L.append('')
                L.append('  ```')
                L += ['  ' + x for x in c['log_before']]
                L.append('  ```')
            L.append('')

    groups = d['groups']
    for c, title in CATEGORIES.items():
        gs = [g for g in groups if g['category'] == c and not g['known']]
        extra = section_extra(c, d)
        if not gs and not extra:
            continue
        L.append('## ' + title)
        L.append('')
        L += extra
        if gs:
            L.append('| Count | First | Last | Severity | Message |')
            L.append('|---:|---|---|---|---|')
            for g in gs[:60]:
                L.append('| %d | %s | %s | %s | %s |' % (g['count'], fmt_t(g['first']), fmt_t(g['last']), g['severity'],
                                                       esc(g['title'])[:200]))
            if len(gs) > 60:
                L.append('')
                L.append('… %d more groups in triage.json.' % (len(gs) - 60))
        L.append('')

    kn = [g for g in groups if g['known']]
    if kn:
        L.append('## Expected (known harmless)')
        L.append('')
        L.append('| Count | Category | Message | Why |')
        L.append('|---:|---|---|---|')
        for g in kn:
            L.append('| %d | %s | %s | %s |' % (g['count'], g['category'], esc(g['title'])[:120], esc(g['known'])))
        L.append('')

    L.append('## Log volume by tag')
    L.append('')
    L.append(', '.join('`[%s]` %s' % (k, format(v, ',')) for k, v in list(d['tags'].items())[:15]))
    L.append('')
    if s.get('env'):
        dbg = [k for k in s['env'] if k in DEBUG_KNOBS]
        if dbg:
            L.append('Debug knobs on: ' + ', '.join('`%s=%s`' % (k, s['env'][k]) for k in dbg))
            L.append('')
    return '\n'.join(L)


DEBUG_KNOBS = ('PS3RECOMP_ENG_FPSLOG', 'PS3_EDGE_WATCH', 'LV2_ERRDBG', 'AUDIO_RATE', 'PS3RECOMP_ENG_DROPLOG',
               'PS3_THREAD_DUMP', 'PS3RECOMP_ENG_TEXLOG', 'PS3RECOMP_METAL_FPS')


def section_extra(c, d):
    L = []
    if c == 'crash':
        cr = d.get('crash_report')
        if cr:
            L.append('macOS crash report `%s`: %s%s' % (cr.get('file'), cr.get('exception', '?'),
                                                       ', thread %s' % cr['thread'] if cr.get('thread') else ''))
            for fr in cr.get('frames', [])[:10]:
                L.append('    ' + fr)
            L.append('')
        end = d['session'].get('end') or {}
        if end.get('note'):
            L += [end['note'], '']
    elif c == 'stall':
        st = [s for s in d['stalls'] if not s['expected']]
        if st:
            L.append('| Time | Line | ref | get | put | Labels 134-137 | |')
            L.append('|---|---:|---|---|---|---|---|')
            for s in st[:20]:
                L.append('| %s | %d | %s | %s | %s | %s | %s |' % (
                    fmt_t(s['t']), s['line'], s['ref'], s['get'], s['put'], s['labels'],
                    'FIFO drained (get == put): the PPU stopped feeding it' if s['drained'] else 'walker stuck at get'))
            L.append('')
        td = d.get('last_thread_dump')
        if td and (st or (d['session'].get('end') or {}).get('signal')):
            L.append('Last guest thread dump (%s):' % fmt_t(td['t']))
            L.append('')
            L.append('```')
            L += td['threads'][:40]
            L.append('```')
            L.append('')
    elif c == 'shader':
        sh = d['shaders']
        if sh['pipelines_dumped']:
            L.append('%d pipelines dumped (shaders/manifest.txt).' % sh['pipelines_dumped'])
        for s in sh['with_todos'][:20]:
            L.append('- p%d %s shader (fp `%s`, vp `%s`): %s' % (
                s['pipeline'], s['stage'], s['fp'] or '?', s['vp'] or '?',
                '; '.join('%s ×%d' % (t['what'], t['count']) for t in s['todos'])))
        if L:
            L.append('')
    elif c == 'texture':
        tx = d['textures']
        if tx['created']:
            L.append('%d textures created; formats: %s.' % (
                tx['created'], ', '.join('0x%s ×%d' % kv for kv in list(tx['formats'].items())[:12])))
        if tx['failed_by_format']:
            L.append('Failures by format: %s.' % ', '.join('0x%s ×%d' % kv for kv in tx['failed_by_format'].items()))
        if L:
            L.append('')
    elif c == 'unimplemented' and d['unimplemented_nids']:
        L.append('Unresolved NIDs: ' + ', '.join('`%s` ×%d' % (n['nid'], n['count']) for n in d['unimplemented_nids']))
        L.append('')
    elif c == 'audio' and d['audio']:
        a = d['audio']
        L.append('Mixer: median %s blocks/s (real time 187.5), lowest %s, %d of %d 5-s windows behind; '
                 'median silent %s%%; ~%d frames dropped.' % (
                     a['blocks_per_s_median'], a['blocks_per_s_min'], a['windows_behind'], a['samples'],
                     a['silent_pct_median'], a['dropped_frames_total']))
        L.append('')
    elif c == 'perf' and d['perf']:
        p = d['perf']
        if 'window' in p:
            w = p['window']
            L.append('Window ([RSX FPS], once a second): guest median %s fps, 5th percentile %s; display median %s; '
                     'frame time p95 median %s ms, worst %s ms; %d second(s) with a frame over 250 ms.' % (
                         w['guest_fps_median'], w['guest_fps_p5'], w['display_fps_median'], w['p95_ms_median'],
                         w['max_ms'], w['hitches_250ms']))
            for h in w['worst'][:5]:
                L.append('- %s: a %s ms frame (guest %s fps)' % (fmt_t(h['t']), h['max_ms'], h['guest_fps']))
        if 'engine' in p:
            e = p['engine']
            L.append('Engine ([rsx fps], per 10 s, %d world windows): median %s fps, min %s, max %s.' % (
                e['world_windows'], e['fps_median'], e['fps_min'], e['fps_max']))
            for w in e['slow_windows'][:5]:
                L.append('- %s: %s fps' % (fmt_t(w['t']), w['fps']))
        if 'present_ms_per_frame' in p:
            q = p['present_ms_per_frame']
            L.append('Present, ms per frame: in-flight wait %s, nextDrawable %s, encode %s, commit %s.' % (
                q['in_flight_wait'], q['next_drawable'], q['encode'], q['commit']))
        if 'presents' in p:
            q = p['presents']
            L.append('Presents (frame-dump stamps every 120): %d in %s s, median %s fps, 5th percentile %s.' % (
                q['frames'], q['seconds'], q['fps_median'], q['fps_p5']))
            for g in q['slow_spans'][:5]:
                L.append('- %s: %d presents took %s s' % (fmt_t(g['t']), g['presents'], g['seconds']))
        L.append('')
    return L


# ---- self-test -------------------------------------------------------------------
SAMPLE_LOG = r"""=== ps3recomp game runner ===
[boot] VFS root: /tmp/vfs
[ppu] lv2_syscall 871 (stub)
[hle] UNRESOLVED NID 0x1A2B3C4D -> faking CELL_OK (unregistered import; PS3_HLE_UNRESOLVED=fail to find load-bearing ones)
[fs] open FAIL '/dev_hdd0/game/NPUA80638/USRDIR/patch/psarc/patch1.psarc_s' -> '/tmp/vfs/x' (flags=0x0)
[fs] open FAIL '/dev_hdd0/game/NPUA80638/USRDIR/levels/missing.lvl' -> '/tmp/vfs/y' (flags=0x0)
[cellGcmSys] ref stuck at 0x6C2 for 2 s: get=0x005D3F4C put=0x00012058 word@get=0x00041808 park_off=0x00000000 sema_off=0x0 labels 134/135/136/137 = 1/1/2/2
[FIFODUMP]  +0 io=005D3F4C w=00041808   <-- bad word
[rsx engine/metal] captured frame 120 to /tmp/frame.ppm t=1000.000
[rsx texlog] h12 <- loc 0 off 0x00100000 fmt 0x86 256x256 mips 9 pitch 0 cube 0 remap 0xAAE4 span 43690
[rsx texlog] h0 <- loc 1 off 0x00200000 fmt 0x9F 64x64 mips 1 pitch 256 cube 0 remap 0xAAE4 span 16384
[rsx engine] texture bind FAILED: loc 1 off 0x00200000 fmt 0x9F 64x64 mips 1 pitch 256 cube 0 depth 0 remap 0xAAE4 -> black
[rsx engine/metal] captured frame 240 to /tmp/frame.ppm t=1004.000
[WAIT] semaphore_wait(sem=38 timeout=10000)
[cellNetCtl] Init()[WAIT] event_queue_receive(q=15 timeout=0) tid=25 cia=0x009C5308 lr=0x00000000
[rsx engine] pipeline 7a187d4e426aa46e: guest vp 2, fp 9, 1 constants -> FAILED (draw dropped)
[RSX metal] fragment program 00000000DEADBEEF: decompile failed (-3)
[edge-watch] spu=2003 r11 changed after wrch MFC_LSA (getllar): was 00000080 0 0 0 now 00000123 0 0 0 (steps=1 ie=0 irq_saved=0 resume=0x00000 host_depth=1)
[lv2err] syscall 92(r3=0x00000026 r4=0x00002710 r5=0x0 r6=0x0 r7=0x0 r8=0x0 r9=0x0) -> 0x8001000B lr=0x00123456
[lv2err] syscall 102(r3=0x00000003 r4=0x00000000 r5=0x0 r6=0x0 r7=0x0 r8=0x0 r9=0x0) -> 0x80010008 lr=0x00123456
[audio-rate] 150.2 blocks/s (real time 187.5), silent 3.0%, dropped 0 frames/s, device buf 0
[audio-rate] 187.4 blocks/s (real time 187.5), silent 3.0%, dropped 0 frames/s, device buf 0
[rsx fps] 24.50 fps (245 frames in 10s: 240 world, 5 light); 250 presents, 5 with no draws
[rsx fps] 12.00 fps (120 frames in 10s: 118 world, 2 light); 125 presents, 5 with no draws
[RSX FPS] display=30.00 guest=29.50 avg_ms=33.40 p95_ms=40.10 max_ms=320.00 age_s=0.01
[rsx engine] drop #1 (no vertices): prim=5 packets=1 arr=0 idx=0 verts=0 fetch_ok=1 rt=0x0 clip=1280x720 frame=40
[cellGcmSys] ref stuck at 0x12C3 for 2 s: get=0x00012058 put=0x00012058 word@get=0x20014400 park_off=0x0051B450 sema_off=0x870 labels 134/135/136/137 = 13E6/13E6/13E7/13E7
[tdump] ---- guest threads ----
[tdump] tid=1   main                         lr=0x00351768 sp=0x0FEFF000 prof=0x00000000 chain: 0x1234
[surf-dump] triggered (present 1)
[pass] ---- frame 77: 1385 records (flush) ----
[rsx engine/metal] captured frame 360 to /tmp/frame.ppm t=1012.000
[sys_fs] open FAILED: /tmp/vfs/dev_hdd0/game/BCUS98125_INSTALL/USRDIR/download_manifest.dat (errno=2 No such file or directory, exists=0, size=-1)
something odd: invalid header 0x1234 in 5 records
"""


def selftest():
    import tempfile
    import shutil
    tmp = tempfile.mkdtemp(prefix='log_triage_')
    try:
        sess = os.path.join(tmp, '20261018-120000')
        os.makedirs(os.path.join(sess, 'shaders'))
        log = os.path.join(sess, 'game.log')
        with open(log, 'w') as f:
            f.write(SAMPLE_LOG)
        size = os.path.getsize(log)
        cap_off = SAMPLE_LOG.index('[tdump] ---- guest')
        base = 1760788800000
        with open(os.path.join(sess, 'events.tsv'), 'w') as f:
            f.write('# utc\tunix_ms\tlog_bytes\tevent\tdetail\n')
            f.write('2026-10-18 12:00:00\t%d\t0\tstart\t\n' % base)
            f.write('2026-10-18 12:00:02\t%d\t%d\ttick\t\n' % (base + 2000, cap_off // 2))
            f.write('2026-10-18 12:00:04\t%d\t%d\tcapture\tn=1 frame=240 shot=shots/shot1.png\n' % (base + 4000, cap_off))
            f.write('2026-10-18 12:00:06\t%d\t%d\ttick\t\n' % (base + 6000, size))
            f.write('2026-10-18 12:00:08\t%d\t%d\tstop\t\n' % (base + 8000, size))
            f.write('2026-10-18 12:00:08\t%d\t%d\texit\tsignal=15\n' % (base + 8000, size))
        with open(os.path.join(sess, 'shaders', 'manifest.txt'), 'w') as f:
            f.write('p=4 fp=00000000deadbeef vp=1111222233334444 targets=1 guest\n')
        with open(os.path.join(sess, 'shaders', 'p4_ps.hlsl'), 'w') as f:
            f.write('float4 main() { /* TODO: unhandled FP opcode 0x3F */ return 0; }\n')
        data, md_path, js_path = triage(sess)
        assert os.path.isfile(md_path) and os.path.isfile(js_path), 'outputs written'
        assert md_path.endswith('triage.md')
        j = json.load(open(js_path))
        titles = {(g['category'], g['title']): g for g in j['groups']}

        def has(cat, sub, known=None):
            hits = [g for (c, t), g in titles.items() if c == cat and sub in t]
            assert hits, 'no %s group with %r in %s' % (cat, sub, sorted(titles))
            if known is not None:
                assert bool(hits[0]['known']) == known, '%s %r known=%r' % (cat, sub, hits[0]['known'])
            return hits[0]

        has('unimplemented', 'NID 0x1A2B3C4D', known=False)
        has('unimplemented', 'stub syscall 871', known=True)
        has('fs', 'patch1.psarc', known=True)
        has('fs', 'missing.lvl', known=False)
        has('fs', '/dev_hdd0/game/BCUS98125_INSTALL/USRDIR/download_manifest.dat', known=True)
        has('texture', 'fmt 0x9F', known=False)
        has('shader', 'pipeline 7a187d4e426aa46e', known=False)
        has('shader', 'decompile failed', known=False)
        has('shader', 'decompiler TODO: unhandled FP opcode', known=False)
        has('spu', 'r11 changed', known=False)
        has('syscall', 'semaphore_wait', known=True)
        g = has('syscall', 'mutex_lock', known=False)
        assert 'EDEADLK' in g['title']
        has('audio', 'behind real time', known=False)
        has('draw', 'no vertices', known=False)
        has('other', 'invalid header', known=False)
        assert not any('NOT_INITIALIZED' in t for (_, t) in titles), 'the glued [WAIT] line was split off'
        # stalls: the boot wait is expected, the in-game one is not
        st = j['stalls']
        assert len(st) == 2 and st[0]['expected'] and not st[1]['expected'] and st[1]['drained']
        assert j['unimplemented_nids'][0]['nid'] == '0x1A2B3C4D'
        assert j['textures']['created'] == 2 and j['textures']['failed_by_format'].get('9F') == 2
        # times from events.tsv; the capture lines up with the log
        assert j['time_source'] == 'events.tsv'
        c = j['captures'][0]
        assert c['n'] == 1 and c['t'] == 4.0 and c['surf_present'] == 1 and c['pass_frame'] == 77
        assert c['present_before'] == 240, c
        assert c['problems_near'], 'problems near the capture'
        assert j['session']['end']['signal'] == '15' and j['session']['end']['stopped_by_player']
        assert not any(g['category'] == 'crash' for g in j['groups']), 'Stop is not a crash'
        # perf
        assert j['perf']['engine']['fps_median'] == 18.2 or j['perf']['engine']['fps_median'] == 18.3
        assert j['perf']['window']['hitches_250ms'] == 1
        assert j['perf']['presents']['frames'] == 360
        # top issues: no expected lines in them
        assert j['top'] and all(not t['known'] for t in j['top'])
        assert j['summary']['errors'] > 0
        md = open(md_path).read()
        for want in ('## Top issues', '## Captures', '### Capture 1 at 00:04', '## Expected (known harmless)',
                     'FIFO drained', 'p4 fragment shader'):
            assert want in md, want
        # a bare log (no session): times from the log's own stamps, <name>.triage.md next to it
        bare = os.path.join(tmp, 'run1.log')
        shutil.copy(log, bare)
        data, md_path, _ = triage(bare)
        assert md_path == os.path.join(tmp, 'run1.triage.md')
        assert data['time_source'] == 'log timestamps' and data['captures'][0]['surf_present'] == 1
        # normalization
        assert normalize('a 0x1F at 1234 and 7a187d4e426aa46e, 3.5 ms') == 'a 0x# at # and #, # ms'
        # the known list parses and every pattern compiles
        assert load_known(KNOWN_FILE)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    print('log_triage selftest: ok')
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('target', nargs='?', help='a session folder or a log file')
    ap.add_argument('--out', help='write triage.md / triage.json here instead')
    ap.add_argument('--known', default=KNOWN_FILE, help='known-harmless list (default: %(default)s)')
    ap.add_argument('--quiet', action='store_true', help='print only the output paths')
    ap.add_argument('--selftest', action='store_true', help='run the self-test on a synthetic log')
    a = ap.parse_args(argv)
    if a.selftest:
        return selftest()
    if not a.target:
        ap.error('a session folder or a log file')
    try:
        data, md_path, js_path = triage(a.target, a.out, a.known)
    except (OSError, ValueError) as e:
        print('log_triage: %s' % e, file=sys.stderr)
        return 1
    if not a.quiet:
        sm = data['summary']
        print('%s: %d error(s), %d warning(s), %d expected, %d capture(s); %d lines in %.1f s' % (
            data['session']['id'], sm['errors'], sm['warnings'], sm['expected'], sm['captures'], data['lines'],
            data['parse_seconds']))
        for g in data['top'][:10]:
            print('  [%s] %s  x%d  (%s .. %s)' % (g['category'], g['title'][:110], g['count'], fmt_t(g['first']),
                                                 fmt_t(g['last'])))
    print(redact(md_path))
    print(redact(js_path))
    return 0


if __name__ == '__main__':
    sys.exit(main())
