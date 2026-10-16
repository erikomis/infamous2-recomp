/*
 * inFAMOUS 2's SPU light pass, moved to the GPU (PS3RECOMP_ENG_GPU_LIGHT).
 *
 * Each frame the RSX copies the normals target and the depth buffer into two
 * main-memory buffers, the WWS job jobmod_c6e7383dc013 lights them in place in
 * 32x40 tiles on the SPUs, and the composite samples the results back. With
 * PS3RECOMP_ENG_GPU_LIGHT=07000B80:07384B80 the draw engine computes the same
 * light on the GPU (rsx_draw_engine.c, light_pass) and binds it for those two
 * buffers. What is left here is the job's side: its per-tile routine (LS
 * 0x4AC8, the callback the job hands to its tile loop) is replaced by one that
 * does no pixel work and, on tile (0,0), passes the frame's constants and light
 * list -- exactly what the job would have used -- to the engine.
 *
 * The routine's parameter block (r3): +0x04 tile x, +0x08 tile y. The job keeps
 * an LS pointer to its frame block at 0x9C80 (matrix at +0x10, light count at
 * +0x90, scale at +0x98) and to its light array (0x30 bytes per light) at
 * 0x9CB0. Worked out from the lifted job; the GPU shader reproduces its output
 * on a captured frame (ROADMAP, "SPU lighting on the GPU").
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

#include "spu_context.h"
#include "spu_helpers.h"
#include "rsx_draw_engine.h"

typedef void (*spu_fn)(spu_context*);
spu_fn spu_replace_function(int image_id, uint32_t addr, spu_fn fn);
void jobmod_c6e7383dc013_e5400_spu_func_00004AC8(spu_context* ctx);
void jobmod_c6e7383dc013_e5400_spu_func_00004578(spu_context* ctx);

/* The lifted image id jobmods_register.c gives the light job. */
#define LIGHT_JOB_IMAGE 219
#define LIGHT_JOB_TILE  0x4AC8u

