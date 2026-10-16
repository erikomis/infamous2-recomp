/*
 * inFAMOUS 2 SSAO job (WWS jobmod_e6fe749031f8, lifted image 226): skip every
 * pass's tile work (datamod DMA, tile GETL/PUTL, datamod run), keep the
 * iterator and completion protocol (labels 136/137, pass chaining).
 *
 * Replaces lifted 0x4330 (reached from 0x4308 through `bie`, i.e. through
 * spu_indirect_branch, so spu_replace_function(226, 0x4330, ...) works; 0x437C
 * and 0x5258 are direct calls and cannot be swapped).
 *
 * Original 0x4330..0x43A8: build the config at sp+0x20/0x30 (callbacks
 * {0x4030 time check -> 0, 0x4098 datamod init, 0x4038 datamod run, 0}),
 * 0x4598 copies it to LS 0x6400/0x6410, then
 *     do rc = step(buf1.w0) while (rc != 0);   step = 0x5258
 * step: 0x45C8 header GET (EA -> LS 0x6500, LS 0x6420 = EA), then per tile
 * 0x4A50 claim (atomic h1++; loads the 128-byte pass block hdr.w3 + 128*h0 ->
 * LS 0x6380 when the pass changes; 0xFFFE = pass handed out, not finished;
 * 0xFFFF = all passes done), datamod GET to 0x6C00, GETL inputs, datamod,
 * PUTL outputs, and 0x46D0 completion after the PUT (atomic h2++; the job
 * that makes h2 == h3 PUTs pass+8 to the EA at pass+4 if pass byte 3 bit 0,
 * then h0++ (h1 = h2 = 0, h3 = tiles of the next pass) or h0 = 0xFFFF).
 */
#include <stdint.h>
#include "spu_context.h"
#include "spu_helpers.h"
typedef void (*spu_fn)(spu_context*);
u128 spu_rdch(spu_context* ctx, uint32_t channel);
void spu_wrch(spu_context* ctx, uint32_t channel, u128 value);
#define SJ(a) jobmod_e6fe749031f8_e2060_spu_func_##a
void SJ(00004598)(spu_context*);   /* copy config sp+0x20/0x30 -> LS 0x6400/0x6410 */
void SJ(000045C8)(spu_context*);   /* header GET -> LS 0x6500, LS 0x6420 = state EA */
void SJ(00004A50)(spu_context*);   /* claim one tile (atomic h1++) */
void SJ(000046D0)(spu_context*);   /* tile finished (atomic h2++, label on last, next pass) */

static uint32_t ls32(const spu_context* c, uint32_t a) { const uint8_t* p = c->ls + (a & SPU_LS_MASK); return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }
static uint32_t ls16(const spu_context* c, uint32_t a) { const uint8_t* p = c->ls + (a & SPU_LS_MASK); return (uint32_t)p[0] << 8 | p[1]; }
static void ls16w(spu_context* c, uint32_t a, uint32_t v) { uint8_t* p = c->ls + (a & SPU_LS_MASK); p[0] = (uint8_t)(v >> 8); p[1] = (uint8_t)v; }
static void lcall(spu_context* ctx, spu_fn f, uint32_t ret)
{
    ctx->gpr[0] = spu_link(ret);
    const int32_t si = (int32_t)ctx->image_id;
    ctx->host_depth++; f(ctx); spu_drain_call(ctx, ret); ctx->host_depth--;
    spu_img_restore(ctx, si);
}
#define ST_LINE_LS 0x6480u   /* the job's own reservation buffer (0x4A70/0x4758) */
#define MFC_GETLLAR 0xD0u
#define MFC_PUTLLC  0xB4u
static uint32_t mfc_atomic(spu_context* ctx, uint32_t ea_line, uint32_t cmd)
{
    spu_wrch(ctx, MFC_LSA, spu_splat_u32(ST_LINE_LS)); spu_wrch(ctx, MFC_EAH, spu_splat_u32(0));
    spu_wrch(ctx, MFC_EAL, spu_splat_u32(ea_line)); spu_wrch(ctx, MFC_Size, spu_splat_u32(128));
    spu_wrch(ctx, MFC_TagID, spu_splat_u32(0)); spu_wrch(ctx, MFC_Cmd, spu_splat_u32(cmd));
    return spu_rdch(ctx, MFC_RdAtomicStat)._u32[0];
}
/* state EA = LS 0x6420.w0 (0x45C8); line mask = LS 0x6264 (0xFFFFFF80) */
static uint32_t grab_rest(spu_context* ctx)
{
    const uint32_t ea = ls32(ctx, 0x6420), line = ea & ls32(ctx, 0x6264), st = ST_LINE_LS + (ea & 0x78);
    for (;;) {
        mfc_atomic(ctx, line, MFC_GETLLAR); spu_arch_fence();
        const uint32_t h0 = ls16(ctx, st), h1 = ls16(ctx, st + 2), h3 = ls16(ctx, st + 6);
        if (h0 == 0xFFFFu || h1 >= h3) return 0;
        ls16w(ctx, st + 2, h3); spu_arch_fence();
        if (mfc_atomic(ctx, line, MFC_PUTLLC) == 0) return h3 - h1;
    }
}
static void add_done(spu_context* ctx, uint32_t k)
{
    const uint32_t ea = ls32(ctx, 0x6420), line = ea & ls32(ctx, 0x6264), st = ST_LINE_LS + (ea & 0x78);
    for (;;) {
        mfc_atomic(ctx, line, MFC_GETLLAR); spu_arch_fence();
        ls16w(ctx, st + 4, ls16(ctx, st + 4) + k); spu_arch_fence();
        if (mfc_atomic(ctx, line, MFC_PUTLLC) == 0) return;
    }
}
uint64_t g_ssaoskip_steps, g_ssaoskip_tiles, g_ssaoskip_passes;
/* optional hook: a pass is about to be retired (LS 0x6380 = its 128-byte block) */
void ssao_pass_hook(spu_context* ctx) __attribute__((weak));
/* 0x5258's contract: 0 = iterator finished, 2 = pass handed out but other jobs
 * still hold tiles (call again); 1 (callback yield) cannot happen here. */
