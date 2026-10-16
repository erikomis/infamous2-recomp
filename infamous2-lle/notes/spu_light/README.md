> In the repo: this write-up, `reference.py` (the numpy model) with `setup.py`/`emu.py`.
> Run: `SNAP=<capture dir> ../../../.venv/bin/python3 reference.py` on a capture taken with
> `RSX_SNAP_AT=270 RSX_SNAP_LABEL_B=870 RSX_SNAP_DIR=<dir> RSX_SNAP_RANGES=47500B80:384000,47884B80:384000,00a93800:800,47e70000:10000`.
> The C harness that ran the lifted job on a hand-built LS stayed in the session scratchpad.
> The GPU version is `eng_light_fs` in the toolkit's Metal backend (PS3RECOMP_ENG_GPU_LIGHT).

# inFAMOUS 2 SPU light job (`jobmod_c6e7383dc013`): per-pixel lighting, reverse-engineered

All line numbers point to
`infamous2-lle/spu/jobmod_c6e7383dc013_e5400/spu_recomp.c`, unless they are marked `f880:`
(`datamod_0087f880`) or `ec80:` (`datamod_0087ec80`).

## 0. Correction to the premise: the math is in the jobmod, not in the datamods

* The jobmod occupies LS 0x4000..0x9C80 (header `00005400 00005c80`, entry 0x9400). Its
  per-pixel code sits at LS 0x7140..0x93FF (functions 7140, 7170/7300, 7448..766C,
  77B8/7918, 8A00..9248). A module loaded at LS 0x6C00 would overwrite that code, so
  neither datamod can belong to this job.
* `datamod_0087f880` (hot func_7320) is most likely the **SSAO** kernel (inference):
  - It reads every other pixel (8-byte stride) of a normals tile, which means half
    resolution. It takes bytes 1..3, computes `b*2/255-1` (f880:258-272, const 0x40008081
    and -1) and rotates the result by a 3x3 matrix (f880:270-317).
  - It projects 9 sample vectors (r8..r16 from param+0..0x80, f880:168-176) with a
    1/w divide (f880:346-351). It converts the result to integer texel coordinates and
    reads depth back from an LS tile that has a 32-px apron (`(y-32)*pitch + x*4-128`,
    f880:398-430).
  - It then subtracts depth, scales by 0.5 and saturates (f880:550-700).
* `datamod_0087ec80` (func_6F80) is a **depth-aware 2x bilateral upsample/blur**. It uses
  bilinear weights 9/3/3/1 (consts 9.0, 3.0, 1.0 at ec80:232-235). Each weight is
  multiplied by `max(0, 1 - |z_i - z_c| * k / z_c)` and gets a small epsilon
  (0x3A6BEDFA, 0x399D4952, 0x38D1B717, ec80:203-215).
* Both datamods match the SSAO B8 1280x720 buffer at VRAM 0x0F800000 that the composite
  multiplies all lighting by. I did not find which job loads them.
* **Profiling implication:** the "lighting ~23%" share for the datamods is SSAO. The
  light job's own per-pixel cost is in jobmod c6e7 (the 11%).

## 1. How the result was verified

I built a host harness (scratchpad `decomp/harness/`). It compiles the lifted
`spu_recomp.c` with the real `spu_helpers.h`, loads the jobmod binary at LS 0x4000 and
calls the per-tile function `func_4AC8` directly.

Inputs came from the captured frame `scratchpad/snap2/` (`a_*` before the job, `b_*` after
it, plus the frame block and the light list), placed in a hand-built LS.

* **All 720 tiles came out bit-exact** against the captured output buffers (`b_47500b80`,
  `b_47884b80`).
* A float64 numpy model of the pseudocode below (`harness/reference.py`) reproduces the
  job:
  - Fed the job's own decoded positions and normals, the model matches **100.000% of
    bytes to within ±1**, in all 5 output channels of the 97 lit tiles.
  - Fed my own float64 depth reconstruction, the model gets 99.3% of bytes within ±1, with
    a worst case of 44 on grazing-angle pixels (see §3 precision note).
* The parameter struct of `func_4AC8` was guessed and confirmed by the bit-exact
  match (§6).

