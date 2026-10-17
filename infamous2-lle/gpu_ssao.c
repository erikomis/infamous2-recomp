/*
 * inFAMOUS 2's SSAO, moved to the GPU (PS3RECOMP_ENG_GPU_SSAO=0F800000, with
 * PS3RECOMP_ENG_GPU_LIGHT on for the depth and normals copies).
 *
 * The SSAO is WWS job jobmod_e6fe749031f8 (lifted image 226), a generic tiled
 * image pipeline running five passes with code plugins: half-res linear depth
 * (label 136), a 12-sample AO, two depth-aware blurs and a bilateral 2x
 * upsample into the composite's B8 texture at VRAM 0x0F800000 (label 137).
 * The draw engine now runs those passes on the GPU (the backend's ssao_pass)
 * and binds the result for that texture. Here: the job's tile walk (lifted
 * 0x4330, reached through `bie`) is replaced by one that keeps the iterator,
 * the label PUTs and the pass chaining but does no tile work, and on each
 * frame's first step hands the engine the passes' constants, decoded from the
 * pass table the PPU wrote (ssao_gpu.c). Worked out from the lifted job and
 * checked against whole-job host runs (notes/ssao/).
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include "spu_context.h"
#include "spu_helpers.h"
#include "rsx_draw_engine.h"
#include "ssao_gpu.h"

extern uint8_t* vm_base;
static const uint8_t* ssao_guest(uint32_t ea, uint32_t size, void* u) { (void)u; (void)size; return vm_base + ea; }
static uint32_t ls32t(const spu_context* c, uint32_t a) { const uint8_t* p = c->ls + (a & 0x3FFFFu); return (uint32_t)p[0] << 24 | p[1] << 16 | p[2] << 8 | p[3]; }

/* LS 0x6500..: the job's header copy (h0..h3, pass count at +8, pass table EA
 * at +0xC), fetched by 0x45C8 on every step. Tap on the first step of a frame. */
static void ssao_tap_frame(spu_context* ctx)
{
    const uint8_t* h = ctx->ls + 0x6500;
    const uint32_t tab = ls32t(ctx, 0x650C);
    const int npass = h[8] << 8 | h[9];
    if (h[0] == 0xFF || npass < 5 || npass > 8) return;
    if ((h[0] << 8 | h[1]) != 0 || (h[2] << 8 | h[3]) != 0) return;
    const uint8_t* pass[8];
    for (int k = 0; k < npass; k++) pass[k] = vm_base + tab + 128u * k;
    SsaoGpuConsts c; SsaoJobInfo info;
    if (ssao_consts_from_passes(pass, npass, 0, ssao_guest, 0, &c, &info) == 0)
        rsx_draw_engine_gpu_ssao_data(&c, (uint32_t)sizeof c, c.dims[0], c.dims[1], c.dims[2], c.dims[3]);
}

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
/* 0x5258's contract: 0 = iterator finished, 2 = pass handed out but other jobs
 * still hold tiles (call again); 1 (callback yield) cannot happen here. */
static uint32_t ssao_skip_step(spu_context* ctx, u128 param)
{
    __atomic_add_fetch(&g_ssaoskip_steps, 1, __ATOMIC_RELAXED);
    ctx->gpr[3] = param;
    lcall(ctx, SJ(000045C8), 0x531C);
    ssao_tap_frame(ctx);
    for (;;) {
        lcall(ctx, SJ(00004A50), 0x532C);
        const uint32_t t = ctx->gpr[3]._u32[0] & 0xFFFFu;
        if (t == 0xFFFEu) return 2;
        if (t == 0xFFFFu) return 0;
        const uint32_t n = 1 + grab_rest(ctx);          /* tiles t .. t+n-1 are ours */
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

void jobmod_e6fe749031f8_e2060_spu_func_00004330(spu_context* ctx);
spu_fn spu_replace_function(int image_id, uint32_t addr, spu_fn fn);
#define SSAO_JOB_IMAGE 226

void infamous2_gpu_ssao_install(void)
{
    const char* e = getenv("PS3RECOMP_ENG_GPU_SSAO");
    const char* l = getenv("PS3RECOMP_ENG_GPU_LIGHT");
    if (!e || !*e || !l || !*l) return;
    const spu_fn old = spu_replace_function(SSAO_JOB_IMAGE, 0x4330u, ssao_skip_4330);
    if (old != jobmod_e6fe749031f8_e2060_spu_func_00004330) {
        if (old) spu_replace_function(SSAO_JOB_IMAGE, 0x4330u, old);
        fprintf(stderr, "[gpu ssao] image %d at 0x4330 is not the SSAO job's walk; SPU SSAO kept\n", SSAO_JOB_IMAGE);
        return;
    }
    fprintf(stderr, "[gpu ssao] SSAO job's tile work replaced (image %d, LS 0x4330); AO on the GPU\n", SSAO_JOB_IMAGE);
}
