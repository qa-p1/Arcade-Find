"""The published-peer verifier also runs on Ubuntu 22.04's Python 3.10."""
from contextlib import redirect_stdout
import hashlib
import io
import json
from pathlib import Path
import runpy
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]


class PeerFetch(unittest.TestCase):
    def test_fetch_and_hash_verification_without_python_311_file_digest(self):
        image = b'A fixture image; extraction is checked by the native CI job.'
        image_name = 'Fixture.AppImage'
        digest = hashlib.sha256(image).hexdigest()
        manifest = json.dumps({'assets': [{'os': 'linux', 'kind': 'appimage',
                                          'file': image_name, 'sha256': digest, 'size': len(image)}]}).encode()
        files = {'arcade-release.json': manifest, image_name: image,
                 'SHA256SUMS.txt': (digest + '  ' + image_name + '\n').encode()}
        assets = [{'id': index, 'name': name, 'size': len(data),
                   'digest': 'sha256:' + hashlib.sha256(data).hexdigest()}
                  for index, (name, data) in enumerate(files.items(), start=1)]
        release = json.dumps({'tag_name': 'v0.4.3', 'assets': assets}).encode()
        by_id = {str(asset['id']): files[asset['name']] for asset in assets}
        extracted = []

        def run(arguments, **kwargs):
            if arguments[0] == 'gh':
                kwargs['stdout'].write(by_id[arguments[-1].rsplit('/', 1)[1]])
            else:
                self.assertEqual(arguments[1:], ['--appimage-extract'])
                extracted.append(arguments[0])

        absent = object()
        previous = getattr(hashlib, 'file_digest', absent)
        if previous is not absent:
            delattr(hashlib, 'file_digest')
        try:
            with tempfile.TemporaryDirectory() as temporary:
                destination = Path(temporary) / 'download'
                with patch.object(sys, 'argv', ['fetch-peer.py', 'qa-p1/Fixture', str(destination), 'v0.4.3']), \
                     patch('subprocess.check_output', return_value=release), patch('subprocess.run', side_effect=run), \
                     redirect_stdout(io.StringIO()):
                    runpy.run_path(str(ROOT / 'scripts/fetch-peer.py'), run_name='__main__')
                self.assertEqual((destination / image_name).read_bytes(), image)
                self.assertEqual(len(extracted), 1)
                self.assertEqual(json.loads((destination / 'source.json').read_text())['sha256'], digest)
        finally:
            if previous is not absent:
                hashlib.file_digest = previous


if __name__ == '__main__':
    unittest.main()