## 2. Data layout

### Per-frame block
LS pointer at `[0x9C80]`, set at line 365. The copy at EA 0x00a93c80 has this layout:

| offset | content | used at |
|---|---|---|
| +0x10..+0x4F | **M1**: rows r0..r3, maps `(u, v, z, 1)` to camera-relative world (homogeneous) | 41B0: 135-143 |
| +0x50..+0x8F | **M2**: the same unprojection taking NDC `(x, y∈[-1,1], y up)`, up to a scale of 0.5 | 4698: 536, 558, 564, 570 |
| +0x90 | light count | 802, 852 (r94) |
| +0x94 | EA of the light array | |
| +0x98 | 1.6, multiplies every light colour | 923-938 → r104; 1076, 1256 |

M2 relates to M1 like this: `M2.r0 = M1.r0/2`, `M2.r1 = -M1.r1/2`, and
`M2.r3 = (M1.r0 + M1.r1)/2 + M1.r3` (the overall 0.5 scale is homogeneous). M2 is only
used to build the tile's side planes.

### Light list
* LS pointer at `[0x9CB0]` = frame-block LS + 0x100 (line 355 `r35 = r84 + 256`, line 369).
  The lights sit directly after the 0x100-byte frame block, e.g. at LS 0xA100 when the
  frame block is at 0xA000.
* Each record is 48 bytes, walked with `r81 = list + 12`, `r81 += 48` (lines 929, 950).
* **There is no per-tile light index list.** Every tile loops over all `count` lights of
  the frame (lines 948-954, 957-1026) and culls them itself (§5).

Record fields:

| off | field | used by this job? |
|---|---|---|
| +0x00 | float x, y, z, camera-relative world, cm | yes (965-1004) |
| +0x0C | half R, G, B | yes (1055-1073 → 7140 half→float, 3694-3706) |
| +0x12 | half "X" (0.915) | **no** (changing it alters nothing) |
| +0x14 | u32 type: 1 = point, 2 = spot, anything else = ignored | 1028, 943 |
| +0x18 | u32 (0) | **no** |
| +0x1C | f32 (1.0) | **no** |
| +0x20 | half `inner`, half `outer` (cm) | yes |
| +0x24 | half `cA` (spot: -1/(cos_in - cos_out), e.g. -1.592; point: 65504) | spot only |
| +0x26 | half `cB` (spot: cos_outer, e.g. 0.2588 = cos 75°; point: 1.0) | spot only |
| +0x28 | half dir x, y, z (spot axis, unit) | spot only (1157-1189) |
| +0x2E | half `near` (15/20/130): **spot near cut-off distance in cm** | spot only (5260, 5338) |

Spot-record values after half→float are `r80 = (inner, outer, cA, cB)` (line 1008),
`r84 = (cA, cB, cB, cB)` (1155-1186) and `r98 = (dx, dy, dz, near)` (1189). The light
colour vector is `C = rgb * 1.6` (1076/1256).

### Tile I/O (param struct `r3` of `func_4AC8`, lines 782-857)

| field | meaning |
|---|---|
| +0x00 | → tile descriptor: byte 0 = tiles across (40), byte 1 = tiles down (18), half +0x48 = tile w (32), half +0x4A = tile h (40) |
| +0x04, +0x08 | tile index tx, ty |
| +0x0C | LS out A: **diffuse**, written in place over the normals tile |
| +0x10 | LS in: normals tile, ARGB8, pitch 128 B |
| +0x14 | LS in: depth tile, Z24S8 |
| +0x18 | LS out B: **specular**, in place over the depth tile |

### Scratch buffers
`base = [0x9C90]` (line 367).

* `base + 0xA800 .. +0x18400`: 320 SoA records of 176 B, one per 4 horizontally adjacent
  pixels, in row-major order (8 records per tile row). Each record holds 11 vec4s:
  `Px, Py, Pz, Nx, Ny, Nz, s, accR, accG, accB, accSpec`.
* `base + 0x18400`: the per-light list of record offsets (one quadword per entry).

### Constant pool (in the jobmod image)

