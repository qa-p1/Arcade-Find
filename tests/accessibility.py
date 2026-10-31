#!/usr/bin/env python3
"""Verify the real AT-SPI tree, editable query and result announcements."""
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time

import dbus
import pyatspi
from gi.repository import GLib


def until(fn, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def descendants(node):
    yield node
    for child in node:
        if child is not None:
            yield from descendants(child)


def main():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and root != Path('/')
    assert Path(os.environ['HOME']).resolve().is_relative_to(root)
    binary = Path(sys.argv[1]).resolve(strict=True)
    output = Path(__file__).resolve().parents[1] / 'target/e2e/accessibility'
    output.mkdir(parents=True, exist_ok=True)
    files = root / 'screenreader-files'
    files.mkdir()
    names = ['reader-file-alpha-' + 'long-readable-name-' * 8 + '.txt', 'reader-file-beta.txt']
    for name in names:
        (files / name).write_text('Accessible results.\n')
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({'schema': 1, 'roots': [str(files)], 'welcomeComplete': True,
                                  'showRecentOnOpen': False}))
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')
    env.pop('NO_AT_BRIDGE', None)
    announcements = []
    tree = []

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        answer = command('--status')
        return json.loads(answer.stdout) if answer.returncode == 0 else {}

    def window():
        desktop = pyatspi.Registry.getDesktop(0)
        for app in desktop:
            if app is None:
                continue
            try:
                for node in descendants(app):
                    if node.name == 'Search — Arcade Find':
                        return node
            except (RuntimeError, GLib.Error):
                continue
        return None

    def listener(event):
        announcements.append(str(event.any_data))

    bus = dbus.SessionBus()
    props = dbus.Interface(bus.get_object('org.a11y.Bus', '/org/a11y/bus'), 'org.freedesktop.DBus.Properties')
    props.Set('org.a11y.Status', 'IsEnabled', dbus.Boolean(False))
    pyatspi.Registry.registerEventListener(listener, 'object:announcement')
    events = threading.Thread(target=pyatspi.Registry.start, daemon=True)
    events.start()
    with (output / 'resident.log').open('w') as log:
        process = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
        try:
            until(lambda: status().get('engine', {}).get('phase') == 'ready', 'accessible index did not finish')
            props.Set('org.a11y.Status', 'IsEnabled', dbus.Boolean(True))
            props.Set('org.a11y.Status', 'ScreenReaderEnabled', dbus.Boolean(True))
            assert command('--show').returncode == 0
            until(window, 'search window was not exposed to the accessibility bus')
            assert command('--show', 'reader-file ext:txt').returncode == 0
            until(lambda: status().get('ui', {}).get('rows') == 2, 'accessible results did not populate')
            until(lambda: '2 results' in announcements, 'two-result count was not announced')
            nodes = list(descendants(window()))
            for name in names:
                assert any(node.name == name for node in nodes), ('full result name missing from AT-SPI', name)
            full = next(node for node in nodes if node.name == names[0])
            assert full.description == str(files / names[0])
            assert full.queryComponent().grabFocus(), 'reader could not focus the full-name result'
            until(lambda: status().get('ui', {}).get('selectedPath') == str(files / names[0]),
                  'accessible focus did not select the result')
            assert status().get('ui', {}).get('visible'), 'focusing a result unexpectedly opened it'
            # AccessKit's Unix adapter exposes editable state, focus and Text,
            # but does not implement AT-SPI EditableText. Exercise the reader's
            # real focus/text interfaces and normal keyboard entry instead.
            def enter_query(value):
                query = next(node for node in descendants(window()) if node.name == 'Search your files')
                assert query.getState().contains(pyatspi.STATE_EDITABLE), 'query is exposed as read-only'
                assert query.queryComponent().grabFocus(), 'reader could not focus search'
                # The keyboard types into the focused window. Xvfb has no window
                # manager and X focus follows the pointer, so give Find's window
                # the keyboard as a desktop would (as os-handoffs.py does).
                found = subprocess.check_output(['xdotool', 'search', '--onlyvisible', '--class', '^dev.arcade.Find$'], env=env, text=True)
                subprocess.run(['xdotool', 'windowfocus', '--sync', found.splitlines()[-1]], env=env, check=True)
                subprocess.run(['xdotool', 'key', 'ctrl+a', 'type', '--clearmodifiers', value], env=env, check=True)
                read = {}
                def entered():
                    read['text'] = next(node for node in descendants(window()) if node.name == 'Search your files').queryText().getText(0, -1)
                    return read['text'] == value
                try:
                    until(entered, 'reader could not read the entered query')
                except AssertionError as error:
                    raise AssertionError(f'{error}: read {read.get("text")!r}, status {status().get("ui")}') from None
            enter_query('reader-file-beta ext:txt')
            until(lambda: status().get('ui', {}).get('rows') == 1, 'accessible query did not search')
            until(lambda: '1 result' in announcements, 'singular result count was not announced')
            enter_query('reader-file-missing ext:txt')
            until(lambda: '0 results for “reader-file-missing ext:txt”' in announcements, 'empty result count and query were not announced')
            tree = [{'role': node.getRoleName(), 'name': node.name, 'description': node.description}
                    for node in descendants(window())]
            assert command('--hide').returncode == 0
            until(lambda: not status().get('ui', {}).get('visible'), 'accessible hide did not complete')
            assert command('--quit').returncode == 0
            assert process.wait(timeout=10) == 0
        finally:
            (output / 'evidence.json').write_text(json.dumps({'announcements': announcements, 'tree': tree}, indent=2))
            command('--quit')
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
            pyatspi.Registry.stop()
            events.join(timeout=3)
    print('Real AT-SPI: complete long names and paths; focus selects without opening; reader focus/text and keyboard query search work; 2, 1 and 0 result announcements received.')


if __name__ == '__main__':
    main()
