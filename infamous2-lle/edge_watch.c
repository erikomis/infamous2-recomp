/*
 * PS3_EDGE_WATCH=1: a tripwire for the rare Edge geometry SPU fault.
 *
 * The Edge job (jobmod_d0d15ec83992, image 223) allocates output slots in an
 * atomic loop: LS 0x5398 disables interrupts (bid) and branches to 0x53A0,
 * which sets the loop's constant registers (r10 = the lock-line EA, r11 = LSA
 * 0x80, r13/r15 = the PUTLLC/GETLLAR commands, r14 = 0xFFFF); 0x53B8 does the
 * GETLLAR and the update, 0x5400 the PUTLLC, looping back to 0x53B8 on a lost
 * reservation, and finally re-enables interrupts (bie) into 0x5420. In the
 * faulting runs r11 held exactly r7's value by the time the loop went wrong,
 * so the lock line landed at the wrong LSA and the job soon branched into
 * vertex data. Nothing in the lifted loop writes r11 after 0x53A0.
 *
 * 0x53A0 is reached through the indirect branch of the `bid`, so it can be
 * replaced (spu_replace_function only sees registry dispatches). This is the
 * same loop as the lifted 0x53A0 + 0x53B8 + 0x5400, with the constant
 * registers checked after every runtime call: the report names the call that
 * changed one. PS3_EDGE_WATCH=2 also puts the register back and goes on.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "spu_context.h"
#include "spu_helpers.h"

typedef void (*spu_fn)(spu_context*);
void spu_wrch(spu_context* ctx, uint32_t channel, u128 value);
u128 spu_rdch(spu_context* ctx, uint32_t channel);
spu_fn spu_replace_function(int image_id, uint32_t addr, spu_fn fn);
void spu_irq_ring_dump(spu_context* ctx);

#define EDGE_JOB_IMAGE 223

static int s_mode;   /* 1 = report, 2 = report and repair */

typedef struct { u128 r10, r11, r13, r14, r15; } edge_regs;

static int edge_same(const u128* a, const u128* b) { return !memcmp(a, b, sizeof *a); }

static void edge_check(spu_context* ctx, const edge_regs* want, const char* where)
{
    struct { int n; const u128* w; } regs[5] = {
        { 10, &want->r10 }, { 11, &want->r11 }, { 13, &want->r13 }, { 14, &want->r14 }, { 15, &want->r15 } };
    for (int i = 0; i < 5; i++) {
        const int n = regs[i].n;
        if (edge_same(&ctx->gpr[n], regs[i].w)) continue;
        static int reports;
        if (reports++ < 16) {
            fprintf(stderr, "[edge-watch] spu=%X r%d changed after %s: was %08X %08X %08X %08X now %08X %08X %08X %08X"
                    " (steps=%llu ie=%d irq_saved=%d resume=0x%05X host_depth=%u)\n",
                    ctx->spu_id, n, where,
                    regs[i].w->_u32[0], regs[i].w->_u32[1], regs[i].w->_u32[2], regs[i].w->_u32[3],
                    ctx->gpr[n]._u32[0], ctx->gpr[n]._u32[1], ctx->gpr[n]._u32[2], ctx->gpr[n]._u32[3],
                    (unsigned long long)ctx->steps, ctx->int_enable, ctx->irq_saved,
                    ctx->irq_resume_pc & SPU_LS_MASK, ctx->host_depth);
            spu_irq_ring_dump(ctx);
            fflush(stderr);
        }
        if (s_mode >= 2) ctx->gpr[n] = *regs[i].w;
    }
}

