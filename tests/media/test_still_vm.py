"""Synthetic PNGs exercise the explicit still profile on an owned VM runner."""
import os
import pathlib
import struct
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zlib
from owned_process import run_owned
from test_vm import check_probe_report

REPO = pathlib.Path(__file__).resolve().parents[2]
OUTPUT_BYTES = 16 * 1024 * 1024


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def png(width, height):
    compressor = zlib.compressobj()
    compressed = bytearray()
    row = b'\0' + bytes([80, 40, 20]) * width
    for _ in range(height):
        compressed.extend(compressor.compress(row))
    compressed.extend(compressor.flush())
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))
            + chunk(b'tEXt', b'Comment\0SYNTHETIC-METADATA')
            + chunk(b'IDAT', bytes(compressed)) + chunk(b'IEND', b''))


class StillVmTest(unittest.TestCase):
    input_kind = "still-v6"
    def run_job(self, source, *, probe=False, measure=False, expect_failure=False):
        with tempfile.TemporaryDirectory(prefix='owned-still-vm-') as directory:
            root = pathlib.Path(directory)
            source_path, output = root / 'input', root / 'output'
            source_path.write_bytes(source)
            stop = threading.Event()
            peaks = []
            def observe():
                roots = [pathlib.Path('/sys/fs/cgroup'), pathlib.Path('/sys/fs/cgroup/memory')]
                while not stop.wait(0.02):
                    for mount in roots:
                        for unit in (mount / 'system.slice').glob('26chan-media-*.service'):
                            for field in ('memory.peak', 'memory.max_usage_in_bytes'):
                                try:
                                    peaks.append(int((unit / field).read_text().strip()))
                                except FileNotFoundError:
                                    pass
            observer = threading.Thread(target=observe, daemon=True) if measure else None
            if observer:
                observer.start()
            try:
                result = run_owned([sys.executable, str(REPO / 'scripts/media/run-job.py'),
                                    '--input-kind', self.input_kind, '--owned-diagnostics',
                                    os.environ['MEDIA_VM_PROBE_CONFIG' if probe else 'MEDIA_VM_TEST_CONFIG'], str(source_path), str(output)],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=70)
            finally:
                stop.set()
                if observer:
                    observer.join(timeout=2)
            if measure:
                self.assertTrue(peaks, 'no live VMM memory usage observed')
                self.assertGreater(max(peaks), 250 * 1024 * 1024)
                self.assertLessEqual(max(peaks), 2560 * 1024 * 1024)
                print(f'Still-image VMM observed cgroup peak: {max(peaks)} bytes', flush=True)
            if expect_failure:
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists())
            else:
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors='replace'))
            self.assertEqual(sorted(path.name for path in pathlib.Path('/run/26chan-media-jobs').iterdir()), ['runner.lock'])
            if expect_failure:
                return None
            data = output.read_bytes()
            self.assertEqual(len(data), OUTPUT_BYTES)
            return data

    def test_lossless_jpeg_cleanup_and_explicit_retention(self):
        metadata = b'Exif\0\0SYNTHETIC-PRIVATE-METADATA'
        for name in ('baseline', 'progressive', 'grayscale', 'cmyk'):
            with self.subTest(name=name):
                original = (REPO / 'tests/media/fixtures/jpeg' / f'{name}.jpg').read_bytes()
                tagged = original[:2] + b'\xff\xe1' + struct.pack('>H', len(metadata) + 2) + metadata + original[2:]
                for strip in (False, True):
                    policy = b'IBSTIL06' + bytes([4 | int(strip), 2]) + struct.pack('>HH', 250, 250) + bytes(2)
                    output = self.run_job(policy + tagged)
                    self.assertEqual(output[:8], b'IBIMG006')
                    self.assertEqual(output[8:24], policy)
                    length = struct.unpack('>I', output[48:52])[0]
                    retained = output[64:64 + length]
                    self.assertTrue(retained.startswith(b'\xff\xd8\xff'))
                    self.assertTrue(retained.endswith(b'\xff\xd9'))
                    if strip:
                        self.assertNotIn(metadata, retained)
                    else:
                        self.assertEqual(retained, tagged)

    def test_live_still_vmm_has_only_the_approved_larger_memory_budget(self):
        from test_vm import VmTest
        VmTest().check_live_vmm_limits('still-v6', 2560 * 1024 * 1024)

    def test_still_profile_preserves_worker_boundaries(self):
        for mode, checks in [('inspect', 9), ('limits', 4), ('memory', 1), ('process', 1), ('disk', 1)]:
            with self.subTest(mode=mode):
                output = self.run_job(mode.encode().ljust(17, b' '), probe=True)
                check_probe_report(output[:4_194_816], checks)
                self.assertFalse(any(output[4_194_816:]))

    def test_wall_deadline_removes_the_sleeping_worker(self):
        started = time.monotonic()
        self.run_job(b'sleep'.ljust(17, b' '), probe=True, expect_failure=True)
        elapsed = time.monotonic() - started
        self.assertGreaterEqual(elapsed, 40)
        self.assertLess(elapsed, 70)
        output = self.run_job(b'inspect'.ljust(17, b' '), probe=True)
        check_probe_report(output[:4_194_816], 9)

    def test_cancellation_removes_live_vm_and_partial_output(self):
        from test_vm import VmTest
        vm = VmTest()
        for signum in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signal=signum), vm.sleeping_vm('still-v6') as (runner, unit, workspace, disk, vmm):
                runner.send_signal(signum)
                runner.communicate(timeout=12)
                self.assertNotEqual(runner.returncode, 0)
                self.assertFalse(disk.exists())
                self.assertFalse(workspace.exists())
                remaining = vmm.exists()
                details = []
                if remaining:
                    for field in ('comm', 'status', 'cgroup'):
                        try:
                            details.append(field + ': ' + (vmm / field).read_text()[:4096])
                        except (FileNotFoundError, ProcessLookupError):
                            details.append(field + ': exited during inspection')
                self.assertFalse(remaining, 'Cancelled VMM proc entry remained: ' + '\n'.join(details))
                vm.assert_clean()
        output = self.run_job(b'inspect'.ljust(17, b' '), probe=True)
        check_probe_report(output[:4_194_816], 9)

    @staticmethod
    def policy():
        return b'IBSTIL06' + bytes([7, 1]) + struct.pack('>HH', 250, 250) + bytes(2)

    def test_large_png_cleanup_geometry_and_recovery(self):
        for width, height in [(17, 9), (2048, 1024), (10_000, 10_000)]:
            with self.subTest(width=width, height=height):
                input_image = png(width, height)
                output = self.run_job(self.policy() + input_image, measure=width == 10_000)
                self.assertEqual(output[:8], b'IBIMG006')
                self.assertEqual(output[8:24], self.policy())
                self.assertEqual(struct.unpack('>II', output[24:32]), (width, height))
                ratio = min(1, 250 / width, 250 / height)
                self.assertEqual(struct.unpack('>II', output[32:40]), (int(width * ratio), int(height * ratio)))
                retained, thumbnail, mobile = struct.unpack('>III', output[48:60])
                image = output[64:64 + retained]
                self.assertTrue(image.startswith(b'\x89PNG\r\n\x1a\n'))
                self.assertNotIn(b'SYNTHETIC-METADATA', image)
                self.assertEqual(retained, len(input_image) - len(chunk(b'tEXt', b'Comment\0SYNTHETIC-METADATA')))
                thumb = output[64 + retained:64 + retained + thumbnail]
                self.assertTrue(thumb.startswith(b'\xff\xd8\xff'))
                self.assertTrue(thumb.endswith(b'\xff\xd9'))
                self.assertFalse(any(output[60:64]))
                self.assertFalse(any(output[64 + retained + thumbnail + mobile:]))
        for malformed in [b'invalid', png(8, 8)[:-1], png(10_001, 1)]:
            self.assertEqual(self.run_job(self.policy() + malformed), bytes(OUTPUT_BYTES))
        self.assertEqual(self.run_job(self.policy() + png(8, 8))[:8], b'IBIMG006')


if __name__ == '__main__':
    if os.geteuid() != 0 or 'MEDIA_VM_TEST_CONFIG' not in os.environ:
        raise SystemExit('requires an owned root VM qualification environment')
    unittest.main()
