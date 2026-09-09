#!/usr/bin/env python3
"""Resource-gate fixtures and strict probe-report checks; VM tests run separately."""
import importlib.util
import pathlib
import tempfile
import unittest

import test_vm

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


if __name__ == '__main__':
    unittest.main()
