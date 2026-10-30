#!/usr/bin/env python3
"""Verify published Find release bytes and Tools 0.4.1's actual update decision."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import os

ROOT = Path(__file__).resolve().parents[1]
REPO = 'qa-p1/Arcade-Find'


def api(endpoint):
    return json.loads(subprocess.check_output(['gh', 'api', endpoint], text=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tag', required=True)
    parser.add_argument('--sha', required=True)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--beta', action='store_true')
    parser.add_argument('--tools', type=Path)
    args = parser.parse_args()
    assert re.fullmatch('[0-9a-f]{40}', args.sha), 'use the full merge SHA'
    evidence = args.output.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    releases = api(f'repos/{REPO}/releases?per_page=100')
    selected = next(release for release in releases if release['tag_name'] == args.tag)
    assert selected['draft'] == args.beta and selected['prerelease'] == args.beta
    assert selected['target_commitish'] in (args.sha, 'main')
    assert len(selected['assets']) == 5, 'three installers and both metadata files are required'
    (evidence / 'release-api.json').write_text(json.dumps(selected, indent=2) + '\n')
    for asset in selected['assets']:
        name = asset['name']
        assert name == Path(name).name and name not in ('.', '..')
        path = evidence / name
        with path.open('wb') as output:
            subprocess.run(['gh', 'api', '-H', 'Accept: application/octet-stream',
                            f'repos/{REPO}/releases/assets/{asset["id"]}'], stdout=output, check=True)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        assert path.stat().st_size == asset['size'], name
        if asset.get('digest'):
            assert asset['digest'] == 'sha256:' + digest, name
    subprocess.run(['sha256sum', '-c', 'SHA256SUMS.txt'], cwd=evidence, check=True)
    manifest = json.loads((evidence / 'arcade-release.json').read_text())
    assert manifest['id'] == 'arcade.find'
    assert manifest['channel'] == ('nightly' if args.beta else 'stable')
    assert len(manifest['assets']) == 3
    assert {(asset['os'], asset['kind']) for asset in manifest['assets']} == {
        ('linux', 'appimage'), ('windows', 'inno'), ('macos', 'dmg'),
    }
    assert next(asset for asset in manifest['assets'] if asset['os'] == 'macos')['arch'] == 'universal'
    for asset in manifest['assets']:
        path = evidence / asset['file']
        assert path.name == asset['file'] and path.is_file()
        assert hashlib.sha256(path.read_bytes()).hexdigest() == asset['sha256'], path.name
        assert path.stat().st_size == asset['size'], path.name
    catalog = base64.b64decode(api('repos/qa-p1/Arcade-catalog/contents/catalog.json?ref=main')['content'])
    find = next(app for app in json.loads(catalog)['apps'] if app['id'] == 'arcade.find')
    assert find['repo'] == REPO
    (evidence / 'catalog.json').write_bytes(catalog)
    if not args.beta:
        assert args.tag == 'v' + manifest['version']
        assert api(f'repos/{REPO}/releases/latest')['id'] == selected['id']
        tagged = api(f'repos/{REPO}/git/ref/tags/{args.tag}')['object']
        while tagged['type'] == 'tag':
            tagged = api(f'repos/{REPO}/git/tags/{tagged["sha"]}')['object']
        assert tagged['type'] == 'commit' and tagged['sha'] == args.sha
    if args.tools:
        assert not args.beta, 'Tools verifies the published stable update'
        tools = args.tools.resolve()
        assert subprocess.check_output(['git', 'describe', '--tags', '--exact-match'], cwd=tools, text=True).strip() == 'v0.4.1'
        test = tools / 'tests/verify_find_release.rs'
        assert not test.exists(), 'do not overwrite another test'
        shutil.copyfile(ROOT / 'scripts/verify-tools-update.rs', test)
        try:
            subprocess.run(['cargo', 'test', '--locked', '-p', 'arcade-tools-core', '--test',
                            'verify_find_release', '--', '--nocapture'], cwd=tools,
                           env=dict(os.environ, FIND_RELEASE_EVIDENCE=str(evidence)), check=True)
        finally:
            test.unlink()
    (evidence / 'verification.json').write_text(json.dumps({
        'tag': args.tag, 'sha': args.sha, 'release': selected['id'],
        'assets': [asset['name'] for asset in selected['assets']],
        'tools041': bool(args.tools), 'beta': args.beta,
    }, indent=2) + '\n')
    print(f'{args.tag}: every REST asset, checksum, manifest hash/size and catalog entry verified.' +
          (' Tools 0.4.1 verified all four platform selections.' if args.tools else ''))


if __name__ == '__main__':
    main()
