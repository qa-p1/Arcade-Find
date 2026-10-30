#!/usr/bin/env python3
"""Capture a real full-name result tooltip at 125% scale in both themes."""
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
    output = Path(__file__).resolve().parents[1] / 'target/e2e/tooltips'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'tooltip-files'
    files.mkdir()
    name = 'full-tooltip-' + 'a-very-long-filename-' * 10 + '.txt'
    (files / name).write_text('A full filename must remain readable.\n')
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1', WINIT_X11_SCALE_FACTOR='1.25')

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        result = command('--status')
        return json.loads(result.stdout) if result.returncode == 0 else {}

    def xdo(*args):
        return subprocess.check_output(['xdotool', *map(str, args)], env=env, text=True).strip()

    with (output / 'tooltip.log').open('w') as log:
        for theme in ('light', 'dark'):
            config.write_text(json.dumps({'schema': 1, 'roots': [str(files)], 'theme': theme, 'welcomeComplete': True}))
            process = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
            try:
                until(lambda: status().get('engine', {}).get('phase') == 'ready', 'tooltip index did not finish')
                assert command('--show', 'full-tooltip ext:txt').returncode == 0
                until(lambda: status().get('ui', {}).get('rows') == 1, 'tooltip result did not populate')
                identifier = xdo('search', '--onlyvisible', '--class', '^dev.arcade.Find$').splitlines()[-1]
                xdo('windowfocus', '--sync', identifier)
                geometry = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', identifier).splitlines())
                scale = int(geometry['WIDTH']) / 680
                assert abs(scale - 1.25) < .02, ('native scale was not 125%', geometry)
                xdo('mousemove', '--window', identifier, round(120 * scale), round(88 * scale))
                until(lambda: status().get('ui', {}).get('tooltip') == name, 'full filename tooltip did not appear')
                subprocess.run(['import', '-window', identifier, str(output / f'full-name-{theme}-125.png')], check=True)
                xdo('mousemove', 0, 0)
                until(lambda: status().get('ui', {}).get('tooltip') is None, 'tooltip stayed after the pointer left')
                assert command('--quit').returncode == 0
                assert process.wait(timeout=10) == 0
            finally:
                if process.poll() is None:
                    process.terminate(); process.wait(timeout=10)
    print('Real tooltips: full long filename appears after hover in both themes at 125%; leaving the row dismisses it.')


if __name__ == '__main__':
    main()
