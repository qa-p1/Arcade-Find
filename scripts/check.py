#!/usr/bin/env python3
"""The same source, lint, test and native checks CI runs, without silent skips."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
os.chdir(ROOT)


def run(*args, env=None):
    subprocess.run(args, check=True, env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--core-only', action='store_true')
    parser.add_argument('--native-only', action='store_true')
    parser.add_argument('--binary', type=Path)
    args = parser.parse_args()
    if not args.native_only:
        run(sys.executable, 'scripts/check-docs.py')
        run(sys.executable, 'scripts/check-vendored.py')
        run(sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_*.py')
        run(sys.executable, 'scripts/check-design.py', '--source-only')
        run('cargo', 'fmt', '--all', '--', '--check')
        run('cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings')
        run(sys.executable, 'scripts/isolate.py', 'cargo', 'test', '--workspace', '--locked')
        run(sys.executable, 'scripts/check-design.py')
    if args.core_only or sys.platform != 'linux':
        return
    binary = (args.binary or Path(os.environ.get('CARGO_TARGET_DIR', 'target')) / 'release/arcade-find').resolve()
    link = Path(os.environ['ARCADE_LINK_DIR']).resolve()
    if not binary.is_file():
        run('cargo', 'build', '--release', '--locked', '-p', 'arcade-find', '--example', 'e2e-pointer', '--bin', 'arcade-find')
    pointer = Path(os.environ.get('ARCADE_E2E_POINTER', str(binary.parent / 'examples/e2e-pointer')))
    if not pointer.is_file():
        raise SystemExit('Build --example e2e-pointer alongside the native binary before running this check.')
    cli = Path(os.environ['ARCADE_LINK_CLI']).resolve()
    env = dict(os.environ, ARCADE_LINK_CLI=str(cli))
    old = os.environ.get('ARCADE_FIND_OLD_WAYLAND_BIN')
    if old:
        for script, expected, label in (
            ('clipboard-paste.py', 'file-list paste did not restore the copied files', 'clipboard'),
            ('hover-tooltip.py', 'full filename tooltip did not appear', 'tooltip'),
            ('accessibility.py', 'search window was not exposed to the accessibility bus', 'accessibility'),
        ):
            regression = subprocess.run([sys.executable, str(link / 'tools/e2e.py'), 'run', '--',
                                         ('/usr/bin/python3' if script == 'accessibility.py' else sys.executable), 'tests/' + script, old],
                                        env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            evidence = ROOT / ('target/e2e/' + label + '-old-regression.log')
            evidence.parent.mkdir(parents=True, exist_ok=True)
            evidence.write_text(regression.stdout)
            if regression.returncode == 0 or expected not in regression.stdout:
                raise SystemExit('The old ' + label + ' regression was not reproduced; see ' + str(evidence))
            print('Old ' + label + ' regression reproduced: ' + expected + '.')
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'tests/clipboard-paste.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'tests/file-actions.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'tests/hover-tooltip.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', '/usr/bin/python3', 'tests/accessibility.py', str(binary), env=env)
    for peer in ('LOOK', 'BOX'):
        if not os.environ.get('ARCADE_' + peer + '_BIN'):
            raise SystemExit('The real released ' + peer.title() + ' is required for peer verification.')
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'tests/real-peers.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'scripts/check-native.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', sys.executable, 'scripts/check-install-ui.py', str(binary), env=env)
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', '/usr/bin/python3', 'tests/tray-registration.py', str(binary), str(cli), 'find', env=env)
    if not os.environ.get('ARCADE_SHELF_BIN'):
        raise SystemExit('ARCADE_SHELF_BIN must name the real Shelf binary; peer/drag checks have not passed.')
    run(sys.executable, str(link / 'tools/e2e.py'), 'run', '--', 'scripts/e2e-linux.sh', str(binary), 'target/e2e', env=env)


if __name__ == '__main__': main()
