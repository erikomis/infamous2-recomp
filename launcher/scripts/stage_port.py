#!/usr/bin/env python3
"""Stage the port's own sources and tools for the launcher bundle.

    python3 launcher/scripts/stage_port.py [--out launcher/stage/port]

Copies exactly what infamous2-lle/tools/rebuild.py needs on the player's
machine into <out>, in the repository's layout (the launcher looks for
<resources>/port/infamous2-lle/tools/rebuild.py):

    port/BUILD_INFO.json            commit the sources come from (no git there)
    port/LICENSE, CREDITS.md
    port/tools/setup_toolkit.sh     fetches and patches the ps3recomp toolkit
    port/infamous2-lle/             rebuild.PORT_FILES + README.md
    port/infamous2-lle/tools/       the *.py / *.sh (and rebuild.TOOL_DATA) that
                                    step_port copies; the launcher runs
                                    log_triage.py from here
    port/infamous2-lle/patches/     the one toolkit patch rebuild.py applies

Only files tracked by git are taken, so nothing local (a decrypted EBOOT, a
licence, lifted code, a build) can slip in; check_bundle.py then verifies the
result. CI (.github/workflows/launcher.yml) and local packaging share this.
"""
from __future__ import annotations

import argparse
import re
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
DEFAULT_OUT = os.path.join(REPO, 'launcher', 'stage', 'port')


def load_rebuild():
    sys.dont_write_bytecode = True      # leave no __pycache__ in the sources
    path = os.path.join(REPO, 'infamous2-lle', 'tools', 'rebuild.py')
    spec = importlib.util.spec_from_file_location('rebuild', path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)       # no side effects at import: main() is guarded
    return mod


def git(*args):
    return subprocess.run(['git', '-C', REPO, *args], check=True, capture_output=True,
                          text=True).stdout


def tracked(paths):
    """The subset of repo-relative paths git tracks (in git's spelling)."""
    out = git('ls-files', '-z', '--', *paths)
    return sorted(p for p in out.split('\0') if p)


def wanted_files(rb):
    port_files = list(rb.PORT_FILES) + ['README.md']
    want = ['LICENSE', 'CREDITS.md', 'tools/setup_toolkit.sh']
    want += ['infamous2-lle/' + f for f in port_files]
    # step_port copies every *.py / *.sh of infamous2-lle/tools, and the
    # port's step key hashes them: ship the same set.
    want += [p for p in tracked(['infamous2-lle/tools'])
             if p.count('/') == 2 and rb.is_tool(p.rsplit('/', 1)[1])]
    want.append(os.path.relpath(rb.PATCH, REPO).replace(os.sep, '/'))
    return want


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', default=DEFAULT_OUT, help='staging folder (default: %(default)s); emptied first')
    a = ap.parse_args(argv)

    rb = load_rebuild()
    want = wanted_files(rb)
    # Every plain source the game's add_executable() names must ship, or the
    # player's rebuild stops at cmake (edge_watch.c once went missing that way).
    cm = open(os.path.join(REPO, 'infamous2-lle', 'CMakeLists.txt')).read()
    m = re.search(r'add_executable\(\$\{PROJECT_NAME\}(.*?)\)', cm, re.S)
    srcs = [w for w in (m.group(1).split() if m else []) if not w.startswith('$')]
    unshipped = [s for s in srcs if s not in rb.PORT_FILES]
    if not m or unshipped:
        sys.exit('stage_port: CMakeLists.txt add_executable() sources missing from rebuild.PORT_FILES: %s'
                 % (', '.join(unshipped) or '(add_executable not found)'))
    have = set(tracked(want))
    missing = [p for p in want if p not in have]
    if missing:
        sys.exit('stage_port: not tracked by git (or missing): %s' % ', '.join(missing))

    out = os.path.abspath(a.out)
    if os.path.isdir(out) and os.listdir(out) and not os.path.isfile(os.path.join(out, 'BUILD_INFO.json')):
        sys.exit('stage_port: refusing to empty %s (not a previous staging folder)' % out)
    if os.path.isdir(out):
        shutil.rmtree(out)
    os.makedirs(out)

    total = 0
    for rel in want:
        src = os.path.join(REPO, *rel.split('/'))
        dst = os.path.join(out, *rel.split('/'))
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.copy2(src, dst)
        if rel.endswith('.sh') or rel.endswith('rebuild.py'):
            os.chmod(dst, 0o755)
        total += os.path.getsize(dst)

    try:
        commit = git('rev-parse', '--short', 'HEAD').strip()
        dirty = bool(git('status', '--porcelain', '--untracked-files=no', '--', *want).strip())
    except subprocess.CalledProcessError:
        commit, dirty = '', False
    info = {
        'schema': 1,
        'port_commit': commit + ('-dirty' if dirty else ''),
        'staged_at': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
        'files': len(want),
    }
    with open(os.path.join(out, 'BUILD_INFO.json'), 'w') as f:
        json.dump(info, f, indent=2)
        f.write('\n')
    print('staged %d files (%d KB) from %s into %s' % (len(want), total // 1024, info['port_commit'] or '?', out))
    return 0


if __name__ == '__main__':
    sys.exit(main())