static uint32_t ssao_skip_step(spu_context* ctx, u128 param)
{
    __atomic_add_fetch(&g_ssaoskip_steps, 1, __ATOMIC_RELAXED);
    ctx->gpr[3] = param;
    lcall(ctx, SJ(000045C8), 0x531C);
    for (;;) {
        lcall(ctx, SJ(00004A50), 0x532C);
        const uint32_t t = ctx->gpr[3]._u32[0] & 0xFFFFu;
        if (t == 0xFFFEu) return 2;
        if (t == 0xFFFFu) return 0;
        const uint32_t n = 1 + grab_rest(ctx);          /* tiles t .. t+n-1 are ours */
        if (ssao_pass_hook) ssao_pass_hook(ctx);
        if (n > 1) add_done(ctx, n - 1);
        lcall(ctx, SJ(000046D0), 0x5748);               /* last one: label, next pass */
        __atomic_add_fetch(&g_ssaoskip_tiles, n, __ATOMIC_RELAXED);
        __atomic_add_fetch(&g_ssaoskip_passes, 1, __ATOMIC_RELAXED);
    }
}
/* Drop-in for lifted 0x4330. Same registers in, same continuation out. */
void ssao_skip_4330(spu_context* ctx)
{
    /* 0x4330..0x4378 verbatim (spu_recomp.c 277-295): the config record. */
    ctx->gpr[17] = spu_ila(0x4030);
    ctx->gpr[5] = spu_fsmbi(0x0);
    ctx->gpr[8] = spu_ila(0x4098);
    ctx->gpr[6] = spu_ila(0x4038);
    ctx->gpr[16] = spu_ls_read128(ctx, ctx->gpr[1]._u32[0] + 0x30);
    ctx->gpr[3] = spu_ai(ctx->gpr[1], 32);
    ctx->gpr[10] = spu_cwd(ctx->gpr[1], 8);
    ctx->gpr[13] = spu_ls_read128(ctx, ctx->gpr[1]._u32[0] + 0x20);
    ctx->gpr[15] = spu_chd(ctx->gpr[1], 12);
    ctx->gpr[9] = spu_cwd(ctx->gpr[1], 12);
    ctx->gpr[12] = spu_shufb(ctx->gpr[17], ctx->gpr[16], ctx->gpr[81]);
    ctx->gpr[11] = spu_shufb(ctx->gpr[14], ctx->gpr[13], ctx->gpr[15]);
    ctx->gpr[7] = spu_shufb(ctx->gpr[8], ctx->gpr[12], ctx->gpr[82]);
    spu_ls_write128(ctx, ctx->gpr[1]._u32[0] + 0x20, ctx->gpr[11]);
    ctx->gpr[4] = spu_shufb(ctx->gpr[6], ctx->gpr[7], ctx->gpr[10]);
    ctx->gpr[2] = spu_shufb(ctx->gpr[5], ctx->gpr[4], ctx->gpr[9]);
    spu_ls_write128(ctx, ctx->gpr[1]._u32[0] + 0x30, ctx->gpr[2]);
    lcall(ctx, SJ(00004598), 0x437C);
    /* 0x437C loop, with the step replaced */
    for (;;) {
        u128 q = spu_ls_read128(ctx, ctx->gpr[83]._u32[0]);
        u128 p = spu_rotqby(q, ctx->gpr[83]);
        if (ssao_skip_step(ctx, p) == 0) break;
    }
    /* 0x4390..0x43A4 verbatim: tail to 0x43A8 (free the three tags) */
    ctx->gpr[83] = spu_ila(0xFFFF);
    ctx->gpr[75] = spu_ls_read128(ctx, ctx->gpr[1]._u32[0] + 0x20);
    ctx->gpr[74] = spu_rotqbyi(ctx->gpr[75], 6);
    ctx->gpr[3] = spu_and(ctx->gpr[74], ctx->gpr[83]);
    ctx->gpr[2] = spu_ila(0x43A8);
    ctx->int_enable = 0; ctx->pc = ctx->gpr[2]._u32[0]; g_spu_trampoline_fn = spu_indirect_branch; return;
}
