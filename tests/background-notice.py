#!/usr/bin/env python3
"""A real Settings close keeps the resident and sends its explanation once."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def require_isolation():
    root = Path(os.environ.get('ARCADE_E2E_ROOT', '/')).resolve()
    assert os.environ.get('ARCADE_E2E_INNER') == '1' and root != Path('/')
    assert Path(os.environ['HOME']).resolve().is_relative_to(root)
    assert not os.environ.get('WAYLAND_DISPLAY')
    return root


ROOT = require_isolation()
import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

DBusGMainLoop(set_as_default=True)
INTERFACE = 'org.freedesktop.Notifications'


class Notifications(dbus.service.Object):
    def __init__(self):
        self.bus = dbus.SessionBus()
        self.name = dbus.service.BusName(INTERFACE, self.bus, do_not_queue=True)
        super().__init__(self.name, '/org/freedesktop/Notifications')
        self.messages = []

    @dbus.service.method(INTERFACE, out_signature='as')
    def GetCapabilities(self):
        return ['body']

    @dbus.service.method(INTERFACE, out_signature='ssss')
    def GetServerInformation(self):
        return ('Find isolated test', 'Arcade', '1', '1.2')

    @dbus.service.method(INTERFACE, in_signature='susssasa{sv}i', out_signature='u')
    def Notify(self, app, replaces, icon, summary, body, actions, hints, timeout):
        self.messages.append({'app': str(app), 'summary': str(summary), 'body': str(body)})
        return len(self.messages)

    @dbus.service.method(INTERFACE, in_signature='u')
    def CloseNotification(self, identifier):
        pass


def pump():
    context = GLib.MainContext.default()
    while context.pending():
        context.iteration(False)


def until(fn, message, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        pump()
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(message)


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    config = Path(os.environ['ARCADE_FIND_HOME']) / 'config/settings.json'
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_text(json.dumps({'schema': 1, 'roots': [], 'welcomeComplete': True, 'notifications': True}))
    output = Path(__file__).resolve().parents[1] / 'target/e2e/background-notice'
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, ARCADE_NO_INSTALL_PROMPT='1')
    notifications = Notifications()

    def command(*args):
        return subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=15)

    def status():
        result = command('--status')
        return json.loads(result.stdout) if result.returncode == 0 else {}

    def close_settings(log, key='ctrl+w'):
        helper = subprocess.Popen([str(binary), '--settings-window'], env=env, stdout=log, stderr=log)
        try:
            def window():
                result = subprocess.run(['xdotool', 'search', '--onlyvisible', '--pid', str(helper.pid)],
                                        env=env, capture_output=True, text=True)
                return result.stdout.splitlines()[-1] if result.returncode == 0 else None
            identifier = until(window, 'Settings did not map for close notice')
            subprocess.run(['xdotool', 'windowfocus', '--sync', identifier, 'key', key], env=env, check=True)
            until(lambda: helper.poll() is not None, 'Settings did not exit after closing')
            assert helper.returncode == 0
        finally:
            if helper.poll() is None:
                helper.terminate(); helper.wait(timeout=10)

    with (output / 'resident.log').open('w') as log:
        for cycle in range(2):
            resident = subprocess.Popen([str(binary), '--background'], env=env, stdout=log, stderr=log)
            try:
                until(lambda: status().get('running'), 'notice resident did not start')
                close_settings(log)
                until(lambda: len(notifications.messages) == 1, 'first close background notice was not delivered')
                assert status().get('running') and resident.poll() is None, 'closing Settings quit the background app'
                close_settings(log)
                deadline = time.monotonic() + 1
                while time.monotonic() < deadline:
                    pump(); time.sleep(.02)
                assert len(notifications.messages) == 1, notifications.messages
                assert notifications.messages[0]['body'] == 'Arcade Find is still running. Quit it from the tray.'
                if cycle == 1:
                    close_settings(log, 'ctrl+q')
                    until(lambda: resident.poll() is not None, 'Settings Quit returned before stopping the resident')
                else:
                    assert command('--quit').returncode == 0
                assert resident.wait(timeout=10) == 0
                assert len(notifications.messages) == 1, 'Quit sent an incorrect still-running notice'
            finally:
                command('--quit')
                if resident.poll() is None:
                    resident.terminate()
                resident.wait(timeout=10)
        (output / 'evidence.json').write_text(json.dumps({'residentRestarted': True,
            'settingsClosed': 4, 'settingsQuit': True, 'notifications': notifications.messages}, indent=2) + '\n')
    print('Settings close: resident stays running; exact notice once across four closes and restart; Settings Quit stops the resident.')


if __name__ == '__main__':
    main()
