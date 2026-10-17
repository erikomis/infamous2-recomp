// inFAMOUS 2 SSAO on the GPU: the five SPU datamods of WWS job jobmod_e6fe749031f8,
// reproduced from the lifted code (scratchpad/ssao/ssao_notes.md, sections 4 and 9).
// Append to the engine helper library (needs `struct BOut` / eng_fullscreen_vs).
// All passes are full-screen triangles with point loads (texture.read), no samplers.
//
//   ssao_hd_fs    -> R32Float  halfW x halfH   in: depth (R32Float, [0,1], full res)
//   ssao_ao_fs    -> R8Unorm   halfW x halfH   in: hd, normals (RGBA8, r,g,b = guest bytes/255)
//   ssao_blur_h_fs-> R8Unorm   halfW x halfH   in: ao, hd
//   ssao_blur_v_fs-> R8Unorm   halfW x halfH   in: ao (from blur_h), hd
//   ssao_up_fs    -> R8Unorm   fullW x fullH   in: ao (from blur_v), hd, depth  == VRAM 0x0F800000
//
// Constants: buffer(0) = SsaoGpuConsts (ssao_gpu.h), filled by ssao_consts_from_passes().
// R8 outputs hold the job's byte values (value = byte/255, max 254). The SPU's
// reciprocal (frest + fi, no Newton step where the datamods have none), its
// saturate-by-conversion and its truncating final multiplies are emulated: they
// decide the last bit of every floor(), e.g. flat areas come out 253, not 254.

struct SsaoC {
    float4 depth_ab;          // E: A, B
    float  X[12];
    float  Y[12];
    float  Z[12];
    float4 M[3];              // rows (xyz)
    float4 C0, D0, E0, F0;    // D0 = norm,Kx,Ky,Kz  E0 = e0,e1,dmin,dmax  F0 = scale,bias
    float4 blur;              // k, w2 (+-2), w1 (+-1)
    float4 up;                // A, B, k
    uint4  dims;              // fullW, fullH, halfW, halfH
};

// ---- SPU arithmetic pieces (runtime spu_helpers.h: spu_frest LUT, spu_fi) ----
constant uint ssao_frest_lut[32] = {
    0x7FFBE0, 0x7F87A6, 0x70EF72, 0x708B40, 0x638B12, 0x633AEA, 0x5792C4, 0x574AA0,
    0x4CCA7E, 0x4C9262, 0x430A44, 0x42D62A, 0x3A2E12, 0x39FDFA, 0x3215E4, 0x31F1D2,
    0x2AA9BE, 0x2A85AC, 0x23D59A, 0x23BD8E, 0x1D8576, 0x1D8576, 0x17AD5A, 0x17AD5A,
    0x124543, 0x124543, 0x0D392D, 0x0D392D, 0x08851A, 0x08851A, 0x041D07, 0x041D07 };
// fi(x, frest(x)): the SPU's reciprocal estimate (~1e-7 rel., truncated).
static inline float ssao_recip(float x) {
    uint b = as_type<uint>(x);
    uint sign = b & 0x80000000u, e = (b >> 23) & 0xFFu;
    uint re = (e == 0u) ? 255u : (e >= 253u ? 0u : 253u - e);
    uint est = sign | (re << 23) | (ssao_frest_lut[(b >> 18) & 31u] & 0x7FFFFFu);
    int ee = int((est >> 23) & 0xFFu);
    if (ee == 0) return 0.0f;
    ulong base = (est >> 10) & 0x1FFFu, step = est & 0x3FFu, y = b & 0x7FFFFu;
    ulong m = ((8192ul | base) << 19) - step * y;
    if (m < (1ul << 32)) { m <<= 1; if (--ee == 0) return 0.0f; }
    return as_type<float>(sign | (uint(ee) << 23) | uint((m >> 9) & 0x7FFFFFu));
}
// a*b rounded toward zero (SPU fm); a, b >= 0 here.
static inline float ssao_mul_rz(float a, float b) {
    float p = a * b;
    return (fma(a, b, -p) < 0.0f && p > 0.0f) ? as_type<float>(as_type<uint>(p) - 1u) : p;
}
// cfltu(x,141) -> cuflt(.,123): saturate; 1.0 becomes 1 - 2^-24.
static inline float ssao_sat(float x) { return x >= 1.0f ? as_type<float>(0x3F7FFFFFu) : max(x, 0.0f); }

