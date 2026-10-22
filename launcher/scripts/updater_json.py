#!/usr/bin/env python3
"""The app updater's manifest (latest.json), as tauri-plugin-updater reads it.

    # the release job (a v* tag): from the signed macOS bundle
    python3 launcher/scripts/updater_json.py make --tag v0.2.7 \\
        --asset dist/infamous2-recomp_0.2.7_aarch64.app.tar.gz \\
        --sig dist/infamous2-recomp_0.2.7_aarch64.app.tar.gz.sig \\
        --notes-file notes.md --out latest.json

    # updater.yml (the release was published): the notes and date of the
    # published release, checked against its assets
    python3 launcher/scripts/updater_json.py refresh --manifest latest.json --tag v0.2.7 \\
        --notes-file body.md --pub-date 2026-10-23T12:00:00Z --assets assets.txt --out latest.json

The manifest:

    {"version": "0.2.7", "notes": "...", "pub_date": "2026-10-23T12:00:00Z",
     "platforms": {"darwin-aarch64": {"url": "https://github.com/<repo>/releases/download/v0.2.7/<asset>",
                                      "signature": "<the .sig file's text>"}}}

Only darwin-aarch64: the game builds and runs on Apple Silicon Macs only, and
the other systems' installers are not published. `check` refuses a manifest
whose version is not the tag's, whose download is not that release's asset,
or whose signature does not carry that version (the app requires it:
`requireSignedVersion`).
"""
from __future__ import annotations

import argparse
import base64
import binascii
import datetime
import json
import os
import re
import sys

REPO = 'erikomis/infamous2-recomp'
PLATFORM = 'darwin-aarch64'
VERSION = re.compile(r'^\d+\.\d+\.\d+$')


def version_of(tag: str) -> str:
    v = tag[1:] if tag.startswith('v') else tag
    if not VERSION.match(v):
        raise ValueError('tag %r is not vX.Y.Z' % tag)
    return v


def download_url(repo: str, tag: str, asset: str) -> str:
    return 'https://github.com/%s/releases/download/%s/%s' % (repo, tag, asset)


def now_rfc3339() -> str:
    return datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace('+00:00', 'Z')


def signed_version(sig: str) -> str | None:
    """The version in a Tauri signature's trusted comment (`version:X.Y.Z`)."""
    try:
        text = base64.b64decode(sig.strip(), validate=True).decode('utf-8')
    except (binascii.Error, UnicodeDecodeError):
        return None
    lines = text.splitlines()
    if len(lines) < 4 or not lines[0].startswith('untrusted comment:'):
        return None
    trusted = lines[2]
    if not trusted.startswith('trusted comment:'):
        return None
    m = re.search(r'(?:^|\t| )version:(\S+)', trusted[len('trusted comment:'):])
    return m.group(1) if m else None


def make(tag: str, asset_name: str, signature: str, notes: str = '', pub_date: str | None = None,
         repo: str = REPO) -> dict:
    v = version_of(tag)
    return {
        'version': v,
        'notes': notes.strip(),
        'pub_date': pub_date or now_rfc3339(),
        'platforms': {PLATFORM: {'url': download_url(repo, tag, asset_name), 'signature': signature.strip()}},
    }


def check(m: dict, tag: str, assets: list[str] | None = None, repo: str = REPO) -> None:
    """Raise ValueError when the manifest does not fit the release `tag`."""
    v = version_of(tag)
    if m.get('version') != v:
        raise ValueError('manifest version %r, tag %s' % (m.get('version'), tag))
    plats = m.get('platforms') or {}
    if set(plats) != {PLATFORM}:
        raise ValueError('platforms %s: only %s is published' % (sorted(plats), PLATFORM))
    p = plats[PLATFORM]
    prefix = download_url(repo, tag, '')
    url = p.get('url', '')
    if not url.startswith(prefix) or '/' in url[len(prefix):] or not url.endswith('.app.tar.gz'):
        raise ValueError('url %r is not a .app.tar.gz of release %s' % (url, tag))
    if assets is not None and url[len(prefix):] not in assets:
        raise ValueError('%s is not an asset of release %s' % (url[len(prefix):], tag))
    sv = signed_version(p.get('signature', ''))
    if sv is None:
        raise ValueError('the signature is not a Tauri signature with a version')
    if sv.lstrip('v') != v:
        raise ValueError('the signature is for version %s, the manifest says %s' % (sv, v))
    pd = m.get('pub_date')
    try:
        datetime.datetime.fromisoformat(str(pd).replace('Z', '+00:00'))
    except ValueError:
        raise ValueError('pub_date %r is not RFC 3339' % pd)


def read(path: str) -> str:
    with open(path, encoding='utf-8') as f:
        return f.read()


def write(path: str, m: dict) -> None:
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(m, f, indent=2, ensure_ascii=False)
        f.write('\n')


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest='cmd', required=True)
    mk = sub.add_parser('make')
    mk.add_argument('--tag', required=True)
    mk.add_argument('--asset', required=True, help='the .app.tar.gz (its file name is the release asset name)')
    mk.add_argument('--sig', required=True, help='its .sig')
    mk.add_argument('--notes-file')
    mk.add_argument('--pub-date')
    mk.add_argument('--repo', default=REPO)
    mk.add_argument('--out', required=True)
    rf = sub.add_parser('refresh')
    rf.add_argument('--manifest', required=True)
    rf.add_argument('--tag', required=True)
    rf.add_argument('--notes-file')
    rf.add_argument('--pub-date')
    rf.add_argument('--assets', help='a file with the release asset names, one per line')
    rf.add_argument('--repo', default=REPO)
    rf.add_argument('--out', required=True)
    a = ap.parse_args(argv)
    try:
        if a.cmd == 'make':
            m = make(a.tag, os.path.basename(a.asset), read(a.sig), read(a.notes_file) if a.notes_file else '',
                     a.pub_date, a.repo)
            check(m, a.tag, repo=a.repo)
        else:
            m = json.loads(read(a.manifest))
            if a.notes_file:
                m['notes'] = read(a.notes_file).strip()
            if a.pub_date:
                m['pub_date'] = a.pub_date
            assets = [l.strip() for l in read(a.assets).splitlines() if l.strip()] if a.assets else None
            check(m, a.tag, assets, repo=a.repo)
    except (OSError, ValueError, json.JSONDecodeError) as e:
        print('updater_json: %s' % e, file=sys.stderr)
        return 1
    write(a.out, m)
    print('%s: %s %s -> %s' % (a.out, m['version'], m['pub_date'], m['platforms'][PLATFORM]['url']))
    return 0


if __name__ == '__main__':
    sys.exit(main())
