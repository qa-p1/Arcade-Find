#!/usr/bin/env python3
"""Verify all three installers and stage compatible release metadata."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('built', type=Path)
parser.add_argument('output', type=Path)
parser.add_argument('--channel', choices=['stable', 'beta', 'nightly'], required=True)
parser.add_argument('--notes', required=True)
args = parser.parse_args()
version = subprocess.check_output([sys.executable, str(ROOT / 'scripts/check-version.py')], text=True).strip()
args.output.mkdir(parents=True, exist_ok=False)
for path in sorted(args.built.iterdir()):
    if path.is_file() and path.suffix.lower() in ('.appimage', '.exe', '.dmg'):
        shutil.copyfile(path, args.output / path.name)
channel = 'stable' if args.channel == 'stable' else 'nightly'
subprocess.run([sys.executable, str(ROOT / 'tools/arcade-release.py'), '--id', 'arcade.find', '--version', version,
                '--channel', channel, '--notes', args.notes, '--windows-installer', 'inno', str(args.output)], check=True)
manifest = json.loads((args.output / 'arcade-release.json').read_text())
assert manifest['channel'] == channel
assert {(a['os'], a['kind']) for a in manifest['assets']} == {('linux', 'appimage'), ('windows', 'inno'), ('macos', 'dmg')}
assert len(manifest['assets']) == 3
assert next(a for a in manifest['assets'] if a['os'] == 'macos')['arch'] == 'universal'
checksums = dict((name.lstrip('*'), digest) for digest, name in (line.split(None, 1) for line in (args.output / 'SHA256SUMS.txt').read_text().splitlines()))
for asset in manifest['assets']:
    path = args.output / asset['file']
    with path.open('rb') as stream:
        hasher = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            hasher.update(block)
        digest = hasher.hexdigest()
    assert digest == asset['sha256'] == checksums[path.name], path.name
    assert path.stat().st_size == asset['size'], path.name
print(f'Verified all {args.channel} installers and release metadata; nothing published.')
