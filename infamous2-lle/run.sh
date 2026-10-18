#!/bin/sh
# Boot the recompiled inFAMOUS 2 (NPUA80638) against the cloned game data.
#
#   ./run.sh              window on Metal
#   HEADLESS=1 ./run.sh   no window (log-only bring-up runs)
#
# The VFS is ../infamous2/vfs, a copy-on-write clone of RPCS3's
# dev_hdd0/game/NPUA80638, so nothing the port writes touches the emulator's
# install. Firmware files (/dev_flash) are read from RPCS3's own dev_flash.
here=$(cd "$(dirname "$0")" && pwd)
vfs="${INFAMOUS2_VFS:-$here/../infamous2/vfs}"

export PS3_VFS_ROOT="$vfs"
export PS3_HDD0_ROOT="$vfs/dev_hdd0"
export PS3_DEV_FLASH="${PS3_DEV_FLASH:-$HOME/Library/Application Support/rpcs3/dev_flash}"
export PS3_TITLE="inFAMOUS 2"
# Dub, subtitles and menus in Brazilian Portuguese (cache/psarc/brazilian*.psarc).
# The game (2011) treats every system language above 16 as unknown, so the
# console's later "Portuguese (Brazil)" (17) falls back to English; on this
# Americas build plain "Portuguese" (7) is what selects the Brazilian files
# (func_0009D9DC). PS3_LANG=en (or es, fr, ...) to change.
export PS3_LANG="${PS3_LANG:-pt}"
[ -n "$HEADLESS" ] && export PS3RECOMP_METAL_HEADLESS=1
# Saves and trophies live in one fixed place for windowed play, so opening the
# game from another directory no longer starts without a save. The first run
# copies the old cwd-relative infamous2-lle/gamedata there (the original stays
# as a backup). Headless runs keep the toolkit's ./gamedata: the tools run each
# one in its own out/<name>.cwd/ and must never touch the player's save.
# PS3_GAMEDATA_ROOT=<dir> picks another place (=gamedata restores the old way).
if [ -z "$HEADLESS" ] && [ -z "$PS3_GAMEDATA_ROOT" ]; then
    case $(uname) in
        Darwin) data="$HOME/Library/Application Support/infamous2-recomp" ;;
        *)      data="${XDG_DATA_HOME:-$HOME/.local/share}/infamous2-recomp" ;;
    esac
    if [ ! -d "$data/gamedata" ] && [ -d "$here/gamedata/dev_hdd0" ]; then
        mkdir -p "$data" && cp -Rp "$here/gamedata" "$data/gamedata" &&
            echo "run.sh: saves copied from $here/gamedata to $data/gamedata" >&2
    fi
    export PS3_GAMEDATA_ROOT="$data/gamedata"
fi
# Controls (keyboard, mouse buttons, controller; format in the toolkit's
# libs/input/pad_input_map.h, README "Playing"): input.toml in the same folder
# (or input.ini), as the launcher's Controls screen writes it. Windowed runs
# only; PS3_INPUT_MAP=<file> picks another one, PS3_INPUT_MAP= (empty) none.
if [ -z "$HEADLESS" ] && [ -z "${PS3_INPUT_MAP+set}" ]; then
    case $(uname) in
        Darwin) cfg="$HOME/Library/Application Support/infamous2-recomp" ;;
        *)      cfg="${XDG_DATA_HOME:-$HOME/.local/share}/infamous2-recomp" ;;
    esac
    for f in "$cfg/input.toml" "$cfg/input.ini"; do
        [ -f "$f" ] && { export PS3_INPUT_MAP="$f"; break; }
    done
