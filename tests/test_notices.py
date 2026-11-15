"""scripts/notices.py: the AppImage library check and the standard licence fallback."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('notices', ROOT / 'scripts/notices.py')
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)

ELF = b'\x7fELF' + b'\0' * 12


class AppImageLibraries(unittest.TestCase):
    def image(self, root, extra=()):
        (root / 'usr/bin').mkdir(parents=True)
        (root / 'usr/bin/arcade-find').write_bytes(ELF)
        (root / 'usr/share').mkdir()
        (root / 'usr/share/readme.md').write_text('not a program')
        for name in extra:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(ELF)

    def test_an_image_with_only_find_passes(self):
        with tempfile.TemporaryDirectory(prefix='find-notices-') as tmp:
            self.image(Path(tmp))
            self.assertEqual(notices.unexpected_elf(tmp), [])

    def test_a_bundled_library_is_reported(self):
        with tempfile.TemporaryDirectory(prefix='find-notices-') as tmp:
            self.image(Path(tmp), ['usr/lib/libwebkit2gtk-4.1.so.0', 'usr/bin/helper'])
            self.assertEqual(notices.unexpected_elf(tmp), ['usr/bin/helper', 'usr/lib/libwebkit2gtk-4.1.so.0'])


class StandardTexts(unittest.TestCase):
    def test_a_crate_without_a_licence_file_gets_the_standard_text_with_its_authors(self):
        text = notices.standard_text('MIT OR Apache-2.0', ['A B <a@example.org>'], 'demo')[0]
        self.assertIn('Copyright (c) A B', text)
        self.assertIn('demo is offered under MIT OR Apache-2.0', text)
        self.assertEqual(notices.standard_text('GPL-9.0', [], 'x'), [])


if __name__ == '__main__':
    unittest.main()
