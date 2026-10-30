#!/usr/bin/env python3
"""Fetch a real peer's published AppImage through the release asset REST API."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

repo, destination = sys.argv[1], Path(sys.argv[2]).resolve()
destination.mkdir(parents=True, exist_ok=False)
release = json.loads(subprocess.check_output(['gh', 'api', f'repos/{repo}/releases/latest'], text=True))
assets = {a['name']: a for a in release['assets']}

def download(name):
    asset = assets[name]
    path = destination / name
    with path.open('wb') as stream:
        subprocess.run(['gh', 'api', '-H', 'Accept: application/octet-stream', f'repos/{repo}/releases/assets/{asset["id"]}'], stdout=stream, check=True)
    digest = hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
    assert path.stat().st_size == asset['size'], name
    if asset.get('digest'):
        assert asset['digest'] == 'sha256:' + digest, name
    return path, digest

manifest_path, _ = download('arcade-release.json')
checksums_path, _ = download('SHA256SUMS.txt')
manifest = json.loads(manifest_path.read_text())
checksums = dict((name.lstrip('*'), digest) for digest, name in (line.split(None, 1) for line in checksums_path.read_text().splitlines()))
images = [a for a in manifest['assets'] if a['os'] == 'linux' and a['kind'] == 'appimage']
assert len(images) == 1, images
selected = images[0]
image, digest = download(selected['file'])
assert digest == selected['sha256'] == checksums[image.name], image.name
assert image.stat().st_size == selected['size'], image.name
image.chmod(0o700)
subprocess.run([str(image), '--appimage-extract'], cwd=destination, check=True, stdout=subprocess.DEVNULL)
(destination / 'source.json').write_text(json.dumps({'repo': repo, 'tag': release['tag_name'], 'asset': assets[image.name]['id'], 'sha256': digest}))
print(f'Using verified real {repo} {release["tag_name"]} for peer checks')
