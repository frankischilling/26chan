#!/usr/bin/env python3
"""Pinned font data cannot relax executable or ordinary resource ownership."""
import hashlib
import importlib.util
import json
import os
import pathlib
import tempfile
import unittest
from unittest import mock

REPO = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('pdf_runtime', REPO / 'scripts/media/pdf_runtime.py')
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)


class PdfRuntimeTest(unittest.TestCase):
    def test_provenance_and_runtime_pin_agree(self):
        provenance = json.loads((REPO / 'docs/pdf-runtime-font.json').read_text())
        self.assertEqual(runtime.PINNED_FONTS, {
            pathlib.Path(provenance['font_path']):
                (provenance['font_size'], provenance['font_sha256']),
        })

    def test_exact_bounded_bytes_are_returned(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'font.ttf'
            data = b'owned font integrity fixture'
            path.write_bytes(data)
            expected = (len(data), hashlib.sha256(data).hexdigest())
            self.assertEqual(runtime.pinned_font_bytes(path, expected), data)
            for changed in (data[:-1], data + b'x', b'x' * len(data)):
                path.write_bytes(changed)
                with self.assertRaises(ValueError):
                    runtime.pinned_font_bytes(path, expected)

    def test_symlink_directory_fifo_and_oversize_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            path = root / 'font.ttf'
            path.write_bytes(b'font')
            expected = (4, hashlib.sha256(b'font').hexdigest())
            link = root / 'link'
            link.symlink_to(path)
            fifo = root / 'fifo'
            os.mkfifo(fifo)
            for invalid in (link, root, fifo):
                with self.subTest(path=invalid), self.assertRaises((ValueError, OSError)):
                    runtime.pinned_font_bytes(invalid, expected)
            with mock.patch.object(runtime, 'MAX_FILE', 3), self.assertRaises(ValueError):
                runtime.pinned_font_bytes(path, expected)

    def test_only_exact_pinned_path_uses_content_authentication(self):
        path, expected = next(iter(runtime.PINNED_FONTS.items()))
        root_file = os.stat_result((0o100644, 0, 0, 1, 0, 0, expected[0], 0, 0, 0))
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', return_value=root_file), \
                mock.patch.object(runtime, 'pinned_font_bytes', return_value=b'checked') as read_pin:
            self.assertEqual(runtime.checked_bytes(path), b'checked')
            read_pin.assert_called_once_with(path, expected)
        writable_file = os.stat_result((0o100666, 0, 0, 1, 0, 0, expected[0], 0, 0, 0))
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', return_value=writable_file), \
                mock.patch.object(runtime, 'pinned_font_bytes') as read_pin:
            with self.assertRaises(ValueError):
                runtime.checked_bytes(path)
            read_pin.assert_not_called()

    def test_ordinary_resource_still_requires_trusted_parents(self):
        # Model the CI directory metadata without requiring root or changing
        # any system permissions. This path is deliberately not a pinned font.
        path = pathlib.Path('/usr/share/fonts/truetype/other.ttf')
        root_file = os.stat_result((0o100644, 0, 0, 1, 0, 0, 4, 0, 0, 0))
        writable_directory = os.stat_result((0o40777, 0, 0, 1, 0, 0, 0, 0, 0, 0))
        def metadata(current):
            return root_file if current == path else writable_directory
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', metadata), \
                mock.patch.object(runtime, 'pinned_font_bytes') as read_pin:
            with self.assertRaisesRegex(ValueError, 'directory is not trusted'):
                runtime.checked_bytes(path)
            read_pin.assert_not_called()


if __name__ == '__main__':
    unittest.main()
