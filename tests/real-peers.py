#!/usr/bin/env python3
"""Use Find's real action list against the verified released Look and Box."""
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import zlib


def until(fn, message, seconds=25):
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
    assert Path(os.environ['HOME']).resolve().is_relative_to(root)
    find = Path(sys.argv[1]).resolve(strict=True)
    peers = {name: Path(os.environ['ARCADE_' + name.upper() + '_BIN']).resolve(strict=True) for name in ('look', 'box')}
    cli = Path(os.environ['ARCADE_LINK_CLI']).resolve(strict=True)
    output = Path(__file__).resolve().parents[1] / 'target/e2e/real-peers'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'peer-files'
    files.mkdir()
    image = files / 'handoff-image.png'

    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))

    image.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!IIBBBBB', 24, 16, 8, 2, 0, 0, 0)) +
                      chunk(b'IDAT', zlib.compress((b'\0' + bytes([90, 130, 170]) * 24) * 16)) + chunk(b'IEND', b''))
    original = hashlib.sha256(image.read_bytes()).hexdigest()
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({'schema': 1, 'roots': [str(files)], 'welcomeComplete': True}))
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1', WEBKIT_DISABLE_COMPOSITING_MODE='1')

    def command(binary, *args, check=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)
        if check:
            assert result.returncode == 0, (args, result.stdout, result.stderr)
        return result

    def status():
        result = command(find, '--status', check=False)
        return json.loads(result.stdout) if result.returncode == 0 else {}

    def xdo(*args):
        return subprocess.check_output(['xdotool', *map(str, args)], env=env, text=True).strip()

    def window(title):
        result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', title], env=env, capture_output=True, text=True)
        return result.stdout.splitlines()[-1] if result.returncode == 0 else None

    def show_result():
        command(find, '--show', 'handoff-image ext:png')
        until(lambda: status().get('ui', {}).get('selectedPath') == str(image), 'real image result did not populate')
        result = until(lambda: subprocess.run(['xdotool', 'search', '--onlyvisible', '--class', '^dev.arcade.Find$'],
                                             env=env, capture_output=True, text=True).stdout.strip(), 'Find did not map')
        xdo('windowfocus', '--sync', result.splitlines()[-1])

    def action(title):
        show_result()
        xdo('key', 'Tab')
        until(lambda: status().get('ui', {}).get('mode') == 'actions', 'Find action list did not open')
        xdo('type', '--clearmodifiers', title.rstrip('…'))
        until(lambda: status().get('ui', {}).get('actions') == [{'title': title, 'disabled': False}],
              'real peer action was not available: ' + title)
        subprocess.run(['import', '-window', 'root', str(output / ('action-' + ('convert' if 'WebP' in title else 'box') + '.png'))], check=True)
        xdo('key', 'Return')

    processes = []
    with (output / 'peers.log').open('w') as log:
        try:
            for binary in [find, *peers.values()]:
                processes.append(subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log))
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'Find did not become ready')
            for name in peers:
                until(lambda: command(cli, 'status', 'arcade.' + name, check=False).returncode == 0, 'real ' + name + ' did not register')
                document = json.loads((Path(env['ARCADE_HOME']) / 'apps' / ('arcade.' + name + '.json')).read_text())
                assert document['version'] == '0.4.0', (name, document['version'])
            show_result()
            xdo('key', 'Return')
            until(lambda: window('handoff-image.png'), 'Find Enter did not open the real selected image in Look')
            until(lambda: not status().get('ui', {}).get('visible'), 'Find did not hide for Look preview')
            subprocess.run(['import', '-window', 'root', str(output / 'look-preview.png')], check=True)
            command(peers['look'], '--hide')
            before = set(root.rglob('*.webp'))
            action('Convert to WebP')
            until(lambda: any('Convert to WebP with Arcade Box' in item['text'] for item in status().get('activity', [])),
                  'real Box conversion did not finish through Find')
            converted = until(lambda: [path for path in root.rglob('*.webp') if path not in before and path.stat().st_size > 0],
                              'Box conversion produced no real WebP file')
            for path in converted:
                data = path.read_bytes()
                assert data[:4] == b'RIFF' and data[8:12] == b'WEBP', path
                dimensions = subprocess.check_output(['identify', '-format', '%wx%h', str(path)], text=True)
                assert dimensions == '24x16', (path, dimensions)
            assert status()['ui']['visible'], 'Find hid for a noninteractive conversion'
            # The generic fallback must also reach the real Box Island.
            action('More in Arcade Box…')
            until(lambda: any('More in Arcade Box… with Arcade Box' in item['text'] for item in status().get('activity', [])),
                  'Box did not acknowledge the selected input')
            until(lambda: window('^Arcade Box$'), 'real Box Island did not open')
            subprocess.run(['import', '-window', 'root', str(output / 'box-island.png')], check=True)
            assert hashlib.sha256(image.read_bytes()).hexdigest() == original, 'peer actions changed the source image'
            (output / 'result.json').write_text(json.dumps({'look': '0.4.0', 'box': '0.4.0',
                'lookPreview': image.name, 'boxConversion': '24x16 WebP', 'boxIsland': True,
                'sourceUnchanged': True}, indent=2) + '\n')
        finally:
            command(find, '--quit', check=False)
            for name in peers:
                command(cli, 'quit', 'arcade.' + name, '--force', check=False)
            for process in processes:
                try:
                    process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=10)
    print('Real peers: Find Enter previews the selected PNG in Look 0.4.0; Find runs Box 0.4.0 WebP conversion and opens its Island; source bytes unchanged.')


if __name__ == '__main__':
    main()