static uint32_t ls_u32(const spu_context* ctx, uint32_t a)
{
    const uint8_t* p = ctx->ls + (a & 0x3FFFFu);
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

static spu_fn s_spu_tile;   /* the job's own routine, when PS3RECOMP_ENG_GPU_LIGHT_SPU=1 keeps it */

/* The frame's constants and light list, as the job holds them in its LS. */
void light_tap_constants(spu_context* ctx)
{
    const uint32_t frame = ls_u32(ctx, 0x9C80) & 0x3FFF0u;
    const uint32_t lights = ls_u32(ctx, 0x9CB0) & 0x3FFF0u;
    uint32_t count = ls_u32(ctx, frame + 0x90);
    if (count > RSX_GPU_LIGHT_MAX) count = RSX_GPU_LIGHT_MAX;
    if (frame + 0x100 <= 0x40000u && lights + count * 0x30u <= 0x40000u)
        rsx_draw_engine_gpu_light_data(ctx->ls + frame, ctx->ls + lights, count);
}

static void light_tile_gpu(spu_context* ctx)
{
    const uint32_t prm = ctx->gpr[3]._u32[0];
    if (ls_u32(ctx, prm + 4) == 0 && ls_u32(ctx, prm + 8) == 0) light_tap_constants(ctx);
    if (s_spu_tile) { s_spu_tile(ctx); return; }   /* it returns (or chains) itself */
    SPU_RET(ctx);
}

/* ---- The job's tile walk, skipped (LS 0x4578) ------------------------------
 * With the pixel work gone the job still walked all 720 tiles: GET lists of
 * both buffers' rows, the callback, PUT lists back (of output LS buffers the
 * stub never filled, i.e. garbage over the normals and depth copies the SSAO
 * job also reads), ~115k list elements a frame, and the RSX waited for it on
 * label 135. This replaces main()'s loop continuation: same tile iterator
 * atomics, same final completion (label 135, next pass, 0xFFFF marker), same
 * yield bookkeeping, no tile DMA and no callbacks. Worked out and checked
 * against a whole-job host run of a captured frame (same label write, same
 * iterator end state, same yield counter). */

/* Channel entry points (declared by the lifted images' spu_recomp.h). */
u128 spu_rdch(spu_context* ctx, uint32_t channel);
void spu_wrch(spu_context* ctx, uint32_t channel, u128 value);

#define LJ(a) jobmod_c6e7383dc013_e5400_spu_func_##a
void LJ(00005610)(spu_context*);   /* copy config sp+0x20/0x30 -> LS 0x9E00/0x9E10 */
void LJ(00005640)(spu_context*);   /* iterator header EA -> LS 0x9F00, 0x9E20 = EA */
void LJ(00005AC8)(spu_context*);   /* grab next tile (atomic h1++) */
void LJ(00005748)(spu_context*);   /* tile finished (atomic h2++, label on last) */
void LJ(00009548)(spu_context*);   /* atomic_add32(r3 = EA, r4 = value) */
void LJ(000045E0)(spu_context*);   /* main(): free the DMA tags, return to the WWS kernel */

/* The GPU light path's per-frame tap (gpu_light.c light_tile_gpu, tile (0,0)
 * branch): frame constants at [0x9C80], light list at [0x9CB0]. */

static uint32_t ls32(const spu_context* ctx, uint32_t a)
{
    const uint8_t* p = ctx->ls + (a & SPU_LS_MASK);
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}
static uint32_t ls16(const spu_context* ctx, uint32_t a)
{
    const uint8_t* p = ctx->ls + (a & SPU_LS_MASK);
    return ((uint32_t)p[0] << 8) | p[1];
}
static void ls16w(spu_context* ctx, uint32_t a, uint32_t v)
{
    uint8_t* p = ctx->ls + (a & SPU_LS_MASK);
    p[0] = (uint8_t)(v >> 8); p[1] = (uint8_t)v;
}

/* A brsl/brasl to a lifted function, the way the lifted code makes one. */
static void lcall(spu_context* ctx, spu_fn f, uint32_t ret)
{
    ctx->gpr[0] = spu_link(ret);
    const int32_t si = (int32_t)ctx->image_id;
    ctx->host_depth++;
    f(ctx);
    spu_drain_call(ctx, ret);
    ctx->host_depth--;
    spu_img_restore(ctx, si);
}
/* A bisl to an LS address (registry dispatch), as 0x62D0 calls 0x9E10.w0. */
static void lcall_ls(spu_context* ctx, uint32_t target, uint32_t ret)
{
    ctx->gpr[0] = spu_link(ret);
    const int32_t si = (int32_t)ctx->image_id;
    ctx->pc = target;
    ctx->host_depth++;
    spu_indirect_branch(ctx);
    spu_drain_call(ctx, ret);
    ctx->host_depth--;
    spu_img_restore(ctx, si);
}

/* 0x62D0 calls the config's time check (0x4388: frame block +8 = budget in
 * decrementer ticks, 0 = none) at entry (link 0x639C) and after each tile
 * (link 0x68E8); nonzero = budget spent, the job yields. */
static int time_up(spu_context* ctx, uint32_t ret)
{
    lcall_ls(ctx, ls32(ctx, 0x9E10), ret);
    return ctx->gpr[3]._u32[0] != 0;
}

/* GETLLAR / PUTLLC of the iterator's line into LS 0x9E80, with the channel
 * sequence and operands 0x5870 / 0x5814 use (tag 0, size 128). */
#define ST_LINE_LS 0x9E80u
static uint32_t mfc_atomic(spu_context* ctx, uint32_t ea_line, uint32_t cmd)
{
    spu_wrch(ctx, MFC_LSA, spu_splat_u32(ST_LINE_LS));
    spu_wrch(ctx, MFC_EAH, spu_splat_u32(0));
    spu_wrch(ctx, MFC_EAL, spu_splat_u32(ea_line));
    spu_wrch(ctx, MFC_Size, spu_splat_u32(128));
    spu_wrch(ctx, MFC_TagID, spu_splat_u32(0));
    spu_wrch(ctx, MFC_Cmd, spu_splat_u32(cmd));
    return spu_rdch(ctx, MFC_RdAtomicStat)._u32[0];
}
#define MFC_GETLLAR 0xD0u
#define MFC_PUTLLC  0xB4u

/* Hand out every tile left in the current pass in one reservation: h1 = h3.
 * Returns how many were taken. Only called right after 0x5AC8 gave this job a
 * tile, so the pass cannot change underneath (that needs h2 == h3, and our
 * tile is not finished). */
static uint32_t grab_rest(spu_context* ctx)
{
    const uint32_t ea = ls32(ctx, 0x9E20);
    const uint32_t line = ea & ls32(ctx, 0x9884);          /* 0xFFFFFF80, as 0x5870 */
    const uint32_t st = ST_LINE_LS + (ea & 0x78);
    for (;;) {
        mfc_atomic(ctx, line, MFC_GETLLAR);
        spu_arch_fence();
        const uint32_t h0 = ls16(ctx, st), h1 = ls16(ctx, st + 2), h3 = ls16(ctx, st + 6);
        if (h0 == 0xFFFFu || h1 >= h3) return 0;          /* 0x5DC4: no store */
        ls16w(ctx, st + 2, h3);
        spu_arch_fence();
        if (mfc_atomic(ctx, line, MFC_PUTLLC) == 0) return h3 - h1;
    }
}

/* k completions that cannot be the pass's last one: h2 += k. */
static void add_done(spu_context* ctx, uint32_t k)
{
    const uint32_t ea = ls32(ctx, 0x9E20);
    const uint32_t line = ea & ls32(ctx, 0x9884);
    const uint32_t st = ST_LINE_LS + (ea & 0x78);
    for (;;) {
        mfc_atomic(ctx, line, MFC_GETLLAR);
        spu_arch_fence();
        ls16w(ctx, st + 4, ls16(ctx, st + 4) + k);
        spu_arch_fence();
        if (mfc_atomic(ctx, line, MFC_PUTLLC) == 0) return;
    }
}

/* Counters the harness / a perf log can read (SPU threads share them). */
uint64_t g_lightskip_steps, g_lightskip_tiles, g_lightskip_yields;

/* 0x62D0's contract: 0 = iterator finished, 1 = time budget spent (yield),
 * 2 = this pass is handed out but other jobs still hold tiles (call again). */
static uint32_t light_skip_step(spu_context* ctx, uint32_t state_ea)
{
    __atomic_add_fetch(&g_lightskip_steps, 1, __ATOMIC_RELAXED);
    ctx->gpr[3] = spu_splat_u32(state_ea);
    lcall(ctx, LJ(00005640), 0x6394);
    if (time_up(ctx, 0x639C)) return 1;
    for (;;) {
        lcall(ctx, LJ(00005AC8), 0x63A4);
        const uint32_t t = ctx->gpr[3]._u32[0] & 0xFFFFu;
        if (t == 0xFFFEu) return 2;
        if (t == 0xFFFFu) return 0;
        const uint32_t n = 1 + grab_rest(ctx);           /* tiles t .. t+n-1 are ours */
        if (t == 0) light_tap_constants(ctx);            /* tile (0,0): what light_tile_gpu did */
        if (n > 1) add_done(ctx, n - 1);
        { /* PS3RECOMP_ENG_GPU_LIGHT_DELAY_US=<us>: hold the final completion
           * that long (A/B for a timing assumption in the PPU kicker). */
          static int dly = -1;
          if (dly < 0) { const char* e = getenv("PS3RECOMP_ENG_GPU_LIGHT_DELAY_US"); dly = e ? atoi(e) : 0; }
          if (dly > 0) usleep((useconds_t)dly); }
        lcall(ctx, LJ(00005748), 0x67C0);                /* last one: label 135, next pass */
        __atomic_add_fetch(&g_lightskip_tiles, n, __ATOMIC_RELAXED);
        if (time_up(ctx, 0x68E8)) return 1;
    }
}

/* Drop-in for lifted 0x4578. Same registers in, same continuation out. */
void light_skip_4578(spu_context* ctx)
{
    /* 0x4578..0x45C0, verbatim (spu_recomp.c 413-430): the config record. */
    ctx->gpr[17] = spu_ila(0x4388);
    ctx->gpr[5] = spu_fsmbi(0x0);
    ctx->gpr[8] = spu_ila(0x4030);
    ctx->gpr[6] = spu_ila(0x4038);
    ctx->gpr[16] = spu_ls_read128(ctx, ctx->gpr[1]._u32[0] + 0x30);
    ctx->gpr[3] = spu_ai(ctx->gpr[1], 32);
    ctx->gpr[10] = spu_cwd(ctx->gpr[1], 8);
    ctx->gpr[13] = spu_ls_read128(ctx, ctx->gpr[1]._u32[0] + 0x20);
    ctx->gpr[15] = spu_chd(ctx->gpr[1], 12);
    ctx->gpr[9] = spu_cwd(ctx->gpr[1], 12);
    ctx->gpr[12] = spu_shufb(ctx->gpr[17], ctx->gpr[16], ctx->gpr[82]);
    ctx->gpr[11] = spu_shufb(ctx->gpr[14], ctx->gpr[13], ctx->gpr[15]);
    ctx->gpr[7] = spu_shufb(ctx->gpr[8], ctx->gpr[12], ctx->gpr[83]);
    spu_ls_write128(ctx, ctx->gpr[1]._u32[0] + 0x20, ctx->gpr[11]);
    ctx->gpr[4] = spu_shufb(ctx->gpr[6], ctx->gpr[7], ctx->gpr[10]);
    ctx->gpr[2] = spu_shufb(ctx->gpr[5], ctx->gpr[4], ctx->gpr[9]);
    spu_ls_write128(ctx, ctx->gpr[1]._u32[0] + 0x30, ctx->gpr[2]);
    lcall(ctx, LJ(00005610), 0x45C4);

    /* 0x45C4 loop / 0x466C yield, with 0x62D0 replaced. r84 = frame block LS
     * (callee-saved in main, preserved by every lifted callee used here). */
    for (;;) {
        const uint32_t fb = ctx->gpr[84]._u32[0];
        const uint32_t rc = light_skip_step(ctx, ls32(ctx, fb + 0));
        if (rc == 1) {
            __atomic_add_fetch(&g_lightskip_yields, 1, __ATOMIC_RELAXED);
            ctx->gpr[3] = spu_splat_u32(ls32(ctx, fb + 4));   /* 0x466C: EA of the yield counter */
            ctx->gpr[4] = spu_il(1);
            lcall(ctx, LJ(00009548), 0x4688);
            break;
        }
        if (rc == 0) break;
    }
    ctx->pc = 0x45E0; g_spu_trampoline_fn = LJ(000045E0); return;
}



void infamous2_gpu_light_install(void)
{
    const char* e = getenv("PS3RECOMP_ENG_GPU_LIGHT");
    if (!e || !*e) return;
    const spu_fn old = spu_replace_function(LIGHT_JOB_IMAGE, LIGHT_JOB_TILE, light_tile_gpu);
    if (old != jobmod_c6e7383dc013_e5400_spu_func_00004AC8) {
        if (old) spu_replace_function(LIGHT_JOB_IMAGE, LIGHT_JOB_TILE, old);
        fprintf(stderr, "[gpu light] image %d at 0x%X is not the light job's tile routine; "
                        "SPU pixel work left in place\n", LIGHT_JOB_IMAGE, LIGHT_JOB_TILE);
        return;
    }
    /* PS3RECOMP_ENG_GPU_LIGHT_SPU=1 keeps the SPU job's own pixel work too (A/B:
     * the GPU result is still what the composite samples). */
    const char* keep = getenv("PS3RECOMP_ENG_GPU_LIGHT_SPU");
    if (keep && *keep == '1') s_spu_tile = old;
    /* PS3RECOMP_ENG_GPU_LIGHT_WALK=1 keeps the tile walk (A/B); it is kept
     * anyway when the SPU still lights (its PUTs then carry the light). */
    const char* walk = getenv("PS3RECOMP_ENG_GPU_LIGHT_WALK");
    if (!s_spu_tile && !(walk && *walk == '1')) {
        const spu_fn w = spu_replace_function(LIGHT_JOB_IMAGE, 0x4578u, light_skip_4578);
        if (w != jobmod_c6e7383dc013_e5400_spu_func_00004578) {
            if (w) spu_replace_function(LIGHT_JOB_IMAGE, 0x4578u, w);
            fprintf(stderr, "[gpu light] 0x4578 is not the light job's tile walk; walk kept\n");
        } else
            fprintf(stderr, "[gpu light] tile walk skipped (no tile DMA; label 135 protocol kept)\n");
    }
    fprintf(stderr, "[gpu light] SPU light job's per-tile work %s (image %d, LS 0x%X)\n",
            s_spu_tile ? "kept, constants tapped" : "replaced", LIGHT_JOB_IMAGE, LIGHT_JOB_TILE);
}
