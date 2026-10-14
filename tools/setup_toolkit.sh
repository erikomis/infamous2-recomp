#!/bin/sh
# Prepare the ps3recomp toolkit this port builds against.
#
#   tools/setup_toolkit.sh            # -> ./ps3recomp-fork
#
# The port uses NoahBPeterson's ps3recomp fork (the inFAMOUS 1 bring-up,
# upstream PR #213) at the commit below, plus this repo's fixes and the
# upstream pull requests it depends on, all in one patch.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
dest="${1:-$here/ps3recomp-fork}"
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
else
    echo "patch does not apply to $dest -- is it a clean checkout of $FORK_COMMIT?" >&2
    exit 1
fi