| LS | value |
|---|---|
| 0x9780 / 0x9790 | rotate-by-4/8 masks (horizontal dot products) |
| 0x97A0 | (0, 0, 0, 1) |
| 0x9910..0x9940 | byte-extract masks |
| 0x9960 / 0x9980 / 0x99A0 | half→float masks |
| 0x99B0 / 0x99C0 / 0x99D0 / 0x9A10 | output byte-packing masks |
| 0x99E0..0x9A00, 0x9A40 | word splats |
| 0x9A20 | FLT_MIN |
| 0x9A30 | (0, 1, 2, 3 \| 10..13 \| zeros) |
| 0x9A50 | Z24 extract mask `80 00 01 02` |
| 0x9A60 | (1, .75, .5, .25), lane offsets |
| 0x9AB0 | 0x00FFFFFF, the sky depth |
| 0x9B80..0x9BAF | 1e8 (dummy far "record" used to pad lists) |

## 3. Per-pixel decode

### Normals and gloss (77B8 / 7918, lines 4138-4402)
This runs once per tile, lazily, when the first light passes culling (lines 1048-1049,
1228-1229 → 51F8/5210 → 77B8; flag r102). For each pixel, with normals bytes
`[A, R, G, B]` (byte 0 = A):

```
n   = float3(R, G, B) * (2/255) - 1                  // byte/256 * 0x40008081 (2.00784), + 0xBF800000 (-1): 4153-4157, 4209, 4214
n   = n * rsqrt(dot(n, n))                           // frsqest+fi: 4244-4269, 4336-4391
s   = exp2(A * 8/255 - 0.8889600)                    // 0x41008081 = 2048/255 applied to A/256, 0xBF6392E2: 4143-4149, 4203
                                                     // exp2 = 2^int (shli exp+127 <<23: 4247/4262) * poly(frac): 4169-4201, ln2 0x3F317218 4159/4163
                                                     // (exact for A >= 29; up to 1.2% low for A < 29, where x < 0)
acc = 0                                              // 4271-4285
```

`s` is "specular exponent / 8": the effective Blinn-Phong exponent is `n_exp = 8*s`, which
runs from about 4.3 (A=0) to about 1100 (A=255).

### Depth → position (41B0 → 7448..766C, lines 127-246 and 3898-4136)

```
z24 = (b0 << 16) | (b1 << 8) | b2                    // bytes 0..2 of the Z24S8 word; byte 3 (stencil) ignored. Mask 0x9A50: 3942, 3958
u   = px / 1280,  v = py / 720                       // integer pixel = top-left corner, NOT +0.5 (best fit offset is exactly 0)
h   = u*M1.r0 + v*M1.r1 + (z24 * 2^-24)*M1.r2 + M1.r3   // 2^-24 = 0x33800000: 170/174; tile corner terms 189-204
P   = h.xyz * (1 / h.w)                              // frest+fi, no Newton step: 3976/4011/4032/4057/4061
```

* `P` is **camera-relative world space** (world axes, eye at the origin). Evidence:
  `M1.r2.xyz ≈ 0`, so every pixel lies on a ray through 0, and the shading uses
  `V = -P/|P|` (lines 4870-4936). Light positions must therefore be camera-relative too.
* Sky pixels (`z24 == 0xFFFFFF`, constant 0x9AB0) still get a (huge) P but are excluded
  from the tile min/max (3961, 4024, 4037).
* The tile AABB (min/max of P) and bounding sphere (`centre = (min+max)/2`,
  `radius = |max-min|/2`) are stored on the stack (217-243). If every pixel is sky, the
  tile is skipped (lines 909-922).
* **Precision note:** u is stepped incrementally (-4·du per 4 pixels, right to left) with
  SPU *truncating* float adds. With `w = 0.1·(1-0.99999·z)` tiny, P comes out 2-15 z24
  units deeper along the view ray than an exact evaluation. That is about 1-4 cm at 70 m;
  the error is purely along the ray (perpendicular error < 0.001 cm). It only matters at
  grazing N·L.

## 4. Per-light shading

