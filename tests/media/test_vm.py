#!/usr/bin/env python3
"""Owned Linux VM smoke test; requires explicit reviewed artifacts and root."""
import os
import contextlib
import json
import pathlib
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import threading
import time
import unittest
import zlib

from owned_process import run_owned

REPO = pathlib.Path(__file__).resolve().parents[2]


def check_probe_report(data, expected_checks):
    if not 1 <= expected_checks <= 32 or len(data) != 4_194_816 or data[:8] != b'IBRGBA01':
        raise ValueError('invalid probe report framing')
    width, height = struct.unpack('>II', data[8:16])
    if (width, height) != (expected_checks, 1):
        raise ValueError('probe did not report every requested check')
    for index in range(width):
        if data[16 + index * 4:20 + index * 4] != b'\0\xff\0\xff':
            raise ValueError(f'probe check {index} failed')
    if any(data[16 + width * 4:]):
        raise ValueError('probe report has trailing data')


def red_png():
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' +
            chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 2, 0, 0, 0)) +
            chunk(b'IDAT', zlib.compress(b'\0\xff\0\0')) + chunk(b'IEND', b''))


class VmTest(unittest.TestCase):
    def test_stopped_output_passes_real_rust_validation_before_private_promotion(self):
        validator = os.environ['MEDIA_VM_VALIDATOR']
        with tempfile.TemporaryDirectory(prefix='vm-pipeline-', dir=os.environ.get('MEDIA_VM_TEST_TEMP')) as name:
            root = pathlib.Path(name)
            private = root / 'private'
            private.mkdir()
            source = private / 'input.png'
            source.write_bytes(red_png())
            disk = private / 'result.disk'
            subprocess.run([sys.executable, str(REPO / 'scripts/media/run-job.py'), os.environ['MEDIA_VM_TEST_CONFIG'],
                            str(source), str(disk)], check=True, timeout=30)
            self.assert_clean()

            def validate(destination):
                paths = [str(disk), str(destination)]
                if validator.endswith('.exe'):
                    paths = [subprocess.check_output(['wslpath', '-w', path], text=True).strip() for path in paths]
                return subprocess.run([validator, *paths], capture_output=True, text=True, timeout=10)

            approved = root / 'approved'
            result = validate(approved)
            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = json.loads(result.stdout)
            self.assertEqual((receipt['width'], receipt['height']), (1, 1))
            self.assertEqual(len(list(approved.glob('*.png'))), 1)
            with disk.open('r+b') as stream:
                stream.seek(20)
                stream.write(b'\x01')
            result = validate(root / 'rejected')
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / 'rejected').exists())

    def assert_clean(self):
        jobs = pathlib.Path('/run/26chan-media-jobs')
        if jobs.exists():
            self.assertEqual(sorted(p.name for p in jobs.iterdir()), ['runner.lock'])
        result = subprocess.run(['systemctl', 'list-units', '--all', '--no-legend',
                                 '26chan-media-*.service'], capture_output=True, text=True, check=True)
        self.assertEqual(result.stdout.strip(), '')

    def run_probe(self, payload, success=True, valid=True, expected_checks=1):
        with tempfile.TemporaryDirectory(prefix='26chan-probe-test-') as name:
            root = pathlib.Path(name)
            source = root / 'input'
            source.write_bytes(payload)
            started = time.monotonic()
            result = run_owned(
                [sys.executable, str(REPO / 'scripts/media/run-job.py'), os.environ['MEDIA_VM_PROBE_CONFIG'],
                 str(source), str(root / 'result.disk')], stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                text=True, timeout=35)
            elapsed = time.monotonic() - started
            self.assert_clean()
            if not success:
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((root / 'result.disk').exists())
                return elapsed
            self.assertEqual(result.returncode, 0, result.stderr)
            data = (root / 'result.disk').read_bytes()
            if not valid:
                self.assertEqual(data, bytes(4_194_816))
                return elapsed
            check_probe_report(data, expected_checks)
            return elapsed

    def test_decodes_one_input_and_removes_job(self):
        self.assertEqual(os.geteuid(), 0, 'run only on an owned disposable Linux host as root')
        config = os.environ['MEDIA_VM_TEST_CONFIG']
        with tempfile.TemporaryDirectory(prefix='26chan-vm-test-') as name:
            root = pathlib.Path(name)
            source = root / 'input.png'
            source.write_bytes(red_png())
            result = subprocess.run(
                [sys.executable, str(REPO / 'scripts/media/run-job.py'), config, str(source), str(root / 'result.disk')],
                capture_output=True, text=True, timeout=45,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            output = (root / 'result.disk').read_bytes()
            self.assertEqual(len(output), 4_194_816)
            self.assertEqual(output[:20], b'IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff')
            self.assertEqual(output[20:], bytes(4_194_796))
            self.assert_clean()

    def test_jpeg_variants_decode_under_the_same_guest_boundary(self):
        self.assertEqual(os.geteuid(), 0, 'owned disposable Linux root required')
        config = os.environ['MEDIA_VM_TEST_CONFIG']
        for name in ('baseline', 'progressive', 'grayscale', 'cmyk'):
            with self.subTest(format=name), tempfile.TemporaryDirectory(prefix='26chan-jpeg-vm-') as name_root:
                root = pathlib.Path(name_root)
                source = root / 'input.jpg'
                source.write_bytes((REPO / 'tests/media/fixtures/jpeg' / f'{name}.jpg').read_bytes())
                result = subprocess.run(
                    [sys.executable, str(REPO / 'scripts/media/run-job.py'), config, str(source), str(root / 'result.disk')],
                    capture_output=True, text=True, timeout=45)
                self.assert_clean()
                self.assertEqual(result.returncode, 0, result.stderr)
                output = (root / 'result.disk').read_bytes()
                self.assertEqual(len(output), 4_194_816)
                self.assertEqual(output[:16], b'IBRGBA01\0\0\0\x01\0\0\0\x01')
                expected = (80, 80, 80) if name == 'grayscale' else (255, 0, 0)
                self.assertTrue(all(abs(actual - target) <= 3 for actual, target in zip(output[16:19], expected)))
                self.assertEqual(output[19], 255)
                self.assertEqual(output[20:], bytes(4_194_796))

    @contextlib.contextmanager
    def sleeping_vm(self):
        self.assert_clean()
        with tempfile.TemporaryDirectory(prefix='26chan-cancel-test-') as name:
            source = pathlib.Path(name) / 'input'
            source.write_bytes(b'sleep')
            destination = pathlib.Path(name) / 'result.disk'
            runner = subprocess.Popen(
                [sys.executable, str(REPO / 'scripts/media/run-job.py'),
                 os.environ['MEDIA_VM_PROBE_CONFIG'], str(source), str(destination)],
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
            workspace = None
            unit = None
            try:
                deadline = time.monotonic() + 8
                while time.monotonic() < deadline:
                    self.assertIsNone(runner.poll(), 'runner exited before VM became ready')
                    try:
                        roots = [p for p in pathlib.Path('/run/26chan-media-jobs').iterdir() if p.is_dir()]
                    except FileNotFoundError:
                        # The first runner creates this root after artifact validation.
                        roots = []
                    if roots:
                        self.assertEqual(len(roots), 1)
                        workspace = roots[0]
                        unit = '26chan-media-' + workspace.name.split('-')[0] + '.service'
                        for process in pathlib.Path('/proc').glob('[0-9]*'):
                            try:
                                matches = ((process / 'comm').read_text().strip() == 'firecracker'
                                           and unit in (process / 'cgroup').read_text())
                            except FileNotFoundError:
                                continue
                            if matches:
                                # The sleep probe remains alive; allow PID1 to launch it.
                                time.sleep(0.3)
                                yield runner, unit, workspace, destination, process
                                return
                    time.sleep(0.05)
                self.fail('sleep VM did not start within eight seconds')
            finally:
                if runner.poll() is None:
                    runner.terminate()
                runner.communicate(timeout=12)
                # Regression RED must also clean only the exact resources it owns.
                if unit is not None:
                    stopped = subprocess.run(['systemctl', 'stop', unit], stdout=subprocess.DEVNULL,
                                             stderr=subprocess.DEVNULL, timeout=10)
                    self.assertIn(stopped.returncode, (0, 5))  # already collected is also clean
                if workspace is not None and workspace.exists():
                    if os.path.ismount(workspace):
                        subprocess.run(['umount', str(workspace)], check=True, timeout=5)
                    workspace.rmdir()

    def test_catchable_cancellation_cleans_service_and_workspace(self):
        for first_signal in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(first_signal=first_signal), self.sleeping_vm() as (runner, unit, workspace, disk, vmm):
                runner.send_signal(first_signal)
                time.sleep(0.02)
                if runner.poll() is None:
                    runner.send_signal(signal.SIGTERM)
                    runner.send_signal(signal.SIGINT)
                runner.wait(timeout=12)
                _, diagnostics = runner.communicate(timeout=2)
                self.assertNotEqual(runner.returncode, 0)
                self.assertFalse(disk.exists())
                self.assertFalse(workspace.exists(), 'cancelled runner left reusable workspace: ' + diagnostics[:4096])
                self.assertFalse(vmm.exists(), 'cancelled runner left a live VMM')
                self.assert_clean()
        self.run_probe(b'disk')

    def test_live_vmm_has_effective_host_resource_limits(self):
        with self.sleeping_vm() as (_, unit, _, _, vmm):
            groups = {}
            for line in (vmm / 'cgroup').read_text().splitlines():
                _, controllers, path = line.split(':', 2)
                for controller in controllers.split(','):
                    groups[controller] = path
            if 'memory' in groups:
                mounts = {}
                for line in pathlib.Path('/proc/mounts').read_text().splitlines():
                    _, mount, kind, options, *_ = line.split()
                    if kind == 'cgroup':
                        for controller in options.split(','):
                            mounts[controller] = pathlib.Path(mount)
                for controller in ('memory', 'cpu', 'pids'):
                    self.assertEqual(groups[controller], '/system.slice/' + unit)
                memory = mounts['memory'] / groups['memory'].lstrip('/')
                cpu = mounts['cpu'] / groups['cpu'].lstrip('/')
                pids = mounts['pids'] / groups['pids'].lstrip('/')
                self.assertEqual((memory / 'memory.limit_in_bytes').read_text().strip(), '268435456')
                self.assertEqual((memory / 'memory.memsw.limit_in_bytes').read_text().strip(), '268435456')
                self.assertEqual((memory / 'memory.swappiness').read_text().strip(), '0')
                self.assertEqual((cpu / 'cpu.cfs_quota_us').read_text(), (cpu / 'cpu.cfs_period_us').read_text())
            else:
                self.assertEqual(groups[''], '/system.slice/' + unit)
                memory = cpu = pids = pathlib.Path('/sys/fs/cgroup') / groups[''].lstrip('/')
                self.assertEqual((memory / 'memory.max').read_text().strip(), '268435456')
                self.assertEqual((memory / 'memory.swap.max').read_text().strip(), '0')
                quota, period = (cpu / 'cpu.max').read_text().split()
                self.assertEqual(quota, period)
            self.assertEqual((pids / 'pids.max').read_text().strip(), '32')

    def test_worker_identity_files_credentials_and_network(self):
        # A real owned TCP service responds before and after guest denial.
        with socket.socket() as listener:
            listener.bind(('127.0.0.1', 0))
            listener.listen()
            listener.settimeout(0.2)
            stopped = threading.Event()

            def serve():
                while not stopped.is_set():
                    try:
                        connection, _ = listener.accept()
                    except TimeoutError:
                        continue
                    with connection:
                        connection.sendall(b'healthy')

            thread = threading.Thread(target=serve)
            thread.start()

            def healthy():
                with socket.create_connection(listener.getsockname(), timeout=1) as client:
                    self.assertEqual(client.recv(7), b'healthy')
                subprocess.run(['pg_isready', '-h', '127.0.0.1', '-p', '55432'],
                               check=True, stdout=subprocess.DEVNULL)

            try:
                healthy()
                self.run_probe(f'inspect\n127.0.0.1:{listener.getsockname()[1]}\n127.0.0.1:55432\n'.encode(),
                               expected_checks=11)
                healthy()
            finally:
                stopped.set()
                thread.join(timeout=2)

    def test_guest_memory_process_and_disk_limits(self):
        for mode in ('memory', 'process', 'disk'):
            with self.subTest(mode=mode):
                self.run_probe(mode.encode())

    def test_wall_clock_limit_terminates_vm_and_removes_workspace(self):
        elapsed = self.run_probe(b'sleep', success=False)
        self.assertGreaterEqual(elapsed, 14)
        self.assertLess(elapsed, 25)

    def test_cpu_limit_stops_worker_before_its_own_deadline(self):
        elapsed = self.run_probe(b'cpu', valid=False)
        self.assertGreaterEqual(elapsed, 4)
        self.assertLess(elapsed, 8)


if __name__ == '__main__':
    unittest.main()
