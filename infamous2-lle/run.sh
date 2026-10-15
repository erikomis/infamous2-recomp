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
[ -n "$HEADLESS" ] && export PS3RECOMP_METAL_HEADLESS=1
# The RSX must block on its semaphore ACQUIREs: each frame the FIFO acquires
# the label the SPU light task writes when it has turned the normals/depth
# copies in XDR into the light buffers, and only then composites. With the
# toolkit default (acquires fall through) the composite sampled the normals
# copy instead: glossy props blew out white, lighting came out in normal
# colours with per-target MRT masks, and the SPU's 32x40 tiles showed. The
# bound keeps a label nobody services from wedging the FIFO (none did here).
export GCM_SEMA_ACQUIRE="${GCM_SEMA_ACQUIRE:-1}"
export GCM_SEMA_ACQUIRE_MS="${GCM_SEMA_ACQUIRE_MS:-200}"

exec "$here/build/Infamous2Recomp" "$vfs/PS3_GAME/USRDIR/EBOOT.elf" "$@"
