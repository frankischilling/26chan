#!/usr/bin/env python3
"""Recovery checks on an owned Linux host; uses real services, mounts and VMs."""
import contextlib
import os
import pathlib
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock
import uuid

import test_vm

JOBS = pathlib.Path('/run/26chan-media-jobs')
RUNNER = test_vm.REPO / 'scripts/media/run-job.py'


def descriptor_targets(process):
    targets = []
    for fd in (process / 'fd').iterdir():
        try:
            targets.append(os.readlink(fd))
        except FileNotFoundError:
            continue
    return targets


class RecoveryTest(unittest.TestCase):
    def setUp(self):
        self.assertEqual(os.geteuid(), 0, 'requires an owned disposable Linux host')
        self.vm = test_vm.VmTest()
        self.vm.assert_clean()
        JOBS.mkdir(mode=0o700, exist_ok=True)

    def recover(self):
        return subprocess.run([sys.executable, str(RUNNER), '--reconcile'],
                              capture_output=True, text=True, timeout=30)

    def assert_start_blocked(self, source, destination):
        before = sorted(p.name for p in JOBS.iterdir())
        result = subprocess.run([sys.executable, str(RUNNER), os.environ['MEDIA_VM_PROBE_CONFIG'],
                                 str(source), str(destination)], capture_output=True, timeout=30)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(destination.exists(), 'uncertain state still allowed output collection')
        self.assertEqual(sorted(p.name for p in JOBS.iterdir()), before)

    @contextlib.contextmanager
    def launch_client(self, runner, unit):
        children = pathlib.Path(f'/proc/{runner.pid}/task/{runner.pid}/children').read_text().split()
        self.assertEqual(len(children), 1)
        monitor = pathlib.Path('/proc') / children[0]
        self.assertEqual((monitor / 'comm').read_text().strip(), 'timeout')
        self.assertEqual(os.getpgid(int(monitor.name)), int(monitor.name))
        self.assertIn(str(JOBS / 'runner.lock'), descriptor_targets(monitor))
        children = (monitor / 'task' / monitor.name / 'children').read_text().split()
        clients = []
        for pid in children:
            process = pathlib.Path('/proc') / pid
            if (process / 'comm').read_text().strip() == 'systemd-run':
                self.assertIn(('--unit=' + unit).encode(), (process / 'cmdline').read_bytes().split(b'\0'))
                clients.append(process)
        self.assertEqual(len(clients), 1, 'expected the actual owned launch client')
        client = clients[0]
        descriptors = [os.pidfd_open(int(process.name)) for process in (monitor, client)]
        try:
            self.assertIn(str(JOBS / 'runner.lock'), descriptor_targets(client),
                          'launch client must retain exclusion')
            yield descriptors
        finally:
            for descriptor in descriptors:
                os.close(descriptor)

    def wait_for_exit(self, descriptor, seconds):
        poller = select.poll()
        poller.register(descriptor, select.POLLIN)
        self.assertTrue(poller.poll(seconds * 1000), 'owned launch client did not exit by its deadline')

    def test_sigkill_is_recovered_before_the_next_vm(self):
        with self.vm.sleeping_vm() as (runner, unit, workspace, disk, vmm):
            with self.launch_client(runner, unit) as client:
                self.assertNotIn(str(JOBS / 'runner.lock'), descriptor_targets(vmm),
                                 'the VMM must not inherit the coordinator lock')
                runner.kill()
                runner.wait(timeout=5)
                self.assertEqual(runner.returncode, -signal.SIGKILL)
                self.assertTrue(vmm.exists(), 'the test must leave a live VM for recovery')
                self.assertTrue(os.path.ismount(workspace))
                self.assertFalse(disk.exists())
                self.assertNotEqual(self.recover().returncode, 0, 'pending launch still owns exclusion')
                self.assert_start_blocked(disk.parent / 'input', disk)
                # The actual external service deadline drains the surviving client.
                for descriptor in client:
                    self.wait_for_exit(descriptor, 22)
                self.vm.run_probe(b'disk')
                self.assertFalse(vmm.exists())
                self.assertFalse(workspace.exists())
                self.assertFalse(disk.exists(), 'recovery must never collect an abandoned result')
                self.vm.assert_clean()

    def test_explicit_recovery_is_idempotent_after_sigkill(self):
        with self.vm.sleeping_vm() as (runner, unit, workspace, disk, vmm):
            with self.launch_client(runner, unit) as client:
                runner.kill()
                runner.wait(timeout=5)
                # The service is already proven live. Kill only this verified
                # client's pidfd too, leaving a live VM with no launch owner.
                for descriptor in client:
                    signal.pidfd_send_signal(descriptor, signal.SIGKILL)
                    self.wait_for_exit(descriptor, 5)
                self.assertTrue(vmm.exists())
                for _ in range(2):
                    result = self.recover()
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertFalse(vmm.exists())
                    self.assertFalse(workspace.exists())
                    self.assertFalse(disk.exists())
                    self.vm.assert_clean()

    def test_live_runner_excludes_recovery(self):
        with self.vm.sleeping_vm() as (runner, _, workspace, _, vmm):
            result = self.recover()
            self.assertNotEqual(result.returncode, 0)
            self.assertIsNone(runner.poll())
            self.assertTrue(vmm.exists())
            self.assertTrue(os.path.ismount(workspace))

    def test_prelaunch_and_stopped_workspaces_are_removed(self):
        empty = JOBS / (uuid.uuid4().hex + '-fixture0')
        mounted = JOBS / (uuid.uuid4().hex + '-fixture0')
        empty.mkdir(mode=0o700)
        mounted.mkdir(mode=0o700)
        try:
            subprocess.run(['mount', '-t', 'tmpfs', '-o', 'size=1M,nosuid,mode=0700',
                            'tmpfs', str(mounted)], check=True, timeout=5)
            (mounted / 'discarded-output').write_bytes(b'untrusted abandoned bytes')
            result = self.recover()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(empty.exists())
            self.assertFalse(mounted.exists())
            self.vm.assert_clean()
        finally:
            if os.path.ismount(mounted):
                subprocess.run(['umount', str(mounted)], check=True, timeout=5)
            for root in (empty, mounted):
                if root.exists():
                    root.rmdir()

    def test_lock_symlink_cannot_modify_an_outside_file(self):
        with tempfile.TemporaryDirectory(prefix='26chan-lock-recovery-witness-', dir='/run') as name:
            witness = pathlib.Path(name)
            saved_lock = witness / 'saved-lock'
            marker = witness / 'marker'
            marker.write_bytes(b'outside the job namespace')
            lock = JOBS / 'runner.lock'
            if not lock.exists():
                lock.touch(mode=0o600)
            lock.rename(saved_lock)
            try:
                lock.symlink_to(marker)
                self.assertNotEqual(self.recover().returncode, 0)
                self.assertTrue(lock.is_symlink())
                self.assertEqual(marker.read_bytes(), b'outside the job namespace')
            finally:
                lock.unlink()
                saved_lock.rename(lock)

    def test_launch_deadline_survives_parent_sigkill_before_any_service(self):
        # Exercise the actual external monitor with a harmless stalled child,
        # before any systemd service or its RuntimeMaxSec deadline exists.
        code = (f'import signal, sys; sys.path.insert(0, {str(RUNNER.parent)!r})\n'
                'from job_lifecycle import locked_jobs, run_service\n'
                'signal.signal(signal.SIGTERM, lambda *_: sys.exit(1))\n'
                'with locked_jobs() as lock:\n'
                '    run_service(["/usr/bin/sleep", "60"], lock, deadline=2)\n')
        parent = subprocess.Popen([sys.executable, '-c', code], stdout=subprocess.DEVNULL,
                                  stderr=subprocess.PIPE, text=True)
        monitors = []
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                self.assertIsNone(parent.poll(), 'external launch monitor is unavailable')
                children = pathlib.Path(f'/proc/{parent.pid}/task/{parent.pid}/children').read_text().split()
                if children:
                    monitor = pathlib.Path('/proc') / children[0]
                    if (monitor / 'comm').read_text().strip() == 'timeout':
                        grandchildren = (monitor / 'task' / monitor.name / 'children').read_text().split()
                        if grandchildren:
                            child = pathlib.Path('/proc') / grandchildren[0]
                            # fork exposes the child before exec changes its name.
                            if (child / 'comm').read_text().strip() == 'sleep':
                                for process in (monitor, child):
                                    monitors.append(os.pidfd_open(int(process.name)))
                                self.assertIn(str(JOBS / 'runner.lock'), descriptor_targets(monitor))
                                break
                time.sleep(0.01)
            self.assertEqual(len(monitors), 2, 'external monitor and owned stalled child must start')
            parent.kill()
            parent.wait(timeout=5)
            self.assertNotEqual(self.recover().returncode, 0)
            for descriptor in monitors:
                self.wait_for_exit(descriptor, 6)
            result = self.recover()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.vm.assert_clean()
        finally:
            try:
                if parent.poll() is None:
                    # The fixture's handler unwinds run_service and drains its group.
                    parent.terminate()
                parent.communicate(timeout=8)
                for descriptor in monitors:
                    self.wait_for_exit(descriptor, 6)
            finally:
                for descriptor in monitors:
                    os.close(descriptor)

    def test_launch_deadline_fixture_drains_after_failed_assertion(self):
        with mock.patch(__name__ + '.descriptor_targets', return_value=[]):
            with self.assertRaisesRegex(AssertionError, 'runner.lock'):
                self.test_launch_deadline_survives_parent_sigkill_before_any_service()
        result = self.recover()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.vm.assert_clean()

    def test_launch_deadline_waits_for_child_exec(self):
        read_text = pathlib.Path.read_text
        observed_pre_exec = False

        def read_during_exec(path, *args, **kwargs):
            nonlocal observed_pre_exec
            value = read_text(path, *args, **kwargs)
            if path.name == 'comm' and value.strip() == 'sleep' and not observed_pre_exec:
                # A forked timeout child retains its parent's name until exec.
                observed_pre_exec = True
                return 'timeout\n'
            return value

        with mock.patch.object(pathlib.Path, 'read_text', new=read_during_exec):
            self.test_launch_deadline_survives_parent_sigkill_before_any_service()
        self.assertTrue(observed_pre_exec, 'the controlled pre-exec observation must occur')

    def test_uncertain_storage_is_retained_and_blocks_new_jobs(self):
        with tempfile.TemporaryDirectory(prefix='26chan-recovery-witness-') as name:
            witness = pathlib.Path(name)
            marker = witness / 'untouched'
            marker.write_bytes(b'owned recovery witness')
            source = witness / 'input'
            source.write_bytes(b'disk')
            destination = witness / 'result.disk'
            for kind in ('symlink', 'file', 'unknown-name', 'open-permissions', 'nonempty'):
                with self.subTest(kind=kind):
                    entry = JOBS / (uuid.uuid4().hex + '-fixture0' if kind != 'unknown-name' else 'unknown')
                    try:
                        if kind == 'symlink':
                            entry.symlink_to(witness, target_is_directory=True)
                        elif kind == 'file':
                            entry.write_bytes(b'unknown file')
                        else:
                            entry.mkdir(mode=0o755 if kind == 'open-permissions' else 0o700)
                            if kind == 'nonempty':
                                (entry / 'retain').write_bytes(b'uncertain storage')
                        result = self.recover()
                        self.assertNotEqual(result.returncode, 0)
                        self.assertTrue(os.path.lexists(entry))
                        self.assertEqual(marker.read_bytes(), b'owned recovery witness')
                        self.assert_start_blocked(source, destination)
                    finally:
                        if entry.is_symlink() or entry.is_file():
                            entry.unlink()
                        elif entry.exists():
                            if (entry / 'retain').exists():
                                (entry / 'retain').unlink()
                            entry.rmdir()
                        if destination.exists():
                            destination.unlink()

    def test_nested_mount_is_retained(self):
        root = JOBS / (uuid.uuid4().hex + '-fixture0')
        root.mkdir(mode=0o700)
        child = root / 'nested'
        try:
            subprocess.run(['mount', '-t', 'tmpfs', '-o', 'size=1M,nosuid,mode=0700',
                            'tmpfs', str(root)], check=True, timeout=5)
            child.mkdir(mode=0o700)
            subprocess.run(['mount', '-t', 'tmpfs', '-o', 'size=16K,nosuid,mode=0700',
                            'tmpfs', str(child)], check=True, timeout=5)
            result = self.recover()
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue(os.path.ismount(root))
            self.assertTrue(os.path.ismount(child))
        finally:
            if os.path.ismount(child):
                subprocess.run(['umount', str(child)], check=True, timeout=5)
            if os.path.ismount(root):
                subprocess.run(['umount', str(root)], check=True, timeout=5)
            root.rmdir()

    def test_unexpected_service_policy_is_retained(self):
        unit = '26chan-media-' + uuid.uuid4().hex + '.service'
        try:
            subprocess.run(['systemd-run', '--quiet', '--collect', '--unit=' + unit,
                            '--service-type=exec', '--property=RuntimeMaxSec=15s',
                            '--property=KillMode=process', '/usr/bin/sleep', '20'],
                           check=True, timeout=5)
            result = self.recover()
            self.assertNotEqual(result.returncode, 0)
            state = subprocess.run(['systemctl', 'is-active', unit], capture_output=True, text=True)
            self.assertEqual(state.stdout.strip(), 'active')
        finally:
            subprocess.run(['systemctl', 'stop', unit], stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, check=True, timeout=10)


if __name__ == '__main__':
    unittest.main()
