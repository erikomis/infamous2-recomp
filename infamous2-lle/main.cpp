/*
 * ps3recomp game project -- entry point template.
 *
 * This is the runner. It boots a lifted title through the toolkit's PPU
 * scaffold: allocate the flat guest VM, load the PPU ELF named on the command
 * line, register the lifted function table and the HLE NID handlers, start a
 * frame clock, and dispatch the entry OPD.
 *
 * The scaffold does the work; nothing below reimplements it. ppu_load_elf,
 * ppu_recomp_register, ppu_hle_init, ppu_sysprx_register, ppu_fs_register,
 * lv2_init_syscalls and ppu_run all come from the toolkit, compiled into this
 * project from PS3RECOMP_DIR (see CMakeLists.txt). What is left here is the
 * part a port owns: which backend to present through, how the frame clock is
 * paced, and whatever diagnostics the title turns out to need.
 *
 * runtime/ppu/tests/boot_main.cpp in the toolkit is the same boot in its
 * fully-instrumented form -- crash filter, hang watchdog, guest-PC sampling
 * profiler. Read it when a boot goes wrong; copy from it what the title
 * needs. It is deliberately not what a fresh project starts with.
 *
 * The one part that IS duplicated from it is the frame clock below, because a
 * port is expected to change its pacing and a shared one would be the wrong
 * shape for that. It was copied faithfully, comments and all. If a pacing bug
 * is fixed in one of the two, look at the other.
 *
 * This builds on Windows, macOS and Linux. CI builds and runs it against the
 * boot smoke title on the last two, and compile-checks it against clang-cl on
 * the first (tools/check_ppu_scaffold.py).
 */

/* The lifter's generated header, which declares ppu_context and the function
 * table. It comes first: everything below is written against that struct. */
#include "ppu_recomp.h"

/* The Win32 names -- Sleep, GetTickCount64, CreateThread, InterlockedIncrement,
 * VirtualAlloc, the scalar typedefs -- from one place. On Windows this is a
 * passthrough to <windows.h>; off Windows the toolkit's shim supplies the same
 * names over pthreads and mmap, so the call sites below are written once. */
#include "win32_compat.h"
#include "ps3emu/ps3_ftrace.h"   /* PS3_FTRACE: present spans */
#include "ps3emu/ps3_toggles.h"  /* PS3_TOGGLE_FILE: knobs switched while the game runs */
#if defined(__APPLE__)
#include <CoreFoundation/CoreFoundation.h>
#include <pthread.h>
/* macos_host.m: App Nap off for the run; USER_INTERACTIVE QoS with PS3_QOS=1. */
extern "C" void infamous2_host_begin_activity(void);
extern "C" int  infamous2_host_qos_enabled(void);
extern "C" void infamous2_host_qos_self(void);
#else
static inline void infamous2_host_begin_activity(void) {}
static inline int  infamous2_host_qos_enabled(void) { return 0; }
static inline void infamous2_host_qos_self(void) {}
#endif

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
/* timeBeginPeriod: <windows.h> arrives with WIN32_LEAN_AND_MEAN set, which
 * excludes the multimedia timer API, so it is asked for by name -- and after
 * windows.h, since timeapi.h uses UINT. No shim exists off Windows because
 * there is nothing to shim: the 15.6 ms default timer granularity this works
 * around is a Windows problem. */
#include <timeapi.h>
#endif

/* ---------------------------------------------------------------------------
 * What the scaffold gives this file
 * -----------------------------------------------------------------------*/
