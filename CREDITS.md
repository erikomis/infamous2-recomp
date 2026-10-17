# Credits

🌐 **English** · [Português (BR)](CREDITS.pt-BR.md)

This port only exists because of the work below. Thank you all.

## Toolkit and base

- **[ps3recomp](https://github.com/sp00nznet/ps3recomp)** — Ned Heller
  ([@sp00nznet](https://github.com/sp00nznet)) and
  [contributors](https://github.com/sp00nznet/ps3recomp/blob/master/CONTRIBUTORS.md).
  Runtime, HLE, PPU/SPU lifters and the project template (the ports'
  `main.cpp`, `stubs.cpp`, `CMakeLists.txt` and `config.toml` start from it). MIT.
- **inFAMOUS 1 fork — [NoahBPeterson/ps3recomp](https://github.com/NoahBPeterson/ps3recomp)**
  ([@NoahBPeterson](https://github.com/NoahBPeterson)), branch
  `infamous/rebase-v0.13.0`, upstream
  [PR #213](https://github.com/sp00nznet/ps3recomp/pull/213). The current port
  (`infamous2-lle/`) runs on this fork: boot with the firmware in LLE
  (liblv2, libsysmodule, libsre), the PPU/SPU/lv2 fixes and the conformance
  suites that got inFAMOUS 1 running on macOS. Without that work inFAMOUS 2
  would not have made it past the first frame.

## Upstream pull requests applied

Original diffs in `infamous2-lle/patches/upstream-prs/`.

- [@undecuple](https://github.com/undecuple) —
  [#297](https://github.com/sp00nznet/ps3recomp/pull/297) (lifter: `sc` does not end a function),
  [#275](https://github.com/sp00nznet/ps3recomp/pull/275) (flip in the FIFO),
  [#274](https://github.com/sp00nznet/ps3recomp/pull/274) (EDAT klicensee),
  [#278](https://github.com/sp00nznet/ps3recomp/pull/278) (predicated writes in the fragment program),
  [#268](https://github.com/sp00nznet/ps3recomp/pull/268) (`lqa`/`stqa` in the micro-interpreter; already in the fork).
- [@tekulvw](https://github.com/tekulvw) —
  [#266](https://github.com/sp00nznet/ps3recomp/pull/266) (resume translated SPU code after the interpreter),
  [#302](https://github.com/sp00nznet/ps3recomp/pull/302) (depth compare / shadows),
  [#304](https://github.com/sp00nznet/ps3recomp/pull/304) (sRGB on framebuffer writes),
  [#305](https://github.com/sp00nznet/ps3recomp/pull/305) (per-channel texture gamma),
  [#293](https://github.com/sp00nznet/ps3recomp/pull/293) (clears on Metal),
  [#303](https://github.com/sp00nznet/ps3recomp/pull/303) (clamping of input colors).

## Ports used as reference

- [sp00nznet/tornado](https://github.com/sp00nznet/tornado) — recipe for
  capturing and translating the Wwise SPURS jobs (`tools/lift_spu_jobs.sh`).
- [sp00nznet/youdontknowjack](https://github.com/sp00nznet/youdontknowjack) —
  loading pattern for the translated libsre (`infamous2/prx_init.cpp`).
- The toolkit's LittleBigPlanet WWS job-module path
  (`tools/lift_jobmods.py`), used here for inFAMOUS 2's 27 modules.

## Tools

- [RPCS3](https://github.com/RPCS3/rpcs3) — decryption of the player's own
  EBOOT and firmware (`rpcs3 --decrypt`), and the toolkit's behavior
  reference. Its `Loader/PUP.cpp` and `Crypto/unself.cpp` (SCEDecrypter)
  were read, not copied, for the PUP and firmware-package layout in
  `tools/pup_extract.py`, together with the
  [PS3 Developer Wiki](https://www.psdevwiki.com/ps3/).
- [sp00nznet/twistedmetal](https://github.com/sp00nznet/twistedmetal) —
  `tools/decrypt_self.py` (MIT), the base of the port's own
  `tools/decrypt_self.py` (decryption without RPCS3, NPDRM with the
  player's `.rap`).
- [SDL2](https://www.libsdl.org/), [glslang](https://github.com/KhronosGroup/glslang),
  [SPIRV-Cross](https://github.com/KhronosGroup/SPIRV-Cross),
  [SPIRV-Tools](https://github.com/KhronosGroup/SPIRV-Tools),
  [Capstone](https://www.capstone-engine.org/), CMake, Ninja.

## Game

inFAMOUS 2 © 2011 Sony Computer Entertainment America LLC, developed by
Sucker Punch Productions. This project is not affiliated with or endorsed by
Sony or Sucker Punch, and contains no game or firmware data. To use it you
need a legitimate copy of the game and your own PS3/firmware.
