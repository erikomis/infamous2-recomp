/*
 * inFAMOUS 2's game clock, read from guest memory once per presented frame
 * (diagnostics, all off by default).
 *
 * The game times its frames itself: func_00089610, called once per pass of
 * the main loop (func_001EF000), reads the timebase (mftb), takes
 * dt = (tb - clock.last_tb) / tb_frequency and clamps it to [0, 1/30 s], or
 * to [0, 1/15 s] while the byte at 0xA7F556 is set. The clock lives at
 * 0xA2C500:
 *
 *   +0x00 f32 dt (clamped, real seconds)
 *   +0x08 f32 dt * scale0   +0x10 f32 scale0   (the main loop runs the world
 *   +0x18 f32 dt * scale1   +0x20 f32 scale1    update while +0x08 > 0)
 *   +0x28 f32 dt * scale2   +0x30 f32 scale2
 *   +0x04/+0x14/+0x24 s32 elapsed time of each scaled clock in 1/1440 s,
 *   +0x0C/+0x1C/+0x2C f32 the remainder
 *   +0xE0 u64 timebase of the last tick
 *
 * So the simulation is variable-step, driven by real time; the vblank and
 * flip counters the frame clock synthesizes do not enter it. (The flag was
 * set in every gameplay frame measured: dt <= 1/15 s, so below 15 fps the
 * game runs slower than real time, as on the PS3.)
 *
 *   PS3_GCLOCK_LOG=<file>      one line per presented frame: host time (s),
 *                              the fields above, the 1/15 flag, PS3_TB_SCALE
 *                              (toolkit; the guest timebase rate) and probes
 *   PS3_GCLOCK_PROBE=<ea>[@<off>...]:<n>[,...]  also log n big-endian floats
 *                              at each ea (hex); each @<off> loads the u32
 *                              there and adds off (a pointer chain)
 *   PS3_MEMSNAP=<ea>:<len>[+<ea>;<len>...]:<prefix>:<s>[/<s>...]  raw dumps
 *                              of guest memory, <prefix>_<s>_<ea>.bin, at <s>
 *                              seconds after start (hex ea/len)
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#ifdef _WIN32
#include <windows.h>
#endif

extern uint8_t* vm_base;
#if defined(_MSC_VER)
extern volatile int ps3_tb_scale_pct;   /* toolkit sys_timer.c: PS3_TB_SCALE */
#else
extern volatile int ps3_tb_scale_pct __attribute__((weak));   /* weak: older toolkits lack it */
#endif

#define GCLOCK_EA      0x00A2C500u
#define GCLOCK_FLAG_EA 0x00A7F556u   /* byte 6 of the object at 0xA7F550 */

static uint32_t rd32(uint32_t ea)
{
    const uint8_t* p = vm_base + ea;
    return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3];
}
static float rdf(uint32_t ea) { uint32_t u = rd32(ea); float f; memcpy(&f, &u, 4); return f; }

/* Seconds since the process started (the clock PAD_SCRIPT times use too). */
static double now_s(void)
{
#ifdef _WIN32
    static ULONGLONG t0; if (!t0) t0 = GetTickCount64();
    return (double)(GetTickCount64() - t0) / 1000.0;
#else
    static struct timespec t0; static int init;
    struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t);
    if (!init) { t0 = t; init = 1; }
    return (double)(t.tv_sec - t0.tv_sec) + (double)(t.tv_nsec - t0.tv_nsec) * 1e-9;
#endif
}
#if defined(__GNUC__) || defined(__clang__)
__attribute__((constructor)) static void gclock_t0(void) { (void)now_s(); }
#endif

#define MAX_PROBES 8
#define MAX_DEREF 4
static uint32_t s_probe_ea[MAX_PROBES]; static int s_probe_n[MAX_PROBES]; static int s_nprobes;
static uint32_t s_probe_off[MAX_PROBES][MAX_DEREF]; static int s_probe_nderef[MAX_PROBES];

/* <ea>@<off>@<off>: load the u32 at ea, add off, repeat. 0 on a null or
 * out-of-range pointer. */
static uint32_t probe_addr(int i)
{
    uint32_t a = s_probe_ea[i];
    for (int k = 0; k < s_probe_nderef[i]; k++) {
        if (a < 0x10000u || a > 0xFFFFFFF0u) return 0;
        a = rd32(a);
        if (!a) return 0;
        a += s_probe_off[i][k];
    }
    return a;
}

#define MAX_SNAPS 32
#define MAX_SNAP_REGIONS 4
static uint32_t s_snap_ea[MAX_SNAP_REGIONS], s_snap_len[MAX_SNAP_REGIONS]; static int s_snap_regions;
static char s_snap_prefix[512];
static double s_snap_at[MAX_SNAPS]; static int s_nsnaps, s_snap_next;

