# inFAMOUS 2 SSAO job (`jobmod_e6fe749031f8`): reverse-engineered

Line numbers point to the lifted files below. Prefixes:

| prefix | file |
|---|---|
| `job:` | `infamous2-lle/spu/jobmod_e6fe749031f8_e2060/spu_recomp.c` |
| `d500:`, `f880:`, `d880:`, `e480:`, `ec80:` | `infamous2-lle/spu/datamod_0087xxxx/spu_recomp.c` |
| `ppu6:` | `infamous2-lle/recompiled/ppu_recomp_006.cpp` |
| `ppu10:` | `infamous2-lle/recompiled/ppu_recomp_010.cpp` |
| `dma:` | `ps3recomp-fork/runtime/spu/spu_dma.h` |

The compacted listings in `scratchpad/ssao/*.txt` keep these original line numbers.

## 0. Summary

* **The job is a generic tiled image-pipeline runner.** One WWS job walks an ordered list of *passes*. Each pass names a code module (a "datamod", DMA'd to LS 0x6C00), a parameter block, and up to 4 image *streams* (input/output, with tile size, apron and bpp). The job tiles the frame and runs the datamod per tile. A shared atomic iterator spreads the tiles over the SPUs and acts as a barrier between passes.
* **The SSAO pipeline has 5 passes, in this order** (PPU builder `ppu6:61584`):
  * **E `0x87D500`**: depth downsample. Z24S8 1280x720 → linear depth f32 640x360. Writes **label 136**.
  * **A `0x87F880`**: half-res SSAO, 12 line samples ("volumetric obscurance"), normal-offset. Output: AO B8 640x360.
  * **D `0x87D880`**: horizontal 5-tap depth-aware blur, in place.
  * **C `0x87E480`**: vertical 5-tap depth-aware blur, in place.
  * **B `0x87EC80`**: depth-aware 2x bilateral upsample (9/3/3/1). Output: AO B8 1280x720. Writes **label 137**.
* **The "PUTs into the EBOOT text" are not real.** They are a logging artefact, not an MFC bug.
  - The `[mfc] PUTJ` log (`dma:1223-1235`) prints the *command's* EAL and size before the list is expanded.
  - For a PUTL, EAL is the **LS address of the list** and size is **8 × rows**. So 0x00025600/0x168 means "list at LS 0x25600, 45 elements".
  - The list path itself (`dma:817-914`, `954-964`, dispatch `1537-1563`) is CBEA-correct for this job.
  - All the observed sizes match real tilings: 0x40 = 8 rows (D, E), 0x168 = 45 rows (C, 128x45 tiles), 0x1E0 = 60 rows (B, full-res tile height 60).
* **Every formula was validated by running the lifted code on the host.**
  - Each datamod matches a numpy model (bytes exact or ±1; the f880 raw accumulator is within 3e-4 relative).
  - The **whole job** also ran: entry 0x6060, fake WWS, immediate MFC, all 5 lifted datamods swapped in by DMA, on a synthetic 1280x720 frame, 430 tiles. The chain matches the model, final AO 99.91% within ±1, max 2. Labels 136 and 137 were written with the pass values. The iterator ended at `FFFF0078 00780078`.
* **A bookkeeping-only replacement is written and tested.** `skip.c` replaces lifted `0x4330`, which is reachable through `spu_indirect_branch`.
  - It runs the job's own claim and completion functions and drops all tile DMA and datamod work.
  - It gives the same final iterator state and the same two label PUTs, with 6 GETs and 2 PUTs in total. The full run needs 935 GETLs and 430 PUTLs.

## 1. The suspected list-PUT bug: a logging artefact

### What the log prints

`dma:1223-1235` (SPU_PUT_JOB) sits in `mfc_submit_body` *before* the list or non-list dispatch at `dma:1537`. Its comment says "every PUT (plain or list element)", but it prints:

```
"[mfc] PUTJ ea=0x%08x size=0x%x lsa=0x%05x cmd=0x%02x eah=0x%08x", (uint32_t)ea, size, lsa, cmd, ea>>32
```

For `cmd 0x24` (PUTL), per CBEA:
* `ea` = MFC_EAL = **LS address of the list** (8-byte aligned).
* `size` = list byte size = 8 × number of elements.
* `lsa` = destination LS of the data.

Elements are never logged here. They are logged only by `SPU_PUT_EA` (`[mfc-list] PUTL-elem`, `dma:865`), which has no job filter.

### The guest code is consistent with this reading

* PUTL helper `job:2381-2430` (0x5D98/0x5DB8):
  ```
  MFC_LSA = r3 (data LSA)    MFC_EAH = 0 (il 0, job:2417/2420)
  MFC_EAL = r4 (list LSA)    MFC_Size = r5 (list bytes)
  cmd = 36
  ```
  It asserts `r3&15==0`, `r4&7==0`, `size<=16K`, `size&7==0`, `tag<=31`.
* Its only caller is `job:1921-1927` (0x57DC):
  - LSA = `[0x6510+16*buf+4*i]`
  - list = `[0x6530+16*buf+4*i]`
  - size = `[0x6550+4*i]`
  - tag = r90 (tag 2)
* The lists are built by `job:373-462` (0x4448) right after the datamod has run (`job:1831-1866` → 0x57F4). Each element is:
  ```
  { size = (tile_w + 2ax)*bpp,
    EA   = stream.ea + (ty*tile_h - ay + row)*pitch + (tx*tile_w - ax)*bpp }
  ```
  Two elements are written per quadword, stepping by 2·pitch.
* `stream.ea` comes from the 128-byte pass block in main memory. The PPU fills it (`ppu10:50077` `func_0054EEA8`, word +4) with:
  - `local_base + offset` for location 1 (local memory, `ppu6:60744-60755`; local_base = `[0xB77680+0]` = 0xC0000000 in our HLE, `cellGcmSys.c:422`);
  - otherwise the IO-offset → EA table lookup `ea = (tab[off>>20]<<20)|(off&0xFFFFF)`, or 0 if `tab[] > 0xFFF`.
* The observed "EAs" 0x21200..0x39A00 all lie in buffer 2 (LS 0x6C00..0x3D400), next to the data LSAs 0x1C200..0x34B80. The sizes are 8·rows:

| size | elements | pass |
|---|---|---|
| 0x40 | 8 | D (640x8 tiles) or E (out tile 640x8) |
| 0x168 | 45 | C (128x45 tiles, from the PPU tiling table at 0x74A930) |
| 0x1E0 | 60 | B: s3 is the full-res AO tile, 160x60 when the half-res tile is 80x30 |

* The observed 112-byte GETs × 14 fit B's s0 too: (80+2·16) bytes per row, first-row EAs stepping 0x4B00 = 30 rows × 640.

### Runtime list path

* `dma:817-914` runs the list: element word0 = `[S:1][rsvd][size:15]`, `eal` = word1, `EA = (EAH<<32)|eal`, `lsa = (dest&~15)|(eal&15)`, dest advances by `roundup16(size)`.
* `dma:954-964` and `dma:1537-1563` take the list address from MFC_EAL.

This is correct. One case looks dangerous but is not: input (GET) lists are placed at the *end* of their own destination buffer (`job:2262-2277`, list LSA = data + size − roundup16(8·rows)). The sequential element processing never overwrites an unread element. The whole-job harness, using the same semantics, produced correct results.

### Fix (only to the log, optional)

In `dma:1228` skip list commands, or print them as `PUTL list@LSA n=size/8`. To see real element EAs of this job, use `SPU_PUT_EA=lo:hi`, e.g. `SPU_PUT_EA=C0000000:D0000000` for VRAM-bound rows. Adding the same `spu_ls_read32(spu,0x4010)==job` filter that the GET log has (`dma:847`) would make it per-job.

## 2. The job framework (jobmod e6fe, LS 0x4000-0x6580)

### Entry and setup

**Entry 0x6060** (`job:2609`):
* `[0x6310]` = WWS API pointer.
* Zeroes BSS 0x6330-0x6580. Ctor and dtor lists at 0x6300/0x630C are empty.
* Calls main 0x4180 (`job:2662` → 0x4440).

**main** (`job:132-367`):
* `getbuf(1)` (API+52) → buffer 1 (LS in r83), whose word 0 = **state EA**.
* `getbuf(2)` → buffer 2 = LS 0x6C00 (size 0x36800, datamod code + tiles). It must be 0x6C00, because the datamods are linked there.
* API+68. Three `alloctag` (API+60): tag1 = GET, tag2 = PUT, tag3.
* Then **0x4330** (`job:276-297`) builds the config at sp+0x20/0x30:
  ```
  { buf2 LS, 0x36800, tag1|tag2, tag3 }
  { 0x4030 "yield?" -> always 0, 0x4098 datamod init (bss/ctors), 0x4038 datamod run, 0 }
  ```
  0x4598 copies the config to LS 0x6400/0x6410.
* **0x437C**: `do rc = step(buf1.w0) while (rc)`, step = 0x5258. Then 0x43A8.. frees the tags (`job:299-367`).

### Shared state and header

**Shared state** = one doubleword at the state EA S. Its line is `S & ~127` (mask at LS 0x6260); the dword is at `S & 0x78`. Fields (big-endian u16):

| field | meaning |
|---|---|
| h0 | current pass (0xFFFF = all done) |
| h1 | next tile to hand out |
| h2 | tiles finished |
| h3 | tiles in the current pass |

**Header** = 16 bytes GET from S (0x45C8, `job:487-517` → LS 0x6500):
* bytes 0-7 = the state;
* bytes 8-9 = **number of passes**;
* word 3 = **EA of the pass table**.

Pass k = 128 bytes at `hdr.w3 + 128k`, loaded by 0x4640 (`job:519-552`) → LS 0x6380 when h0 changes.

### Pass block (built by `ppu10:50127` `func_0054EF50`, read by the job)

| off | field | used at |
|---|---|---|
| +0x00 | u8 tiles_x, u8 tiles_y | job:1603 (divide), job:2111-2117 (double-buffer if >1), job:700-706 (h3) |
| +0x03 | bit0 = write label when the pass completes | job:716 |
| +0x04 | label EA (4-byte PUT) | job:793-819 |
| +0x08 | label value | job:795-799 |
| +0x0C | datamod EA | job:1564-1583 (GET if ≠ last loaded) |
| +0x10 | datamod code+bss size (rounded 16), for LS allocation | job:2100-2102 |
| +0x14 | datamod DMA size | job:1577-1580 |
| +0x18 / +0x1C | parameter EA / size, GET to buf2 + code size | job:2025-2037 |
| +0x40+16i | stream i: `{u32 ea, u32 pitch, u16 tile_w, u16 tile_h, u8 ax, u8 ay, u8 bpp, u8 planes<<2 \| type}`; type 1 = in (GETL), 2 = out (PUTL), 3 = LS-only, 0 = none; planes = LS buffer size multiplier (outputs ×4 room for floats) | 4448, 4D60, 5960 |

### Per-tile flow (0x5258, `job:1491-2006`)

1. **Claim** 0x4A50/0x4AAC (`job:826-969`): GETLLAR, `h1++`, PUTLLC.
   - h0 == 0xFFFF → return 0 (done).
   - h1 == h3 → 0xFFFE: the pass is handed out but other SPUs still hold tiles; step returns 2 and 0x437C calls it again (spin barrier).
   - Otherwise it loads the pass block (0x4640) and allocates the LS buffers (0x5960, `job:2049-2293`, two buffers when tiles > 1). It returns the tile index; `tx = idx % tiles_x`, `ty = idx / tiles_x` (`job:1595-1643`).
2. **Load the datamod** (if changed) and the params. Build the GET lists (0x4448) and issue GETL per input stream (`job:1937-1952`). The next tile is prefetched into the other buffer while the current one is processed (`job:1690-1817`).
3. **Fix up the apron** (0x4D60, `job:1082-1490`): clamp-to-edge.
   - Rows above or below the image are copied from the first or last real row (ty == 0 / ty == tiles_y−1).
   - Columns left or right are filled by splatting the edge pixel (masks 0x6270-0x62E0, per bpp, tx == 0 / tx == tiles_x−1).
   - It also writes S+12+4i = LS pointer to each stream's tile origin.
4. **Run the datamod**: entry `0x6C00 + hdr[+0x14]` = 0x6C30, r3 = S, where S is:
   ```
   S+0x00 = 0x6380 (pass block)   S+0x04 = tx   S+0x08 = ty
   S+0x0C.. = stream pointers     S+0x1C = param LSA
   ```
5. **Build the PUT lists** (0x4448) and issue PUTL per output stream (0x5D98).
6. **Complete** 0x46D0/0x47F8 (`job:562-777`) after waiting on the previous tile's PUT tag:
   - GETLLAR, `h2++`. If `h2 == h3`: h0++, h1 = h2 = 0, h3 = tiles of the new pass (or h0 = 0xFFFF after the last pass). PUTLLC.
   - If the finished pass has flag bit0, PUT the 4-byte `pass+8` to `pass+4` (0x49C8, `job:789-824`, tag1, waits).
   - The final pass callback (0x6410 w3) is 0, so there is none.

## 3. The 5 passes (PPU `func_0035451C`, `ppu6:61584`)

### Objects

* `rt = [r30-0x7FD8]` = 0x00A3C990 (render-target manager). Its surface pointers are at +0x3660, +0x37D0, +0x34F0, +0x3940, +0x3AB0. Each surface is `{u32 offset, u32 location&3, …, u32 (w<<16)|h at +0x18}`.
* `obj` = the SSAO object (r3).
* Label EAs: `obj+0x294 = base + 16·136` (label 136), `obj+0x298 = base + 16·137` (label 137). The PPU zeroes both labels every frame (`ppu6:61610-61640`).
* There is a per-frame toggle at `obj+0x290` (`ppu6:62159`), so the header and job handle are double-buffered. That would explain two state EAs being seen (0x00A94800 and 0x00A94A00).

### Passes

| # | pass block | builder | datamod | streams (surface, bpp, apron ax/ay, planes, type) | params | tiling |
|---|---|---|---|---|---|---|
| E | obj+0x000 | `func_00354180` `ppu6:61312` | 0x87D500 | s0 = rt+0x3660 full-res **Z24S8** depth (4, 0/0, ×1, in); s1 = rt+0x37D0 **half-res linear depth** (4, ×1, out) | obj+0x380 (16 B) | 1280x16 (table 0x74A940) → 1×45 |
| A | obj+0x080 | `func_0035379C` `ppu6:60592` | 0x87F880 | s0 = rt+0x37D0 half depth (4, **12/9**, ×1, in); s1 = rt+0x34F0 full-res **normals ARGB8** (4, 0/0, in); s2 = rt+0x3940 **half AO** (1, ×4, out) | obj+0x390 (0x100 B) | automatic (`func_0054EC04`, LS budget) |
| D | obj+0x100 | `func_00353F38` `ppu6:61146` | 0x87D880 | s0 = rt+0x3940 AO (1, 16/0, ×4, in); s1 = rt+0x37D0 depth (4, 4/0, in); s2 = rt+0x3940 AO (1, ×4, out, **in place**) | obj+0x490 (16 B) | 640x8 (0x74A938) → 1×45 |
| C | obj+0x180 | `func_00353CF4` `ppu6:60981` | 0x87E480 | s0 = rt+0x3940 AO (1, 0/2, ×4, in); s1 = rt+0x37D0 depth (4, 0/2, in); s2 = rt+0x3940 (out, in place) | obj+0x490 (same) | 128x45 (0x74A930) → 5×8 |
| B | obj+0x200 | `func_003539E4` `ppu6:60758` | 0x87EC80 | s0 = rt+0x3940 AO (1, 16/1, ×4); s1 = rt+0x37D0 depth (4, 4/1); s2 = rt+0x3660 **full Z24S8** (4, 0/0); s3 = rt+0x3AB0 **full-res AO** (1, ×4, out) | obj+0x4A0 (16 B) | automatic; in-game it looks like half 80x30 / full 160x60 (8×12) |

The f880 parameters at obj+0x390 are recomputed by the PPU every frame (`ppu6:61735-62040`): 12 sample vectors normalised and scaled, stored SoA, plus scalars. Per the composite, rt+0x3AB0 should be the B8 1280x720 texture at VRAM 0x0F800000. Confirm it with the capture in §7.

## 4. Datamod math (validated)

### Conventions

* "Sat" = clamp to [0,1].
* The SPU packs to bytes by truncation:
  - f880 output: `cfltu(sat,141)→cuflt(·,123)` saturates and truncates to 1−2⁻²⁴, then `×255/256 ×256` and trunc. **The maximum value is 254** ("fully open" = 254/255 = 0.996).
  - The blurs and the upsample work in the 0..255 domain and truncate (`cfltu(x,173)`).
* Reciprocals are frest+fi (one Newton step only in d500/ec80's depth linearisation). Our runtime's fi has ~2e-4 relative error.

### E: d500, depth downsample (`d500:119-182`)

```
z24 = Z24S8 word >> 8 at full-res (2x, 2y)       // mask 0x6EA0 picks bytes 0-2 of pixels 0,2,4,6
HD(x,y) = 1 / (P.x * 2^-24*(1+2^-23) * z24 + P.y)  // 0x33800001 at d500:122/126; frest+fi+Newton d500:151-166
```

* The input row stride is 2 rows (`d500:80`, r83 = 2·bpp·w), so the sample is a **point sample of the top-left texel**.
* With D3D depth, `P = (-(f-n)/(f·n), 1/n)` gives view depth w.
* Harness: max relative error 7e-8.

### A: f880, half-res SSAO (`f880:6-235` setup, `237-320` normals, `322-482` row prologue, `484-1062` sample loop, `1108-1183` pack)

**Parameter block (0x100 bytes):**

| offset | content |
|---|---|
| +0x00 | X[12] (SoA) |
| +0x30 | Y[12] |
| +0x60 | Z[12] |
| +0x90/+0xA0/+0xB0 | rows of a 3x3 matrix M |
| +0xD0 | D0 = (norm, Kx, Ky, Kz) |
| +0xE0 | E0 = (e0, e1, dmin, dmax) |
| +0xF0 | F0 = (scale, bias) |

**Per half-res pixel (x, y):**

```
d   = HD(x,y);  dc = clamp(d, dmin, dmax);  q = d/dc               // f880:335-351, 367
n   = (R,G,B of normals(2x,2y)) / 127.5 - 1                         // cuflt 147 + fma(·,0x40008081,-1), f880:258-271 (A byte unused)
n'  = n.x*M[0] + n.y*M[1] + n.z*M[2]                                // f880:270-317 (stored SoA in place of the normals tile)
dref = d + n'.z * q                                                 // f880:453 r60 = fma(n'z, q, d)
acc = 0
for k in 0..11:
    tx = x + floor(0.5 + (X[k]*Kx + n'.x)/dc)                       // f880:359-365 (bias 28.5+lane / 32.5, cflts trunc, -32: 405-429)
    ty = y + floor(0.5 + (Y[k]*Ky + n'.y)/dc)
    ds = HD(tx, ty)                                                 // tile has apron 12 px x 9 rows; clamp-to-edge outside the image
    h  = Z[k] * Kz * q                                              // f880:433, 449
    t  = (ds - dref + h) * 0.5                                      // 0.5 = ilhu 0x3F00 f880:550; f880:566-582
    acc += clamp(t, 0, h) + h * sat(t * e0/q + e1*q)                // f880:598-630 (clamp), 524/526 (e0/q, e1*q), 616/626/644/658 (sat), fma 700/732
AO = acc / (norm * q)                                               // f880:566 r88 = norm*q
out = min(254, floor(255 * sat(AO*scale + bias)))                   // f880:1108-1183
```

What this means:
* Each sample is a vertical line segment of half-length h through the point on a disc offset along the normal.
* The visible fraction of each segment is accumulated, which gives "unoccludedness". On a flat surface every segment is half visible: acc = Σh/2, so norm = ΣZ/2 maps it to 1.
* e0/e1 add a soft extra term.
* Offsets beyond the apron (more than 12 px or 9 rows) read outside the tile, so the game must keep `dmin` large enough. The GPU version has no such limit; clamp to the edge.

Validation:
* Raw accumulator vs model: ≤5e-3 relative (usually ~2e-4).
* Bytes: 99.7% exact on random tests.
* In the whole job, 99.5% exact, 99.94% within ±1. All 131 outliers are texel-boundary flips explained by a ≤4e-4 error in 1/dc, i.e. the emulated frest/fi.

### D: d880, horizontal blur, and C: e480, vertical blur

Same math, 5 taps (`d880:157-476`, `e480:113-232`). Params `P = obj+0x490 = (k, w2, w1, -)`:

```
wgt(o) = max(0, 1 - |D(o) - D(0)| * k / D(0))                       // e480:132-164, |·| via mask 0x7FFFFFFF
out = floor( (A0 + w1*(wgt(-1)A-1 + wgt(+1)A+1) + w2*(wgt(-2)A-2 + wgt(+2)A+2))
           / (1 + w1*(wgt(-1)+wgt(+1)) + w2*(wgt(-2)+wgt(+2))) )     // e480:173-228 (1/sum: frest+fi)
```

* A = AO in 0..255 (`cuflt(·,155)`, `e480:234-280`). D = half-res linear depth. Taps lie along x for d880 and along y for e480.
* **Both run in place on the AO surface.**
  - D is deterministic: full-width tiles, ay = 0.
  - C uses 128x45 tiles with ay = 2. A tile's top apron rows are therefore usually the *already vertically blurred* last rows of the tile above. The whole-job harness models this by processing tiles in order and matches exactly. On the GPU, a clean ping-pong is visually identical.
* Harness: within ±1 (97-98% exact).

### B: ec80, depth-aware 2x upsample (`ec80:188-470`; depth linearisation `ec80:616-677`; pack `ec80:528-575`)

Params `P = obj+0x4A0 = (A, B, k, -)`:

```
Z  = 1/(A*2^-24*(1+2^-23)*z24(X,Y) + B)          // full-res linear depth from raw Z24S8, same as d500
x = X>>1, y = Y>>1, sx = (X odd ? +1 : -1), sy = (Y odd ? +1 : -1)
taps (dx,dy,bw): (0,0,9) (sx,0,3) (0,sy,3) (sx,sy,1)   // consts 9,3,1: ec80:233-235
w  = bw * ( max(0, 1 - |HD(x+dx,y+dy) - Z| * k / Z) + 1e-4 )   // eps 9e-4/3e-4/1e-4 = 0x3A6BEDFA/0x399D4952/0x38D1B717 ec80:203-215
out(X,Y) = floor( Σ w*AO(x+dx,y+dy) / Σ w )       // clamp-to-edge at the borders (ay = 1, ax = 16 bytes)
```

Harness: within ±1 (88% exact).

## 5. Harness results (all in `scratchpad/ssao`; rerun with `run_all.sh`)

* **`h/dm.c` + `h/emu.py`**: runs one lifted datamod (entry 0x6C30) on a hand-built LS (pass block at 0x6380, S at 0x3F000, params at 0x3E000).
  - `t_d500.py`, `t_e480.py`, `t_d880.py`, `t_ec80.py` and `t_f880o.py` compare each datamod with its model.
  - `h/f880t/` is a copy of the f880 lift with LS-read tracing and a raw-accumulator mode (`F880_RAW=1`). It was used to map the sample layout and the response curves (`t_f880c..n.py`).
* **`job/jobrun.c`**: the whole job, run like `fullrun.c` but with a datamod swap on the GET to 0x6C00.
  - `job/build_run.py` builds the synthetic frame, pass table, params and labels.
  - `job/stages.py` checks each pass against `job/pipeline_ref.py`, the numpy reference of the full chain.

| result | value |
|---|---|
| E | HD relative error 2e-5 |
| A | 99.5% exact, 99.94% within ±1 |
| D, C, B | 100% within ±1 |
| full chain | 99.91% within ±1, max 2 |
| labels | 136 = pass E value, 137 = pass B value |
| final state | `FFFF0078 00780078` |

* **`job/skip.c`** (`ssao_skip_4330`): bookkeeping only.
  - Same state and labels, no tile DMA (GET = 6 headers/pass blocks, PUT = 2 labels).
  - 262 hops vs 84k.
  - The weak hook `ssao_pass_hook` prints each pass block as it is retired. That is where a GPU path would read the surface EAs, pitches and param EAs.

## 6. GPU (Metal) plan

### Inputs per frame

All of these can be read from guest memory when the SPU job runs; the hook in `skip.c` sees each pass block at LS 0x6380:
* Each pass's stream EAs and pitches, giving the surfaces (rt+0x3660, 0x34F0, 0x37D0, 0x3940, 0x3AB0).
* Its param block (EA pass+0x18, size +0x1C): E/B `(A, B, k)`, A's 0x100-byte block, and the blur `(k, w2, w1)`.

On the GPU, use the RSX's own depth (D24S8) and normal (ARGB8) render targets for this frame. These are the sources of the depth copy at 0x47884B80 and the normals copy. The rt surface offsets tell which RTs they are.

### Passes

These can be fragment shaders on fullscreen quads, or compute.

1. **HD** (640x360, R32F): `1/(A*z24(2x,2y)*2^-24 + B)`, point sample. Bind it at rt+0x37D0 *only if* the RSX samples that surface (who waits on label 136?).
2. **AO_half** (640x360, R8Unorm or R16F): the f880 formula in §4. The 12 samples come from the param block, which goes into a constant buffer as is (X[12], Y[12], Z[12], M, D0, E0, F0). Use texel-exact loads of HD (`floor(p+0.5+off)`, clamp to edge). Write `min(254, floor(255·v))/255`, or just `v·254/255`.
3. **Blur H then V** (R8, ping-pong): 5-tap weights from `(k, w2, w1)`, depth from HD.
4. **Upsample** to 1280x720: the 9/3/3/1 bilateral above, with Z from the full-res depth RT. **Write into the Metal texture that backs local offset 0x0F800000** (B8 → R8Unorm). Mark that VRAM range GPU-owned so the backend does not re-upload stale guest bytes over it.

Cost: four cheap full-screen passes.

### When to encode them

On the RSX thread, at the semaphore acquire of label 137 that guards the composite, or at the first draw after it that samples offset 0x0F800000. That keeps them in order before the composite. Depth and normals must still hold this frame's contents there. If later draws modify them, encode right after the depth/normal copy instead (where the RSX writes 0x47884B80).

### What the SPU must still do

Exactly what `skip.c` does: run the job, keep the WWS buffers, tags and header GET, claim through the job's own 0x4A50, complete through 0x46D0, and free the tags. That gives:
* label 136 = pass E's `+8` value after E;
* label 137 = pass B's value after B;
* state h0 = 0xFFFF, so the PPU sees the pipeline finished.

The replacement cannot be a no-op. The PPU zeroes labels 136/137 every frame and the RSX waits on 137.

Use `spu_replace_function(226, 0x4330, ssao_skip_4330)`. 0x4330 is reached through `bie` at `job:273` (0x5258/0x437C are direct calls).

Batching:
* A single job grabs the rest of a pass in one reservation (`h1 = h3`), then does `h2 += n−1` and one real 0x46D0 completion (label + next pass).
* Other SPUs in the pass get 0xFFFE and spin as before. This is the same invariant as the light-job replacement.
* The step contract is 0 = done, 2 = spin. There is no time-budget yield in this job: 0x4030 always returns 0.

## 7. Capture to validate on real data

### What to dump

**(a) LS and GPRs at a real PUTL of this job**, using the existing dump hook (`dma:1546-1559`):
* `SPU_LS_DUMP_LIST=0x30200 SPU_LS_DUMP_FILE=/tmp/ssao_putl.bin`. 0x30200 is one of the 60-row lists, pass B. Also dump 0x25600 (45 rows, pass C).
* The dump holds:
  - 0x6380: the current pass block (stream EAs, pitches, tiles, param EA);
  - 0x6500: the header (pass count at +8, pass table EA at +12);
  - 0x6510-0x656F: the LS layout;
  - the S struct at r1+0x20 of 0x5258;
  - the params in LS (`[S+0x1C]`);
  - the tile buffers, and the list itself, which shows the real element EAs.

**(b) Main memory**, with `RSX_SNAP` or a manual dump: before = at the label-136 PUT (or earlier), after = at the label-137 PUT.
* The pass table: `hdr.w3`, 5×128 B. With obj ≈ 0x00A94600-0x00A94800, dumping 0x00A94000-0x00A96000 covers the passes, header and params.
* The five surfaces, at the EAs in the pass blocks:
  - DZ Z24S8 1280x720;
  - NRM ARGB8 1280x720;
  - HD f32 640x360;
  - HAO B8 640x360 (looks like main memory near 0x47CEExxx, pitch 640);
  - FAO B8 1280x720, expected EA 0xCF800000 = VRAM 0x0F800000.

**(c)** `SPU_PUT_EA=C0000000:D0000000` (and/or the FAO EA range) to confirm that pass B's row PUTs land in VRAM 0x0F800000, and with which pitch.

### What to run with it

Feed (b) to `job/build_run.py` (replace the synthetic scene and params with the dumped ones) and compare HD, HAO and FAO. Or run `stages.py`-style checks pass by pass with `pipeline_ref.py`.

## 8. Open points

1. **The obj and header addresses are not pinned down statically.** Two state EAs were seen; that fits the double-buffered header at obj+0x290 and the per-frame slots. A GPU hook does not need them: read LS 0x6500 / 0x6380 at run time.
2. **Who consumes label 136 / half-res depth rt+0x37D0** on the RSX side is unknown. If nothing does, pass E can be dropped from the GPU path; its label must still be written.
3. **The f880 parameter meaning** (radius, focal length, norm = ΣZ/2?) is inferred from the formula, not from the PPU math (`ppu6:61735-62040`). The GPU path should just consume the block.
4. **Location of rt+0x3AB0**, local (EA 0xC0000000+off) or IO-mapped, is assumed from the composite's texture offset. The capture settles it.
5. **Behaviour for Z[k] < 0** (not physical) differs from the clamp formula. It was ignored.

## 9. Real capture (frame 0xB5F, `scratchpad/ssnap/`) and the GPU port (`scratchpad/ssao/gpu/`)

* obj = **0x00A94600**: pass table there (E,A,D,C,B at +0x000..+0x200), header/state at **0x00A94880**
  (`0005` passes, table EA 0x00A94600), params E 0x00A94980, A 0x00A94990 (0x100), D/C 0x00A94A90, B 0x00A94AA0.
  Labels 0x20000880 / 0x20000890, value = 0xB60 (frame).
* Surfaces: depth copy 0x47884B80 (pitch 0x1400), normals 0x47500B80 (0x1400), half depth 0x47C08B80
  (0xA00), half AO 0x47CE9B80 (0x280), output **0xCF800000 pitch 0x500** (VRAM 0x0F800000).
  Tilings: E 1x45 (1280x16), A 8x8 (80x45 half), D 1x45, C 5x8, B 8x12 (80x30 half / 160x60 full).
* Params: E/B A=-0.099999, B=0.1 (w = 1/(0.1-0.0999999 z): near 10); A: Kx=Ky=5589.4, Kz=15,
  norm=140.86 (= Kz*sum Z), e0=-1/60, e1=-0.75, dmin=1050.5, dmax=2101, scale 1.2, bias -0.2,
  M = diag(Kx,Ky,Kz) x world->view rotation (normals are world space); blur k=30, w2=0.1, w1=0.6; B k=40.
  So a flat surface with n'.z = 0 (sky, normals ~0) gives 0.4 -> byte 102, a camera-facing one 1.0 -> 254.
* The normals copy was not captured: with normals reconstructed from depth the chain correlates 0.84 with
  the real b_cf800000 and sky pixels (n'=0) match 92.8% exactly. Exact validation needs a_47500b80.
* GPU shaders (MSL) reproduce the numpy chain: A 99.99% exact, D/C/B within +-1 (3 far-depth
  pixels of B off by up to 14 from f32 cancellation), 0.45 ms for all five passes on an M4 Pro.

## 10. End-to-end validation with normals (frame 0xB11, `scratchpad/ssnap2/`)

* Pass A reads the **R,G,B bytes of the normals copy 0x47500B80 at full-res (2x, 2y)** (stream s1, tile
  160x90 full res, pitch 0x1400); the copy is unchanged between label 134 and label 137 in this run.
* Lifted job in the host harness on this frame (`gpu/real_job.py`) vs the game's b_cf800000: 98.95% exact
  (the rest: single-SPU in-place order of pass C).
* The f64 numpy chain is only 97.2% within +-1 (max 18): every pass floors `num * recip(den)` and the SPU's
  `fi(frest(x))` reciprocal (no Newton step) sits just below 1/x, so exact quotients come out one lower
  (flat areas 253, not 254), and the AO pass's 1/dc decides sample texels. With the SPU reciprocal, the
  1.0 -> 1-2^-24 saturate and truncating final multiplies emulated (`gpu/spufp.py`, `emu_*.py`), the numpy
  passes become bit-exact against the lifted code (E, D, C 100%, B 100%, A 99.99%).
* Metal chain with the same emulation (`gpu/ssao_gpu.metal` v2; v1 kept as `ssao_gpu_v1.metal`):
  **99.97% exact vs the real b_cf800000, max |d| = 1** (321 scattered pixels, round-to-nearest vs the
  SPU's truncating intermediate ops), 0.64 ms for the five passes. The game's real output matches a
  clean (ping-pong) vertical blur better than the single-SPU tile order.
