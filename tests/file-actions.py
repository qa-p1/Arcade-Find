#!/usr/bin/env python3
"""Use indexed real files through the native overlay; destructive fixtures are private."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def until(fn, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def main():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and root != Path('/')
    assert Path(os.environ['HOME']).resolve().is_relative_to(root)
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = Path(__file__).resolve().parents[1] / 'target/e2e/file-actions'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'action-files'
    files.mkdir()
    source, collision, renamed = [files / name for name in ('rename-source.txt', 'collision.txt', 'renamed.txt')]
    source.write_text('private-content-needle\n')
    collision.write_text('Do not overwrite this file.\n')
    home = Path(os.environ['ARCADE_FIND_HOME'])
    config = home / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({'schema': 1, 'roots': [str(files)], 'welcomeComplete': True,
                                  'showRecentOnOpen': True, 'contentSearch': True}))
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')

    def command(*args):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)
        assert result.returncode == 0, (args, result.stderr)
        return result

    def status():
        result = subprocess.run([str(binary), '--status'], env=env, capture_output=True, text=True, timeout=15)
        return json.loads(result.stdout) if result.returncode == 0 else {}

    def xdo(*args):
        return subprocess.check_output(['xdotool', *map(str, args)], env=env, text=True).strip()

    def show(query, path):
        command('--show', query)
        until(lambda: status().get('ui', {}).get('selectedPath') == str(path), 'expected result was not selected: ' + query)
        identifier = xdo('search', '--onlyvisible', '--class', '^dev.arcade.Find$').splitlines()[-1]
        xdo('windowfocus', '--sync', identifier)
        return identifier

    def mode(value):
        until(lambda: status().get('ui', {}).get('mode') == value, 'overlay mode did not become ' + value)

    with (output / 'actions.log').open('w') as log:
        resident = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'action index did not finish')
            show('/private-content-needle', source)
            assert status()['ui']['rows'] == 1, 'ripgrep did not search real file contents'
            show('rename-source', source)
            xdo('key', 'ctrl+shift+p')
            pins = home / 'data/pins.json'
            until(lambda: pins.exists() and str(source) in json.loads(pins.read_text())['items'], 'pin was not persisted')
            command('--show')
            until(lambda: status().get('ui', {}).get('selectedPath') == str(source), 'empty query did not show the pinned file')
            show('rename-source', source)
            xdo('key', 'F2'); mode('rename')
            xdo('key', 'ctrl+a')
            xdo('type', '--clearmodifiers', 'collision.txt')
            xdo('key', 'Return')
            until(lambda: 'already exists' in (status().get('ui', {}).get('toast') or ''), 'unsafe rename collision was not rejected')
            assert source.read_text() == 'private-content-needle\n'
            assert collision.read_text() == 'Do not overwrite this file.\n'
            xdo('key', 'F2'); mode('rename')
            xdo('key', 'ctrl+a')
            xdo('type', '--clearmodifiers', renamed.name)
            xdo('key', 'Return')
            until(lambda: renamed.exists() and not source.exists(), 'safe rename did not move the real file')
            until(lambda: str(renamed) in json.loads(pins.read_text())['items'], 'rename lost the pin')
            identifier = show('renamed', renamed)
            xdo('key', 'ctrl+i'); mode('details')
            subprocess.run(['import', '-window', identifier, str(output / 'details.png')], check=True)
            xdo('key', 'Escape'); mode('results')
            xdo('key', 'ctrl+Delete'); mode('confirm-trash')
            assert renamed.exists(), 'Trash removed a file before confirmation'
            subprocess.run(['import', '-window', identifier, str(output / 'confirm-trash.png')], check=True)
            xdo('key', 'Escape'); mode('results')
            assert renamed.exists(), 'cancelling Trash removed a file'
            xdo('key', 'ctrl+Delete'); mode('confirm-trash')
            xdo('key', 'Return')
            until(lambda: not renamed.exists(), 'confirmed Trash did not move the file')
            trash = Path(os.environ['XDG_DATA_HOME']) / 'Trash'
            trashed = list((trash / 'files').glob('renamed*'))
            assert len(trashed) == 1 and trashed[0].read_text() == 'private-content-needle\n', 'Trash permanently deleted or changed the file'
            assert list((trash / 'info').glob('renamed*.trashinfo')), 'Trash metadata was not written'
            until(lambda: str(renamed) not in json.loads(pins.read_text())['items'], 'Trash left a stale pin')
            assert collision.read_text() == 'Do not overwrite this file.\n', 'another file was changed'
            command('--quit')
            assert resident.wait(timeout=10) == 0
        finally:
            if resident.poll() is None:
                resident.terminate()
                resident.wait(timeout=10)
    print('Real file actions: ripgrep content search, persistent pin/empty-query result, no-overwrite rename, pin migration, details, cancelled and confirmed Trash passed.')


if __name__ == '__main__':
    main()
