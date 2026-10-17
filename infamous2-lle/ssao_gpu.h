/*
 * inFAMOUS 2 SSAO (WWS job jobmod_e6fe749031f8 + datamods 0x87D500/0x87F880/
 * 0x87D880/0x87E480/0x87EC80) on the GPU: constants and their decoding from
 * the job's own pass blocks / parameter blocks (big-endian guest bytes).
 *
 * Pipeline (pass table at header.w3, 5 x 128-byte blocks, any order here):
 *   E 0x87D500  Z24S8 1280x720 -> linear depth f32 640x360  (params 16 B: A, B)      label 136
 *   A 0x87F880  half-res SSAO, 12 line samples -> B8 640x360 (params 0x100 B)
 *   D 0x87D880  horizontal 5-tap depth-aware blur, in place  (params 16 B: k, w2, w1)
 *   C 0x87E480  vertical   5-tap depth-aware blur, in place  (same params as D)
 *   B 0x87EC80  2x bilateral upsample -> B8 1280x720 at EA 0xCF800000 (params: A, B, k) label 137
 *
 * The layout of SsaoGpuConsts is mirrored 1:1 by `struct SsaoC` in
 * ssao_gpu.metal (all 4-byte scalars, float4/uint4 groups 16-byte aligned).
 */
#pragma once
#include <stdint.h>

#define SSAO_DM_DEPTH    0x0087D500u   /* E */
#define SSAO_DM_AO       0x0087F880u   /* A */
#define SSAO_DM_BLUR_H   0x0087D880u   /* D */
#define SSAO_DM_BLUR_V   0x0087E480u   /* C */
#define SSAO_DM_UPSAMPLE 0x0087EC80u   /* B */

typedef struct SsaoGpuConsts {
    float    depth_ab[4];  /* E params: A, B (w = 1/(A*z24*2^-24*(1+2^-23) + B)), 0, 0          */
    float    X[12];        /* A params +0x00: sample x (SoA)                                     */
    float    Y[12];        /* A params +0x30: sample y                                           */
    float    Z[12];        /* A params +0x60: sample half-length                                 */
    float    M[3][4];      /* A params +0x90/+0xA0/+0xB0: rows; n' = n.x*M0 + n.y*M1 + n.z*M2     */
    float    C0[4];        /* A params +0xC0 (unused by the datamod)                             */
    float    D0[4];        /* A params +0xD0: norm, Kx, Ky, Kz                                    */
    float    E0[4];        /* A params +0xE0: e0, e1, dmin, dmax                                  */
    float    F0[4];        /* A params +0xF0: scale, bias                                         */
    float    blur[4];      /* D/C params: k, w2 (taps +-2), w1 (taps +-1), 0                     */
    float    up[4];        /* B params: A, B, k, 0                                                */
    uint32_t dims[4];      /* full w, full h, half w, half h (from the pass streams)              */
} SsaoGpuConsts;           /* 320 bytes */

typedef struct SsaoJobInfo {
    uint32_t label_ea[2], label_value[2];  /* [0] = pass E (label 136), [1] = pass B (label 137) */
    uint32_t depth_ea, depth_pitch;        /* Z24S8 copy read by E and B (0x47884B80, 0x1400)   */
    uint32_t normals_ea, normals_pitch;    /* ARGB8 normals read by A (0x47500B80, 0x1400)      */
    uint32_t half_depth_ea, half_depth_pitch; /* E out (0x47C08B80, 0xA00)                       */
    uint32_t half_ao_ea, half_ao_pitch;    /* A out / D,C in place (0x47CE9B80, 0x280)          */
    uint32_t out_ea, out_pitch;            /* B out (0xCF800000 = VRAM 0x0F800000, 0x500)       */
    uint32_t found;                        /* bit per pass found: E=1 A=2 D=4 C=8 B=16          */
} SsaoJobInfo;

/* Reads a big-endian guest range; returns a host pointer or NULL. */
typedef const uint8_t* (*ssao_guest_fn)(uint32_t ea, uint32_t size, void* user);

/*
 * Fill `c` and `info` from the job's pass blocks.
 *   passes[i]   128-byte pass blocks (big-endian, as in guest memory / LS 0x6380), npass of them
 *   params_of   if non-NULL, called with each pass's param EA/size (block +0x18/+0x1C) to get its
 *               parameter bytes; if NULL, `params[i]` must hold them (LS copies, e.g. LS [0x6568])
 * Returns 0 when all five passes were recognised and decoded, otherwise a negative code.
 */
int ssao_consts_from_passes(const uint8_t* const* passes, int npass,
                            const uint8_t* const* params, ssao_guest_fn params_of, void* user,
                            SsaoGpuConsts* c, SsaoJobInfo* info);