extern "C" {
uint32_t ppu_load_elf(const char* path);      /* ELF -> guest RAM, returns entry OPD */
void infamous2_apply_patches(const char* elf_path);   /* game_patches.cpp: PS3_PATCHES */
void infamous2_apply_aspect(const char* elf_path);    /* game_patches.cpp: PS3_ASPECT (hor+) */
void     ppu_recomp_register(void);           /* generated: lifted table -> address map */
void     ppu_hle_init(void);                  /* firmware import NID -> HLE handler */
void     ppu_sysprx_register(void);           /* boot-critical CRT (sys_initialize_tls, ...) */
void     ppu_fs_register(void);               /* cellFs over the real game directory */
void     lv2_init_syscalls(void);             /* the lv2 syscall table */
void     cellGame_init_from_paramsfo(const char* sfo_path);  /* libs/system/cellGame.c */
int      ppu_run(uint32_t entry_opd, uint32_t stack_top);

/* This project's own: load whatever real system PRX modules the title needs
 * into guest RAM and register their exports. stubs.cpp has the empty version
 * a game with no lifted PRX wants. */
void     ps3_load_prx_modules(void);

extern const char* ppu_vfs_root;              /* host dir the PS3 mount points map into */

/* The status the guest handed to sys_process_exit. A title that exits that way
 * never returns through ppu_run, so this is only read on the path where the
 * entry function unwound instead. */
extern int     g_sys_process_exit_called;
extern int32_t g_sys_process_exit_code;

/* Host-provided symbols the runtime and the HLE libraries link against. */
uint8_t* vm_base = nullptr;
extern uint32_t ppu_vm_size;                  /* ppu_loader.cpp: the OOB guard */

/* Guest-callback dispatch. g_ps3_guest_caller is the hook the HLE runtime
 * calls back into recompiled code through -- cellSysutil events and the GCM
 * vblank/flip handlers. ppu_guest_call does the OPD -> dispatch. */
typedef void (*ps3_guest_caller_fn)(uint32_t, uint64_t, uint64_t, uint64_t, uint64_t,
                                    uint64_t, uint64_t, uint64_t, uint64_t);
extern ps3_guest_caller_fn g_ps3_guest_caller;
uint64_t ppu_guest_call(uint32_t, uint64_t, uint64_t, uint64_t, uint64_t,
                        uint64_t, uint64_t, uint64_t, uint64_t);

/* The GCM half of the frame clock. */
void     cellGcmTickVBlank(void);
void     cellGcmTickFlip(void);
int      cellGcm_take_flip_pending(void);
void     cellGcm_rsx_process_fifo(void);      /* drain get -> put */
unsigned cellGcm_flip_request_count(void);
int      cellGcm_drain_sema_blocked(void);
}

/* ---------------------------------------------------------------------------
 * RSX present backend
 * -----------------------------------------------------------------------*/
/* D3D12 on Windows, Metal on Apple, and the null backend's headless software
 * path anywhere else -- the same selection runtime/host/host_posix.c and the
 * toolkit's boot harness make. All three expose the same three entry points,
 * so the frame clock below is backend-agnostic. */
#if defined(_WIN32)
extern "C" int  rsx_d3d12_backend_init(uint32_t w, uint32_t h, const char* title);
extern "C" void rsx_d3d12_backend_present(void);
extern "C" int  rsx_d3d12_backend_pump_messages(void);
#  define rsx_backend_init    rsx_d3d12_backend_init
#  define rsx_backend_present rsx_d3d12_backend_present
#  define rsx_backend_pump    rsx_d3d12_backend_pump_messages
#  define RSX_BACKEND_NAME    "D3D12"
#elif defined(__APPLE__)
extern "C" int  rsx_metal_backend_init(uint32_t w, uint32_t h, const char* title);
extern "C" void rsx_metal_backend_present(void);
extern "C" int  rsx_metal_backend_pump_messages(void);
#  define rsx_backend_init    rsx_metal_backend_init
#  define rsx_backend_present rsx_metal_backend_present
#  define rsx_backend_pump    rsx_metal_backend_pump_messages
#  define RSX_BACKEND_NAME    "Metal"
#else
extern "C" int  rsx_null_backend_init(uint32_t w, uint32_t h, const char* title);
extern "C" void rsx_null_backend_present(void);
extern "C" int  rsx_null_backend_pump_messages(void);
#  define rsx_backend_init    rsx_null_backend_init
#  define rsx_backend_present rsx_null_backend_present
#  define rsx_backend_pump    rsx_null_backend_pump_messages
#  define RSX_BACKEND_NAME    "null (headless software)"
#endif

/* ---------------------------------------------------------------------------
 * Configuration
 * -----------------------------------------------------------------------*/
/* The flat VM must span every region the PS3 memory map uses, not just the
 * game image: sys_ppu_thread_create puts thread stacks at 0xD0000000, so a
 * smaller arena makes every spawned thread's stack access out of bounds. */
#define VM_SIZE       0x100010000ull   /* the full 32-bit guest space + a 64K guard */
#define STACK_TOP     0x0FF00000u      /* main-thread stack, below the 0x10000000 segment */
#define WINDOW_WIDTH  1280
#define WINDOW_HEIGHT 720

/* ---------------------------------------------------------------------------
 * Frame clock
 * -----------------------------------------------------------------------*/
