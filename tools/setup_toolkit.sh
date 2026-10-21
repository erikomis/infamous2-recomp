#!/bin/sh
# Prepare the ps3recomp toolkit this port builds against.
#
#   tools/setup_toolkit.sh            # -> ./ps3recomp-fork
#   tools/setup_toolkit.sh DIR --managed
#
# --managed: DIR belongs to the installer (rebuild.py's <out>/ps3recomp-fork),
# so when the patch neither applies nor is already applied -- an install made
# with an older version of the patch -- the checkout is reset to the clean
# commit and the current patch applied. Without it a developer's own checkout
# with changes of their own is never touched.
#
# The port uses NoahBPeterson's ps3recomp fork (the inFAMOUS 1 bring-up,
# upstream PR #213) at the commit below, plus this repo's fixes and the
# upstream pull requests it depends on, all in one patch.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
dest="${1:-$here/ps3recomp-fork}"
managed=""; [ "$2" = "--managed" ] && managed=1
FORK_URL=https://github.com/NoahBPeterson/ps3recomp.git
FORK_BRANCH=infamous/rebase-v0.13.0
FORK_COMMIT=770660a
PATCH="$here/infamous2-lle/patches/ps3recomp-fork-770660a-with-upstream-prs.patch"

if [ ! -d "$dest/.git" ]; then
    git clone --branch "$FORK_BRANCH" "$FORK_URL" "$dest"
fi
cd "$dest"
git checkout --quiet "$FORK_COMMIT"
if git apply --check "$PATCH" 2>/dev/null; then
    git apply "$PATCH"
    echo "toolkit ready: $dest ($FORK_COMMIT + local patch)"
elif git apply --reverse --check "$PATCH" 2>/dev/null; then
    echo "toolkit already patched: $dest"
elif [ -n "$managed" ]; then
    echo "toolkit carries an older patch: resetting $dest to $FORK_COMMIT"
    git reset --quiet --hard "$FORK_COMMIT"
    git clean -qfdx
    git apply "$PATCH"
    echo "toolkit ready: $dest ($FORK_COMMIT + local patch)"
else
    echo "patch does not apply to $dest -- is it a clean checkout of $FORK_COMMIT?" >&2
    exit 1
fi
