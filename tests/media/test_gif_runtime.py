"""The GIF cleaner is pinned and packaged only from trusted system files."""
import importlib.util
import os
import pathlib
import subprocess
import sys
import stat
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

REPO = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('gif_runtime', REPO / 'scripts/media/gif_runtime.py')
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)


class GifRuntimeTest(unittest.TestCase):
    def test_operator_prefix(self):
        self.assertEqual(runtime.PREFIX, pathlib.Path('/usr/lib/26chan-gif'))

    def test_every_parent_is_trusted_before_read(self):
        path = runtime.PREFIX / 'gifsicle'
        data = b'\x7fELFsynthetic'
        file_info = os.stat_result((0o100755, 0, 0, 1, 0, 0, len(data), 0, 0, 0))
        directory_info = os.stat_result((0o40755, 0, 0, 1, 0, 0, 0, 0, 0, 0))
        def metadata(current):
            return file_info if current == path else directory_info
        with mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                mock.patch.object(pathlib.Path, 'stat', metadata), \
                mock.patch.object(pathlib.Path, 'read_bytes', return_value=data):
            self.assertEqual(runtime.checked_bytes(path), data)
        for parent in path.parents:
            for mode, uid in [(0o40777, 0), (0o40755, 1000)]:
                invalid = os.stat_result((mode, 0, 0, 1, uid, 0, 0, 0, 0, 0))
                def changed(current):
                    return invalid if current == parent else metadata(current)
                with self.subTest(parent=parent, uid=uid), \
                        mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                        mock.patch.object(pathlib.Path, 'stat', changed), \
                        mock.patch.object(pathlib.Path, 'read_bytes') as read:
                    with self.assertRaisesRegex(ValueError, 'parent is not trusted'):
                        runtime.checked_bytes(path)
                    read.assert_not_called()

    def collect(self, version=b'LCDF Gifsicle 1.97\n',
                dependencies=b'libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x123)\n/lib64/ld-linux-x86-64.so.2 (0x456)\n'):
        responses = [subprocess.CompletedProcess([], 0, b'', version),
                     subprocess.CompletedProcess([], 0, dependencies, b'')]
        with mock.patch.object(runtime, 'checked_bytes', return_value=b'\x7fELFsynthetic'), \
                mock.patch.object(runtime.subprocess, 'run', side_effect=responses) as run:
            result = runtime.collect()
            self.assertEqual(run.call_args_list[0].kwargs['env'],
                             {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'})
            self.assertEqual(run.call_args_list[0].kwargs['timeout'], 5)
            return result

    def test_fixed_executable_and_manifest(self):
        files, manifest = self.collect()
        self.assertEqual(set(files), {'usr/bin/board-gifsicle',
                                     'lib/x86_64-linux-gnu/libc.so.6',
                                     'lib64/ld-linux-x86-64.so.2'})
        self.assertEqual(set(files), set(manifest))
        self.assertTrue(all(len(digest) == 64 for digest in manifest.values()))

    def test_wrong_version_or_unreviewed_dependencies_rejected(self):
        with self.assertRaisesRegex(ValueError, 'version'):
            self.collect(version=b'LCDF Gifsicle 1.95')
        for dependencies in [b'', b'libc.so.6 => not found',
                             b'/usr/lib/libpng16.so.16 (0x123)',
                             b'/usr/lib/libjpeg.so.62 (0x123)']:
            with self.subTest(dependencies=dependencies), self.assertRaises(ValueError):
                self.collect(dependencies=dependencies)

    def test_guest_archive_has_parent_directories_before_cleaner_and_loader(self):
        spec = importlib.util.spec_from_file_location('gif_initramfs', REPO / 'scripts/media/build-initramfs.py')
        builder = importlib.util.module_from_spec(spec)
        with mock.patch.dict(sys.modules, {'pdf_runtime': SimpleNamespace(collect=None)}):
            spec.loader.exec_module(builder)
        files, manifest = self.collect()
        with tempfile.TemporaryDirectory(prefix='owned-gif-image-') as directory:
            root = pathlib.Path(directory)
            binary, output = root / 'synthetic-elf', root / 'image.cpio'
            binary.write_bytes(b'\x7fELFsynthetic')
            argv = ['build-initramfs.py', str(binary), str(binary), str(output), '--gif-runtime']
            with mock.patch.object(sys, 'argv', argv), \
                    mock.patch.dict(sys.modules, {'gif_runtime': runtime}), \
                    mock.patch.object(runtime, 'collect', return_value=(files, manifest)):
                builder.main()
            image = output.read_bytes()
        offset, directories, packaged = 0, set(), set()
        while offset < len(image):
            self.assertEqual(image[offset:offset + 6], b'070701')
            fields = [int(image[offset + 6 + n * 8:offset + 14 + n * 8], 16) for n in range(13)]
            size, name_size = fields[6], fields[11]
            name = image[offset + 110:offset + 110 + name_size - 1].decode()
            data_offset = (offset + 110 + name_size + 3) // 4 * 4
            for parent in pathlib.PurePosixPath(name).parents:
                if str(parent) != '.':
                    self.assertIn(str(parent), directories, name)
            if stat.S_ISDIR(fields[1]):
                directories.add(name)
            else:
                packaged.add(name)
            offset = (data_offset + size + 3) // 4 * 4
        self.assertTrue(set(files).issubset(packaged))
        self.assertTrue({'usr', 'usr/bin', 'lib', 'lib/x86_64-linux-gnu', 'lib64'}.issubset(directories))

    def test_total_budget(self):
        with mock.patch.object(runtime, 'MAX_TOTAL', 1):
            with self.assertRaisesRegex(ValueError, 'budget'):
                self.collect()


if __name__ == '__main__':
    unittest.main()