/* On real hardware the RSX raises a vblank interrupt about 60 times a second
 * and that is what drives the game's frame loop. With no RSX it is synthesized
 * here: a host thread calls cellGcmTickVBlank/TickFlip, which invoke the
 * handlers the guest registered, and drains the GCM FIFO. Without this the
 * title initialises, registers its handlers and then waits forever.
 *
 * The ticks are driven off real elapsed time rather than off how long present()
 * took. A hidden or occluded window makes present block hard, and pacing the
 * ticks behind it paces the whole game behind it. */
static volatile LONG g_frames_presented = 0;

/* Frames handed to the backend at a guest FLIP boundary -- one per frame the
 * guest actually finished. The presents made before the guest's first flip, so
 * a fresh window is not left blank through a long boot, carry no guest frame
 * and are deliberately not counted. */
extern "C" unsigned ppu_boot_frames_presented(void)
{
    return (unsigned)g_frames_presented;
}

/* GCM_FLIP_IMMEDIATE=1: complete the flip when the frame is presented instead
 * of on the next 16 ms vblank tick. The title waits on the flip before it
 * starts the next frame, and on a frame that takes ~70 ms the tick costs it
 * ~8 ms on average for nothing. */
static int g_flip_immediate = 0;

#if defined(__APPLE__)
/* PS3RECOMP_METAL_FPS=1: the window title shows display and guest FPS. */
extern "C" void rsx_metal_backend_note_guest_frame(void);
#endif

static void present_guest_frame(void)
{
    const uint64_t ft_t0 = PS3_FT_ON() ? ps3_ft_now() : 0;
    rsx_backend_present();
    if (ft_t0) ps3_ft_span(PS3_FT_PRESENT, 0, 0, 0, ft_t0, ps3_ft_now());
#if defined(__APPLE__)
    rsx_metal_backend_note_guest_frame();
#endif
    if (g_flip_immediate) cellGcmTickFlip();
    /* This thread increments and guest threads read, so interlocked rather
     * than a volatile ++, which on arm64 is neither atomic nor a fence. */
    InterlockedIncrement(&g_frames_presented);
}

