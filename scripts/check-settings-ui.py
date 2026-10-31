#!/usr/bin/env python3
"""Capture every real settings page in Link's isolated D-Bus/Xvfb runner."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
PAGES = ('general', 'shortcuts', 'locations', 'index', 'connected', 'about')


def until(fn, description, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        answer = fn()
        if answer:
            return answer
        time.sleep(.05)
    raise AssertionError(description)


def main():
    private = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and private != Path('/')
    for name in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'ARCADE_FIND_HOME'):
        assert Path(os.environ[name]).resolve().is_relative_to(private), name
    assert os.environ.get('DISPLAY') and not os.environ.get('WAYLAND_DISPLAY')
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = ROOT / 'target/e2e/settings'
    output.mkdir(parents=True, exist_ok=True)
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    long_path = private / ('a-long-folder-name-' * 7)
    long_path.mkdir()
    captures = []

    def xdo(*args):
        return subprocess.check_output(['xdotool', *map(str, args)], text=True).strip()

    def window(pid):
        # xdotool's legacy WM_NAME search depends on the runner's locale.
        # Bind discovery to this process and check the actual UTF-8 property.
        result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], text=True, capture_output=True)
        return result.stdout.splitlines()[-1] if result.returncode == 0 else None

    with (output / 'settings.log').open('w') as log:
        for theme in ('light', 'dark'):
            for scale in ('1', '1.25'):
                config.write_text(json.dumps({'schema': 1, 'welcomeComplete': True, 'theme': theme, 'roots': [str(long_path)], 'shortcut': 'Ctrl+Alt+F'}))
                process = subprocess.Popen([str(binary), '--settings-window'], env=dict(os.environ, ARCADE_FIND_UI_SCALE=scale, ARCADE_FIND_DEBUG='1'), stdout=log, stderr=log)
                try:
                    try:
                        identifier = until(lambda: window(process.pid), 'Settings did not map')
                    except AssertionError:
                        log.write(f'\nSettings exit status: {process.poll()}\n')
                        log.flush()
                        for command in (['xdotool', 'search', '--name', '.'], ['ps', '-p', str(process.pid), '-o', 'pid,stat,wchan:32,args']):
                            subprocess.run(command, stdout=log, stderr=log)
                        subprocess.run(['import', '-window', 'root', str(output / 'startup-failure.png')], check=True)
                        raise
                    title = subprocess.check_output(['xprop', '-id', identifier, '_NET_WM_NAME'], text=True, encoding='utf-8')
                    assert '"Settings — Arcade Find"' in title, title
                    xdo('windowfocus', '--sync', identifier)
                    for width, height in ((760, 560), (720, 520)):
                        xdo('windowsize', '--sync', identifier, width, height)
                        time.sleep(.4)
                        zoom = float(scale)
                        for index, page in enumerate(PAGES):
                            xdo('mousemove', '--window', identifier, round(32 * zoom), round((85 + 36 * index) * zoom))
                            xdo('click', 1)
                            time.sleep(.4)
                            geometry = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', identifier).splitlines())
                            assert (int(geometry['WIDTH']), int(geometry['HEIGHT'])) == (width, height), geometry
                            name = f'{page}-{theme}-{width}x{height}-{scale}'
                            xdo('mousemove', '--window', identifier, width - 80, height - 90)
                            xdo('click', '--repeat', 45, '--delay', 2, 4)
                            time.sleep(.2)
                            headers = []
                            for position in ('top', 'bottom'):
                                if position == 'bottom':
                                    xdo('click', '--repeat', 65, '--delay', 2, 5)
                                    time.sleep(.3)
                                full = output / 'root.png'
                                subprocess.run(['import', '-window', 'root', str(full)], check=True)
                                crop = f"{width}x{height}+{geometry['X']}+{geometry['Y']}"
                                destination = output / (name + '-' + position + '.png')
                                subprocess.run(['convert', str(full), '-crop', crop, '+repage', str(destination)], check=True)
                                full.unlink()
                                headers.append(subprocess.check_output(['convert', str(destination), '-crop', f'{width}x{round(95 * zoom)}+0+0', '+repage', 'rgba:-']))
                                captures.append(name + '-' + position)
                            assert headers[0] == headers[1], (name, 'page heading moved while its body scrolled')
                    xdo('key', 'ctrl+w')
                    assert process.wait(timeout=10) == 0
                finally:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=10)
    assert len(captures) == 96
    (output / 'captures.json').write_text(json.dumps(captures, indent=2) + '\n')
    print('Captured 96 real settings views (page tops and bottoms). These images require visual review; capture success is not layout approval.')


if __name__ == '__main__':
    main()