### Point lights (8AE8 / 8D48, lines 4845-5096)
The spot loop (9048 / 9248, lines 5204-5431) is the same plus the cone and near terms.
For each listed 4-pixel record (§5):

```
C      = light.rgb * 1.6                              // 1076 / 1256
Lv     = light.pos - P;   d2 = dot(Lv, Lv)            // 4866-4902
invd   = rsqrt(d2);  d = d2 * invd;  l = Lv * invd    // frsqest+fi 4915/4920, d = r72 (4938)
NdotL  = max(dot(N, l), 0)                            // 4912/4924/4946, clamp 4962/4968
att    = saturate((outer - d) / (outer - inner))^2    // 1/(o-i) 4874/4891/4898; fma(d, -1/(o-i), o/(o-i)) 4952;
                                                      // saturate = cfltu(x,141) → cuflt(.,123) 4964/4977; square 4988
// spot only (type 2):
cosA   = dot(-l, light.dir)
spot   = saturate((cosA - cB) * (-cA))^2              // == saturate((cosA - cos_out)/(cos_in - cos_out))^2: 9048 5290-5323, 9248 5342-5370
spot   = (d > near) ? spot : 0                        // fcgt(d, near) 5338 + selb 5354; near = half +0x2E (r55 = splat(r9[0]) 5260)
att   *= spot
// diffuse: plain Lambert, per channel
accRGB  += att * NdotL * C                            // fma(colour, att*NdotL, acc): 5023/5045/5051, stores 5036/5058/5064
// specular: monochrome, scaled by |C| (length of the RGB vector, 3 components)
V      = -P * rsqrt(dot(P, P))                        // 4870-4936 (eye at origin)
Hn     = l + V;  NdotH = max(dot(N, Hn) * rsqrt(dot(Hn, Hn)), 0)   // 4932/4939/4944, 4956/4976/4985/4993
pw     = (1 + s * (1 - NdotH))^-8                     // ≈ NdotH^(8s); 1-NdotH 4995, fma 5005, x^2 x^4 x^8 5017/5027/5041, frest+fi 5054/5061
norm   = 1 + (25/27) * s                              // 0x3F6D097B = 0.9259259 (4856/4886), fma 4906 = 1 + 0.1157*n_exp
accSpec += att * NdotL * |C| * norm * pw              // |C| 4872-4896/4954 (0 if < FLT_MIN 4979/4987); 5073-5084, store 5092
```

* Validation of the model in §1: diffuse ratio 1.000 ± 0.001; specular ratio 1.000 ± 0.006.
  The residual comes from the frest/frsqest+fi estimates.
* Fields with no effect, tested by changing them: record +0x12, +0x18, +0x1C; for point
  lights, `cA`, `cB`, `dir` and `near`; types other than 1 and 2.

## 5. Culling (per tile)

There is no precomputed per-tile list. For each light of the frame list:

1. **Tile frustum (4698, lines 512-779).** The tile's NDC edges are `x0 = 2tx/40-1`,
   `x1 = 2(tx+1)/40-1`, `y0 = 1-2ty/18`, `y1 = 1-2(ty+1)/18` (lines 513-566). Four side
   planes are built from M2 rows, normalised with frsqest, and stored SoA at stack+0x20..+0x50.
   The light is rejected if `dot(plane.n, Lpos) + plane.d + outer < 0` for any plane
   (lines 1007-1026; outer = r83). There are no near/far planes.
2. **Point lights (5228, lines 1299-1325):** sphere(Lpos, outer) vs the tile AABB of
   reconstructed positions: `|Lpos - clamp(Lpos, min, max)|² < outer²`.
3. **Spot lights:** the same AABB test (1146-1153), then a cone-vs-sphere test against the
   tile bounding sphere (5290/52F0, lines 1327-1444). The test uses `dir` and `cB`.
4. **Per-4-pixel-group list (8A00/8A90, lines 4779-4843).** The list keeps record offsets
   where any lane has `d² < outer²`.
   - For spots, 8EC0/8FB8 (lines 5098-5202) further keep groups where any lane has
     `cosA > cB && d < outer`.
   - 4040 (lines 23-121) pads the list to a multiple of 4 entries with a dummy record at
     LS 0x9B80, whose positions are 1e8 (line 52).

