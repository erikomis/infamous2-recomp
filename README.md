# inFAMOUS 2 — static recompilation for PC (macOS)

🌐 **English** · [Português (BR)](README.pt-BR.md)

A native port of **inFAMOUS 2** (PS3, digital release `NPUA80638`) to
**macOS/arm64**, built with static recompilation: the game's PowerPC and SPU
code is translated to C/C++ and compiled into a native executable, with
graphics on Metal. It is not an emulator.

Built on [ps3recomp](https://github.com/sp00nznet/ps3recomp) and, above all,
on [NoahBPeterson](https://github.com/NoahBPeterson/ps3recomp)'s **inFAMOUS 1
fork**, which brought the firmware LLE and the fixes that got the first game
running on the Mac. See [CREDITS.md](CREDITS.md).

> [!WARNING]
> **Work in progress, not playable from start to finish yet.** The game runs
> from boot to the first missions in New Marais at 7–16 fps (the original runs
> at 30), only on Apple Silicon macOS, and building it means recompiling
> everything from your own copy (there is no prebuilt executable). What is
> missing is in [ROADMAP.md](ROADMAP.md).

> **This repository contains no game data, no firmware, no Sony keys and
> nothing translated from them.** Everyone generates all of that locally from
> their own legitimate copy. A fan project, not affiliated with Sony
> Interactive Entertainment or Sucker Punch Productions.

## Status

| | |
| --- | --- |
| Boot, firmware LLE, SPURS, Wwise audio (SPU jobs) | ✅ |
| Health warning, Bink videos, loading screens | ✅ |
| 3D main menu, pause menu, options | ✅ |
| New game → intro video → in-engine cutscenes | ✅ |
| Empire City prologue (the Beast, the freighter) | ✅ |
| New Marais: first missions, played in a window with a DualSense | ✅ |
| Brazilian Portuguese dub and subtitles (default), saves, DLC | ✅ |
| Performance | 🚶 ~21 fps walking in New Marais, 25–27 with GPU SSAO (the game targets 30) |
| Whole game | ❓ nobody has gone past 1% yet |
| Keyboard and mouse in a window | 🧪 mapped, barely tested |
| Linux and Windows | ⏳ planned |

Details and next steps in [ROADMAP.md](ROADMAP.md).

## Layout

| Folder | What it is |
| ------ | ---------- |
| [`infamous2-lle/`](infamous2-lle/README.md) | **Current port.** inFAMOUS 1 fork + firmware LLE. |
| [`infamous2/`](infamous2/README.md) | First attempt, against upstream ps3recomp v0.14.0 with SPURS in HLE. Stops before the first frame; kept as reference. Its local `game/`, `fw/` and `vfs/` folders are also used by the current port. |
| `tools/setup_toolkit.sh` | Clones the fork at the commit in use and applies this repository's patches. |

## What you need

- An Apple Silicon Mac (tested on an M4 Pro, macOS 27).
- A legitimate copy of inFAMOUS 2 installed in [RPCS3](https://rpcs3.net),
  with its license (`.rap`) and the PS3 firmware installed there.
- Xcode Command Line Tools and Homebrew:

```sh
brew install cmake ninja sdl2 glslang spirv-cross spirv-tools python@3.12
python3.12 -m venv .venv
```

## Building

```sh
git clone https://github.com/erikomis/infamous2-recomp.git
cd infamous2-recomp
tools/setup_toolkit.sh                        # toolkit in ./ps3recomp-fork
.venv/bin/pip install -r ps3recomp-fork/requirements.txt
```

Then follow the port's recipe in
[`infamous2-lle/README.md`](infamous2-lle/README.md#rebuilding):

1. Decrypt the EBOOT and the firmware with RPCS3.
2. Translate the firmware, the PPU and the SPU programs.
3. Clone the game data into `infamous2/vfs/`.
4. Compile.

Translation takes a few minutes and compiling ~80 s on an M4 Pro.

## Playing

```sh
cd infamous2-lle
./run.sh
```

A controller (DualSense tested) works out of the box. Keyboard and mouse add
on top of it:

| Key | PS3 |
| --- | --- |
| W A S D | left stick |
| I J K L or mouse | right stick (camera) |
| Arrow keys | D-pad |
| Space | ✕ |
| Q / E / R | ○ / □ / △ |
| U / O | L1 / R1 |
| Y / P | L2 / R2 |
| G / H | L3 / R3 |
| Return / Tab | START / SELECT |
| Left / right / middle mouse button | R1 / R2 / L1 |

The mouse is tuned with `PS3_MOUSE_SENS`, `PS3_MOUSE_INVERT_Y=1` and
`PS3_MOUSE_DEADZONE`; `PS3_MOUSE=0` turns it off.

Saves and trophies live in `~/Library/Application Support/infamous2-recomp/gamedata`
(Linux: `~/.local/share/infamous2-recomp`), whatever directory the game is launched from;
the first windowed `./run.sh` copies an older `infamous2-lle/gamedata` there.
`PS3_GAMEDATA_ROOT=<dir>` picks another place.

Options worth knowing:

| Variable | What it does |
| --- | --- |
| `PS3RECOMP_ENG_GPU_SSAO=0F800000` | runs the SSAO job on the GPU too: ~21 → 25–27 fps walking (opt-in while it is validated) |
| `PS3RECOMP_ENG_GPU_LIGHT=` (empty) | puts the light pass back on the SPUs (slower; on the GPU by default) |
| `PS3_DISPLAY_GAMMA=<0.8..1.2>` | overrides the display gamma; by default the game's own brightness setting is applied, as on the console |

`tools/play_dump.sh <name>` opens the game in a window and, each time Return is
pressed in its terminal, saves that frame (render targets, shaders, pass log)
under `out/<name>/`: the quickest way to report a rendering bug.

Known problems:

- In some close-up views at dusk the occlusion-volume passes turned whole
  facades (and Cole) black; `run.sh` skips them for now (only the soft
  contact darkening near props is lost) while their math is checked.
  `PS3RECOMP_ENG_SKIP_FP=` (empty) draws them again.
- Rarely, an SPU running the Edge geometry job faults and the game stops;
  diagnosed down to a register clobbered inside an atomic loop, not fixed yet.
- With less than about 3 GB free on the disk the game shows a "not enough
  space" error at boot, which in the window is just a black screen.

For headless tests: `tools/capture.sh <name> <seconds>` saves a PNG of the
frame every 10 s, `PAD_SCRIPT="95:0x4000"` presses ✕ at 95 s
(`760:0x0400:400` holds L1 for 400 s) and `PAD_STICK="lx,ly,rx,ry,from,to;..."`
holds the sticks over a time range (0–255, 128 = center). That is how the
tutorial was played: ✕ at 95 and 110 s for the menu, the right stick in each
direction for 10 s from 470 s, then the left one, the left stick forward from
640 s ("Head towards the disturbance") and L1 held with R1 taps (`0x0800`) at
the pier gate ("Hold L1 to aim"). `tools/play_prologue.sh [name] [seconds]`
does all of that in one command.

## Fixes made here

In the toolkit (in `infamous2-lle/patches/`, applied by `setup_toolkit.sh`):

- **Lifter:** jump tables lost behind a tail-call epilogue.
- **cellGcm:** the FIFO skipped commands when the game "parked" the GPU before re-linking the block; the menu froze.
- **SPU:** the interpreter did not return to translated code inside WWS job modules; the menu ran at ~0.4 fps.
- **Fragment shaders:** the NV40 pack/unpack opcodes (PK4UB, UP4UB, PKG, UPG and their 16-bit siblings) were missing from the decompiler and got skipped; the game uses them for sRGB↔linear conversion in character materials, and without them Cole's scalp came out green and skin tones were wrong. Implemented following RPCS3.
- **Brightness:** the image came out dark and muddy, with characters nearly black against the light. The game asks the RSX for linear→sRGB conversion when writing the framebuffer and sRGB→linear decoding when reading textures, and the engine ignored both; ported upstream PRs #304 and #305.
- **Given back upstream:** six of these fixes landed in ps3recomp v0.15.0
  (#311–#316), and the ps3recomp README mentions this port in its
  "In the community" section.
- **Upstream:** open PRs applied — #297, #275, #274, #278, #293, #303; #266, #302, #304 and #305 ported by hand (credits in [CREDITS.md](CREDITS.md)).

In the port: an extractor for the 27 job modules embedded in the EBOOT,
registration of the `Wws_Job` policy module, PARAM.SFO at boot, and the run
and capture scripts.

## License

Code in this repository: MIT ([LICENSE](LICENSE)). The toolkit and the fork
are MIT by their authors. inFAMOUS 2 © Sony Computer Entertainment America
LLC, developed by Sucker Punch Productions; this project is not affiliated
with Sony.
