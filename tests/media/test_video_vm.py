"""Owned video fixtures through the real explicit disposable guest."""
import os
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
from owned_process import run_owned
from test_vm import check_probe_report
from video_fixtures import generate

REPO = pathlib.Path(__file__).resolve().parents[2]
DISK_BYTES = 9 * 1024 * 1024


class VideoVmTest(unittest.TestCase):
    def run_job(self, data, *, probe=False):
        with tempfile.TemporaryDirectory(prefix='owned-video-vm-') as directory:
            root = pathlib.Path(directory)
            source, output = root / 'input', root / 'output'
            source.write_bytes(data)
            result = run_owned([sys.executable, str(REPO / 'scripts/media/run-job.py'),
                                '--input-kind', 'video-v5', '--owned-diagnostics',
                                os.environ['MEDIA_VM_PROBE_CONFIG' if probe else 'MEDIA_VM_TEST_CONFIG'],
                                str(source), str(output)], stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=35)
            self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            self.assertEqual(sorted(path.name for path in pathlib.Path('/run/26chan-media-jobs').iterdir()), ['runner.lock'])
            data = output.read_bytes()
            self.assertEqual(len(data), DISK_BYTES)
            return data

    def test_container_codecs_audio_preview_and_recovery(self):
        with tempfile.TemporaryDirectory(prefix='owned-video-clips-') as directory:
            generate(directory)
            for name, kind, audio, channel in [('red.webm', 1, 0, 0), ('blue.mp4', 2, 0, 2),
                                               ('red-opus.webm', 1, 1, 0), ('blue-aac.mp4', 2, 1, 2),
                                               ('red-vp9.webm', 1, 1, 0)]:
                with self.subTest(name=name):
                    source = pathlib.Path(directory, name).read_bytes()
                    output = self.run_job(bytes([kind]) + source)
                    self.assertEqual(output[:8], b'IBVID005')
                    self.assertEqual(output[8:10], bytes([kind, audio]))
                    self.assertEqual(struct.unpack('>II', output[16:24]), (64, 48))
                    self.assertTrue(400_000 <= int.from_bytes(output[24:32], 'big') <= 600_000)
                    self.assertEqual(struct.unpack('>II', output[40:48]), (64, 48))
                    length = int.from_bytes(output[48:56], 'big')
                    self.assertTrue(12 <= length <= 8 * 1024 * 1024)
                    self.assertFalse(any(output[10:16] + output[56:64]))
                    used = 64 + length + 64 * 48 * 4
                    for offset in range(64 + length, used, 4):
                        self.assertGreaterEqual(output[offset + channel], 240)
                        self.assertEqual(output[offset + 3], 255)
                    self.assertFalse(any(output[used:]))
            output = self.run_job(b'\x02' + pathlib.Path(directory, 'blue-rotation90.mp4').read_bytes())
            self.assertEqual(output[:8], b'IBVID005')
            self.assertEqual(struct.unpack('>II', output[16:24]), (64, 48))
            self.assertEqual(struct.unpack('>II', output[40:48]), (48, 64))
            for source in [b'\x01invalid', b'\x02invalid', b'\x03invalid']:
                self.assertEqual(self.run_job(source), bytes(DISK_BYTES))
            self.assertEqual(self.run_job(b'\x01' + pathlib.Path(directory, 'red.webm').read_bytes())[:8], b'IBVID005')

    def test_video_profile_preserves_worker_boundaries(self):
        for mode, count in [('inspect', 9), ('memory', 1), ('process', 1), ('disk', 1)]:
            with self.subTest(mode=mode):
                data = self.run_job(mode.encode(), probe=True)
                check_probe_report(data[:4_194_816], count)
                self.assertFalse(any(data[4_194_816:]))


if __name__ == '__main__':
    if os.geteuid() != 0 or 'MEDIA_VM_TEST_CONFIG' not in os.environ:
        raise SystemExit('requires an owned root VM qualification environment')
    unittest.main()
