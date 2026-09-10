import importlib.util
from pathlib import Path
import tempfile
import unittest


class DownloaderTests(unittest.TestCase):
    def test_sha256_check_accepts_intact_and_rejects_changed_content(self):
        path = Path(__file__).resolve().parents[2] / 'scripts/monitoring/download.py'
        self.assertTrue(path.exists(), 'Pinned downloader is not implemented')
        spec = importlib.util.spec_from_file_location('download', path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'release.tar.gz'
            archive.write_bytes(b'abc')
            expected = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad'
            module.verify(archive, expected)
            archive.write_bytes(b'abd')
            with self.assertRaisesRegex(ValueError, 'SHA256'):
                module.verify(archive, expected)


if __name__ == '__main__':
    unittest.main()