// Z24 from the resolved depth: same convention as eng_depth_rgba8_fs (trunc).
static inline float ssao_z24(float d01) { return float(uint(clamp(d01, 0.0f, 1.0f) * 16777215.0f)); }
// d500 / ec80: w = 1 / fma(z24, A * 2^-24*(1+2^-23), B), reciprocal + one Newton step.
static inline float ssao_lin(float z24, float A, float B) {
    float k = A * as_type<float>(0x33800001u);
    float pe = fma(A, as_type<float>(0x33800001u), -k);         // round k toward zero like fm
    if ((pe > 0.0f && k < 0.0f) || (pe < 0.0f && k > 0.0f)) k = as_type<float>(as_type<uint>(k) - 1u);
    float v = fma(z24, k, B);
    float r = ssao_recip(v);
    return fma(fma(-v, r, 1.0f), r, r);
}
static inline float ssao_byte(texture2d<float> t, int2 p) { return rint(t.read(uint2(p)).r * 255.0f); }

// E (0x87D500): point sample of the top-left texel of each 2x2 block.
fragment float ssao_hd_fs(BOut i [[stage_in]],
                          texture2d<float> depth [[texture(0)]],
                          constant SsaoC& c [[buffer(0)]]) {
    uint2 p = uint2(i.pos.xy) * 2u;
    return ssao_lin(ssao_z24(depth.read(p).r), c.depth_ab.x, c.depth_ab.y);
}

// A (0x87F880): 12 vertical line segments around P + n'; visible fraction summed.
fragment float ssao_ao_fs(BOut i [[stage_in]],
                          texture2d<float> hd  [[texture(0)]],
                          texture2d<float> nrm [[texture(1)]],
                          constant SsaoC& c [[buffer(0)]]) {
    int2 p = int2(i.pos.xy);
    int2 hm = int2(c.dims.zw) - 1;
    float d   = hd.read(uint2(p)).r;
    float dc  = clamp(d, c.E0.z, c.E0.w);
    float rcp = ssao_recip(dc);
    float q   = d * rcp;
    // normal bytes (2x,2y) -> b/256 * 2.00784302 - 1  (cuflt 147, fma 0x40008081, -1)
    float3 nb = rint(nrm.read(uint2(p * 2)).rgb * 255.0f);
    float3 n  = fma(nb * (1.0f / 256.0f), float3(2.0078430175781f), float3(-1.0f));
    float3 n2 = fma(float3(n.z), c.M[2].xyz, fma(float3(n.y), c.M[1].xyz, n.x * c.M[0].xyz));
    float dref = fma(n2.z, q, d);
    float bx = fma(n2.x, rcp, 0.5f), by = fma(n2.y, rcp, 0.5f);
    float sx = c.D0.y * rcp, sy = c.D0.z * rcp;
    float hz = c.D0.w * q;
    float e0 = c.E0.x * ssao_recip(q), e1 = c.E0.y * q;
    float acc = 0.0f;
    for (int k = 0; k < 12; k++) {
        int tx = p.x + int(floor(fma(c.X[k], sx, bx)));
        int ty = p.y + int(floor(fma(c.Y[k], sy, by)));
        float ds = hd.read(uint2(clamp(int2(tx, ty), int2(0), hm))).r;
        float h  = c.Z[k] * hz;
        float t  = (ds - (dref - h)) * 0.5f;
        acc += fma(ssao_sat(fma(t, e0, e1)), h, clamp(t, 0.0f, h));
    }
    acc *= ssao_recip(c.D0.x * q);
    float v = ssao_sat(fma(acc, c.F0.x, c.F0.y));
    return floor(ssao_mul_rz(v, 255.0f / 256.0f) * 256.0f) / 255.0f;   // <= 254
}