fi
# The RSX must block on its semaphore ACQUIREs: each frame the FIFO acquires
# the label the SPU light task writes when it has turned the normals/depth
# copies in XDR into the light buffers, and only then composites. With the
# toolkit default (acquires fall through) the composite sampled the normals
# copy instead: glossy props blew out white, lighting came out in normal
# colours with per-target MRT masks, and the SPU's 32x40 tiles showed. The
# bound keeps a label nobody services from wedging the FIFO (none did here).
export GCM_SEMA_ACQUIRE="${GCM_SEMA_ACQUIRE:-1}"
export GCM_SEMA_ACQUIRE_MS="${GCM_SEMA_ACQUIRE_MS:-200}"
# A self-JUMP "park" at the end of a command block is patched by the PPU some
# time after it moves put. Once the SPUs became the bottleneck that gap can pass
# the toolkit's 50 ms (the PPU waits on its light tasks in between), the FIFO
# resyncs past the frame's fence and the game hangs at the New Marais load.
# The hardware waits for ever; 1 s keeps a truly abandoned park to a hitch.
export GCM_PARK_GRACE_MS="${GCM_PARK_GRACE_MS:-1000}"
# The SPU light pass runs on the GPU: the light job's formula as a Metal pass,
# the job itself reduced to its completion bookkeeping (gpu_light.c). New Marais
# walk ~12 -> ~15 fps. PS3RECOMP_ENG_GPU_LIGHT= (empty) turns it off.
export PS3RECOMP_ENG_GPU_LIGHT="${PS3RECOMP_ENG_GPU_LIGHT-07000B80:07384B80}"
# The SSAO job runs on the GPU too (gpu_ssao.c, the job's five passes as Metal
# passes; needs the light pass on): walk ~21 -> 26-28 fps. Its opt-in period
# ended once the flip-status races were fixed (6/6 runs clean, 2026-10-17).
# PS3RECOMP_ENG_GPU_SSAO= (empty) puts it back on the SPUs.
export PS3RECOMP_ENG_GPU_SSAO="${PS3RECOMP_ENG_GPU_SSAO-0F800000}"
# The occlusion-volume passes (props/buildings FP 5f740845528b0e62 with the
# 8x8x8 cone LUT, characters FP 7875289ea6b856a6) multiply the ambient term,
# and in some close-up views at dusk they turn whole facades -- and Cole --
# black, which the console does not do. Skipped until their math is found to
# differ; the only loss is the soft contact darkening near props.
# PS3RECOMP_ENG_SKIP_FP= (empty) draws them again.
export PS3RECOMP_ENG_SKIP_FP="${PS3RECOMP_ENG_SKIP_FP-5f740845528b0e62,7875289ea6b856a6}"

# Priority (macos_host.m, the toolkit's host_qos.h): the game keeps macOS from
# App-Napping it and runs its PPU, SPU and RSX threads at USER_INTERACTIVE QoS
# (P-cores first). PS3_QOS=0 leaves the threads at the system default.
# The window (Metal backend; rsx_metal_backend.h has the details):
#   PS3_WINDOW_MODE=windowed|fullscreen|borderless   how it opens (windowed)
#   PS3_WINDOW_SIZE=1920x1080   windowed size in points (1280x720 by default)
#   PS3_MOUSE_CAPTURE=0         the mouse camera without capturing the cursor
#   PS3_DISPLAY=2|<name>        the screen it opens on (1 = the menu bar's)
#   PS3_VSYNC=0                 present without waiting for the refresh
# Picture options (all off by default; the backend reads them once):
#   PS3_ANISO=2|4|8|16          anisotropic filtering (mipmapped textures)
#   PS3_AA=fxaa                 FXAA on the 1280x720 image
#   PS3_SHARPEN=0..1            contrast-adaptive sharpening (AMD CAS)
#   PS3_UPSCALE=metalfx         MetalFX spatial upscaling to the window (macOS 13+)
# Sound and controller (README "Playing"): PS3_AUDIO_VOLUME=0..100,
# PS3_AUDIO_DEVICE=<output name>, PS3_RUMBLE=0, PS3_RUMBLE_STRENGTH=0..200.
# It resizes freely and the image stays 16:9, letterboxed. Cmd+F or
# Option+Return toggles fullscreen, Cmd+Ctrl+F macOS fullscreen; a click in
# the game captures the mouse, Esc or Cmd releases it.
# The Dock icon is the launcher's original placeholder (no Sony art);
# PS3_WINDOW_ICON=game uses the player's own PS3_GAME/ICON0.PNG instead,
# =<file> any image, =none the system's generic icon.
export PS3_WINDOW_ICON="${PS3_WINDOW_ICON-$here/window_icon.png}"

# macOS builds an app bundle (CMakeLists.txt): run the executable inside it,
# directly, so the process has the bundle's identity -- that is what makes
# Game Mode turn on once the window is fullscreen -- and still gets this
# environment and this stdout/stderr. build/Infamous2Recomp is a symlink to it
# (or, in a build from before the bundle, the executable itself).
# INFAMOUS2_EXE=<file> runs another build.
exe="$here/build/Infamous2Recomp.app/Contents/MacOS/Infamous2Recomp"
[ -x "$exe" ] || exe="$here/build/Infamous2Recomp"
exe="${INFAMOUS2_EXE:-$exe}"
exec "$exe" "$vfs/PS3_GAME/USRDIR/EBOOT.elf" "$@"
