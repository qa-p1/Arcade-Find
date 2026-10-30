#!/usr/bin/env python3
"""Real eframe/softbuffer UI checks, only inside Link's private D-Bus/Xvfb runner."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/e2e/native'


def require_isolation():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    if os.environ.get('ARCADE_E2E_INNER') != '1' or root == Path('/'):
        raise SystemExit('Run through Link tools/e2e.py run --')
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME', 'ARCADE_HOME'):
        assert Path(os.environ[key]).resolve().is_relative_to(root), key
    assert os.environ.get('DISPLAY') and not os.environ.get('WAYLAND_DISPLAY')
    return root


def until(fn, label, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value: return value
        time.sleep(.05)
    raise AssertionError(label)


def main():
    root = require_isolation()
    binary = Path(sys.argv[1]).resolve(strict=True)
    OUT.mkdir(parents=True, exist_ok=True)
    files = root / 'ui-files'
    files.mkdir()
    for name in ('report.pdf', 'café-report.txt', '日本-report.txt', 'a-' + 'very-long-name-' * 12 + 'report.txt'):
        (files / name).write_text('A real search fixture with café and 日本.\n')
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')
    config = Path(env['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    settings = {'schema': 1, 'welcomeComplete': True, 'roots': [str(files)], 'theme': 'light', 'showRecentOnOpen': True, 'shortcut': 'Ctrl+Alt+F'}

    def save():
        temporary = config.with_suffix('.tmp')
        temporary.write_text(json.dumps(settings))
        temporary.replace(config)

    save()
    processes = []
    report = []
    memory = []
    with (OUT / 'native.log').open('w') as log:
        resident = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        processes.append(resident)

        def command(*args, check=True):
            result = subprocess.run([str(binary), *args], env=env, text=True, capture_output=True, timeout=15)
            if check: assert result.returncode == 0, (args, result.stderr)
            return result

        def status():
            answer = command('--status', check=False)
            return json.loads(answer.stdout) if answer.returncode == 0 else {}

        def xdo(*args):
            return subprocess.check_output(['xdotool', *map(str, args)], env=env, text=True).strip()

        def window(pid, title):
            result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(pid)], env=env, text=True, capture_output=True)
            for identifier in result.stdout.splitlines() if result.returncode == 0 else []:
                prop = subprocess.check_output(['xprop', '-id', identifier, '_NET_WM_NAME'], text=True, encoding='utf-8')
                if '"' + title + '"' in prop:
                    return identifier
            return None

        def capture(identifier, name):
            geometry = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', identifier).splitlines())
            full = OUT / (name + '-root.png')
            subprocess.run(['import', '-window', 'root', str(full)], env=env, check=True)
            crop = f'{geometry["WIDTH"]}x{geometry["HEIGHT"]}+{geometry["X"]}+{geometry["Y"]}'
            subprocess.run(['convert', str(full), '-crop', crop, '+repage', str(OUT / (name + '.png'))], check=True)
            full.unlink()
            report.append({'capture': name, 'width': int(geometry['WIDTH']), 'height': int(geometry['HEIGHT'])})

        try:
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'index did not become ready')
            pid = status()['pid']
            for theme in ('light', 'dark'):
                settings['theme'] = theme; save()
                command('--reload')
                command('--show', 'report')
                until(lambda: status().get('ui', {}).get('rows', 0) >= 4, 'real search fixture did not populate')
                overlay = until(lambda: window(pid, 'Search — Arcade Find'), 'overlay did not map')
                xdo('windowfocus', '--sync', overlay)
                time.sleep(.45)
                xdo('mousemove', 0, 0)
                capture(overlay, 'overlay-' + theme)
                command('--hide')
                if not memory:
                    memory.append(int(next(line.split()[1] for line in Path(f'/proc/{pid}/status').read_text().splitlines() if line.startswith('VmRSS:'))))
                for scale in ('1', '1.25'):
                    helper = subprocess.Popen([str(binary), '--settings-window'], env=dict(env, ARCADE_FIND_UI_SCALE=scale), stdout=log, stderr=log)
                    processes.append(helper)
                    settings_window = until(lambda: window(helper.pid, 'Settings — Arcade Find'), 'Settings did not map')
                    xdo('windowfocus', '--sync', settings_window)
                    # An additional Settings request must focus the existing helper.
                    second = subprocess.run([str(binary), '--settings-window'], env=env, stdout=log, stderr=log, timeout=10)
                    assert second.returncode == 0
                    assert window(helper.pid, 'Settings — Arcade Find') == settings_window
                    for width, height in ((760, 560), (720, 520)):
                        xdo('windowsize', '--sync', settings_window, width, height)
                        time.sleep(.3)
                        zoom = float(scale)
                        xdo('mousemove', '--window', settings_window, round(32 * zoom), round(91 * zoom))
                        xdo('click', 1)
                        for index, page in enumerate(('general', 'shortcuts', 'locations', 'index', 'connected', 'about')):
                            xdo('mousemove', '--window', settings_window, round(32 * zoom), round((85 + 36 * index) * zoom))
                            xdo('click', 1)
                            time.sleep(.25)
                            capture(settings_window, f'settings-{page}-{theme}-{width}x{height}-{scale}')
                    xdo('key', 'ctrl+w')
                    assert helper.wait(timeout=10) == 0
                    assert status()['pid'] == pid, 'closing Settings replaced or quit the resident'
            memory.append(int(next(line.split()[1] for line in Path(f'/proc/{pid}/status').read_text().splitlines() if line.startswith('VmRSS:'))))
            assert memory[-1] <= memory[0] + 8192, ('resident memory grew across Settings helpers', memory)
            # Live theme changes reach an already-open native settings process.
            helper = subprocess.Popen([str(binary), '--settings-window'], env=env, stdout=log, stderr=log)
            processes.append(helper)
            identifier = until(lambda: window(helper.pid, 'Settings — Arcade Find'), 'live Settings did not map')
            xdo('windowfocus', '--sync', identifier)
            for theme in ('light', 'dark', 'light'):
                settings['theme'] = theme; save()
                time.sleep(.6)
                capture(identifier, 'live-' + theme)
            light = subprocess.check_output(['convert', str(OUT / 'live-light.png'), '-format', '%[pixel:p{400,20}]', 'info:'], text=True)
            dark = subprocess.check_output(['convert', str(OUT / 'live-dark.png'), '-format', '%[pixel:p{400,20}]', 'info:'], text=True)
            assert light != dark, (light, dark, 'theme did not change on the live window')
            xdo('key', 'ctrl+w')
            assert helper.wait(timeout=10) == 0
            # Native geometry survives closing the helper, independent of the
            # resident. This fails when every launch uses the default size.
            helper = subprocess.Popen([str(binary), '--settings-window'], env=env, stdout=log, stderr=log)
            processes.append(helper)
            identifier = until(lambda: window(helper.pid, 'Settings — Arcade Find'), 'geometry Settings did not map')
            xdo('windowfocus', '--sync', identifier)
            xdo('windowsize', '--sync', identifier, 900, 650)
            xdo('windowmove', '--sync', identifier, 100, 80)
            time.sleep(.4)
            xdo('key', 'ctrl+w')
            assert helper.wait(timeout=10) == 0
            helper = subprocess.Popen([str(binary), '--settings-window'], env=env, stdout=log, stderr=log)
            processes.append(helper)
            identifier = until(lambda: window(helper.pid, 'Settings — Arcade Find'), 'geometry restore did not map')
            geometry = dict(line.split('=', 1) for line in xdo('getwindowgeometry', '--shell', identifier).splitlines())
            assert tuple(int(geometry[key]) for key in ('X', 'Y', 'WIDTH', 'HEIGHT')) == (100, 80, 900, 650), geometry
            xdo('windowfocus', '--sync', identifier)
            xdo('key', 'ctrl+w')
            assert helper.wait(timeout=10) == 0
            # Focus loss caused by mapping is ignored, later focus loss hides.
            focus_title = 'Find focus probe ' + str(os.getpid())
            other = subprocess.Popen(['xmessage', '-title', focus_title, '-buttons', 'OK:0', '-timeout', '20', 'Find focus check'], env=env, stdout=log, stderr=log)
            processes.append(other)
            # Xt xmessage does not publish _NET_WM_PID; its unique ASCII
            # title belongs to the process just created in this private server.
            def other_window():
                result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^' + focus_title + '$'], text=True, capture_output=True)
                return result.stdout.splitlines()[-1] if result.returncode == 0 else None
            focus_target = until(other_window, 'focus target did not map')
            command('--show', 'report')
            overlay = until(lambda: window(pid, 'Search — Arcade Find'), 'focus probe overlay did not map')
            xdo('windowfocus', '--sync', overlay)
            xdo('windowfocus', '--sync', focus_target)
            time.sleep(.15)
            assert status()['ui']['visible'], 'mapping focus loss closed the overlay'
            xdo('windowfocus', '--sync', overlay)
            time.sleep(.45)
            xdo('windowfocus', '--sync', focus_target)
            until(lambda: status().get('ui', {}).get('visible') is False, 'later focus loss did not hide')
            other.terminate(); other.wait(timeout=5)
            # A second launch must show the original process, not create another.
            command('--show', 'café')
            until(lambda: status().get('ui', {}).get('query') == 'café', 'Unicode query was lost')
            assert status()['pid'] == pid
            xdo('windowfocus', '--sync', until(lambda: window(pid, 'Search — Arcade Find'), 'second launch did not show'))
            xdo('key', 'Escape')
            until(lambda: status().get('ui', {}).get('visible') is False, 'Esc did not close the overlay')
            assert status()['pid'] == pid
            print(f'Native UI: {len(report)} real screenshots; settings singleton, close/reopen, Unicode and second-launch lifecycle passed.')
            (OUT / 'results.json').write_text(json.dumps({'captures': report, 'residentPid': pid, 'residentRssKiB': memory}, indent=2))
        finally:
            command('--quit', check=False)
            for process in processes:
                if process.poll() is None:
                    try: process.wait(timeout=5)
                    except subprocess.TimeoutExpired: process.terminate(); process.wait(timeout=5)


if __name__ == '__main__': main()
