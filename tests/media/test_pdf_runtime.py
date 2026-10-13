#!/usr/bin/env python3
"""Pinned resource data cannot relax executable or ordinary resource ownership."""
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
    def test_provenance_and_runtime_pins_agree(self):
        provenance = json.loads((REPO / 'docs/pdf-runtime-resources.json').read_text())
        self.assertEqual(runtime.PINNED_RESOURCES, {
            pathlib.Path(name): (entry['size'], entry['sha256'])
            for name, entry in provenance['files'].items()
        })
        self.assertGreater(len(runtime.PINNED_RESOURCES), 300)
        for path in runtime.PINNED_RESOURCES:
            self.assertTrue(any(path.is_relative_to(root) for root in runtime.DATA_ROOTS)
                            or str(path) == '/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf')

    def test_exact_bounded_bytes_are_returned(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'font.ttf'
            data = b'owned font integrity fixture'
            path.write_bytes(data)
            expected = (len(data), hashlib.sha256(data).hexdigest())
            self.assertEqual(runtime.pinned_resource_bytes(path, expected), data)
            for changed in (data[:-1], data + b'x', b'x' * len(data)):
                path.write_bytes(changed)
                with self.assertRaises(ValueError):
                    runtime.pinned_resource_bytes(path, expected)

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
                    runtime.pinned_resource_bytes(invalid, expected)
            with mock.patch.object(runtime, 'MAX_FILE', 3), self.assertRaises(ValueError):
                runtime.pinned_resource_bytes(path, expected)

    def test_only_exact_pinned_path_uses_content_authentication(self):
        path, expected = next(iter(runtime.PINNED_RESOURCES.items()))
        root_file = os.stat_result((0o100644, 0, 0, 1, 0, 0, expected[0], 0, 0, 0))
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', return_value=root_file), \
                mock.patch.object(runtime, 'pinned_resource_bytes', return_value=b'checked') as read_pin:
            self.assertEqual(runtime.checked_bytes(path, resource=True), b'checked')
            read_pin.assert_called_once_with(path, expected)
        writable_directory = os.stat_result((0o40777, 0, 0, 1, 0, 0, 0, 0, 0, 0))
        def metadata(current):
            return root_file if current == path else writable_directory
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', metadata), \
                mock.patch.object(runtime, 'pinned_resource_bytes') as read_pin:
            with self.assertRaisesRegex(ValueError, 'directory is not trusted'):
                runtime.checked_bytes(path)
            read_pin.assert_not_called()
        writable_file = os.stat_result((0o100666, 0, 0, 1, 0, 0, expected[0], 0, 0, 0))
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', return_value=writable_file), \
                mock.patch.object(runtime, 'pinned_resource_bytes') as read_pin:
            with self.assertRaises(ValueError):
                runtime.checked_bytes(path)
            read_pin.assert_not_called()

    def test_pin_binds_the_requested_archive_name_not_a_symlink_target(self):
        path = pathlib.Path('/usr/share/ghostscript/10.02.1/Resource/Font/NimbusMonoPS-Regular')
        target = pathlib.Path('/usr/share/fonts/type1/urw-base35/NimbusRoman-Regular.t1')
        expected = runtime.PINNED_RESOURCES[path]
        self.assertNotEqual(expected, runtime.PINNED_RESOURCES[target])
        info = os.stat_result((0o100644, 0, 0, 1, 0, 0, expected[0], 0, 0, 0))
        with mock.patch.object(pathlib.Path, 'resolve', return_value=target), \
                mock.patch.object(pathlib.Path, 'stat', return_value=info), \
                mock.patch.object(runtime, 'pinned_resource_bytes', side_effect=ValueError) as read_pin:
            with self.assertRaises(ValueError):
                runtime.checked_bytes(path, resource=True)
            read_pin.assert_called_once_with(target, expected)

    def test_ordinary_resource_still_requires_trusted_parents(self):
        # Model the CI directory metadata without requiring root or changing
        # any system permissions. This path is deliberately not a pinned resource.
        path = pathlib.Path('/usr/share/fonts/truetype/other.ttf')
        root_file = os.stat_result((0o100644, 0, 0, 1, 0, 0, 4, 0, 0, 0))
        writable_directory = os.stat_result((0o40777, 0, 0, 1, 0, 0, 0, 0, 0, 0))
        def metadata(current):
            return root_file if current == path else writable_directory
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', metadata), \
                mock.patch.object(runtime, 'pinned_resource_bytes') as read_pin:
            with self.assertRaisesRegex(ValueError, 'directory is not trusted'):
                runtime.checked_bytes(path, resource=True)
            read_pin.assert_not_called()


if __name__ == '__main__':
    unittest.main()
