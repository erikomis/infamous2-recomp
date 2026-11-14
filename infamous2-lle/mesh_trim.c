/*
 * Mesh trimming off at scaled resolutions (the holes in Cole's face).
 *
 * inFAMOUS 2 culls its skinned meshes on the SPUs with Edge geometry (the job
 * jobmod_d0d15ec83992, image 223): edgeGeomCullTriangles (LS 0xA798) drops
 * back faces, triangles outside the frustum and every triangle too small to
 * cover a pixel centre of the PS3's own 1280x720 grid. At PS3_RES_SCALE or
 * PS3_ASPECT above 1 the GPU rasterises a finer grid, where those triangles DO
 * cover pixel centres, and nothing draws them: the dense meshes (Cole's face,
 * ears, hands, forearms) are left full of pixel-sized holes, and whatever was
 * drawn behind them shows through as white or dark specks. At 1x the dropped
 * triangles would have covered no pixel, so the trimming is invisible there.
 *
 * RPCS3's "Disable Mesh Trimming" patch (Yahfz) nops the routine's first
 * branch, `brnz $4` at LS 0xA7C0 (EBOOT 0x008875C0, module offset 0x67C0),
 * so the culling flavour is never looked at and the segment's triangles all
 * go out (the RSX still culls back faces with the game's own cull state). A
 * patch to the module's bytes never reaches the translated job, so rebuild.py
 * lifts that one instruction with spu_lifter.py --insn-guard and this decides
 * at run time whether it is skipped:
 *
 *   PS3_MESH_TRIM unset  trimming off while the targets are scaled
 *                        (PS3_RES_SCALE or PS3_ASPECT), on at 1x as on a PS3
 *   PS3_MESH_TRIM=1      always trim (the game's own behaviour)
 *   PS3_MESH_TRIM=0      never trim
 */
#include <stdio.h>
#include <stdlib.h>

#include "spu_context.h"

float rsx_fp_wpos_scale(void);   /* rsx_fp_decompiler.c: 1 only when no axis is scaled */

static int s_mode = -2;          /* -2 not read, -1 auto, 0 never trim, 1 always trim */

int infamous2_edge_skip_trim(spu_context* ctx)
{
    (void)ctx;
    int m = __atomic_load_n(&s_mode, __ATOMIC_RELAXED);
    if (__builtin_expect(m == -2, 0)) {
        const char* e = getenv("PS3_MESH_TRIM");
        m = (e && *e == '0') ? 0 : (e && *e == '1') ? 1 : -1;
        __atomic_store_n(&s_mode, m, __ATOMIC_RELAXED);
    }
    const int skip = m >= 0 ? m == 0 : rsx_fp_wpos_scale() != 1.0f;
    static int said = -1;   /* the decision, once per change */
    if (__builtin_expect(__atomic_load_n(&said, __ATOMIC_RELAXED) != skip, 0) &&
        __atomic_exchange_n(&said, skip, __ATOMIC_RELAXED) != skip)
        fprintf(stderr, "[mesh trim] Edge geometry triangle culling %s (PS3_MESH_TRIM=%s)\n",
                skip ? "off" : "on, as on a PS3",
                m < 0 ? "auto" : m ? "1" : "0");
    return skip;
}
