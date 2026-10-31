"""Packaging script regressions; native installers are also exercised in CI."""
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Packaging(unittest.TestCase):
    @unittest.skipIf(sys.platform == 'win32', 'POSIX shell regression runs on Linux and macOS')
    def test_macos_cleanup_does_not_reference_the_removed_icon_build_directory(self):
        with tempfile.TemporaryDirectory(prefix='find-package-regression-') as temporary:
            root = Path(temporary)
            tools = root / 'bin'
            tools.mkdir()
            utility = tools / 'hdiutil'
            utility.write_text('#!/bin/sh\nfor argument do output=$argument; done\n: > "$output"\n')
            utility.chmod(0o700)
            destination = root / 'dist'
            environment = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ['PATH'])
            environment.pop('ICON_ROOT', None)
            result = subprocess.run(['bash', 'packaging/macos/build-app.sh', sys.executable, '0.5.0', 'universal', str(destination)],
                                    cwd=ROOT, env=environment, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            app = destination / 'Arcade Find.app/Contents'
            info = plistlib.loads((app / 'Info.plist').read_bytes())
            self.assertEqual(info['CFBundleIdentifier'], 'dev.arcade.Find')
            self.assertEqual(info['CFBundleShortVersionString'], '0.5.0')
            self.assertEqual((app / 'Resources/arcade-find.icns').read_bytes(), (ROOT / 'assets/app-icon/arcade-find.icns').read_bytes())
            self.assertTrue((destination / 'Arcade-Find-0.5.0-universal.dmg').is_file())


if __name__ == '__main__':
    unittest.main()
