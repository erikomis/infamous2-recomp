# inFAMOUS 2 (NPUA80638) — static recompilation

> **Superseded by [`../infamous2-lle/`](../infamous2-lle/README.md)**, which
> builds against the community fork with firmware LLE and reaches the 3D main
> menu. This tree stays as the upstream v0.14.0 (HLE) attempt, and the other
> port still uses its `game/`, `fw/` and `vfs/`.

A port of inFAMOUS 2 (digital release, `NPUA80638`, app ver 01.03) built with
[ps3recomp](https://github.com/sp00nznet/ps3recomp) v0.14.0 on macOS/arm64.

Nothing from the game or the firmware is in this tree's sources. `game/`,
`fw/`, `vfs/`, `recompiled/`, `prx/` and `spu/` are all generated locally from
your own copy and are git-ignored.

## Status

Boots on native arm64 into the title's main loop, headless or in a Metal
window. No frame has been drawn yet.

What runs:

- CRT, all 23 sysmodule loads, FIOS, the psarc archives (`core`, `install1/2`,
  `patch3/4`) and the SPU zlib inflate task that streams them.
- 22 PPU threads: job manager, trophies, Wwise (`AK::BankManager`,
  `AK::EventManager`), movie loader/player, network.
- SPURS: three `Wws_Job` workloads on the lifted policy module, one taskset,
  the Wwise audio jobs (about 90 a second), the Bink SPU thread.
- cellGcm init, tiles, zcull, display buffers; the RSX engine clears both.

Where it stops: the main thread spins at `lr=0x001EF2A0` reading
`0x00A93C18`, a counter of outstanding "SpuLight" jobs that stays at 3. The
`Wws_Job` policy module issues its job-code load as an MFC list with
stall-and-notify (`[mfc-list] STALL img=3`), the SPU interrupt is taken
(`vector->0x00A2C`), and the stalled transfer is never resumed, so no job
module loads. That is in the runtime's SPU interrupt path, not in this port.

Still unresolved imports seen during boot: `0x2E0A170D`, `0xA7FD2F5B`,
`0x9B882495`, `0xAD218FAF`.

## Layout

| Path | What it is |
| ---- | ---------- |
| `main.cpp`, `stubs.cpp` | The runner, from ps3recomp's project template |
| `prx_init.cpp` | Loads the lifted libsre and serves the SPURS LFQueue from it |
| `run.sh` | Sets the VFS environment and starts the port |
| `tools/boot.sh` | Timed headless run for bring-up |
| `tools/extract_eboot_jobmods.py` | Pulls the 27 `C0DEC0DE` job modules out of the EBOOT |
| `tools/lift_spu_jobs.sh` | Lifts raw SPURS job binaries captured with `SPU_DUMP_MISS` |
| `tools/elf_disasm.py`, `tools/prx_disasm.py` | Disassemble a range of the EBOOT / the relocated libsre |
| `patches/` | Fixes made to the ps3recomp checkout that are not upstream yet |

## Rebuilding from scratch

Paths assume the toolkit is checked out at `../ps3recomp` with
`patches/ps3recomp-v0.14.0-local.patch` applied, a venv at `../.venv`
(`pip install -r ../ps3recomp/requirements.txt`), and RPCS3 with the game
installed. `brew install cmake ninja sdl2 glslang spirv-cross spirv-tools`.

```sh
RPCS3="$HOME/Library/Application Support/rpcs3"
PY=../.venv/bin/python
T=../ps3recomp/tools

# 1. Decrypted EBOOT (RPCS3 does the decryption, with your own licence)
mkdir -p game && cp "$RPCS3/dev_hdd0/game/NPUA80638/USRDIR/EBOOT.BIN" game/
~/Applications/RPCS3.app/Contents/MacOS/rpcs3 --decrypt "$PWD/game/EBOOT.BIN"

# 2. Lift the PPU code. --code-end is the end of .sceStub.text.
$PY $T/ppu_loader.py game/EBOOT.elf -o out/
$PY $T/ppu_lifter.py game/EBOOT.elf --functions out/EBOOT.functions.json \
    --hle-stubs out/EBOOT.imports.json --code-end 0x742e18 \
    --chunk-lines 150000 --output recompiled/

# 3. SPU: the two embedded ELFs and the Wws_Job policy module (a raw blob the
#    title hands to cellSpursAddWorkload: guest 0x8E5B00, 11904 bytes, LS 0xA00)
$PY $T/extract_spu_images.py game/EBOOT.elf -o spu/images
dd if=game/EBOOT.elf of=out/wwsjob_pm.bin bs=1 skip=$((0x8D5B00)) count=11904
$PY $T/wrap_spu_elf.py out/wwsjob_pm.bin --base 0xA00 --entry 0xA00 \
    --out spu/images/wwsjob_pm_at_008E5B00.elf
$PY $T/build_spu_workloads.py --images spu/images --lifted spu \
    --out spu/spu_workloads.c --register-fn infamous2_spu_register_all \
    --constructor --title infamous2

# 4. SPU job modules embedded in the EBOOT
$PY tools/extract_eboot_jobmods.py game/EBOOT.elf --out spu/jobmods
$PY $T/lift_jobmods.py --jobmods spu/jobmods --out spu \
    --register spu/jobmods_register.c --symbol-prefix infamous2

# 5. libsre, from your own firmware
mkdir -p fw && cp "$RPCS3/dev_flash/sys/external/libsre.sprx" fw/
~/Applications/RPCS3.app/Contents/MacOS/rpcs3 --decrypt "$PWD/fw/libsre.sprx"
$PY $T/lift_prx.py fw/libsre.prx --base 0x30000000 --output fw/out/
$PY -c "import json; d=json.load(open('fw/out/libsre_imports.json')); \
json.dump([{'library':l,'nid':n,'stub':s} for l,m in d.items() for n,s in m.items()], \
open('fw/out/libsre_imports_list.json','w'))"
$PY $T/ppu_lifter.py fw/out/libsre_image.bin --raw --base 0x30000000 \
    --toc 0x30039AB0 --functions fw/out/libsre_functions.json \
    --hle-stubs fw/out/libsre_imports_list.json --symbol-prefix libsre_ \
    --code-end 0x3001DB18 --output prx/libsre_ns/

# 6. Game data: a copy-on-write clone, so the port never writes into RPCS3
mkdir -p vfs/dev_hdd0/game vfs/dev_hdd0/home/00000001/savedata vfs/dev_hdd1
cp -Rc "$RPCS3/dev_hdd0/game/NPUA80638" vfs/dev_hdd0/game/
rm vfs/dev_hdd0/game/NPUA80638/USRDIR/EBOOT.BIN
cp game/EBOOT.elf vfs/dev_hdd0/game/NPUA80638/USRDIR/
ln -s dev_hdd0/game/NPUA80638 vfs/PS3_GAME

# 7. Build
cmake -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DPS3RECOMP_DIR="$PWD/../ps3recomp" -DPython3_EXECUTABLE="$PWD/$PY"
cmake --build build -j 8

# 8. The raw SPURS job binaries only show up at run time
mkdir -p spu_miss && SPU_DUMP_MISS=spu_miss tools/boot.sh out/boot.log 40
tools/lift_spu_jobs.sh && cmake -B build && cmake --build build -j 8
```

## Running

```sh
./run.sh                          # Metal window
tools/boot.sh out/boot.log 60     # headless, killed after 60 s
```

`RSX_ENGINE_PASS_DUMP=<dir>` dumps the renderer's passes once the title draws;
`JT_DEBUG=0x<bctr addr>` on the lifter explains one switch dispatcher. The
toolkit's `docs/DIAGNOSTICS.md` lists the rest.

## What the port needed beyond the template

- **`sys_lwcond_t` layout** (toolkit patch). The runtime wrote the lwmutex
  pointer as a be64; the struct is two be32. FIOS tests the first word and
  refused every wait with "wait for invalid cond".
- **Switch behind a tail-call epilogue** (toolkit patch). The lifter took an
  epilogue's `ld r30` before a tail call as the table-base register's
  definition and dropped the jump table; the first one hit was at `0xB9D3C`.
- **SPURS LFQueue from real libsre** (`prx_init.cpp`). The HLE library has no
  `_cellSpursLFQueuePushBody` and its `_cellSpursLFQueueInitialize` does not
  match libsre's line layout. Only the three queue NIDs go to libsre; its
  signal callback at `0x300127CC` is replaced by one that wakes the HLE task.
- **PARAM.SFO at boot** (`main.cpp`), so cellGame reports `NPUA80638`.
