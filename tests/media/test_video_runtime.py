"""Video executable packaging keeps the full parent-chain ownership boundary."""
import importlib.util
import os
import pathlib
import unittest
from unittest import mock

REPO = pathlib.Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('video_runtime', REPO / 'scripts/media/video_runtime.py')
runtime = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runtime)


class VideoRuntimeTest(unittest.TestCase):
    def test_operator_prefix_is_under_system_library_directory(self):
        self.assertEqual(runtime.PREFIX, pathlib.Path('/usr/lib/26chan-video'))

    def test_every_parent_must_be_root_owned_and_not_writable(self):
        path = runtime.PREFIX / 'ffmpeg'
        data = b'\x7fELFowned-test'
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
                with self.subTest(parent=parent, uid=uid, mode=mode), \
                        mock.patch.object(pathlib.Path, 'resolve', return_value=path), \
                        mock.patch.object(pathlib.Path, 'stat', changed), \
                        mock.patch.object(pathlib.Path, 'read_bytes') as read:
                    with self.assertRaisesRegex(ValueError, 'parent is not trusted'):
                        runtime.checked_bytes(path)
                    read.assert_not_called()


if __name__ == '__main__':
    unittest.main()