/* LS 0x53A0..0x5420, as the lifted functions do it, checked. */
static void edge_alloc_loop(spu_context* ctx)
{
    ctx->gpr[11] = spu_il(128);
    ctx->gpr[15] = spu_il(208);
    ctx->gpr[10] = spu_ori(ctx->gpr[3], 0);
    ctx->gpr[14] = spu_ila(0xFFFF);
    ctx->gpr[13] = spu_il(180);
    const edge_regs want = { ctx->gpr[10], ctx->gpr[11], ctx->gpr[13], ctx->gpr[14], ctx->gpr[15] };
    for (;;) {
        /* 0x53B8 */
        ctx->gpr[12] = spu_il(0);
        spu_wrch(ctx, MFC_LSA, ctx->gpr[11]);
        edge_check(ctx, &want, "wrch MFC_LSA (getllar)");
        spu_wrch(ctx, MFC_EAL, ctx->gpr[10]);
        edge_check(ctx, &want, "wrch MFC_EAL (getllar)");
        spu_wrch(ctx, MFC_Cmd, ctx->gpr[15]);
        edge_check(ctx, &want, "wrch MFC_Cmd GETLLAR");
        ctx->gpr[2] = spu_rdch(ctx, MFC_RdAtomicStat);
        edge_check(ctx, &want, "rdch MFC_RdAtomicStat (getllar)");
        ctx->gpr[7] = spu_chd(ctx->gpr[1], 4);
        ctx->gpr[2] = spu_ls_read128(ctx, 0x80);
        ctx->gpr[8] = spu_rotqbyi(ctx->gpr[2], 2);
        ctx->gpr[9] = spu_rotqbyi(ctx->gpr[2], 12);
        ctx->gpr[6] = spu_and(ctx->gpr[14], ctx->gpr[8]);
        ctx->gpr[5] = spu_ahi(ctx->gpr[8], 1);
        ctx->gpr[4] = spu_clgth(ctx->gpr[2], ctx->gpr[8]);
        ctx->gpr[6] = spu_shli(ctx->gpr[6], 5);
        ctx->gpr[2] = spu_shufb(ctx->gpr[5], ctx->gpr[2], ctx->gpr[7]);
        if (ctx->gpr[4]._u16[0] != 0) {
            ctx->gpr[12] = spu_a(ctx->gpr[9], ctx->gpr[6]);
            spu_ls_write128(ctx, 0x80, ctx->gpr[2]);
        }
        edge_check(ctx, &want, "the slot update (no runtime call)");
        /* 0x5400 */
        spu_wrch(ctx, MFC_LSA, ctx->gpr[11]);
        edge_check(ctx, &want, "wrch MFC_LSA (putllc)");
        spu_wrch(ctx, MFC_EAL, ctx->gpr[10]);
        edge_check(ctx, &want, "wrch MFC_EAL (putllc)");
        spu_wrch(ctx, MFC_Cmd, ctx->gpr[13]);
        edge_check(ctx, &want, "wrch MFC_Cmd PUTLLC");
        ctx->gpr[17] = spu_rdch(ctx, MFC_RdAtomicStat);
        edge_check(ctx, &want, "rdch MFC_RdAtomicStat (putllc)");
        if (ctx->gpr[17]._u32[0] == 0) break;
    }
    ctx->gpr[2] = spu_ila(0x5420);
    ctx->int_enable = 1;
    ctx->pc = ctx->gpr[2]._u32[0];
    g_spu_trampoline_fn = spu_indirect_branch;
}

void infamous2_edge_watch_install(void)
{
    const char* e = getenv("PS3_EDGE_WATCH");
    s_mode = e ? atoi(e) : 0;
    if (s_mode <= 0) return;
    if (spu_replace_function(EDGE_JOB_IMAGE, 0x53A0, edge_alloc_loop))
        fprintf(stderr, "[edge-watch] Edge job slot allocator (image %d, LS 0x53A0) watched%s\n",
                EDGE_JOB_IMAGE, s_mode >= 2 ? ", registers repaired" : "");
    else
        fprintf(stderr, "[edge-watch] LS 0x53A0 of image %d not registered: watch inactive\n", EDGE_JOB_IMAGE);
}
