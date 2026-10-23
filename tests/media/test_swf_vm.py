"""Owned SWF bytes through the real disposable guest, without movie playback."""
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib

from owned_process import run_owned
import test_vm

ROOT = Path(__file__).resolve().parents[2]
OUTPUT_BYTES = 11 * 1024 * 1024


class SwfVmTest(unittest.TestCase):
    def run_job(self, data):
        with tempfile.TemporaryDirectory(prefix='owned-swf-vm-') as directory:
            root = Path(directory)
            source, output = root / 'input', root / 'output'
            source.write_bytes(data)
            result = run_owned([sys.executable, str(ROOT / 'scripts/media/run-job.py'),
                                '--input-kind', 'swf-v8', os.environ['MEDIA_VM_TEST_CONFIG'],
                                str(source), str(output)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=35)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            test_vm.VmTest().assert_clean()
            disk = output.read_bytes()
            self.assertEqual(len(disk), OUTPUT_BYTES)
            return disk

    def test_owned_compressed_and_uncompressed_movies_retain_exact_bytes(self):
        cases = json.loads((ROOT / 'fixtures/swf-header-reference.json').read_text())['cases']
        for case in cases:
            with self.subTest(name=case['name'], signature=case['signature']):
                data = bytes.fromhex(case['bytes_hex'])
                disk = self.run_job(data)
                if case['name'] in ('zero', 'reversed'):
                    self.assertEqual(disk, bytes(OUTPUT_BYTES))
                    continue
                self.assertEqual(disk[:8], b'IBSWF008')
                geometry = (40, 20) if case['name'] == 'negative_origin' else (case['php']['0'], case['php']['1'])
                self.assertEqual(struct.unpack('>II', disk[8:16]), geometry)
                self.assertEqual(int.from_bytes(disk[16:24], 'big'), len(data))
                self.assertEqual(disk[24:26], bytes([10, case['signature'] == 'CWS']))
                self.assertEqual(disk[32:32+len(data)], data)
                self.assertFalse(any(disk[30:32] + disk[32+len(data):]))

    def test_invalid_stream_and_expansion_bomb_leave_no_partial_output(self):
        cases = json.loads((ROOT / 'fixtures/swf-header-reference.json').read_text())['cases']
        valid = bytes.fromhex(cases[0]['bytes_hex'])
        bomb = b'CWS\x0a' + struct.pack('<I', 32) + zlib.compress(bytes(1024 * 1024))
        for invalid in [b'notaswf!', valid[:-1], valid + b'\x00', bomb]:
            self.assertEqual(self.run_job(invalid), bytes(OUTPUT_BYTES))
        self.assertEqual(self.run_job(valid)[:8], b'IBSWF008')


if __name__ == '__main__':
    unittest.main()
