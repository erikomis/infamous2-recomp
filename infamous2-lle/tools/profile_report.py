#!/usr/bin/env python3
"""profile_report.py <run name>: fps, per-thread CPU and SPU interpreter share of a run.

Reads out/<name>.log (written by tools/capture.sh or tools/profile_run.sh) and any
out/<name>.sample.<t>.txt (macOS `sample` output, one per sampling point).

  fps          from the "[rsx engine/metal] captured frame N ... t=<s>" lines, which
               the Metal backend prints every 120 presents: frames / seconds between
               consecutive lines, grouped into 10 s buckets of run time.
  thread CPU   per sampled thread: share of samples NOT parked in a blocking call
               (condition wait, semaphore, nanosleep, kevent, workqueue), labelled by
               what the thread is (PPU main, PPU guest thread, SPU, RSX/frame clock).
  hot code     top-of-stack functions across all running samples.
  SPU interp   the last "[spu-hist]" table (needs SPU_INTERP_HIST=1): which (image,
               4 KB LS page) the SPU interpreter spends its steps in.
"""
import collections, glob, os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.join(here, '..', 'out')
name = sys.argv[1]
log = os.path.join(out, name + '.log')

# Innermost kernel traps a parked thread sits in. Only these are counted: the
# wrappers above them (nanosleep, pthread_cond_wait, mach_msg) would count the
# same samples twice.
BLOCKING = {'__psynch_cvwait', '__semwait_signal', '__workq_kernreturn', 'kevent',
            'kevent_id', 'kevent_qos', 'mach_msg2_trap', 'mach_msg_trap',
            '__psynch_mutexwait', '__ulock_wait', '__ulock_wait2', 'semaphore_wait_trap',
            'semaphore_timedwait_trap', '__select', '__sigsuspend', '__psynch_rw_rdlock',
            'iokit_user_client_trap'}

def fps_timeline(path):
    pts = []
    for line in open(path, errors='replace'):
        if 'captured frame' in line:
            m = re.search(r'captured frame (\d+) .* t=([0-9.]+)', line)
            if m: pts.append((float(m.group(2)), int(m.group(1))))
    if len(pts) < 2:
        return []
    t0 = pts[0][0]
    buckets = collections.OrderedDict()
    for (ta, fa), (tb, fb) in zip(pts, pts[1:]):
        if tb <= ta or fb <= fa: continue
        b = int((tb - t0) // 10) * 10
        f, s = buckets.get(b, (0, 0.0))
        buckets[b] = (f + fb - fa, s + tb - ta)
    return [(b, f / s) for b, (f, s) in buckets.items() if s > 0]

def classify(block):
    txt = '\n'.join(block)
    if 'com.apple.main-thread' in block[0]: return 'PPU main (guest)'
    if 'spu_exec_thread_proc' in txt: return 'SPU'
    if 'frame_clock' in txt or 'cellGcm_rsx_process_fifo' in txt: return 'RSX / frame clock'
    if 'ppu_host_thread_proc' in txt: return 'PPU guest thread'
    if 'thread_dump_main' in txt: return 'diagnostics'
    if 'Metal' in txt or 'IOGPU' in txt: return 'Metal'
    if 'SDLAudio' in block[0] or 'AudioToolbox' in txt or 'CoreAudio' in txt: return 'audio'
    return 'other'

def parse_sample(path):
    lines = open(path, errors='replace').read().split('\n')
    threads, cur = [], None
    for l in lines:
        m = re.match(r'^ {4}(\d+) Thread_(\S+)', l)
        if m:
            cur = {'n': int(m.group(1)), 'head': l, 'body': []}
            threads.append(cur); continue
        if cur is not None:
            if l.startswith('Total number in stack') or l.startswith('Sort by top of stack'):
                cur = None; continue
            cur['body'].append(l)
    res = []
    for t in threads:
        blocked = 0
        for l in t['body']:
            m = re.match(r'^[ +!:|]*(\d+) (\S+)', l)
            if m and m.group(2) in BLOCKING:
                blocked += int(m.group(1))
        # nested blocking frames (wrapper + syscall) double-count: cap at total
        blocked = min(blocked, t['n'])
        guest = re.findall(r'(func_[0-9A-F]{8}|liblv2_func_[0-9A-F]{8}|libsre_func_[0-9A-F]{8})', '\n'.join(t['body'][:12]))
        res.append((classify([t['head']] + t['body']), t['n'], t['n'] - blocked, guest[-1] if guest else ''))
    top = collections.Counter()
    in_top = False
    for l in lines:
        if l.startswith('Sort by top of stack'): in_top = True; continue
        if in_top:
            m = re.match(r'^\s+(.+?)\s{2,}(\d+)\s*$', l)
            if m and m.group(1).split('  (in')[0].strip() not in BLOCKING:
                top[m.group(1).split('  (in')[0].strip()] += int(m.group(2))
    return res, top

def spu_hist(path):
    last = []
    for line in open(path, errors='replace'):
        if line.startswith('[spu-hist] after'): last = [line.strip()]
        elif line.startswith('[spu-hist]   ') and last: last.append(line.strip())
    return last

print('=== %s ===' % name)
fps = fps_timeline(log)
if fps:
    vals = [v for _, v in fps]
    print('fps (10 s buckets of run time; %d buckets, median %.1f, min %.1f, max %.1f):'
          % (len(vals), sorted(vals)[len(vals) // 2], min(vals), max(vals)))
    print('  ' + '  '.join('%d:%.0f' % (b, v) for b, v in fps))
else:
    print('fps: no timestamped "captured frame" lines (needs the toolkit with t= on that line)')

for sp in sorted(glob.glob(os.path.join(out, name + '.sample.*.txt'))):
    res, top = parse_sample(sp)
    print('\n--- %s ---' % os.path.basename(sp))
    agg = collections.defaultdict(lambda: [0, 0, 0])
    for kind, n, run, g in res:
        a = agg[kind]; a[0] += 1; a[1] += run; a[2] += n
    for kind, (cnt, run, n) in sorted(agg.items(), key=lambda x: -x[1][1]):
        print('  %-20s %2d thread(s)  busy %5.1f core(s)' % (kind, cnt, run / max(1, max(r[1] for r in res))))
    busy = sorted([r for r in res if r[2] > 0.2 * r[1]], key=lambda r: -r[2])[:10]
    print('  busiest threads: ' + '; '.join('%s %.0f%% %s' % (k, 100.0 * run / n, g) for k, n, run, g in busy))
    print('  hottest running code: ' + ', '.join('%s %d' % (f, c) for f, c in top.most_common(12)))

h = spu_hist(log)
if h:
    print('\nSPU interpreter (last histogram): ' + h[0])
    for l in h[1:]: print('  ' + l)
else:
    print('\nSPU interpreter: no [spu-hist] lines (run with SPU_INTERP_HIST=1)')
