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

exec "$here/build/Infamous2Recomp" "$vfs/PS3_GAME/USRDIR/EBOOT.elf" "$@"
