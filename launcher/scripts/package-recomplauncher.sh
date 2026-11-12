#!/bin/zsh
# The release zip RecompLauncher installs from: "infamous2-recomp.app" and
# recomplauncher.json (the repository's) at the top, named
# infamous2-recomp-<version>-macos-arm64.zip. Local only: it builds and
# writes a file, it never uploads or publishes anything (attaching the zip to
# a GitHub release is a separate, manual step).
#
#   scripts/package-recomplauncher.sh [--app <infamous2-recomp.app>] [--version X.Y.Z] [--out <folder>]
#
#   --app      an .app built already (the release's, e.g. from the local
#              release recipe); without it the port is staged
#              (scripts/stage_port.py) and the release .app is built here
#              (npx tauri build --bundles app, ad-hoc signed)
#   --version  the version in the zip's name and the .app built here
#              (default: launcher/stage/version.conf.json, as the release
#              recipe writes it, else src-tauri/tauri.conf.json)
#   --out      where the zip goes (default: launcher/release/, git-ignored)
#
# Before zipping it checks the .app: its program is where
# recomplauncher.json's start and prepare say, it runs on Apple silicon, the
# bundled port's rebuild.py is in it, and scripts/check_bundle.py finds
# nothing from the game in it.
set -e
setopt pipefail
here=${0:A:h}; launcher=${here:h}; repo=${launcher:h}
app=; out=$launcher/release; version=
while (( $# )); do
    case $1 in
        --app) app=${2:A}; shift 2 ;;
        --out) out=${2:A}; shift 2 ;;
        --version) version=${2#v}; shift 2 ;;
        -h|--help) sed -n '2,23p' $0 | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) print -u2 "unknown argument: $1 (see --help)"; exit 2 ;;
    esac
done
fail() { print -u2 -r -- "ERROR: $*"; exit 1; }

profile=$repo/recomplauncher.json
[ -f "$profile" ] || fail "no $profile"
if [ -z "$version" ]; then
    vconf=$launcher/stage/version.conf.json
    [ -f "$vconf" ] || vconf=$launcher/src-tauri/tauri.conf.json
    version=$(/usr/bin/python3 -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$vconf") ||
        fail "no version in $vconf"
fi
[[ $version =~ '^[0-9]+\.[0-9]+\.[0-9]+$' ]] || fail "$version: not a version (X.Y.Z)"
# the program the profile starts, read with Python's json
program=$(/usr/bin/python3 -I -c '
import json, sys
p = json.load(open(sys.argv[1]))
starts = {p["start"]["program"][0], p["prepare"]["program"][0]}
assert len(starts) == 1, "start and prepare name different programs"
print(starts.pop())' "$profile") || fail "$profile: unreadable, or start and prepare differ"
name=${program%%/*}   # infamous2-recomp.app

if [ -z "$app" ]; then
    print -r -- "==> staging the port (scripts/stage_port.py)"
    /usr/bin/python3 -I "$here/stage_port.py" --out "$launcher/stage/port"
    vfile=$(mktemp "${TMPDIR:-/tmp}/i2-version.XXXXXX")
    printf '{"version": "%s"}\n' "$version" > "$vfile"
    print -r -- "==> building $name $version (release)"
    # this Mac's linker: a stripped release binary comes out with a
    # mis-aligned LINKEDIT
    export CARGO_PROFILE_RELEASE_STRIP=${CARGO_PROFILE_RELEASE_STRIP:-false}
    export APPLE_SIGNING_IDENTITY=${APPLE_SIGNING_IDENTITY:--}
    (cd "$launcher" && npx tauri build --ci --target aarch64-apple-darwin --bundles app \
        --config "$launcher/src-tauri/tauri.bundle.conf.json" --config "$vfile")
    rm -f "$vfile"
    app=$launcher/src-tauri/target/aarch64-apple-darwin/release/bundle/macos/$name
fi
[ -d "$app" ] || fail "$app: no such .app"
[ "${app:t}" = "$name" ] || fail "$app: the .app must be called $name (recomplauncher.json starts $program)"
exe=$app/${program#*/}
[ -x "$exe" ] || fail "$exe: missing or not executable"
/usr/bin/lipo -archs "$exe" | grep -qw arm64 || fail "$exe: not built for Apple silicon (arm64)"
[ -f "$app/Contents/Resources/port/infamous2-lle/tools/rebuild.py" ] || fail "$app has no bundled port (Contents/Resources/port): build it with tauri.bundle.conf.json"
/usr/bin/python3 -I "$here/check_bundle.py" "$app" || fail "$app carries something from the game"

mkdir -p "$out"
zip=$out/infamous2-recomp-$version-macos-arm64.zip
stage=$(mktemp -d "${TMPDIR:-/tmp}/i2-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT
# ditto keeps the bundle as it is (symlinks, permissions, its signature)
/usr/bin/ditto "$app" "$stage/$name"
cp "$profile" "$stage/recomplauncher.json"
rm -f "$zip"
/usr/bin/ditto -c -k --norsrc --noextattr "$stage" "$zip"

print -r -- "==> $zip"
/usr/bin/zipinfo -1 "$zip" | grep -E '^[^/]+/?$|^[^/]+\.app/Contents/MacOS/[^/]+$'
/usr/bin/shasum -a 256 "$zip"
print -r -- "Not uploaded: attach it to a GitHub release (tag v$version) by hand when you publish one."
