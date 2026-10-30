#!/usr/bin/env python3
"""Render real install/update/open/uninstall dialogs; never install anything."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    private = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and private != Path('/')
    assert Path(os.environ['ARCADE_FIND_HOME']).resolve().is_relative_to(private)
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = ROOT / 'target/e2e/install'
    output.mkdir(parents=True, exist_ok=True)
    home = Path(os.environ['ARCADE_FIND_HOME'])
    paths = {key: str(home / key) for key in ('config', 'data', 'runtime')}
    paths.update(profile='', isolated=True)
    for key in ('config', 'data', 'runtime'):
        Path(paths[key]).mkdir(parents=True, exist_ok=True)
    environment = {key: str(private / ('install-' + key)) for key in
                   ('home', 'data', 'config', 'applications', 'bin', 'local_app_data', 'registry', 'runtime', 'handoff')}
    environment.update(path=None, windows_environment_key='Software\\ArcadeFindUiFixture')
    runtime = {'executable': str(binary), 'appimage': None, 'appdir': None, 'dev_build': False}
    detection = {'runningFrom': str(binary), 'isAppImage': False, 'receipt': None, 'installedPath': None, 'state': {'kind': 'notInstalled'}}
    options = {'channel': 'stable', 'start_at_login': True, 'remove_download': True, 'install_cli': False}
    states = [('install', 'Install'), ('update', {'Update': {'from': '0.4.3'}}),
              ('open', {'OpenInstalled': {'version': '0.5.0'}}), ('uninstall', None)]
    captures = []
    with (output / 'dialogs.log').open('w') as log:
        for theme in ('light', 'dark'):
            (home / 'config/settings.json').write_text(json.dumps({'schema': 1, 'theme': theme, 'welcomeComplete': True}))
            for scale in ('1', '1.25'):
                for label, state in states:
                    data = {'environment': environment, 'paths': paths}
                    if state is None:
                        prompt = {'Uninstall': dict(data, remove_data=False)}
                    else:
                        prompt = {'Install': dict(data, runtime=runtime, detection=detection, state=state, options=options)}
                    child = subprocess.Popen([str(binary), '--install-dialog'], env=dict(os.environ, ARCADE_FIND_UI_SCALE=scale),
                                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log)
                    try:
                        child.stdin.write(json.dumps(prompt).encode())
                        child.stdin.close()
                        deadline = time.monotonic() + 15
                        identifier = None
                        while time.monotonic() < deadline and child.poll() is None:
                            result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(child.pid)], capture_output=True, text=True)
                            if result.returncode == 0:
                                identifier = result.stdout.splitlines()[-1]
                                break
                            time.sleep(.05)
                        assert identifier, (label, child.poll(), 'dialog did not map')
                        title = subprocess.check_output(['xprop', '-id', identifier, '_NET_WM_NAME'], text=True, encoding='utf-8')
                        assert ('Uninstall' if state is None else 'Install') + ' — Arcade Find' in title, title
                        subprocess.run(['xdotool', 'windowfocus', '--sync', identifier], check=True)
                        subprocess.run(['xdotool', 'windowsize', '--sync', identifier, '720', '520'], check=True)
                        time.sleep(.35)
                        name = f'{label}-{theme}-720x520-{scale}.png'
                        subprocess.run(['import', '-window', identifier, str(output / name)], check=True)
                        captures.append(name)
                        subprocess.run(['xdotool', 'key', 'Escape'], check=True)
                        assert child.wait(timeout=10) == 0
                        assert json.loads(child.stdout.read()) == 'Exit'
                    finally:
                        if child.poll() is None:
                            child.terminate()
                            child.wait(timeout=10)
    assert len(captures) == 16
    assert not Path(environment['registry']).exists(), 'dialog inspection created integration'
    (output / 'captures.json').write_text(json.dumps(captures, indent=2) + '\n')
    print('16 real install/update/open/uninstall views captured; Esc leaves without installation. Visual review is required.')


if __name__ == '__main__':
    main()
