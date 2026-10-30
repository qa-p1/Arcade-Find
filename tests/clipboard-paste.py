#!/usr/bin/env python3
"""Exercise the real X11 clipboard, including multi-file copy and text paste."""
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
    assert not os.environ.get('WAYLAND_DISPLAY')
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = Path(__file__).resolve().parents[1] / 'target/e2e/clipboard'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'clipboard-files'
    files.mkdir()
    paths = [files / f'clipboard-file-{name}.txt' for name in ('a', 'b')]
    contents = 'A real clipboard reference.\n'
    for path in paths:
        path.write_text(contents)
    home = Path(os.environ['ARCADE_FIND_HOME'])
    (home / 'config').mkdir(parents=True, exist_ok=True)
    (home / 'config/settings.json').write_text(json.dumps({
        'schema': 1, 'roots': [str(files)], 'welcomeComplete': True, 'showRecentOnOpen': False,
    }))
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        answer = command('--status')
        return json.loads(answer.stdout) if answer.returncode == 0 else {}

    def xdo(*args):
        return subprocess.check_output(['xdotool', *map(str, args)], env=env, text=True).strip()

    def window():
        result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--class', '^dev.arcade.Find$'],
                                env=env, capture_output=True, text=True)
        return result.stdout.splitlines()[-1] if result.returncode == 0 else None

    def clipboard(target):
        result = subprocess.run(['xclip', '-selection', 'clipboard', '-target', target, '-o'],
                                env=env, capture_output=True, text=True, timeout=5)
        return result.stdout if result.returncode == 0 else ''

    owner = None
    with (output / 'clipboard.log').open('w') as log:
        resident = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'clipboard index did not finish')
            assert command('--show', 'clipboard-file').returncode == 0
            until(lambda: status().get('ui', {}).get('rows') == 2, 'clipboard files did not populate')
            identifier = until(window, 'clipboard overlay did not map')
            xdo('windowfocus', '--sync', identifier)
            time.sleep(.45)
            xdo('key', 'shift+Down', 'ctrl+shift+c')
            until(lambda: all(path.as_uri() in clipboard('text/uri-list') for path in paths),
                  'copy did not serve both real file references')
            assert command('--show').returncode == 0
            until(lambda: status().get('ui', {}).get('rows') == 0, 'show did not clear the prior results')
            xdo('windowfocus', '--sync', until(window, 'paste overlay did not map'))
            xdo('key', 'ctrl+v')
            until(lambda: status().get('ui', {}).get('rows') == 2,
                  'file-list paste did not restore the copied files')
            ui = status()['ui']
            assert ui['query'] == '' and ui['selectedCount'] == 2, ui
            assert ui['selectedPath'] in {str(path) for path in paths}, ui
            assert all(path.read_text() == contents for path in paths), 'paste changed a source file'
            # Change clipboard ownership to real UTF-8 text. Keep xclip alive
            # and reap it so this test cannot leave a zombie clipboard owner.
            owner = subprocess.Popen(['xclip', '-quiet', '-selection', 'clipboard'], env=env,
                                     stdin=subprocess.PIPE, stdout=log, stderr=log)
            owner.stdin.write('café'.encode())
            owner.stdin.close()
            until(lambda: clipboard('UTF8_STRING') == 'café', 'text clipboard did not change ownership')
            xdo('key', 'ctrl+k', 'ctrl+v')
            until(lambda: status().get('ui', {}).get('query') == 'café', 'UTF-8 text paste regressed')
        finally:
            command('--quit')
            for process in (resident, owner):
                if process is not None:
                    try:
                        process.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        process.terminate()
                        process.wait(timeout=8)
    print('Real clipboard: two file references copied and pasted with selection; UTF-8 text paste preserved; source files unchanged.')


if __name__ == '__main__':
    main()
