#!/usr/bin/env python3
"""Resource-gate fixtures and strict probe-report checks; VM tests run separately."""
import importlib.util
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

import test_vm
from owned_process import cancel_test, run_owned

REPO = pathlib.Path(__file__).resolve().parents[2]


class ResourceGateTest(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('limits', REPO / 'scripts/media/verify-cgroups.py')
        self.gate = importlib.util.module_from_spec(spec)
        # Missing gate is an assertion failure until the enforcement exists.
        self.assertTrue(pathlib.Path(spec.origin).exists(), 'pre-exec resource gate is missing')
        spec.loader.exec_module(self.gate)
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.unit = '26chan-media-' + 'a' * 32 + '.service'
        self.group = '/system.slice/' + self.unit

    def fixture(self, version):
        if version == 1:
            values = {'memory': {'memory.limit_in_bytes': '268435456',
                                 'memory.memsw.limit_in_bytes': '9223372036854771712',
                                 'memory.swappiness': '60', 'memory.use_hierarchy': '1'},
                      'cpu': {'cpu.cfs_quota_us': '100000', 'cpu.cfs_period_us': '100000'},
                      'pids': {'pids.max': '32'}}
        else:
            values = {'': {'memory.max': '268435456', 'memory.swap.max': '0',
                           'cpu.max': '100000 100000', 'pids.max': '32'}}
        memberships, mounts = [], []
        for index, (controller, files) in enumerate(values.items(), 1):
            mount = self.root / (controller or 'unified')
            directory = mount / self.group.lstrip('/')
            directory.mkdir(parents=True)
            for filename, value in files.items():
                (directory / filename).write_text(value + '\n')
            memberships.append(f'{index if version == 1 else 0}:{controller}:{self.group}')
            mounts.append(f'{index} 0 0:{index} / {mount} rw - cgroup{2 if version == 2 else ""} cgroup rw{"," + controller if controller else ""}')
        return '\n'.join(memberships), '\n'.join(mounts)

    def enforce(self, fixture):
        groups = self.gate.resource_groups(self.unit, *fixture)
        self.gate.enforce_limits(groups)

    def test_v1_installs_combined_cap_before_allowing_execution(self):
        fixture = self.fixture(1)
        self.enforce(fixture)
        memory = self.root / 'memory' / self.group.lstrip('/')
        self.assertEqual((memory / 'memory.memsw.limit_in_bytes').read_text().strip(), '268435456')
        self.assertEqual((memory / 'memory.swappiness').read_text().strip(), '0')

    def test_missing_v1_swap_controller_fails_without_creating_a_regular_file(self):
        fixture = self.fixture(1)
        path = self.root / 'memory' / self.group.lstrip('/') / 'memory.memsw.limit_in_bytes'
        path.unlink()
        with self.assertRaises((ValueError, OSError)):
            self.enforce(fixture)
        self.assertFalse(path.exists())

    def test_v2_accepts_all_effective_controls(self):
        self.enforce(self.fixture(2))

    def test_weak_or_missing_controls_fail_closed_for_both_versions(self):
        for version, controller, filename, value in (
            (1, 'memory', 'memory.limit_in_bytes', '536870912'),
            (1, 'memory', 'memory.use_hierarchy', '0'),
            (1, 'cpu', 'cpu.cfs_quota_us', '-1'),
            (1, 'pids', 'pids.max', 'max'),
            (2, 'unified', 'memory.max', 'max'),
            (2, 'unified', 'memory.swap.max', '1024'),
            (2, 'unified', 'cpu.max', 'max 100000'),
            (2, 'unified', 'pids.max', None),
        ):
            with self.subTest(version=version, filename=filename):
                # Each case owns an independent hierarchy fixture.
                self.root = pathlib.Path(self.temp.name) / filename
                fixture = self.fixture(version)
                path = self.root / controller / self.group.lstrip('/') / filename
                if value is None:
                    path.unlink()
                else:
                    path.write_text(value)
                with self.assertRaises((ValueError, OSError)):
                    self.enforce(fixture)

    def test_wrong_unit_or_missing_controller_never_changes_other_cgroups(self):
        fixture = self.fixture(1)
        for membership in (fixture[0].replace(self.group, '/'),
                           '\n'.join(fixture[0].splitlines()[:-1])):
            with self.subTest(membership=membership), self.assertRaises(ValueError):
                self.enforce((membership, fixture[1]))
        path = self.root / 'memory' / self.group.lstrip('/') / 'memory.memsw.limit_in_bytes'
        self.assertEqual(path.read_text().strip(), '9223372036854771712')


class ProbeReportTest(unittest.TestCase):
    def report(self, count, colors=None):
        import struct
        pixels = colors if colors is not None else b'\0\xff\0\xff' * count
        report = b'IBRGBA01' + struct.pack('>II', count, 1) + pixels
        return report + bytes(4_194_816 - len(report))

    def test_every_requested_check_must_be_reported(self):
        test_vm.check_probe_report(self.report(2), 2)
        for reported in (0, 1, 3):
            with self.subTest(reported=reported), self.assertRaises(ValueError):
                test_vm.check_probe_report(self.report(reported), 2)

    def test_access_or_malformed_output_cannot_count_as_denial(self):
        report = self.report(2)
        for invalid in (self.report(2, b'\0\xff\0\xff\xff\0\0\xff'),
                        report[:-1], report + b'\0', report[:-1] + b'\1'):
            with self.subTest(size=len(invalid)), self.assertRaises(ValueError):
                test_vm.check_probe_report(invalid, 2)


class ChildCleanupTest(unittest.TestCase):
    def test_deadline_reaps_separate_session_child_and_runs_post_check(self):
        self.check_cleanup(False)

    def test_signal_reaps_separate_session_child_and_runs_post_check(self):
        self.check_cleanup(True)

    def check_cleanup(self, cancellation):
        with tempfile.TemporaryDirectory(prefix='26chan-owned-child-') as name:
            root = pathlib.Path(name)
            fixture = root / 'fixture.py'
            fixture.write_text('''import pathlib, signal, subprocess, sys, time
root = pathlib.Path(__file__).parent
if len(sys.argv) > 1:
    time.sleep(10)
    raise SystemExit(0)
def cancel(*args):
    (root / 'cancelled').touch()
    raise KeyboardInterrupt
signal.signal(signal.SIGTERM, cancel)
worker = subprocess.Popen([sys.executable, __file__, 'worker'], start_new_session=True)
(root / 'worker.pid').write_text(str(worker.pid))
(root / 'temporary').touch()
try:
    time.sleep(10)
finally:
    worker.terminate()
    worker.wait(timeout=3)
    (root / 'temporary').unlink()
    (root / 'cleaned').touch()
''')
            checked = []
            timer = None
            handlers = {}
            try:
                started = time.monotonic()
                if cancellation:
                    handlers = {sig: signal.signal(sig, cancel_test) for sig in (signal.SIGTERM, signal.SIGINT)}
                    timer = threading.Timer(1, lambda: os.kill(os.getpid(), signal.SIGTERM))
                    timer.start()
                expected = KeyboardInterrupt if cancellation else subprocess.TimeoutExpired
                with self.assertRaises(expected):
                    run_owned([sys.executable, str(fixture)], timeout=5 if cancellation else 1,
                              post_check=lambda: checked.append(True),
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                self.assertEqual(checked, [True])
                self.assertLess(time.monotonic() - started, 4)
                self.assertTrue((root / 'cancelled').exists(), 'child ran to its natural deadline')
                self.assertTrue((root / 'cleaned').exists(), 'deadline bypassed child cleanup')
                self.assertFalse((root / 'temporary').exists())
                self.assertFalse(pathlib.Path('/proc', (root / 'worker.pid').read_text()).exists())
            finally:
                if timer is not None:
                    timer.cancel()
                    timer.join(timeout=2)
                for sig, handler in handlers.items():
                    signal.signal(sig, handler)
                # RED cleanup identifies only the child created from this exact fixture.
                record = root / 'worker.pid'
                if record.exists():
                    pid = int(record.read_text())
                    command = pathlib.Path('/proc', str(pid), 'cmdline')
                    if command.exists() and os.fsencode(fixture) in command.read_bytes().split(b'\0'):
                        os.kill(pid, signal.SIGTERM)


class OperatorDiagnosticsTest(unittest.TestCase):
    def test_failure_location_never_includes_paths_or_exception_values(self):
        scripts = REPO / 'scripts/media'
        spec = importlib.util.spec_from_file_location('operator_runner', scripts / 'run-job.py')
        runner = importlib.util.module_from_spec(spec)
        sys.path.insert(0, str(scripts))
        try:
            spec.loader.exec_module(runner)
        finally:
            sys.path.pop(0)
        with tempfile.TemporaryDirectory() as name:
            source = pathlib.Path(name) / 'private-input'
            source.touch()
            try:
                runner.input_disk(source, pathlib.Path(name) / 'disk')
            except ValueError as error:
                diagnostic = runner.failure_location(error)
            else:
                self.fail('empty input should fail')
            self.assertRegex(diagnostic, r'^ValueError at run-job\.py:[0-9]+$')
            self.assertNotIn(name, diagnostic)
            self.assertNotIn('private-input', diagnostic)
        try:
            raise ValueError('synthetic-secret-marker')
        except ValueError as error:
            self.assertEqual(runner.failure_location(error), 'ValueError at operator boundary')


class ProcessDrainTest(unittest.TestCase):
    def test_transient_process_must_disappear_before_drain_succeeds(self):
        sys.path.insert(0, str(REPO / 'scripts/media'))
        try:
            import job_lifecycle as lifecycle
        finally:
            sys.path.pop(0)
        for duration, deadline, succeeds in ((0.15, 2, True), (5, 0.05, False)):
            with self.subTest(succeeds=succeeds), subprocess.Popen(
                    [sys.executable, '-c', f'import time; time.sleep({duration})']) as child:
                def still_present(units):
                    self.assertEqual(units, {'owned-fixture'})
                    if child.poll() is None:
                        raise lifecycle.JobProcessesRemain('owned process has not exited')
                try:
                    # The membership decision is substituted; lifetime/reaping is real.
                    with mock.patch.object(lifecycle, 'assert_no_processes', side_effect=still_present):
                        if succeeds:
                            lifecycle.wait_for_no_processes({'owned-fixture'}, timeout=deadline)
                            self.assertIsNotNone(child.poll())
                        else:
                            with self.assertRaises(lifecycle.JobProcessesRemain):
                                lifecycle.wait_for_no_processes({'owned-fixture'}, timeout=deadline)
                            self.assertIsNone(child.poll(), 'drain must not kill or hide the remaining process')
                finally:
                    if child.poll() is None:
                        child.terminate()
                    child.wait(timeout=3)


if __name__ == '__main__':
    unittest.main()
