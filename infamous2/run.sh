#!/bin/sh
# Boot the recompiled inFAMOUS 2 (NPUA80638) against the cloned game data in vfs/.
#
#   ./run.sh              window on Metal
#   HEADLESS=1 ./run.sh   no window (log-only bring-up runs)
#
# vfs/ is a copy-on-write clone of RPCS3's dev_hdd0/game/NPUA80638, so nothing
# the port writes touches the emulator's install. Firmware files (/dev_flash)
# are read straight from RPCS3's own dev_flash.
here=$(cd "$(dirname "$0")" && pwd)

export PS3_VFS_ROOT="$here/vfs"
export PS3_HDD0_ROOT="$here/vfs/dev_hdd0"
export PS3_DEV_FLASH="${PS3_DEV_FLASH:-$HOME/Library/Application Support/rpcs3/dev_flash}"
export PS3_TITLE="inFAMOUS 2"
export INFAMOUS2_LIBSRE_IMAGE="${INFAMOUS2_LIBSRE_IMAGE:-$here/fw/out/libsre_image.bin}"
[ -n "$HEADLESS" ] && export PS3RECOMP_METAL_HEADLESS=1

exec "$here/build/Infamous2Recomp" "$here/vfs/PS3_GAME/USRDIR/EBOOT.elf" "$@"
