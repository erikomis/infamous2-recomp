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
| Performance | 🚶 26–28 fps walking in New Marais (the game targets 30) |
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

The mouse is tuned with `PS3_MOUSE_SENS`, `PS3_MOUSE_INVERT_Y=1`,
`PS3_MOUSE_INVERT_X=1` and `PS3_MOUSE_DEADZONE`; `PS3_MOUSE=0` turns it off. A
click in the game captures it (the cursor hides and the camera follows the
mouse); Esc or Cmd releases it, and so does switching to another app.
`PS3_MOUSE_CAPTURE=0` goes back to the uncaptured camera.

Every binding above can be changed in a controls file: the launcher's
**Controls** screen writes it, or write it by hand at
`~/Library/Application Support/infamous2-recomp/input.toml` (`input.ini` also
works; `PS3_INPUT_MAP=<file>` picks another one, `PS3_INPUT_MAP=` none).
Anything the file leaves out keeps the layout above (the mouse's side
buttons press L1, like the middle one), except that an input the file binds
leaves the controls it does not mention: `cross = "Q"` takes Q off ○ (list it
under both to keep both). Names are not case-sensitive, and an unknown one
is reported once in the log and ignored.

```toml
[keyboard]                  # PS3 control = keys
cross    = "Space"
circle   = "F"
start    = "Return, KeypadEnter"   # several keys
l3       = "Shift"
r3       = ""                      # unbound

[mouse_buttons]             # PS3 control = mouse buttons
r1   = "Left"
up   = "WheelUp"            # each wheel notch is one press
down = "WheelDown"
l1   = "Button4"

[mouse]                     # the environment variables above win over these
camera      = true
sensitivity = 1.5
invert_x    = false
invert_y    = false
deadzone    = 30
capture     = true

[controller]                # PS3 control = SDL controller inputs
cross  = "a"                # Cross on a DualSense, A on an Xbox pad
circle = "b"
select = "touchpad"
```

- PS3 controls: `cross circle square triangle l1 r1 l2 r2 l3 r3 start select`,
  `up down left right` (D-pad), `ls_up ls_down ls_left ls_right` and
  `rs_up rs_down rs_left rs_right` (sticks).
- Keys, by their place on a US Mac keyboard: `A`-`Z`, `0`-`9`, `F1`-`F20`,
  `Space Return Tab Escape Backspace ForwardDelete Up Down Left Right Home End
  PageUp PageDown`, `Minus Equal LeftBracket RightBracket Backslash Semicolon
  Quote Comma Period Slash Grave Section`, `Keypad0`-`Keypad9 KeypadEnter
  KeypadPlus KeypadMinus KeypadMultiply KeypadDivide KeypadDecimal
  KeypadEquals KeypadClear`, `Shift RightShift Control RightControl Option
  RightOption`, or a raw key code (`0x00`-`0x7F`). Cmd cannot be bound, and
  Esc releases the captured mouse before it reaches the game.
- Mouse buttons: `Left Right Middle Button4`-`Button8 WheelUp WheelDown`.
- Controller (SDL names): `a b x y back guide start leftstick rightstick
  leftshoulder rightshoulder lefttrigger righttrigger dpup dpdown dpleft
  dpright touchpad misc1 paddle1`-`paddle4`; PlayStation and Xbox names work
  too (`cross`, `options`, `share`, `l1`, `r2`, `lb`, `rt`, ...). The physical
  sticks always drive the PS3 sticks.

The window resizes freely and the image keeps its 16:9 shape, with black bars
when the window has another one.

| Shortcut | What it does |
| --- | --- |
| Cmd+F or Option+Return | fullscreen on/off (borderless with `PS3_WINDOW_MODE=borderless`, macOS fullscreen otherwise) |
| Cmd+Ctrl+F | macOS fullscreen on/off |
| Esc / Cmd | release the mouse |

| Variable | What it does |
| --- | --- |
| `PS3_WINDOW_MODE=windowed\|fullscreen\|borderless` | how the game opens: a window (default), macOS fullscreen (a Space of its own) or borderless fullscreen (covers the screen in place, menu bar and Dock hidden, instant app switching) |
| `PS3_WINDOW_SIZE=<w>x<h>` | the window's size in points, e.g. `1920x1080` (default 1280x720, shrunk to fit the screen) |
| `PS3_WINDOW_ICON=game\|<file>\|none` | the Dock icon: by default the launcher's original placeholder (`infamous2-lle/window_icon.png`, no Sony art); `game` uses your copy's own `PS3_GAME/ICON0.PNG` |

Saves and trophies live in `~/Library/Application Support/infamous2-recomp/gamedata`
(Linux: `~/.local/share/infamous2-recomp`), whatever directory the game is launched from;
the first windowed `./run.sh` copies an older `infamous2-lle/gamedata` there.
`PS3_GAMEDATA_ROOT=<dir>` picks another place.

Options worth knowing:

| Variable | What it does |
| --- | --- |
| `PS3RECOMP_ENG_GPU_SSAO=` (empty) | puts the SSAO job back on the SPUs (on the GPU by default since 2026-10-17: ~21 → 26–28 fps walking) |
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
