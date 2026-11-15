#!/usr/bin/env python3
"""Install a real released Find, retain its configured data, then install and use this package.

  check-upgrade.py PREVIOUS.AppImage CURRENT.AppImage

CI runs it from 0.4.3 (its folders, `arcade-find`, are migrated to `arcade/find` on install); the release gates run it
from the previous stable release as well, whose data already lives in `arcade/find`."""
import json
import os
import hashlib
import shutil
from pathlib import Path
import subprocess
import sys
import time

VERSION = subprocess.run([sys.executable, str(Path(__file__).with_name('check-version.py'))], check=True, capture_output=True, text=True).stdout.strip()


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
    sources = [Path(arg).resolve(strict=True) for arg in sys.argv[1:]]
    # Use real downloaded packages outside a source checkout. Link deliberately
    # refuses to install a file below .git, even when it is a built AppImage.
    downloads = root / 'Downloads'
    downloads.mkdir()
    packages = []
    for name, source in zip(('previous.AppImage', 'current.AppImage'), sources):
        target = downloads / name
        shutil.copy2(source, target)
        assert hashlib.sha256(source.read_bytes()).digest() == hashlib.sha256(target.read_bytes()).digest()
        packages.append(target)
    previous, current = packages
    env = dict(os.environ, APPIMAGE_EXTRACT_AND_RUN='1', ARCADE_NO_INSTALL_PROMPT='1')
    for key in ('ARCADE_FIND_HOME', 'ARCADE_FIND_PROFILE', 'APPIMAGE', 'APPDIR', 'ARCADE_DEV_BUILD'):
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
        old_version = old_receipt['version']
        # The same version is allowed: before the release bump CI's build still carries the previous release's number.
        assert old_version.split('-')[0].count('.') == 2, old_receipt
        installed = Path(old_receipt['path'])
        assert installed.is_relative_to(root)
        # 0.4.x kept its files in `arcade-find`; 0.5.0 moved them to `arcade/find` (migrating on install).
        legacy = old_version.startswith('0.4.')
        folder = 'arcade-find' if legacy else 'arcade/find'
        old_config = Path(env['XDG_CONFIG_HOME']) / folder
        old_data = Path(env['XDG_DATA_HOME']) / folder
        old_config.mkdir(parents=True, exist_ok=True)
        old_data.mkdir(parents=True, exist_ok=True)
        settings = {'schema': 1, 'roots': [str(files)], 'theme': 'light', 'shortcut': 'Ctrl+Alt+J',
                    'showRecentOnOpen': True, 'showHidden': True, 'contentSearch': True}
        (old_config / 'settings.json').write_text(json.dumps(settings))
        (old_data / 'pins.json').write_text(json.dumps({'items': [str(pinned)]}))
        (old_data / 'frecency.json').write_text(json.dumps({
            'schema': 1, 'items': {str(recent): {'count': 3, 'times': [int(time.time())]}},
        }))
        old = subprocess.Popen([str(installed), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status(installed).get('engine', {}).get('phase') == 'ready', f'released {old_version} did not index the configured root')
            command(installed, '--show')
            until(lambda: status(installed).get('ui', {}).get('rows', 0) >= 2, f'{old_version} did not load the configured pins and history')
            command(installed, '--quit')
            assert old.wait(timeout=15) == 0
        finally:
            if old.poll() is None:
                old.terminate(); old.wait(timeout=10)
        kept = {name: (directory / name).read_bytes() for directory, name in (
            (old_config, 'settings.json'), (old_data, 'pins.json'), (old_data, 'frecency.json'),
        )}
        new_receipt = command(current, '--install', '--silent', '--no-launch', '--keep-download')
        assert new_receipt['version'] == VERSION, new_receipt
        installed = Path(new_receipt['path'])
        assert installed.is_relative_to(root)
        new_config = Path(env['XDG_CONFIG_HOME']) / 'arcade/find'
        new_data = Path(env['XDG_DATA_HOME']) / 'arcade/find'
        before_settings = json.loads(kept['settings.json'])
        after_settings = json.loads((new_config / 'settings.json').read_bytes())
        (output / 'settings-before.json').write_text(json.dumps(before_settings, indent=2) + '\n')
        (output / 'settings-after.json').write_text(json.dumps(after_settings, indent=2) + '\n')
        # Installation records Start at login and serializes newly added
        # defaults. Every existing preference must survive that serialization.
        for key, value in before_settings.items():
            assert key in after_settings and after_settings[key] == value, (key, value, after_settings.get(key))
        for name in ('pins.json', 'frecency.json'):
            assert (new_data / name).read_bytes() == kept[name], name + ' was not preserved'
        if legacy:
            assert not old_config.exists() and not old_data.exists(), 'verified migration left the 0.4 folders'
        # The launcher carries Find's identity (icon, window class); the login
        # entry is Link's: it starts this copy in the background and is ours.
        expected = {
            'desktopEntry': ['Icon=dev.arcade.Find', 'StartupWMClass=dev.arcade.Find', 'X-Arcade-Id=arcade.find'],
            'autostart': ['X-Arcade-Id=arcade.find'],
        }
        for key in ('desktopEntry', 'autostart'):
            entry = Path(new_receipt['integration'][key])
            assert entry.is_relative_to(root) and entry.name == 'dev.arcade.Find.desktop', (key, entry)
            assert entry.exists(), (key, entry, 'missing')
            lines = entry.read_text().splitlines()
            for line in expected[key]:
                assert line in lines, (key, entry, line, lines)
            if key == 'autostart':
                assert any(l.startswith('Exec=') and str(installed) in l and l.endswith('--background"') for l in lines), (key, lines)
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
        (output / 'result.json').write_text(json.dumps({'from': old_version, 'to': VERSION,
            'shortcut': 'Ctrl+Alt+J', 'settings': True, 'pins': True, 'history': True,
            'desktopEntry': 'dev.arcade.Find.desktop', 'uninstallKeptData': True}, indent=2) + '\n')
    migration = 'verified folder migration, ' if legacy else ''
    print(f'Real AppImage upgrade {old_version} → {VERSION}: settings, shortcut, pins/history, {migration}desktop/login entries and data-preserving uninstall passed.')


if __name__ == '__main__':
    main()
