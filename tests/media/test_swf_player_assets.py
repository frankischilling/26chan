"""Deterministic WASM memory-limit patch; no browser or movie is executed."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('swf_assets', ROOT / 'scripts/media/prepare-swf-player.py')
assets = importlib.util.module_from_spec(spec)
spec.loader.exec_module(assets)


def module(body, extra=b''):
    return b'\0asm\x01\0\0\0' + b'\x05' + assets.encode(len(body)) + body + extra


class SwfPlayerAssetsTest(unittest.TestCase):
    def test_only_reviewed_memory_declaration_changes(self):
        suffix = b'\x00\x03\x00ab'  # Opaque custom section remains byte-exact.
        original = module(b'\x01\x00\x49', suffix)
        expected = module(b'\x01\x01\x49' + assets.encode(4096), suffix)
        self.assertEqual(assets.cap_memory(original), expected)

    def test_unexpected_shapes_and_truncation_fail_closed(self):
        for original in [b'', b'\0asm\x01\0\0\0', module(b'\x01\x01\x49\x7f'),
                         module(b'\x01\x00\x48'), module(b'\x02\x00\x49'),
                         module(b'\x01\x00\x49\x00'), module(b'\x01\x00\x49', b'\x05\x03\x01\x00\x49'),
                         module(b'\x01\x00\x49')[:-1]]:
            with self.subTest(data=original), self.assertRaises(ValueError):
                assets.cap_memory(original)

    def test_wrong_archive_digest_creates_no_destination(self):
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'assets'
            with self.assertRaisesRegex(ValueError, 'digest mismatch'):
                assets.prepare(b'not a reviewed archive', destination)
            self.assertFalse(destination.exists())

    def test_leb_round_trips_boundaries_and_rejects_overflow(self):
        for value in [0, 1, 73, 127, 128, 4096, 65536, 2**32-1]:
            encoded = assets.encode(value)
            self.assertEqual(assets.uleb(encoded, 0), (value, len(encoded)))
        for data in [b'', b'\x80', b'\xff'*5, b'\xff\xff\xff\xff\x1f']:
            with self.assertRaises(ValueError):
                assets.uleb(data, 0)


if __name__ == '__main__':
    unittest.main()