static DWORD WINAPI frame_clock(LPVOID)
{
    /* PS3_QOS=1: this thread walks the FIFO, encodes the draws and presents --
     * half of every frame. It starts at QOS_CLASS_DEFAULT like any pthread. */
    infamous2_host_qos_self();
    const char* title = getenv("PS3_TITLE");
    if (!title || !*title) title = "ps3recomp";

    int rsx_ok = (rsx_backend_init(WINDOW_WIDTH, WINDOW_HEIGHT, title) == 0);
    fprintf(stderr, "[rsx] %s backend init %s\n", RSX_BACKEND_NAME,
            rsx_ok ? "OK -- window open" : "FAILED");

    /* Pacing knobs (see tools/profile_run.sh for the A/B that sets them). */
    { const char* e = getenv("GCM_FLIP_IMMEDIATE"); g_flip_immediate = e ? atoi(e) : 0; }
    int sema_fast_poll = 0;
    { const char* e = getenv("GCM_SEMA_FASTPOLL"); sema_fast_poll = e ? atoi(e) : 0; }
    /* PS3_FRAMECLOCK_US=<us>: the sleep between two passes of this loop.
     * The FIFO has no doorbell: a put the guest writes, a flip it asks for,
     * waits here until the next pass, so this is also the latency of every
     * one of those. It was a fixed 4 ms; in the New Marais walk the RSX
     * thread sat idle ~4 ms after each flip request; 1 ms gave +2..+5% fps
     * (live A/B 2026-10-22, each stretch against its neighbours; 35.9 vs
     * 33.8 fps over the whole window, mean frame 27.8 vs 29.6 ms).
     * Default 1000; 4000 is the old pacing. Read live (PS3_TOGGLE_FILE). */
    int clock_us_env = 1000;
    { const char* e = getenv("PS3_FRAMECLOCK_US"); if (e && atoi(e) > 0) clock_us_env = atoi(e); }
    /* PS3_HOT_QOS=1: this thread walks the FIFO and encodes every draw; keep
     * it on a performance core alongside the SPU threads (lv2_register.c). */
    { const char* e = getenv("PS3_HOT_QOS");
      if (e && atoi(e)) SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL); }

    ULONGLONG next_tick = GetTickCount64();

    for (;;) {
        /* GCM_SEMA_FASTPOLL=1: while the drain is parked on a semaphore acquire
         * (the SPU lighting label), re-check every 1 ms instead of every 4. */
        {   int us = ps3_toggle_get("PS3_FRAMECLOCK_US", clock_us_env);
            if (us <= 0) us = clock_us_env;
            if (sema_fast_poll && cellGcm_drain_sema_blocked() && us > 1000) us = 1000;
#ifdef _WIN32
            Sleep((DWORD)((us + 999) / 1000));
#else
            usleep((useconds_t)us);
#endif
        }
        ULONGLONG now = GetTickCount64();
        /* PS3_TOGGLE_FILE=<path>: re-read every 250 ms when it changed, so an
         * A/B flips a knob inside one run (tools/ab_live.sh). Without the
         * variable this is one predictable branch. */
        ps3_toggles_poll();

        int fired = 0;
        while ((long long)(now - next_tick) >= 0 && fired < 240) {
            cellGcmTickVBlank();
            cellGcmTickFlip();
            /* Present a pending flip BEFORE draining any further. The flip
             * fires at a get==put frame boundary on the guest thread, so the
             * batch held right now is exactly the completed frame; presenting
             * after the drain races the guest's next-frame writes and shows a
             * mixed one. */
            if (rsx_ok && cellGcm_take_flip_pending()) {
                present_guest_frame();
            }
            /* Drain the FIFO every tick. This is what writes the RSX sync-fence
             * labels the game's per-frame logic blocks on, so it has to keep
             * advancing at 60 Hz even while present() throttles. */
            if (rsx_ok) cellGcm_rsx_process_fifo();
            next_tick += 16;             /* ~60 Hz */
            fired++;
        }
        if (fired >= 240) next_tick = now;   /* fell too far behind -- resync */

        /* Drain at the outer cadence too, not only on the 16 ms tick: titles
         * fence every render pass on a label the drain writes, and one wave of
         * those at 16 ms apiece paces the guest into single-figure frame rates.
         * The real RSX writes them in microseconds. */
        if (rsx_ok) {
            if (cellGcm_take_flip_pending()) {
                present_guest_frame();
            }
            cellGcm_rsx_process_fifo();

            /* Window events at most every 4 ms, as before PS3_FRAMECLOCK_US:
             * a windowed pump is a dispatch_sync to the main thread, and this
             * loop now runs every 1 ms. */
            static ULONGLONG last_pump = 0;
            if (now - last_pump >= 4 || now < last_pump) {
                last_pump = now;
                if (rsx_backend_pump() != 0) {
                    rsx_ok = 0;              /* window closed */
                    continue;
                }
            }
            /* Present a flip the drain just reached, now: the drain stops at
             * the flip, so the batch is exactly the finished frame, and the
             * guest is waiting on this flip before it starts the next one.
             * Consuming the pending flag here matters -- this used to present
             * on the flip count changing and leave the flag set, so the check
             * above re-showed the same frame on the next pass with no draws:
             * every frame was presented twice. Before the first flip present
             * freely, so the window is not blank during boot. */
            if (cellGcm_take_flip_pending())
                present_guest_frame();
            else if (cellGcm_flip_request_count() == 0)
                rsx_backend_present();
        }
    }
    return 0;
}

/* ---------------------------------------------------------------------------
 * VFS root
 * -----------------------------------------------------------------------*/
/* Derive the directory holding PS3_GAME from the EBOOT path
 * <root>/PS3_GAME/USRDIR/EBOOT.elf -> <root>. $PS3_VFS_ROOT overrides. */
static char s_vfs_root[1024];

static void derive_vfs_root(const char* eboot)
{
    const char* env = getenv("PS3_VFS_ROOT");
    if (env && *env) { ppu_vfs_root = env; return; }

    strncpy(s_vfs_root, eboot, sizeof s_vfs_root - 1);
    for (char* p = s_vfs_root; *p; p++) if (*p == '\\') *p = '/';
    /* strip EBOOT.elf, USRDIR and PS3_GAME */
    for (int i = 0; i < 3; i++) { char* s = strrchr(s_vfs_root, '/'); if (s) *s = 0; }
    if (!s_vfs_root[0]) strcpy(s_vfs_root, ".");
    ppu_vfs_root = s_vfs_root;
}

/* ---------------------------------------------------------------------------
 * Guest VM
 * -----------------------------------------------------------------------*/
#ifdef _WIN32
/* Demand-paging for the flat VM: reserve the whole 4 GB guest space, which
 * costs no commit, and commit each 64 KB page on first access. Every 32-bit
 * guest offset is then valid, so a garbage guest pointer reads zero instead of
 * killing the process. Faults outside the arena fall through untouched. */
