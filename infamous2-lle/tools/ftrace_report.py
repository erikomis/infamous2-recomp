#!/usr/bin/env python3
"""Frame-by-frame report of a PS3_FTRACE timeline.

  tools/ftrace_report.py <trace.txt> [--frames N] [--ms-per-col F] [--threads a,b]
  tools/ftrace_report.py <trace.txt> --phases [--settle MS]

--phases is for a live A/B (tools/ab_live.sh): PS3_TOGGLE_FILE switched knobs
during the trace, each switch left a mark, and the frames are split by the knob
values they started under -- every stretch of one value in time order, then
each value's frames together (frame time, SPU busy, RSX drain/present). The
first --settle ms after each switch (default 1000) are left out: a switched
cache refills, a switched code path warms up.

PS3_FTRACE=<start s>:<seconds>:<path> (toolkit, include/ps3emu/ps3_ftrace.h)
records spans: PPU syscalls that blocked and merged usleep poll runs (with the
guest call site), SPU busy/wait spans (busy tagged with the lifted image and the
SPURS workload id), the RSX walker blocked on a semaphore ACQUIRE, FIFO drains,
flips and presents. A frame here is the time between two guest flip requests.

The report says, for the median frame, how each thread spent it, and draws a
few frames as a timeline (one column per --ms-per-col ms):
  PPU   '#' running   '.' usleep poll   's' other blocking syscall
  SPU   '#' busy      ' ' waiting on a channel
  RSX   'S' blocked on ACQUIRE   'D' FIFO drain   'P' present   '|' flip reached
        inside D/P: 'E' Metal encode   'G' waiting for the GPU   'R' render-target readback
                    'C' a shader/pipeline built on first use   'T' a texture decoded + uploaded
"""
import argparse
import collections
import statistics
import sys

(SYSCALL, USLEEP, SPU_WAIT, SPU_BUSY, SEMA, SEMA_GAVEUP, DRAIN, FLIP_FIFO, FLIP_REQ, PRESENT,
 GPU_ENCODE, GPU_WAIT, READBACK, GPU_EXEC, TOGGLE, COMPILE, TEXUP) = range(1, 18)
# include/ps3emu/ps3_toggles.h, enum ps3_toggle_id (append-only there too).
TOGGLE_NAMES = ["PS3RECOMP_ENG_VCACHE", "PS3RECOMP_ENG_TEXHASH_FULL", "PS3RECOMP_ENG_SKIP_FP",
                "PS3_SPU_FASTJOB", "PS3_SPU_STUB_MICRO", "PS3_SPU_STACK_HINT", "PS3_SPU_DRAIN_INLINE",
                "PS3_DISPLAY_GAMMA", "PS3_SHARPEN", "PS3_AA", "PS3_ANISO", "PS3RECOMP_METAL_FPS",
                "PS3_VIBRANCE", "PS3_CONTRAST", "PS3_VIGNETTE", "PS3_GRAIN", "PS3_LUT_STRENGTH", "PS3_BUTTON_ICONS",
                "PS3_SPU_DIRECT"]
TOGGLE_STRING = {2, 7, 8, 9, 12, 13, 14, 15, 16, 17}   # marks carry a hash of the text
# What --phases reads (and tools/ab_live.sh records: PS3_FTRACE_KINDS).
PHASE_KINDS = (SPU_BUSY, SEMA, SEMA_GAVEUP, DRAIN, FLIP_REQ, PRESENT, TOGGLE)


def toggle_name(who):
    if who < len(TOGGLE_NAMES):
        return TOGGLE_NAMES[who]
    return f"extra#{who - 64}" if who >= 64 else f"knob#{who}"


def toggle_val(who, v):
    return f"#{v:08x}" if who in TOGGLE_STRING else str(v - (1 << 32) if v >= 1 << 31 else v)


