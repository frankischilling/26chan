"""Opaque SWF framing and disk limits, without invoking a movie parser."""
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/media'))
import dispatch_protocol as protocol
spec = importlib.util.spec_from_file_location('swf_runner', ROOT / 'scripts/media/run-job.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SwfTransportTest(unittest.TestCase):
    def test_explicit_version_and_exact_limits(self):
        for size in [8, 10 * 1024 * 1024]:
            self.assertEqual(protocol.request_kind(b'IBJOB008' + size.to_bytes(8, 'big')), ('swf-v8', size))
        for size in [0, 7, 10 * 1024 * 1024 + 1, 2**64 - 1]:
            with self.assertRaises(ValueError):
                protocol.request_kind(b'IBJOB008' + size.to_bytes(8, 'big'))
        self.assertEqual(protocol.output_bytes('swf-v8'), 11 * 1024 * 1024)

    def test_length_prefix_sector_padding_and_opaque_contents(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for size in [8, 504, 505, 512, 10 * 1024 * 1024]:
                source, target = root / 'source', root / 'disk'
                source.write_bytes(b'x' * size)
                runner.input_disk(source, target, input_kind='swf-v8')
                wire = target.read_bytes()
                self.assertEqual(wire[:8], size.to_bytes(8, 'big'))
                self.assertEqual(wire[8:8+size], b'x' * size)
                self.assertFalse(any(wire[8+size:]))
                self.assertEqual(len(wire) % 512, 0)
                target.unlink()
            for size in [7, 10 * 1024 * 1024 + 1]:
                source.write_bytes(b'x' * size)
                with self.assertRaises(ValueError):
                    runner.input_disk(source, target, input_kind='swf-v8')
                target.unlink()


if __name__ == '__main__':
    unittest.main()