static LONG WINAPI vm_commit_veh(EXCEPTION_POINTERS* ep)
{
    if (ep->ExceptionRecord->ExceptionCode == EXCEPTION_ACCESS_VIOLATION) {
        ULONG_PTR fault = ep->ExceptionRecord->ExceptionInformation[1];
        uintptr_t base  = (uintptr_t)vm_base;
        if (vm_base && fault >= base && fault < base + VM_SIZE) {
            void* page = (void*)(fault & ~(uintptr_t)0xFFFF);
            if (VirtualAlloc(page, 0x10000, MEM_COMMIT, PAGE_READWRITE))
                return EXCEPTION_CONTINUE_EXECUTION;
        }
    }
    return EXCEPTION_CONTINUE_SEARCH;
}
#endif

static bool alloc_guest_vm(void)
{
#ifdef _WIN32
    AddVectoredExceptionHandler(1, vm_commit_veh);
    vm_base = (uint8_t*)VirtualAlloc(NULL, VM_SIZE, MEM_RESERVE, PAGE_READWRITE);
    ppu_vm_size = 0;              /* the whole space is backed; no OOB guard needed */
#else
    /* No vectored exception handlers off Windows, so the arena is committed up
     * front and lazily backed by the OS: only pages the title touches cost
     * anything. It stops below the 0xE0000000 mark, so ppu_vm_size arms the
     * loader's bounds check for what is left. */
    vm_base = (uint8_t*)calloc(1, 0xE0000000u);
    ppu_vm_size = 0xE0000000u;
#endif
    return vm_base != nullptr;
}

/* ---------------------------------------------------------------------------
 * Main
 * -----------------------------------------------------------------------*/
static void harness_guest_caller(uint32_t opd, uint64_t a0, uint64_t a1,
                                 uint64_t a2, uint64_t a3, uint64_t a4,
                                 uint64_t a5, uint64_t a6, uint64_t a7)
{
    ppu_guest_call(opd, a0, a1, a2, a3, a4, a5, a6, a7);
}

extern "C" void infamous2_gpu_light_install(void);   /* gpu_light.c */
extern "C" void infamous2_gpu_ssao_install(void);    /* gpu_ssao.c */
extern "C" void infamous2_edge_watch_install(void);  /* edge_watch.c (PS3_EDGE_WATCH) */

