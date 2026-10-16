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

#include "spu_context.h"
#include "rsx_draw_engine.h"

typedef void (*spu_fn)(spu_context*);
spu_fn spu_replace_function(int image_id, uint32_t addr, spu_fn fn);
void jobmod_c6e7383dc013_e5400_spu_func_00004AC8(spu_context* ctx);

/* The lifted image id jobmods_register.c gives the light job. */
#define LIGHT_JOB_IMAGE 219
#define LIGHT_JOB_TILE  0x4AC8u

static uint32_t ls_u32(const spu_context* ctx, uint32_t a)
{
    const uint8_t* p = ctx->ls + (a & 0x3FFFFu);
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

static spu_fn s_spu_tile;   /* the job's own routine, when PS3RECOMP_ENG_GPU_LIGHT_SPU=1 keeps it */

static void light_tile_gpu(spu_context* ctx)
{
    const uint32_t prm = ctx->gpr[3]._u32[0];
    if (ls_u32(ctx, prm + 4) == 0 && ls_u32(ctx, prm + 8) == 0) {
        const uint32_t frame = ls_u32(ctx, 0x9C80) & 0x3FFF0u;
        const uint32_t lights = ls_u32(ctx, 0x9CB0) & 0x3FFF0u;
        uint32_t count = ls_u32(ctx, frame + 0x90);
        if (count > RSX_GPU_LIGHT_MAX) count = RSX_GPU_LIGHT_MAX;
        if (frame + 0x100 <= 0x40000u && lights + count * 0x30u <= 0x40000u)
            rsx_draw_engine_gpu_light_data(ctx->ls + frame, ctx->ls + lights, count);
    }
    if (s_spu_tile) { s_spu_tile(ctx); return; }   /* it returns (or chains) itself */
    SPU_RET(ctx);
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
    fprintf(stderr, "[gpu light] SPU light job's per-tile work %s (image %d, LS 0x%X)\n",
            s_spu_tile ? "kept, constants tapped" : "replaced", LIGHT_JOB_IMAGE, LIGHT_JOB_TILE);
}
