#!/usr/bin/env python3
"""Check actual Wayland mapping, not just Find's internal visibility flag."""
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
    assert os.environ.get('ARCADE_E2E_INNER') == '1'
    assert os.environ.get('WAYLAND_DISPLAY') and os.environ.get('SWAYSOCK')
    binary = Path(sys.argv[1]).resolve(strict=True)
    root = Path(os.environ['XDG_RUNTIME_DIR']) / ('lifecycle-' + str(os.getpid()))
    root.mkdir()
    files = root / 'files'
    files.mkdir()
    (files / 'report-café-日本.txt').write_text('A real indexed file.\n')
    home = root / 'find'
    (home / 'config').mkdir(parents=True)
    (home / 'config/settings.json').write_text(json.dumps({'schema': 1, 'roots': [str(files)]}))
    env = dict(os.environ, ARCADE_FIND_HOME=str(home), ARCADE_HOME=str(root / 'arcade'), ARCADE_FIND_BACKEND='winit', ARCADE_NO_INSTALL_PROMPT='1')

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        result = command('--status')
        return json.loads(result.stdout) if result.returncode == 0 else None

    def mapped():
        tree = json.loads(subprocess.check_output(['swaymsg', '-t', 'get_tree'], text=True))
        def walk(node):
            if node.get('app_id') == 'dev.arcade.Find' and node.get('visible'):
                return True
            return any(walk(child) for child in node.get('nodes', []) + node.get('floating_nodes', []))
        return walk(tree)

    def drag_threads(pid):
        return sum(p.read_text().strip() == 'wl-drag' for p in Path(f'/proc/{pid}/task').glob('*/comm'))

    with (root / 'resident.log').open('w') as log:
        process = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            first = until(status, 'Wayland resident did not start')
            until(lambda: (status() or {}).get('engine', {}).get('phase') == 'ready', 'Index did not finish')
            time.sleep(.4)
            assert not mapped(), 'background mapped a Wayland toplevel'
            for cycle in range(8):
                assert command('--show', 'report').returncode == 0
                until(mapped, f'Show did not map cycle {cycle}')
                until(lambda: (status() or {}).get('ui', {}).get('visible'), 'Show did not update visibility')
                assert drag_threads(first['pid']) == 1, 'Wayland drag helpers leaked across recreated windows'
                # A second launch must reuse the resident and focus its surface.
                assert command().returncode == 0
                assert status()['pid'] == first['pid'], 'second launch started another resident'
                time.sleep(.45)
                if cycle % 2:
                    subprocess.run(['wtype', '-s', '100', '-k', 'Escape'], check=True)
                else:
                    assert command('--hide').returncode == 0
                until(lambda: not mapped(), f'Hide did not unmap cycle {cycle}')
                assert not status()['ui']['visible'], 'unmapped window still reports visible'
            assert command('--quit').returncode == 0
            assert process.wait(timeout=10) == 0
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=10)
    print('Wayland: background stays unmapped; 8 Show/Hide/Esc cycles; singleton; one drag worker; clean shutdown.')


if __name__ == '__main__':
    main()
