#!/usr/bin/env python3
"""Install real 0.4.3, retain its configured data, then install and use this package."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def until(fn, message, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError(message)


def main():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and root != Path('/')
    for key in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_RUNTIME_DIR', 'ARCADE_HOME'):
        assert Path(os.environ[key]).resolve().is_relative_to(root), key
    previous, current = [Path(arg).resolve(strict=True) for arg in sys.argv[1:]]
    env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN='1', ARCADE_NO_INSTALL_PROMPT='1')
    for key in ('ARCADE_FIND_HOME', 'ARCADE_FIND_PROFILE', 'APPIMAGE', 'APPDIR'):
        env.pop(key, None)
    files = root / 'old-indexed-files'
    files.mkdir()
    pinned, recent = files / 'kept-pin.txt', files / 'kept-history.txt'
    pinned.write_text('Pinned original stays.\n')
    recent.write_text('History original stays.\n')
    output = Path(__file__).resolve().parents[1] / 'target/e2e/upgrade'
    output.mkdir(parents=True, exist_ok=True)

    def command(binary, *args):
        result = subprocess.run([str(binary), *args], env=env, text=True, capture_output=True, timeout=40)
        assert result.returncode == 0, (args, result.stdout, result.stderr)
        return json.loads(result.stdout) if result.stdout.strip() else None

    def status(binary):
        result = subprocess.run([str(binary), '--status'], env=env, text=True, capture_output=True, timeout=15)
        return json.loads(result.stdout) if result.returncode == 0 else {}

    with (output / 'upgrade.log').open('w') as log:
        old_receipt = command(previous, '--install', '--silent', '--no-launch', '--keep-download')
        assert old_receipt['version'] == '0.4.3', old_receipt
        installed = Path(old_receipt['path'])
        assert installed.is_relative_to(root)
        legacy_config = Path(env['XDG_CONFIG_HOME']) / 'arcade-find'
        legacy_data = Path(env['XDG_DATA_HOME']) / 'arcade-find'
        legacy_config.mkdir(parents=True, exist_ok=True)
        legacy_data.mkdir(parents=True, exist_ok=True)
        settings = {'schema': 1, 'roots': [str(files)], 'theme': 'light', 'shortcut': 'Ctrl+Alt+J',
                    'showRecentOnOpen': True, 'showHidden': True, 'contentSearch': True}
        (legacy_config / 'settings.json').write_text(json.dumps(settings))
        (legacy_data / 'pins.json').write_text(json.dumps({'items': [str(pinned)]}))
        (legacy_data / 'frecency.json').write_text(json.dumps({
            'schema': 1, 'items': {str(recent): {'count': 3, 'times': [int(time.time())]}},
        }))
        old = subprocess.Popen([str(installed), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status(installed).get('engine', {}).get('phase') == 'ready', 'released 0.4.3 did not index the configured root')
            command(installed, '--show')
            until(lambda: status(installed).get('ui', {}).get('rows', 0) >= 2, '0.4.3 did not load the configured pins and history')
            command(installed, '--quit')
            assert old.wait(timeout=15) == 0
        finally:
            if old.poll() is None:
                old.terminate(); old.wait(timeout=10)
        kept = {name: (directory / name).read_bytes() for directory, name in (
            (legacy_config, 'settings.json'), (legacy_data, 'pins.json'), (legacy_data, 'frecency.json'),
        )}
        new_receipt = command(current, '--install', '--silent', '--no-launch', '--keep-download')
        assert new_receipt['version'] == '0.5.0', new_receipt
        installed = Path(new_receipt['path'])
        assert installed.is_relative_to(root)
        new_config = Path(env['XDG_CONFIG_HOME']) / 'arcade/find'
        new_data = Path(env['XDG_DATA_HOME']) / 'arcade/find'
        for name, directory in [('settings.json', new_config), ('pins.json', new_data), ('frecency.json', new_data)]:
            assert (directory / name).read_bytes() == kept[name], name + ' was not preserved'
        assert not legacy_config.exists() and not legacy_data.exists(), 'verified migration left the legacy folders'
        for key in ('desktopEntry', 'autostart'):
            entry = Path(new_receipt['integration'][key])
            assert entry.is_relative_to(root) and entry.name == 'dev.arcade.Find.desktop', (key, entry)
            assert entry.exists() and 'dev.arcade.Find' in entry.read_text(), entry
            old_entry = Path(old_receipt['integration'][key])
            if old_entry != entry:
                assert not old_entry.exists(), old_entry
        new = subprocess.Popen([str(installed), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status(installed).get('engine', {}).get('phase') == 'ready', 'updated Find did not become ready')
            current_settings = json.loads((new_config / 'settings.json').read_text())
            assert current_settings['shortcut'] == 'Ctrl+Alt+J' and current_settings['theme'] == 'light'
            command(installed, '--show')
            until(lambda: status(installed).get('ui', {}).get('rows', 0) >= 2, 'updated Find did not restore pins/history')
            assert status(installed)['ui']['selectedPath'] == str(pinned)
            command(installed, '--quit')
            assert new.wait(timeout=15) == 0
        finally:
            if new.poll() is None:
                new.terminate(); new.wait(timeout=10)
        assert command(installed, '--uninstall', '--silent')['uninstalled']
        assert all((new_data / name).read_bytes() == kept[name] for name in ('pins.json', 'frecency.json'))
        assert (new_config / 'settings.json').exists() and pinned.exists() and recent.exists()
        assert not installed.exists()
        (output / 'result.json').write_text(json.dumps({'from': '0.4.3', 'to': '0.5.0',
            'shortcut': 'Ctrl+Alt+J', 'settings': True, 'pins': True, 'history': True,
            'desktopEntry': 'dev.arcade.Find.desktop', 'uninstallKeptData': True}, indent=2) + '\n')
    print('Real AppImage upgrade 0.4.3 → 0.5.0: settings, shortcut, pins/history, verified folder migration, desktop/login entries and data-preserving uninstall passed.')


if __name__ == '__main__':
    main()
