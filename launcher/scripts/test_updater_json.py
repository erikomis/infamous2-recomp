#!/usr/bin/env python3
"""Tests for updater_json.py (python3 launcher/scripts/test_updater_json.py).

The manifest's shape is what tauri-plugin-updater reads and what
src-tauri/src/update.rs's `manifest` test expects."""
from __future__ import annotations

import base64
import json
import os
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True  # no __pycache__ next to the scripts
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import updater_json as uj  # noqa: E402

ASSET = 'infamous2-recomp_0.2.7_aarch64.app.tar.gz'


def fake_sig(version: str | None = '0.2.7') -> str:
    """A signature file's text in Tauri's layout (base64 of minisign's four lines)."""
    trusted = 'timestamp:1792689255\tfile:infamous2-recomp.app.tar.gz'
    if version:
        trusted += '\tversion:' + version
    text = ('untrusted comment: signature from tauri secret key\nRUQAAAA=\ntrusted comment: %s\nAAAA\n' % trusted)
    return base64.b64encode(text.encode()).decode()


class Manifest(unittest.TestCase):
    def test_shape(self):
        m = uj.make('v0.2.7', ASSET, fake_sig() + '\n', 'Launcher v0.2.7.\n', '2026-10-23T12:00:00Z')
        self.assertEqual(m, {
            'version': '0.2.7',
            'notes': 'Launcher v0.2.7.',
            'pub_date': '2026-10-23T12:00:00Z',
            'platforms': {'darwin-aarch64': {
                'url': 'https://github.com/erikomis/infamous2-recomp/releases/download/v0.2.7/' + ASSET,
                'signature': fake_sig(),
            }},
        })
        uj.check(m, 'v0.2.7', [ASSET, 'latest.json'])
        # the date defaults to now, RFC 3339 in UTC
        self.assertRegex(uj.make('v0.2.7', ASSET, fake_sig())['pub_date'], r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$')

    def test_signed_version(self):
        self.assertEqual(uj.signed_version(fake_sig('0.2.7')), '0.2.7')
        self.assertIsNone(uj.signed_version(fake_sig(None)))
        self.assertIsNone(uj.signed_version('not base64!'))
        self.assertIsNone(uj.signed_version(base64.b64encode(b'hello').decode()))

    def test_refuses_what_does_not_fit(self):
        good = uj.make('v0.2.7', ASSET, fake_sig(), '', '2026-10-23T12:00:00Z')
        bad = [
            ('v0.2.8', None, good),                                     # another tag
            ('v0.2.7', ['other.dmg'], good),                            # not an asset of the release
            ('v0.2.7', None, uj.make('v0.2.7', ASSET, fake_sig('0.2.6'))),  # signed for another version
            ('v0.2.7', None, uj.make('v0.2.7', ASSET, fake_sig(None))),     # signed without a version
            ('v0.2.7', None, uj.make('v0.2.7', 'x.dmg', fake_sig())),       # not the updater bundle
            ('v0.2.7', None, dict(good, pub_date='yesterday')),
            ('v0.2.7', None, dict(good, platforms=dict(good['platforms'], **{'windows-x86_64': {}}))),
        ]
        for tag, assets, m in bad:
            with self.assertRaises(ValueError, msg=(tag, assets, m)):
                uj.check(m, tag, assets)
        with self.assertRaises(ValueError):
            uj.version_of('nightly')
        with self.assertRaises(ValueError):
            uj.make('v1.0', ASSET, fake_sig())

    def test_command_line(self):
        with tempfile.TemporaryDirectory() as d:
            p = lambda n: os.path.join(d, n)  # noqa: E731
            open(p(ASSET), 'wb').write(b'x')
            open(p(ASSET + '.sig'), 'w').write(fake_sig())
            open(p('notes.md'), 'w').write('draft notes\n')
            self.assertEqual(uj.main(['make', '--tag', 'v0.2.7', '--asset', p(ASSET), '--sig', p(ASSET + '.sig'),
                                      '--notes-file', p('notes.md'), '--out', p('latest.json')]), 0)
            m = json.load(open(p('latest.json')))
            self.assertEqual(m['notes'], 'draft notes')
            # published: the release's own notes and date, checked against its assets
            open(p('body.md'), 'w').write('The published notes.')
            open(p('assets.txt'), 'w').write(ASSET + '\nlatest.json\n')
            self.assertEqual(uj.main(['refresh', '--manifest', p('latest.json'), '--tag', 'v0.2.7',
                                      '--notes-file', p('body.md'), '--pub-date', '2026-10-24T08:00:00Z',
                                      '--assets', p('assets.txt'), '--out', p('out.json')]), 0)
            m = json.load(open(p('out.json')))
            self.assertEqual((m['notes'], m['pub_date']), ('The published notes.', '2026-10-24T08:00:00Z'))
            open(p('assets.txt'), 'w').write('infamous2-recomp_0.2.7_aarch64.dmg\n')
            self.assertEqual(uj.main(['refresh', '--manifest', p('latest.json'), '--tag', 'v0.2.7',
                                      '--assets', p('assets.txt'), '--out', p('out2.json')]), 1)
            self.assertFalse(os.path.exists(p('out2.json')))


if __name__ == '__main__':
    unittest.main()