void infamous2_gclock_frame(void)
{
    static int init; static FILE* log;
    if (!init) {
        init = 1;
        const char* e = getenv("PS3_GCLOCK_LOG");
        if (e && *e) {
            log = fopen(e, "w");
            if (log) fprintf(log, "# t_s dt gdt dt1 dt2 n0 n1 n2(1/1440s) scale0 scale1 scale2 flag15 tb_scale_pct probes...\n");
        }
        const char* p = getenv("PS3_GCLOCK_PROBE");
        while (p && *p && s_nprobes < MAX_PROBES) {
            char* end; uint32_t ea = (uint32_t)strtoul(p, &end, 16); int n = 3;
            s_probe_nderef[s_nprobes] = 0;
            while (*end == '@' && s_probe_nderef[s_nprobes] < MAX_DEREF)
                s_probe_off[s_nprobes][s_probe_nderef[s_nprobes]++] = (uint32_t)strtoul(end + 1, &end, 16);
            if (*end == ':') n = (int)strtol(end + 1, &end, 10);
            if (n < 1) n = 1; if (n > 16) n = 16;
            s_probe_ea[s_nprobes] = ea; s_probe_n[s_nprobes] = n; s_nprobes++;
            p = strchr(end, ','); if (p) p++;
        }
        const char* s = getenv("PS3_MEMSNAP");
        if (s && *s) {
            char buf[1024]; snprintf(buf, sizeof buf, "%s", s);
            char* f1 = strtok(buf, ":"); char* f2 = strtok(NULL, ":");
            char* f3 = strtok(NULL, ":"); char* f4 = strtok(NULL, ":");
            if (f1 && f2 && f3 && f4) {
                /* f1 = first ea, f2 = "<len>[+<ea>;<len>...]" (split at the colons). */
                s_snap_ea[0] = (uint32_t)strtoul(f1, 0, 16);
                char* q = f2;
                s_snap_len[0] = (uint32_t)strtoul(q, &q, 16); s_snap_regions = 1;
                while (*q == '+' && s_snap_regions < MAX_SNAP_REGIONS) {
                    s_snap_ea[s_snap_regions] = (uint32_t)strtoul(q + 1, &q, 16);
                    if (*q == ';') s_snap_len[s_snap_regions] = (uint32_t)strtoul(q + 1, &q, 16);
                    s_snap_regions++;
                }
                snprintf(s_snap_prefix, sizeof s_snap_prefix, "%s", f3);
                for (char* t = strtok(f4, "/"); t && s_nsnaps < MAX_SNAPS; t = strtok(NULL, "/"))
                    s_snap_at[s_nsnaps++] = atof(t);
            }
        }
    }
    if (!vm_base) return;
    const double t = now_s();
    if (s_snap_next < s_nsnaps && t >= s_snap_at[s_snap_next]) {
        for (int r = 0; r < s_snap_regions; r++) {
            char path[600];
            snprintf(path, sizeof path, "%s_%03d_%08x.bin", s_snap_prefix, (int)s_snap_at[s_snap_next], s_snap_ea[r]);
            FILE* f = fopen(path, "wb");
            if (f) { fwrite(vm_base + s_snap_ea[r], 1, s_snap_len[r], f); fclose(f); }
            fprintf(stderr, "[gclock] memsnap %s at %.3f s\n", path, t);
        }
        s_snap_next++;
    }
    if (!log) return;
    const uint32_t c = GCLOCK_EA;
    fprintf(log, "%.6f %.6f %.6f %.6f %.6f %d %d %d %g %g %g %u",
            t, rdf(c + 0x00), rdf(c + 0x08), rdf(c + 0x18), rdf(c + 0x28),
            (int32_t)rd32(c + 0x04), (int32_t)rd32(c + 0x14), (int32_t)rd32(c + 0x24),
            rdf(c + 0x10), rdf(c + 0x20), rdf(c + 0x30), (unsigned)vm_base[GCLOCK_FLAG_EA]);
    fprintf(log, " %d", &ps3_tb_scale_pct ? ps3_tb_scale_pct : 100);
    for (int i = 0; i < s_nprobes; i++) {
        const uint32_t a = probe_addr(i);
        for (int k = 0; k < s_probe_n[i]; k++)
            fprintf(log, " %.3f", a ? rdf(a + 4u * (uint32_t)k) : 0.0f);
    }
    fputc('\n', log);
    static unsigned n; if ((++n & 63) == 0) fflush(log);
}
