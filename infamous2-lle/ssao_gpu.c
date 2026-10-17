/*
 * Decode inFAMOUS 2's SSAO pass blocks / parameter blocks into SsaoGpuConsts.
 *
 * Pass block (128 B, big-endian; built by the PPU's func_0054EF50):
 *   +0x00 u8 tiles_x, u8 tiles_y, u8 -, u8 flags (bit0: PUT label on completion)
 *   +0x04 label EA      +0x08 label value
 *   +0x0C datamod EA    +0x10 code+bss size   +0x14 DMA size
 *   +0x18 param EA      +0x1C param size
 *   +0x40 + 16*i stream: u32 ea, u32 pitch, u16 tile_w, u16 tile_h,
 *                        u8 ax, u8 ay, u8 bpp, u8 (planes<<2)|type (1 in, 2 out)
 * Surface size of a stream = tile_w * tiles_x by tile_h * tiles_y.
 */
#include <string.h>
#include "ssao_gpu.h"

static uint32_t be32(const uint8_t* p) { return (uint32_t)p[0] << 24 | (uint32_t)p[1] << 16 | (uint32_t)p[2] << 8 | p[3]; }
static uint32_t be16(const uint8_t* p) { return (uint32_t)p[0] << 8 | p[1]; }
static float bef(const uint8_t* p) { uint32_t v = be32(p); float f; memcpy(&f, &v, 4); return f; }

typedef struct { uint32_t ea, pitch, w, h; uint8_t ax, ay, bpp, type; } Stream;
static Stream stream_of(const uint8_t* pb, int i)
{
    const uint8_t* d = pb + 0x40 + 16 * i;
    Stream s;
    s.ea = be32(d); s.pitch = be32(d + 4);
    s.w = be16(d + 8) * pb[0]; s.h = be16(d + 10) * pb[1];
    s.ax = d[12]; s.ay = d[13]; s.bpp = d[14]; s.type = d[15] & 3;
    return s;
}

int ssao_consts_from_passes(const uint8_t* const* passes, int npass,
                            const uint8_t* const* params, ssao_guest_fn params_of, void* user,
                            SsaoGpuConsts* c, SsaoJobInfo* info)
{
    memset(c, 0, sizeof *c);
    memset(info, 0, sizeof *info);
    for (int k = 0; k < npass; k++) {
        const uint8_t* pb = passes[k];
        if (!pb) continue;
        const uint32_t dm = be32(pb + 0x0C), pea = be32(pb + 0x18), psz = be32(pb + 0x1C);
        const uint8_t* pr = params_of ? params_of(pea, psz, user) : (params ? params[k] : 0);
        if (!pr) return -1;
        switch (dm) {
        case SSAO_DM_DEPTH: {                      /* E: s0 = Z24S8 full (in), s1 = f32 half (out) */
            if (psz < 8) return -2;
            Stream s0 = stream_of(pb, 0), s1 = stream_of(pb, 1);
            c->depth_ab[0] = bef(pr); c->depth_ab[1] = bef(pr + 4);
            c->dims[0] = s0.w; c->dims[1] = s0.h; c->dims[2] = s1.w; c->dims[3] = s1.h;
            info->depth_ea = s0.ea; info->depth_pitch = s0.pitch;
            info->half_depth_ea = s1.ea; info->half_depth_pitch = s1.pitch;
            info->label_ea[0] = (pb[3] & 1) ? be32(pb + 4) : 0; info->label_value[0] = be32(pb + 8);
            info->found |= 1; break; }
        case SSAO_DM_AO: {                         /* A: s0 = half depth, s1 = normals, s2 = half AO */
            if (psz < 0x100) return -3;
            for (int i = 0; i < 12; i++) { c->X[i] = bef(pr + 4 * i); c->Y[i] = bef(pr + 0x30 + 4 * i); c->Z[i] = bef(pr + 0x60 + 4 * i); }
            for (int r = 0; r < 3; r++) for (int j = 0; j < 4; j++) c->M[r][j] = bef(pr + 0x90 + 16 * r + 4 * j);
            for (int j = 0; j < 4; j++) {
                c->C0[j] = bef(pr + 0xC0 + 4 * j); c->D0[j] = bef(pr + 0xD0 + 4 * j);
                c->E0[j] = bef(pr + 0xE0 + 4 * j); c->F0[j] = bef(pr + 0xF0 + 4 * j);
            }
            Stream s1 = stream_of(pb, 1), s2 = stream_of(pb, 2);
            info->normals_ea = s1.ea; info->normals_pitch = s1.pitch;
            info->half_ao_ea = s2.ea; info->half_ao_pitch = s2.pitch;
            info->found |= 2; break; }
        case SSAO_DM_BLUR_H:
        case SSAO_DM_BLUR_V:
            if (psz < 12) return -4;
            c->blur[0] = bef(pr); c->blur[1] = bef(pr + 4); c->blur[2] = bef(pr + 8);
            info->found |= dm == SSAO_DM_BLUR_H ? 4 : 8; break;
        case SSAO_DM_UPSAMPLE: {                   /* B: s0 AO half, s1 depth half, s2 Z24S8 full, s3 out */
            if (psz < 12) return -5;
            Stream s3 = stream_of(pb, 3);
            c->up[0] = bef(pr); c->up[1] = bef(pr + 4); c->up[2] = bef(pr + 8);
            info->out_ea = s3.ea; info->out_pitch = s3.pitch;
            info->label_ea[1] = (pb[3] & 1) ? be32(pb + 4) : 0; info->label_value[1] = be32(pb + 8);
            if (!c->dims[0]) { c->dims[0] = s3.w; c->dims[1] = s3.h; }
            info->found |= 16; break; }
        default: break;
        }
    }
    if (!c->dims[2]) { c->dims[2] = c->dims[0] / 2; c->dims[3] = c->dims[1] / 2; }
    return info->found == 31 ? 0 : -10;
}