def phases(spans, frames, settle):
    """Frames grouped by the toggle values they started under."""
    marks = [s for s in spans if s[0] == TOGGLE]
    if not marks:
        sys.exit("no toggle marks in the trace (PS3_TOGGLE_FILE unset, or no switch inside the window)")
    ids = sorted({m[1] for m in marks})
    state = {}
    for m in marks:                       # before its first switch a knob held that switch's old value
        state.setdefault(m[1], m[2])
    # Per-frame cost: SPU busy (all SPUs), RSX drain, present, acquire wait.
    per = []
    j0, mi, last_switch = 0, 0, -1e18
    for f0, f1 in frames:
        while mi < len(marks) and marks[mi][4] <= f0:
            state[marks[mi][1]] = marks[mi][3]
            last_switch = marks[mi][4]
            mi += 1
        acc = collections.defaultdict(float)
        while j0 < len(spans) and spans[j0][5] < f0 - 5000:   # spans are sorted by start
            j0 += 1
        for k, who, a, b, t0, t1 in spans[j0:]:
            if t0 > f1:
                break
            o = overlap(t0, t1, f0, f1)
            if o <= 0:
                continue
            if k == SPU_BUSY:
                acc["spu"] += o
            elif k == DRAIN:
                acc["drain"] += o
            elif k == PRESENT:
                acc["present"] += o
            elif k in (SEMA, SEMA_GAVEUP):
                acc["acquire"] += o
        key = tuple(state[i] for i in ids)
        per.append((f0, f1 - f0, key, f0 - last_switch < settle, acc))

    def label(key):
        return " ".join(f"{toggle_name(i)}={toggle_val(i, v)}" for i, v in zip(ids, key))

    def stats(rows):
        d = [r[1] for r in rows]
        q = sorted(d)
        p90 = q[min(len(q) - 1, int(len(q) * 0.9))]
        n = len(rows)
        mean = lambda k: sum(r[4][k] for r in rows) / n
        return (f"{n:5d} frames  median {statistics.median(d):6.1f} ms  mean {sum(d) / n:6.1f}"
                f"  p90 {p90:6.1f}  ({1000 * n / sum(d):5.1f} fps)  spu busy (all) {mean('spu'):6.1f}"
                f"  drain {mean('drain'):5.1f}  present {mean('present'):5.1f}  acquire {mean('acquire'):5.1f}")

    print(f"\nPhases (frames that start within {settle:.0f} ms of a switch left out):")
    seg = []
    for r in per + [None]:
        if seg and (r is None or r[2] != seg[0][2]):
            kept = [x for x in seg if not x[3]]
            if kept:
                print(f"  {seg[0][0] / 1000:8.1f} s  {label(seg[0][2])}\n      {stats(kept)}")
            seg = []
        if r is not None:
            seg.append(r)
    print("\nBy value (all stretches together):")
    medians = {}
    for key in sorted({r[2] for r in per}):
        kept = [r for r in per if r[2] == key and not r[3]]
        if kept:
            medians[key] = statistics.median(r[1] for r in kept)
            print(f"  {label(key)}\n      {stats(kept)}")
    if len(medians) == 2:
        (ka, ma), (kb, mb) = sorted(medians.items())
        print(f"\n  median frame {label(kb)} vs {label(ka)}: {mb:.1f} vs {ma:.1f} ms ({100 * (mb - ma) / ma:+.1f}%)")

GPU_WAIT_KIND = {0: "gpu wait (flush)", 1: "gpu wait (present)", 2: "gpu wait (in-flight)"}

SYSCALL_NAMES = {   # runtime/syscalls/lv2_syscall_table.h
    43: "thread_yield", 44: "thread_join", 85: "event_flag_wait", 92: "semaphore_wait",
    97: "lwmutex_lock", 102: "mutex_lock", 107: "cond_wait", 113: "lwcond_wait",
    122: "rwlock_rlock", 125: "rwlock_wlock", 130: "event_queue_receive", 138: "event_port_send",
    141: "usleep", 142: "sleep", 178: "spu_group_join", 801: "fs_open", 802: "fs_read", 818: "fs_lseek",
}


RSX_ROWS = ("acquire wait", "drain", "present", "encode (flush)", "encode (present)",
            "gpu wait (flush)", "gpu wait (present)", "gpu wait (in-flight)", "readback",
            "first-use compile", "texture upload")


def load(path, kinds=None):
    """kinds: keep only these span kinds (a long trace does not fit in memory whole)."""
    names, spans, header = {}, [], ""
    keep = {str(k) for k in kinds} if kinds else None
    with open(path) as f:
        for line in f:
            if line.startswith("#"):
                header = line.strip()
                continue
            if line.startswith("T "):
                _, tid, name = line.rstrip("\n").split(" ", 2)
                names[int(tid)] = name
                continue
            k, who, a, b, t0, t1 = line.split()
            if keep is not None and k not in keep:
                continue
            spans.append((int(k), int(who), int(a, 16), int(b, 16), float(t0) / 1e3, float(t1) / 1e3))
    spans.sort(key=lambda s: s[4])
    return header, names, spans