Culling only removes zero contributions: `att` is 0 beyond `outer`, and the cone term is
0 outside `cB`. A GPU version can skip it and evaluate every light per pixel, with one
exception: tiles where **no** light survives culling are memset to 0 (§6). The only
visible effect of that is alpha 0 instead of 90.

## 6. Output packing (7170 / 7300, lines 3709-3895)

```
if no light passed culling for the tile (or tile all sky / light count 0):
    outA = outB = 0 (memset 9690, lines 1096-1106)        // ARGB = 0,0,0,0
else (51E0 → 7170, lines 1272-1278):
    K   = 0.35355339 * 0.99609375 * 256 ≈ 90.15           // 0x3EB504F3 = 1/sqrt(8) (3739/3741), 0x3F7F0000 = 255/256 (3748), cfltu(.,165) = *256 (3760)
    q   = sqrt(accRGB)                                    // frsqest+fi then x*rsqrt(x): 3719-3729 (values < FLT_MIN → 0, 3731-3737)
    m   = max(1, q.r, q.g, q.b)                           // 3736-3750
    A   = min(255, trunc(m * K))                          // 3760, 3772-3776
    RGB = min(255, trunc(q * K * 255 / A))                // 1/(A/255): cuflt(A,147)*0x3F808081 3778/3787, frest+fi 3799/3806; same K chain, clamp 255
    outA pixel = [A, R, G, B]                             // byte 0 = A (masks 0x99B0/0x99C0/0x99D0, stores 3890)
    outB pixel = [min(255, trunc(sqrt(accSpec) * K)), 0, 0, 0]   // spec in byte 0 only, RGB bytes = 0 (mask 0x9A10, store 3892)
```

* This is an RGBM-style encoding with M = A/255. The composite's
  `light = (tex4.rgb * tex4.a)^2 * 8` decodes it back to `accRGB * 0.9922`.
* The specular buffer is decoded as `(tex5.a)^2 * 8 ≈ accSpec`.
* Unlit pixels inside a lit tile get A = 90, RGB = 0. That is why snap2 shows alpha
  values 0 and 90..117.
* Packing model vs emulator: 99.6% exact, the rest ±1 (estimate precision).

## 7. Open questions / uncertainty

1. **Datamods = SSAO + bilateral upsample** is an inference from their code shape. I did
   not find the job that loads them, and I did not trace their I/O.
2. **Light-list DMA.** Lights are read at `[0x9CB0] = frameLS + 0x100`. I did not trace
   the DMA that fills it (WWS service calls through `[0x9C60]+52/+60/+68`, lines 285-430),
   or the real LS values of `[0x9C80]`, `[0x9C90]` and the tile buffers. The harness used
   frame 0xA000, base 0, tiles at 0x1A000/0x1B800.
3. **Exact exp2 / rsqrt behaviour.** The emulator's frest/frsqest+fi has about 2e-4
   relative error. Real Cell hardware may differ slightly; I did not check this.
4. **Cone-vs-sphere test (5290).** I read it only enough to know it uses
   outer/dir/cB/the tile sphere; I did not write out the exact test. It only affects
   culling.
5. **Meaning of the record's +0x12 half (0.915), +0x18 and +0x1C.** They are unused here;
   the PPU or another pass may use them.
6. **M1 / M2 derivation from the camera.** I did not derive it; both were treated as
   given per-frame inputs. M1 takes `u = px/1280`, `v = py/720` (pixel corner) and
   `z24/2^24`.

## Files (scratchpad `decomp/`)

* `harness/harness.c`, `scenario.inc`: host runner for single lifted functions.
* `harness/emu.py`, `setup.py`, `exp.py`: LS builder, snapshot loader, experiments.
* `harness/reference.py`: numpy reference of §3-§6. Run with the repo venv python.
* `harness/spuf.py`: Python frest/frsqest/fi.
* `job.txt`, `f880_7320.txt`: compacted listings that keep the original line numbers.
* `dm_0087f880.bin`, `dm_0087ec80.bin`: datamod blobs extracted from EBOOT.elf.