// D (0x87D880) / C (0x87E480): 5 taps, w = max(0, 1 - |D - D0| * k / D0), +-1 * w1, +-2 * w2.
static inline float ssao_blur(texture2d<float> ao, texture2d<float> hd, int2 p, int2 dir, constant SsaoC& c) {
    int2 hm = int2(c.dims.zw) - 1;
    float d0 = hd.read(uint2(p)).r;
    float kk = c.blur.x * ssao_recip(d0);
    float w[5], a[5];
    for (int o = -2; o <= 2; o++) {
        int2 s = clamp(p + o * dir, int2(0), hm);
        a[o + 2] = ssao_byte(ao, s);
        w[o + 2] = max(0.0f, fma(-fabs(hd.read(uint2(s)).r - d0), kk, 1.0f)) * ((o == -2 || o == 2) ? c.blur.y : c.blur.z);
    }
    float num = fma(w[4], a[4], fma(w[3], a[3], fma(w[1], a[1], fma(w[0], a[0], a[2]))));
    float den = w[4] + (w[3] + (1.0f + (w[0] + w[1])));
    return floor(ssao_mul_rz(num, ssao_recip(den))) / 255.0f;
}
fragment float ssao_blur_h_fs(BOut i [[stage_in]], texture2d<float> ao [[texture(0)]],
                              texture2d<float> hd [[texture(1)]], constant SsaoC& c [[buffer(0)]]) {
    return ssao_blur(ao, hd, int2(i.pos.xy), int2(1, 0), c);
}
fragment float ssao_blur_v_fs(BOut i [[stage_in]], texture2d<float> ao [[texture(0)]],
                              texture2d<float> hd [[texture(1)]], constant SsaoC& c [[buffer(0)]]) {
    return ssao_blur(ao, hd, int2(i.pos.xy), int2(0, 1), c);
}

// B (0x87EC80): 2x upsample; taps (0,0) 9, (0,sy) 3, (sx,0) 3, (sx,sy) 1, s = odd ? +1 : -1,
// weight fma(max(0, 1 - |Dh - Z| * k / Z), bw, bw * 1e-4), Z = full-res linear depth.
fragment float ssao_up_fs(BOut i [[stage_in]],
                          texture2d<float> ao    [[texture(0)]],
                          texture2d<float> hd    [[texture(1)]],
                          texture2d<float> depth [[texture(2)]],
                          constant SsaoC& c [[buffer(0)]]) {
    int2 P  = int2(i.pos.xy);
    int2 hm = int2(c.dims.zw) - 1;
    float Z  = ssao_lin(ssao_z24(depth.read(uint2(P)).r), c.up.x, c.up.y);
    float kz = c.up.z * ssao_recip(Z);
    int2 b = P >> 1;
    int2 s = int2((P.x & 1) ? 1 : -1, (P.y & 1) ? 1 : -1);
    int2  off[4] = { int2(0, 0), int2(0, s.y), int2(s.x, 0), s };
    float bw[4]  = { 9.0f, 3.0f, 3.0f, 1.0f };
    float eps[4] = { as_type<float>(0x3A6BEDFAu), as_type<float>(0x399D4952u), as_type<float>(0x399D4952u), as_type<float>(0x38D1B717u) };
    float num = 0.0f, den = 0.0f;
    for (int k = 0; k < 4; k++) {
        int2 t = clamp(b + off[k], int2(0), hm);
        float w = fma(max(0.0f, fma(-fabs(hd.read(uint2(t)).r - Z), kz, 1.0f)), bw[k], eps[k]);
        num = fma(w, ssao_byte(ao, t), num);
        den += w;
    }
    return floor(ssao_mul_rz(num, ssao_recip(den))) / 255.0f;
}
