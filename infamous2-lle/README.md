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

Known slow path: an SPU job enters LS `0x6C64` unlifted and runs about 22,000
instructions on the interpreter each time before it returns to `0x56B8` /
`0x56D0`. Which image holds that code is still open.

## Fixes in `patches/` (against the fork)

`ps3recomp-fork-770660a-with-upstream-prs.patch` is what
`../tools/setup_toolkit.sh` applies: the three fixes below plus the upstream
pull requests in `patches/upstream-prs/` (see `../CREDITS.md`).
`ps3recomp-fork-770660a-local.patch` is the three fixes alone.

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