def overlap(s0, s1, f0, f1):
    return max(0.0, min(s1, f1) - max(s0, f0))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("trace")
    ap.add_argument("--frames", type=int, default=3, help="frames drawn as a timeline")
    ap.add_argument("--ms-per-col", type=float, default=1.0)
    ap.add_argument("--threads", default="", help="PPU tids to draw (default: the six least blocked)")
    ap.add_argument("--phases", action="store_true", help="split frames by PS3_TOGGLE_FILE switches (A/B)")
    ap.add_argument("--settle", type=float, default=1000.0, help="--phases: ms left out after each switch")
    args = ap.parse_args()

    header, names, spans = load(args.trace, PHASE_KINDS if args.phases else None)
    print(header)
    flips = [s[4] for s in spans if s[0] == FLIP_REQ]
    if len(flips) < 3:
        sys.exit("fewer than 3 flip requests in the window")
    frames = list(zip(flips, flips[1:]))
    durs = [b - a for a, b in frames]
    print(f"{len(frames)} frames, median {statistics.median(durs):.1f} ms "
          f"({1000 / statistics.median(durs):.1f} fps), min {min(durs):.1f}, max {max(durs):.1f}")
    if args.phases:
        phases(spans, frames, args.settle)
        return

    # Per-frame totals, then the median of each.
    ppu = collections.defaultdict(lambda: collections.defaultdict(list))   # tid -> (kind, site) -> [ms per frame]
    spu_busy = collections.defaultdict(list)                                # spu -> [ms per frame]
    spu_wid = collections.defaultdict(float)                                # (wid) -> ms total
    spu_img = collections.defaultdict(float)
    rsx = collections.defaultdict(list)
    tids = sorted({s[1] for s in spans if s[0] in (SYSCALL, USLEEP)})
    spus = sorted({s[1] for s in spans if s[0] in (SPU_BUSY, SPU_WAIT)})
    lo = frames[0][0]
    j0 = 0
    for f0, f1 in frames:
        acc = collections.defaultdict(float)
        while j0 < len(spans) and spans[j0][5] < lo:
            j0 += 1
        for s in spans[j0:]:
            k, who, a, b, t0, t1 = s
            if t0 > f1:
                break
            o = overlap(t0, t1, f0, f1)
            if o <= 0 and k not in (FLIP_FIFO,):
                continue
            if k == USLEEP:
                acc[("ppu", who, "usleep", b)] += o
            elif k == SYSCALL:
                acc[("ppu", who, SYSCALL_NAMES.get(a, f"sc{a}"), b)] += o
            elif k == SPU_BUSY:
                acc[("spu", who)] += o
                spu_wid[b] += o
                spu_img[a] += o
            elif k in (SEMA, SEMA_GAVEUP):
                acc[("rsx", "acquire wait")] += o
            elif k == DRAIN:
                acc[("rsx", "drain")] += o
            elif k == PRESENT:
                acc[("rsx", "present")] += o
            elif k == GPU_ENCODE:
                acc[("rsx", "encode (present)" if a else "encode (flush)")] += o
            elif k == GPU_WAIT:
                acc[("rsx", GPU_WAIT_KIND.get(a, "gpu wait"))] += o
            elif k == READBACK:
                acc[("rsx", "readback")] += o
            elif k == COMPILE and who == 0:   # the engine's whole miss (1-4 are inside it)
                acc[("rsx", "first-use compile")] += o
            elif k == TEXUP:
                acc[("rsx", "texture upload")] += o
        for t in tids:
            keys = [key for key in acc if key[0] == "ppu" and key[1] == t]
            for key in keys:
                ppu[t][(key[2], key[3])].append(acc[key])
        for sp in spus:
            spu_busy[sp].append(acc[("spu", sp)])
        for what in RSX_ROWS:
            rsx[what].append(acc[("rsx", what)])
        lo = f0

    n = len(frames)
    med = statistics.median(durs)

    def mean(xs):
        return sum(xs) / n   # frames where it never happened count as 0

    print(f"\nPer frame (mean over {n} frames, ms; frame median {med:.1f}):")
    print("  PPU threads (time blocked, by kind and guest call site):")
    rows = []
    for t in tids:
        total = sum(mean(v) for v in ppu[t].values())
        rows.append((total, t))
    for total, t in sorted(rows, reverse=True)[:12]:
        nm = names.get(t, "?")
        sites = sorted(((mean(v), k) for k, v in ppu[t].items()), reverse=True)[:4]
        desc = ", ".join(f"{kind}@{site:08X} {ms:.1f}" for ms, (kind, site) in sites)
        print(f"    tid {t:3d} {nm[:28]:28s} blocked {total:5.1f}  ({desc})")
    print("  SPUs (busy):")
    for sp in spus:
        print(f"    spu {sp:3d} busy {mean(spu_busy[sp]):5.1f}  ({100 * mean(spu_busy[sp]) / med:.0f}%)")
    tot = sum(spu_wid.values()) or 1
    print("  SPU busy time by SPURS workload id (LS 0x1DC) / lifted image:")
    print("    " + ", ".join(f"wid {w:#x} {100 * v / tot:.0f}%" for w, v in sorted(spu_wid.items(), key=lambda x: -x[1])[:8]))
    print("    " + ", ".join(f"img {i} {100 * v / tot:.0f}%" for i, v in sorted(spu_img.items(), key=lambda x: -x[1])[:8]))
    print("  RSX / present thread:")
    for what in RSX_ROWS:
        print(f"    {what:20s} {mean(rsx[what]):5.1f}")

    # GPU execution per command buffer (headless commits wait for each one).
    ex = [s for s in spans if s[0] == GPU_EXEC and flips[0] <= s[5] <= flips[-1]]
    if ex:
        print("  Metal command buffers (GPU's own execution time):")
        for kind, label in ((1, "present"), (0, "flush")):
            xs = [s for s in ex if s[3] == kind]
            if not xs:
                continue
            ms = [s[5] - s[4] for s in xs]
            print(f"    {label:8s} {len(xs) / n:4.1f}/frame  gpu {statistics.median(ms):6.2f} ms median"
                  f"  ({sum(ms) / n:5.1f} ms/frame)  passes {statistics.median([s[1] for s in xs]):.0f}"
                  f"  depth resolves {statistics.median([s[2] & 0xFFFF for s in xs]):.0f}"
                  f"  colour copies {statistics.median([s[2] >> 16 for s in xs]):.0f}")

    # Timelines.
    if args.threads:
        draw = [int(x) for x in args.threads.split(",")]
    else:
        draw = [t for _, t in sorted(rows)[:6]]   # the threads that work, not the idle ones
    pick = [frames[len(frames) // 2 + i] for i in range(args.frames) if len(frames) // 2 + i < len(frames)]
    for f0, f1 in pick:
        cols = int((f1 - f0) / args.ms_per_col) + 1
        print(f"\nframe at {f0 / 1000:.3f} s, {f1 - f0:.1f} ms ({args.ms_per_col:g} ms/col)")
        lines = {}
        for t in draw:
            lines[("ppu", t)] = ["#"] * cols
        for sp in spus:
            lines[("spu", sp)] = [" "] * cols
        lines[("rsx",)] = [" "] * cols

        def paint(row, t0, t1, ch, prio=False):
            c0 = max(0, int((t0 - f0) / args.ms_per_col))
            c1 = min(cols - 1, int((t1 - f0) / args.ms_per_col))
            for c in range(c0, c1 + 1):
                if prio or row[c] in " #":
                    row[c] = ch

        for k, who, a, b, t0, t1 in spans:
            if t1 < f0 or t0 > f1:
                continue
            if k == USLEEP and ("ppu", who) in lines:
                paint(lines[("ppu", who)], t0, t1, ".", True)
            elif k == SYSCALL and ("ppu", who) in lines:
                paint(lines[("ppu", who)], t0, t1, "s", True)
            elif k == SPU_BUSY and ("spu", who) in lines:
                paint(lines[("spu", who)], t0, t1, "#")
            elif k in (SEMA, SEMA_GAVEUP):
                paint(lines[("rsx",)], t0, t1, "S", True)
            elif k == DRAIN:
                paint(lines[("rsx",)], t0, t1, "D")
            elif k == PRESENT:
                paint(lines[("rsx",)], t0, t1, "P", True)
            elif k == FLIP_FIFO:
                paint(lines[("rsx",)], t0, t0, "|", True)
        for k, who, a, b, t0, t1 in spans:   # second pass: what the drain/present was doing
            if t1 < f0 or t0 > f1:
                continue
            if k == GPU_ENCODE:
                paint(lines[("rsx",)], t0, t1, "E", True)
            elif k == GPU_WAIT:
                paint(lines[("rsx",)], t0, t1, "G", True)
            elif k == READBACK:
                paint(lines[("rsx",)], t0, t1, "R", True)
            elif k == COMPILE and who == 0:
                paint(lines[("rsx",)], t0, t1, "C", True)
            elif k == TEXUP:
                paint(lines[("rsx",)], t0, t1, "T", True)
        for key, row in lines.items():
            if key[0] == "ppu":
                label = f"{key[1]:3d} {names.get(key[1], '?')[:14]:14s}"
            elif key[0] == "spu":
                label = f"spu {key[1]:<14d}"
            else:
                label = "rsx" + " " * 15
            print(f"  {label} {''.join(row)}")


if __name__ == "__main__":
    main()
