#!/usr/bin/env python3
"""Use real files through default-open, FileManager1 reveal and copy-path."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
assert os.environ.get('ARCADE_E2E_INNER') == '1' and root != Path('/')
assert Path(os.environ['HOME']).resolve().is_relative_to(root)
assert not os.environ.get('WAYLAND_DISPLAY')

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
INTERFACE = 'org.freedesktop.FileManager1'


class FileManager(dbus.service.Object):
    def __init__(self):
        self.bus = dbus.SessionBus()
        self.name = dbus.service.BusName(INTERFACE, self.bus, do_not_queue=True)
        super().__init__(self.name, '/org/freedesktop/FileManager1')
        self.items = []

    @dbus.service.method(INTERFACE, in_signature='ass')
    def ShowItems(self, items, startup_id):
        self.items.append(list(map(str, items)))


def until(fn, message, seconds=15):
    deadline = time.monotonic() + seconds
    context = GLib.MainContext.default()
    while time.monotonic() < deadline:
        while context.pending():
            context.iteration(False)
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = Path(__file__).resolve().parents[1] / 'target/e2e/os-handoffs'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'os-handoff-files'
    files.mkdir()
    path = files / 'handoff café 日本.txt'
    contents = 'Do not modify a selected file.\n'
    path.write_text(contents)
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({'schema': 1, 'roots': [str(files)], 'welcomeComplete': True}))
    recorded = root / 'opened.json'
    helper = root / 'record-open.py'
    helper.write_text('import json,sys\nfrom pathlib import Path\nPath(' + repr(str(recorded)) + ').write_text(json.dumps(sys.argv[1:]))\n')
    # xdg-utils 1.1.3 (Ubuntu 24.04) word-splits Exec without honouring
    # quotes, so a quoted program argument reaches python3 with its quote
    # characters. Keep the handler's own path plain; the opened file keeps
    # its spaces and Unicode, which is what this test is about.
    assert not any(c in str(helper) for c in ' \t"\'\\$`'), helper
    apps = Path(os.environ['XDG_DATA_HOME']) / 'applications'
    apps.mkdir(parents=True, exist_ok=True)
    (apps / 'find-test-open.desktop').write_text('[Desktop Entry]\nType=Application\nName=Find test default handler\nExec=/usr/bin/python3 ' + str(helper) + ' %f\nTerminal=false\nMimeType=text/plain;\n')
    mime = Path(os.environ['XDG_CONFIG_HOME']) / 'mimeapps.list'
    mime.parent.mkdir(parents=True, exist_ok=True)
    mime.write_text('[Default Applications]\ntext/plain=find-test-open.desktop\n')
    manager = FileManager()
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        result = command('--status')
        return json.loads(result.stdout) if result.returncode == 0 else {}

    def select():
        assert command('--show', 'handoff ext:txt').returncode == 0
        until(lambda: status().get('ui', {}).get('selectedPath') == str(path), 'OS handoff result did not populate')
        result = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--class', '^dev.arcade.Find$'], env=env, text=True)
        subprocess.run(['xdotool', 'windowfocus', '--sync', result.splitlines()[-1]], env=env, check=True)

    with (output / 'resident.log').open('w') as log:
        resident = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'OS handoff index did not finish')
            select()
            # --show QUERY opens with the query selected, and Ctrl+C on selected
            # search text copies that text (docs/user/shortcuts.md). End moves
            # the caret like a user who typed the query, so Ctrl+C copies the path.
            subprocess.run(['xdotool', 'key', 'End', 'ctrl+c'], env=env, check=True)
            last = {}
            def copied():
                result = subprocess.run(['xclip', '-selection', 'clipboard', '-target', 'UTF8_STRING', '-o'], env=env, capture_output=True, text=True)
                last['clipboard'] = result.stdout
                return result.stdout == str(path)
            try:
                until(copied, 'Copy path did not copy the selected Unicode path')
            except AssertionError as error:
                raise AssertionError(f'{error}: clipboard held {last.get("clipboard")!r}') from None
            assert status()['ui']['toast'] == 'Copied'
            subprocess.run(['xdotool', 'key', 'ctrl+Return'], env=env, check=True)
            until(lambda: manager.items == [[path.as_uri()]], 'Show in folder did not select the exact file through FileManager1')
            select()
            subprocess.run(['xdotool', 'key', 'shift+Return'], env=env, check=True)
            until(recorded.exists, 'Open with default app did not launch the configured handler')
            assert json.loads(recorded.read_text()) == [str(path)]
            assert path.read_text() == contents
            (output / 'evidence.json').write_text(json.dumps({'copiedPath': str(path), 'revealed': manager.items,
                'opened': json.loads(recorded.read_text()), 'sourceUnchanged': True}, indent=2) + '\n')
            assert command('--quit').returncode == 0
            assert resident.wait(timeout=10) == 0
        finally:
            command('--quit')
            if resident.poll() is None:
                resident.terminate()
            resident.wait(timeout=10)
    print('Real OS handoffs: Unicode Copy path plus Copied toast, exact FileManager1 selection, default MIME handler receives the file, source bytes unchanged.')


if __name__ == '__main__':
    main()