int main(int argc, char** argv)
{
    if (argc < 2) {
        printf("usage: %s <PPU ELF>\n", argv[0]);
        return 2;
    }
    /* PS3RECOMP_ENG_GPU_LIGHT: the SPU light job's pixel work moves to the GPU. */
    infamous2_gpu_light_install();
    infamous2_gpu_ssao_install();
    infamous2_edge_watch_install();

#ifdef _WIN32
#pragma comment(lib, "winmm.lib")
    /* 1 ms timer resolution. The default granularity is about 15.6 ms, which
     * inflates every shorter wait the title makes and throttles the whole
     * thing. POSIX timers are already fine-grained. */
    timeBeginPeriod(1);
    setvbuf(stdout, NULL, _IONBF, 0);   /* unbuffered: do not lose prints on a kill */
#endif

    printf("=== ps3recomp game runner ===\n");

    /* Before any thread exists: keep macOS from App-Napping or timer-throttling
     * the run, and with PS3_QOS=1 put the main thread (the guest's own when
     * headless) at USER_INTERACTIVE; the PPU/SPU threads then do the same at
     * their entry. */
    infamous2_host_begin_activity();
    infamous2_host_qos_self();
#if defined(__APPLE__)
    printf("[host] App Nap off; thread QoS %s\n", infamous2_host_qos_enabled()
           ? "USER_INTERACTIVE for PPU/SPU/RSX threads (PS3_QOS=1)"
           : "left at the system default (PS3_QOS=1 raises it)");
#endif

    if (!alloc_guest_vm()) {
        fprintf(stderr, "ERROR: could not allocate the guest address space\n");
        return 1;
    }

    uint32_t entry = ppu_load_elf(argv[1]);
    if (!entry) {
        fprintf(stderr, "ERROR: could not load %s\n", argv[1]);
        return 1;
    }
    infamous2_apply_patches(argv[1]);   /* community patches from the player's RPCS3 patch.yml */
    infamous2_apply_aspect(argv[1]);    /* PS3_ASPECT: the cameras' aspect ratio, hor+ */

    derive_vfs_root(argv[1]);
    printf("[boot] VFS root: %s\n", ppu_vfs_root);

    /* Real title id, from the game's own PARAM.SFO. Without it cellGame keeps
     * its BLES00000 placeholder and builds every /dev_hdd0/game/<id> path from
     * that. Same lookup as runtime/ppu/tests/boot_main.cpp. */
    {
        static const char* const kSfoLayouts[] = {
            "%s/PS3_GAME/PARAM.SFO",   /* vfs root = disc root      */
            "%s/../PARAM.SFO",         /* vfs root = .../USRDIR     */
            "%s/PARAM.SFO",            /* vfs root = the game dir   */
        };
        char sfo[1100];
        int found = 0;
        for (size_t i = 0; i < sizeof kSfoLayouts / sizeof kSfoLayouts[0]; i++) {
            snprintf(sfo, sizeof sfo, kSfoLayouts[i], ppu_vfs_root);
            FILE* f = fopen(sfo, "rb");
            if (f) { fclose(f); found = 1; break; }
        }
        if (!found)
            snprintf(sfo, sizeof sfo, "%s/PS3_GAME/PARAM.SFO", ppu_vfs_root);
        cellGame_init_from_paramsfo(sfo);
    }

    ppu_recomp_register();   /* lifted function table -> address map */
    ps3_load_prx_modules();  /* this game's lifted system PRX, if it has any */
    ppu_hle_init();          /* firmware import NID -> HLE handlers */
    ppu_sysprx_register();   /* boot-critical CRT */
    ppu_fs_register();       /* cellFs over the game directory */
    lv2_init_syscalls();     /* the lv2 syscall table */

    /* Install the guest-callback hook, then start the frame clock. It no-ops
     * until the title registers its vblank and flip handlers during init. */
    g_ps3_guest_caller = harness_guest_caller;
    CreateThread(NULL, 4u * 1024 * 1024, frame_clock, NULL, 0, NULL);

    printf("\n[boot] dispatching entry OPD 0x%08X (stack top 0x%08X)\n\n",
           entry, STACK_TOP);

#if defined(__APPLE__)
    /* AppKit only runs on the process's main thread: the backend creates the
     * window and pumps its events with dispatch_sync onto the main queue. With
     * the guest on the main thread nothing ever serviced that queue, so the
     * frame clock blocked in its first event pump, the FIFO stopped draining
     * and the title spun for ever on a fence -- a grey window that never drew.
     * Windowed, the guest runs on its own thread (the 256 MB host stack every
     * guest thread gets) and the main thread turns the run loop, which drains
     * the main queue. Headless never touches AppKit and keeps the old layout. */
    int rc;
    if (!getenv("HEADLESS") && !getenv("PS3RECOMP_METAL_HEADLESS")) {
        struct boot_args { uint32_t entry; int rc; volatile int done; };
        static boot_args ba;
        ba.entry = entry; ba.rc = 0; ba.done = 0;
        pthread_attr_t attr;
        pthread_attr_init(&attr);
        pthread_attr_setstacksize(&attr, 256u * 1024u * 1024u);
        pthread_t th;
        auto body = [](void* a) -> void* {
            boot_args* b = (boot_args*)a;
            infamous2_host_qos_self();   /* the guest's main thread */
            b->rc = ppu_run(b->entry, STACK_TOP);
            __atomic_store_n(&b->done, 1, __ATOMIC_RELEASE);
            CFRunLoopStop(CFRunLoopGetMain());
            return nullptr;
        };
        if (pthread_create(&th, &attr, body, &ba) != 0) {
            fprintf(stderr, "[boot] could not start the guest thread\n");
            return 1;
        }
        pthread_attr_destroy(&attr);
        while (!__atomic_load_n(&ba.done, __ATOMIC_ACQUIRE))
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, false);
        pthread_join(th, nullptr);
        rc = ba.rc;
    } else {
        rc = ppu_run(entry, STACK_TOP);
    }
#else
    int rc = ppu_run(entry, STACK_TOP);
#endif
    printf("\n[boot] ppu_run returned %d (entry function unwound)\n", rc);

    /* A title that called sys_process_exit never reaches this line: that path
     * ends in the host exit(). Getting here means the entry function returned
     * instead, so hand back the status the guest published if it published
     * one. Returning a hardcoded 0 would report success for a run that never
     * got anywhere. */
    return g_sys_process_exit_called ? (int)g_sys_process_exit_code : 0;
}
