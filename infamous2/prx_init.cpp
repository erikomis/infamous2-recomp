/*
 * inFAMOUS 2 -- real libsre for the SPURS lock-free queue.
 *
 * The title streams its psarc data through an SPU inflate task fed by a
 * CellSpursLFQueue (ANY2ANY). The consumer is the title's own recompiled SPU
 * code, which reads and commits the queue's 128-byte line itself, so the PPU
 * side has to speak Sony's exact protocol. The HLE library has no
 * _cellSpursLFQueuePushBody and lays the line out differently from libsre, so
 * the queue's PPU entry points are served by the user's own lifted libsre
 * instead (fw/ and prx/ are local, never redistributed -- see
 * ps3recomp/docs/FIRMWARE_LLE.md).
 *
 * Only the queue NIDs below are published. Everything else in cellSpurs stays
 * on the HLE library, whose taskset and kernel model libsre's own SPURS
 * internals would not recognise.
 */

#include "ppu_recomp.h"
#include "prx_loader.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define LIBSRE_BASE 0x30000000u   /* must match tools/lift_prx.py --base */

extern "C" {
/* prx/libsre_ns: the lifter's func_entry is binary-compatible with
 * prx_func_entry. */
extern const prx_func_entry libsre_function_table[];
extern const uint64_t       libsre_function_table_count;

void ppu_register_function(uint64_t addr, void (*fn)(void*));
void spu_taskset_signal_task(uint32_t taskset_ea, uint32_t taskId);
extern uint32_t g_ydkj_real_taskset_ea;   /* libs/spurs: the HLE taskset */
}

/* NID -> base-0 OPD vaddr, from fw/out/libsre_exports.json. */
static const prx_export k_queue_exports[] = {
    { 0x011EE38Bu, 0x031844u },   /* _cellSpursLFQueueInitialize */
    { 0x8A85674Du, 0x03184Cu },   /* _cellSpursLFQueuePushBody   */
    { 0x35DAE22Bu, 0x031854u },   /* _cellSpursLFQueuePopBody    */
};

/* libsre's queue code wakes a waiting consumer through a callback,
 * fpSendSignal(eaSignal, value) at 0x300127CC. eaSignal is the taskset, or
 * (spurs | 1) for a queue owned by the SPURS instance, in which case value
 * carries the waiter's workload id above its task id. The real one goes on
 * into libsre's SPURS kernel state; ours is the HLE one, so deliver the signal
 * through the runtime's task-signal channel instead. */
#define LIBSRE_FP_SEND_SIGNAL 0x300127CCu

static void libsre_send_signal_hle(void* vctx)
{
    ppu_context* ctx = (ppu_context*)vctx;
    uint32_t owner  = (uint32_t)ctx->gpr[3];
    uint32_t value  = (uint32_t)ctx->gpr[4];
    uint32_t taskId = value & 0xFFu;
    uint32_t taskset = ((owner & 0xFu) == 1u) ? g_ydkj_real_taskset_ea
                                              : (owner & ~0xFu);

    static int _n = 0;
    if (_n++ < 16)
        fprintf(stderr, "[libsre] LFQueue signal owner=0x%08X value=0x%X -> taskset 0x%08X task %u\n",
                owner, value, taskset, taskId);

    spu_taskset_signal_task(taskset, taskId);
    ctx->gpr[3] = 0;
}

static void prx_register(uint32_t addr, void (*host)(void*))
{
    ppu_register_function((uint64_t)addr, host);
}

extern "C" void ps3_load_prx_modules(void)
{
    const char* env = getenv("INFAMOUS2_LIBSRE_IMAGE");
    const char* candidates[] = {
        env && *env ? env : "fw/out/libsre_image.bin",
        "fw/out/libsre_image.bin",
        "../fw/out/libsre_image.bin",
    };
    uint8_t* img = nullptr; uint32_t sz = 0;
    for (size_t i = 0; i < sizeof candidates / sizeof candidates[0] && !img; i++)
        img = prx_image_load_file(candidates[i], &sz);
    if (!img) {
        fprintf(stderr, "[init] libsre image not found -- SPURS LFQueue stays unimplemented "
                        "(set INFAMOUS2_LIBSRE_IMAGE)\n");
        return;
    }

    prx_module m;
    memset(&m, 0, sizeof m);
    m.name         = "libsre";
    m.base         = LIBSRE_BASE;
    m.image        = img;
    m.image_size   = sz;
    m.funcs        = libsre_function_table;
    m.func_count   = libsre_function_table_count;
    m.exports      = k_queue_exports;
    m.export_count = sizeof k_queue_exports / sizeof k_queue_exports[0];

    prx_load_result r = prx_load_module(&m, prx_register);
    free(img);
    if (!r.ok) return;

    /* After the load, so it replaces the lifted function at that address. */
    ppu_register_function(LIBSRE_FP_SEND_SIGNAL, libsre_send_signal_hle);
}
