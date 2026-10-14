# inFAMOUS 2 (NPUA80638) — static recompilation, firmware LLE

The current port. It builds against
[NoahBPeterson/ps3recomp](https://github.com/NoahBPeterson/ps3recomp) branch
`infamous/rebase-v0.13.0` (commit `770660a`, upstream PR #213) with the fixes
in `patches/`, and runs Sony's own liblv2, libsysmodule and libsre, lifted from
your firmware.

`../infamous2/` is the earlier port against upstream v0.14.0 (HLE SPURS). It
stops before the first frame. This port shares its `game/`, `fw/` and `vfs/`.

## Status

On macOS/arm64, headless:

- Health warning, the Bink "Sony Computer Entertainment presents" movie, the
  loading screen.
- The 3D main menu: Cole, the lightning effects, "START A NEW GAME", "Press ✕
  to continue". About 16 fps averaged over a 120 s run that includes boot.
- 2,094 draws a frame into 21 render targets on Metal, with the title's own
  shaders.

Not checked: whether a press of ✕ starts a new game. Every run that tried it
had the Mac go to sleep part-way, which stretches all of the timing.

Fixed since: the saturated-green patch on Cole's scalp (untranslated
pack/unpack opcodes, see below). The intro cutscene ends on its own after
about 7 minutes at this frame rate and the first combat tutorial comes up
("Use the right stick to look up"). The mixer produces sound (`AUDIO_WAV`);
every firmware import the title makes is answered; the reviewer DLC decrypts
once `tools/rap2rifkey.py` has written its key.

The slow path that interpreted 22,000 SPU instructions at a time from LS
`0x6C64` is gone: those were the EBOOT's five `DA7ABA5E` modules, a second
kind of WWS job module the job manager streams to LS 0x6C00, now lifted by
`tools/lift_datamods.py` (menu frame rate went from ~17 to ~30 fps).

## Fixes in `patches/` (against the fork)

`ps3recomp-fork-770660a-with-upstream-prs.patch` is what
`../tools/setup_toolkit.sh` applies: every fix below plus the upstream pull
requests in `patches/upstream-prs/` (see `../CREDITS.md`).
`ps3recomp-fork-770660a-local.patch` is the first three fixes alone, kept for
reference.

- **`tools/ppu_lifter.py`, jump tables.** A tail-call epilogue (`ld r30` ... `b`)
  between the prologue and a switch was read as the base register's
  definition, and the switch was dropped (first seen at `0xB9D3C`).
- **`libs/video/cellGcmSys.c`, FIFO park.** The drain jumped straight to `put`
  the moment it found a self-JUMP park with `put` elsewhere. This title moves
  `put` first and patches the park second, so the frame's fence was skipped and
  the menu froze. A park must now stand for 50 ms before the drain follows `put`.
- **`runtime/spu/spu_channels.c`, interpreter rejoin.** After the job manager's
  stall-and-notify interrupt, the register-save stub returns into the job on
  the interpreter. The "is this address lifted?" probe ignored the resident WWS
  job overlay, so the rest of every interrupted job ran interpreted (83,000
  instructions on average). That was why the menu ran at about 0.4 fps.
- **`libs/video/rsx_fp_decompiler.c`, pack/unpack opcodes.** PK4UB, UP4UB,
  PKG and UPG (and PK4B/UP4B, PK2H/UP2H, PK2US/UP2US) were not translated at
  all: the instruction was skipped and the destination kept its old value.
  The character materials use PK4UB→UPG and PKG→UP4UB as their sRGB↔linear
  conversion right before the outputs, so Cole's scalp came out saturated
  green and skin tones were off. Implemented after RPCS3's
  FragmentProgramDecompiler (`_builtin_pkg`/`_builtin_upg`). An unhandled
  opcode now prints its number and raw words in the dumped shader.
- **`libs/hardware/cellGem.c`, `cellCamera.c`.** `cellGemGetMemorySize`,
  `cellCameraSetNotifyEventQueue2` and `RemoveNotifyEventQueue2`, the three
  imports that still fell through to the unresolved stub (with upstream PR
  #247 for `cellFsGetDirectoryEntries`).
- **`libs/input/cellPad.c`.** `PAD_STICK` takes several `lx,ly,rx,ry,from,until`
  holds separated by `;`.
- **`libs/video/rsx_texture_layout.c`, one-channel crossbar.** X16, X32_FLOAT and
  the DEPTH16/24 texture formats are one real channel uploaded as R16/R32F, and
  the sampler presents it on every lane (as B8 already was). This title binds
  its 1024x8 X16 lighting ramp with crossbar 0xAAEE (R from B), which read the
  empty B component: zero. Unit test `test_remap_one_channel_lanes`.
- **`libs/video/rsx_draw_engine.c`, GPU -> guest write-back.** Every frame the
  game NV3089-copies its normal and depth targets into main memory for the
  SPUs' lighting; the copy ran on the CPU from guest memory, which never holds
  what the GPU drew, so the SPUs lit an empty G-buffer. `rsx_draw_engine_write_back()`
  (called from `nv3089_blit()`) submits the pending work and reads the target
  (or the depth buffer, as Z24S8 bytes) back into guest memory first. Only a
  target that starts at the blit's offset, or within whose first row it lies,
  qualifies (the phantom target at offset 0 must not claim the depth buffer at
  0x10000), and the depth decode it queues is invalidated again afterwards so a
  fog or depth-of-field pass later in the frame still resolves at its own point
  -- reusing the early decode blacked out every cutscene. No measurable
  frame-rate cost. `PS3RECOMP_ENG_WRITEBACK=0` turns it off, 1 colour only,
  2 depth only.
- **`libs/video/cellGcmSys.c`, semaphore waits.** `GCM_SEMA_ACQUIRE=1` with
  `GCM_SEMA_ACQUIRE_MS=<ms>` blocks the FIFO walker on an NV406E ACQUIRE for at
  most that long (the stuck-get watchdog no longer resyncs while it waits), so
  the RSX samples the SPUs' output after they wrote it. Off by default: this
  title's menu looks the same either way. `GCM_SEMA_TRACE=<n>` prints the
  semaphore methods with the label's value, and the RSX's own label writes.
- **`PS3RECOMP_ENG_TEXLOG=1`.** One line per texture the engine creates (guest
  address, format, size, crossbar, handle), which render target a unit's
  address aliases on each draw, every surface with its handle, and each
  write-back. Pass-log `tex=` numbers drift between runs; addresses do not.
- **`libs/video/rsx_metal_backend.m`, diagnostics.** `PS3RECOMP_METAL_PASS_LOG=2`
  splits a pass at every pipeline/texture change; `_MIN` and `_EVERY` set the
  record threshold and period for the pass log, `PS3RECOMP_METAL_SURF_DUMP_MIN`
  for the surface dump (the defaults only fire on frames with more than 1000
  records, which a cutscene does not reach).

## Layout

| Path | What it is |
| ---- | ---------- |
| `main.cpp`, `stubs.cpp`, `CMakeLists.txt` | The fork's project template, plus PARAM.SFO at boot, the `lle/` modules and the Wws_Job code region |
| `lle/` | liblv2, libsysmodule, libsre (`tools/lift_firmware_module.py`) |
| `lle_spu/` | libsre's SPURS kernel and code bodies (`tools/lift_firmware_spu.py`) |
| `recompiled/`, `spu/` | The title's PPU and SPU lifts |
| `tools/boot.sh` | Timed headless run |
| `tools/capture.sh` | Headless run that keeps a PNG of the presented frame every 10 s |
| `tools/lldb_barrier.py` | Prints the WWS job barrier lanes (attach did not work here; kept for a debug launch) |
| `tools/rap2rifkey.py` | Derives an EDAT's RIF key from the player's own `.rap` and writes the `<edat>.rifkey` sidecar |
| `tools/relift_jobmod.py` | Re-lifts one WWS job module with extra entry addresses |
| `tools/lift_datamods.py` | Extracts, lifts and registers the five `DA7ABA5E` modules the job manager streams to LS 0x6C00 |
| `tools/passlog.py` | Summarises a `PS3RECOMP_METAL_PASS_LOG` log: passes, targets, pipelines, textures |
| `tools/psfeatures.py` | One line per dumped pixel shader: units sampled, targets, risky ops |
| `tools/surfstats.py`, `tools/greenshare.py` | Fill/green/brightness stats of dumped render targets and captured frames |

## Rebuilding

From `../infamous2/README.md`, steps 1, 4 (job modules) and 6 (vfs) are
shared. With `T=../ps3recomp-fork/tools` and `P=../.venv/bin/python`:

```sh
# firmware: decrypt your own liblv2.sprx, libsysmodule.sprx, libsre.sprx into
# ../infamous2/fw/ with `rpcs3 --decrypt`, then
$P $T/lift_firmware_module.py ../infamous2/fw/liblv2.prx       --base 0x30000000 --out lle/liblv2
$P $T/lift_firmware_module.py ../infamous2/fw/libsysmodule.prx --base 0x30180000 --out lle/libsysmodule
$P $T/lift_firmware_module.py ../infamous2/fw/libsre.prx       --base 0x30100000 --out lle/libsre
$P $T/lift_firmware_spu.py --module-dir lle/libsre --base 0x30100000 --name libsre \
    --first-image-id 100 --body 0x21480:0x2200:0xA00:0xA00:0x2110 \
    --body 0x23680:0x1E40:0xA00:0xA00:0x1C80 --out lle_spu

# the title
$P $T/ppu_loader.py ../infamous2/game/EBOOT.elf -o out/
$P $T/ppu_lifter.py ../infamous2/game/EBOOT.elf --functions out/EBOOT.functions.json \
    --hle-stubs out/EBOOT.imports.json --code-end 0x742e18 \
    --max-chunk-lines 150000 --output recompiled/
$P $T/build_spu_workloads.py --images spu/images --lifted spu --out spu/spu_workloads.c \
    --register-fn infamous2_spu_register_all --constructor --title infamous2 \
    --eboot ../infamous2/game/EBOOT.elf --extra-funcs wwsjob_pm_at_008E5B00=0xA2C
$P $T/lift_jobmods.py --jobmods spu/jobmods --out spu \
    --register spu/jobmods_register.c --symbol-prefix infamous2
$P tools/lift_datamods.py ../infamous2/game/EBOOT.elf --extra 0x6C44,0x6C64
$P tools/rap2rifkey.py ../infamous2/vfs/dev_hdd0/game/NPUA80638/USRDIR/dlc/ulf/unlock_review.edat

cmake -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DPS3RECOMP_DIR="$PWD/../ps3recomp-fork" -DPython3_EXECUTABLE="$PWD/$P"
cmake --build build -j 8
```

`spu/images/` holds the two SPU ELFs from `extract_spu_images.py` and the
wrapped policy module (see `../infamous2/README.md`, step 3). The policy
module's image id (3) is hard-coded in `stubs.cpp`.

## Running

```sh
./run.sh                                   # Metal window
tools/capture.sh menu 120                  # headless, PNGs in out/menu/
PAD_SCRIPT="95:0x4000,110:0x4000" tools/capture.sh newgame 180   # press ✕ at 95 s and 110 s
```

Useful diagnostics: `SPU_REJOIN_DBG=<n>` (long interpreter runs),
`PPU_HOTMAP=1` (what a spinning PPU thread polls), `PS3_MEMDUMP_AT=ea:len:sec:file`,
`PS3_THREAD_DUMP=<sec>`, and `sample <pid> 3` for native stacks (lifted
functions are named `func_<guest address>`).
