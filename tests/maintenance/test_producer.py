"""Portable state checks plus explicitly Linux-only real recorder operations."""

import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
PRODUCER = ROOT / 'scripts/maintenance/run.py'


def load_state(test):
    path = ROOT / 'scripts/maintenance/state.py'
    test.assertTrue(path.is_file(), 'Maintenance state transitions are not implemented')
    spec = importlib.util.spec_from_file_location('maintenance_state_test', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_producer():
    spec = importlib.util.spec_from_file_location('maintenance_producer_test', PRODUCER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FinalPublicationTests(unittest.TestCase):
    def test_termination_during_success_publication_republishes_failure(self):
        producer = load_producer()
        for signum in (signal.SIGTERM, signal.SIGINT):
            stop = producer.StopRequest()
            running = producer.state.start_attempt('application', None, 1000)
            published = []

            def publish(value):
                published.append(value)
                if value['outcome'] == 'success':
                    stop.receive(signum, None)

            with mock.patch.object(producer.signal, 'pthread_sigmask', create=True), \
                    mock.patch.object(producer.signal, 'SIG_BLOCK', 0, create=True), \
                    mock.patch.object(producer.signal, 'sigpending', return_value=set(), create=True), \
                    mock.patch.object(producer.time, 'time_ns', return_value=1100_000_000):
                result = producer.finalize(mock.Mock(publish=publish), running, True, stop)
            self.assertEqual(result, 128 + signum)
            self.assertEqual([value['outcome'] for value in published], ['success', 'failure'])
            self.assertTrue(published[-1]['failure_pending'])
            self.assertIsNone(published[-1]['last_success_ms'])

    def test_pending_signal_and_commit_point_have_coherent_outcomes(self):
        producer = load_producer()
        running = producer.state.start_attempt('application', None, 1000)
        for pending, outcomes, result in (([set(), {signal.SIGTERM}], ['success', 'failure'], 143),
                                           ([{signal.SIGINT}, set()], ['failure'], 130),
                                           ([set(), set()], ['success'], 0)):
            published = []
            with mock.patch.object(producer.signal, 'pthread_sigmask', create=True) as mask, \
                    mock.patch.object(producer.signal, 'SIG_BLOCK', 0, create=True), \
                    mock.patch.object(producer.signal, 'sigpending', side_effect=pending, create=True), \
                    mock.patch.object(producer.time, 'time_ns', return_value=1100_000_000):
                actual = producer.finalize(mock.Mock(publish=published.append), running, True, producer.StopRequest())
            self.assertEqual(actual, result)
            self.assertEqual([value['outcome'] for value in published], outcomes)
            # The CLI must keep these signals blocked from the commit point
            # through ExitStack teardown, diagnostics, and process exit.
            self.assertEqual(mask.call_count, 1)


class HarnessCleanupTests(unittest.TestCase):
    def test_normal_cleanup_requests_term_before_waiting_and_never_kills(self):
        process = mock.Mock(stdout=None, stderr=None)
        process.poll.return_value = None
        NativeProducerTests.stop(process)
        self.assertEqual(process.method_calls, [mock.call.poll(), mock.call.terminate(), mock.call.wait(timeout=5)])


class StateTests(unittest.TestCase):
    def test_configuration_is_strict_bounded_and_uses_canonical_absolute_paths(self):
        state = load_state(self)
        valid = {'target': 'application', 'state_directory': '/var/lib/owned',
                 'command': ['/usr/bin/true', 'literal argument'], 'timeout_seconds': 2}
        self.assertEqual(state.parse_config(json.dumps(valid).encode()), valid)
        for key, value in [('target', 'other'), ('timeout_seconds', True), ('timeout_seconds', 0),
                           ('timeout_seconds', 3601), ('state_directory', 'relative'),
                           ('state_directory', '/a/../b'), ('state_directory', '/a//b'),
                           ('command', []), ('command', ['/bin/true'] * 33), ('command', ['relative']),
                           ('command', ['/bin/true', '']), ('command', ['/bin/true', 'x' * 1025]),
                           ('command', ['/bin/true', 'é' * 513]), ('command', ['/bin/true', 'a\0b'])]:
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                state.parse_config(json.dumps({**valid, key: value}).encode())
        duplicate = json.dumps(valid).replace('"target": "application"', '"target":"application","target":"host"').encode()
        for raw in (duplicate, b'[]', b'x' * 16385,
                    json.dumps({**valid, 'unknown': 1}).encode(), b'\xff'):
            with self.assertRaises(ValueError):
                state.parse_config(raw)

    def test_failure_latches_through_retry_and_only_completed_success_clears_it(self):
        state = load_state(self)
        running = state.start_attempt('application', None, 1000)
        self.assertIsNone(running['finished_ms'])
        self.assertFalse(running['failure_pending'])
        success = state.finish_attempt(running, True, 1100)
        failed = state.finish_attempt(state.start_attempt('application', success, 1200), False, 1300)
        self.assertEqual(failed['last_success_ms'], 1100)
        self.assertTrue(failed['failure_pending'])
        retry = state.start_attempt('application', failed, 1400)
        self.assertTrue(retry['failure_pending'])
        self.assertEqual(retry['last_success_ms'], 1100)
        restored = state.finish_attempt(retry, True, 1500)
        self.assertFalse(restored['failure_pending'])
        self.assertEqual(restored['last_success_ms'], 1500)
        self.assertEqual(state.parse_journal(json.dumps(restored).encode(), 'application'), restored)

    def test_abandoned_running_attempt_latches_failure_and_rollback_is_rejected(self):
        state = load_state(self)
        running = state.start_attempt('host', None, 1000)
        retry = state.start_attempt('host', running, 1100)
        self.assertTrue(retry['failure_pending'])
        complete = state.finish_attempt(retry, False, 1200)
        for operation in (lambda: state.start_attempt('host', complete, 1199),
                          lambda: state.start_attempt('host', running, 999),
                          lambda: state.finish_attempt(running, True, 999),
                          lambda: state.finish_attempt(complete, True, 1300)):
            with self.assertRaises(ValueError):
                operation()

    def test_invalid_existing_journal_is_never_reset(self):
        state = load_state(self)
        valid = state.finish_attempt(state.start_attempt('monitoring', None, 1000), True, 1100)
        for key, value in [('schema', True), ('target', 'host'), ('started_ms', 0), ('started_ms', 2**53),
                           ('finished_ms', None), ('finished_ms', 999), ('last_success_ms', 1001),
                           ('outcome', 'other'), ('failure_pending', True)]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                state.parse_journal(json.dumps({**valid, key: value}).encode(), 'monitoring')
        for prior in ({}, {**valid, 'unknown': 1}, {**valid, 'failure_pending': True}):
            with self.assertRaises(ValueError):
                state.start_attempt('monitoring', prior, 1200)
        duplicate = json.dumps(valid).replace('"schema": 1', '"schema":1,"schema":1').encode()
        for raw in (b'x' * 4097, duplicate, b'null'):
            with self.assertRaises(ValueError):
                state.parse_journal(raw, 'monitoring')


@unittest.skipUnless(sys.platform.startswith('linux'), 'Actual recorder commands and native filesystem semantics require Linux')
class NativeProducerTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue(PRODUCER.is_file(), 'Maintenance producer CLI is not implemented')
        temporary = tempfile.TemporaryDirectory(prefix='maintenance-producer-test-')
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.root.chmod(0o700)
        self.directory = self.root / 'state'
        self.directory.mkdir(mode=0o755)
        self.config = self.root / 'config.json'
        self.payload = self.root / 'update.py'
        self.input = self.root / 'input'
        self.marker = self.root / 'version'
        self.input.write_text('owned-version-2')
        self.payload.write_text('''import os, pathlib, sys, time
mode, root = sys.argv[1], pathlib.Path(sys.argv[2])
assert os.getcwd() == '/'
assert set(os.environ) <= {'PATH', 'LANG', 'LC_CTYPE'}
if mode == 'update':
    expected = (root / 'input').read_bytes()
    (root / 'version.new').write_bytes(expected)
    (root / 'version.new').replace(root / 'version')
    assert (root / 'version').read_bytes() == expected
elif mode == 'wait':
    (root / 'ready').write_text(str(os.getpid()))
    time.sleep(30)
elif mode == 'fail':
    raise SystemExit(7)
''')
        self.payload.chmod(0o700)
        self.python = Path(sys.executable).resolve()

    def configure(self, mode='update', timeout=2, command=None):
        value = {'target': 'application', 'state_directory': str(self.directory),
                 'command': command or [str(self.python), str(self.payload), mode, str(self.root)],
                 'timeout_seconds': timeout}
        self.config.write_text(json.dumps(value))
        self.config.chmod(0o600)

    def start(self, wrapper=None):
        arguments = [str(PRODUCER), str(self.config)] if wrapper is None else ['-c', wrapper, str(PRODUCER), str(self.config)]
        process = subprocess.Popen([str(self.python), '-I', *arguments],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   env={'PATH': '/usr/bin:/bin', 'LANG': 'C', 'UNRELATED_SECRET': 'must-not-escape'})
        self.addCleanup(self.stop, process)
        return process

    @staticmethod
    def stop(process):
        try:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    NativeProducerTests.force_stop(process)
        finally:
            if process.stdout:
                process.stdout.close()
            if process.stderr:
                process.stderr.close()

    @staticmethod
    def force_stop(process):
        # Freeze our unreaped recorder before discovering its direct command.
        # It can neither spawn nor reap while stopped, so each child leader's
        # PID/group stays pinned until we finish signalling that group. Never
        # reconstruct ownership from a stale ready file or signal an orphan's
        # numeric PID after killing/reaping the recorder.
        descriptors = []
        try:
            process.send_signal(signal.SIGSTOP)
            deadline = time.monotonic() + 5
            while True:
                observed = os.waitid(os.P_PID, process.pid,
                                     os.WSTOPPED | os.WEXITED | os.WNOHANG | os.WNOWAIT)
                if observed is not None:
                    if observed.si_code != os.CLD_STOPPED:
                        process.wait(timeout=5)
                        return
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError('Owned recorder could not be stopped for cleanup')
                time.sleep(0.01)
            children = Path('/proc', str(process.pid), 'task', str(process.pid), 'children').read_text().split()
            for child in children:
                pid = int(child)
                descriptor = os.pidfd_open(pid)
                descriptors.append(descriptor)
                if os.getpgid(pid) == pid:
                    os.killpg(pid, signal.SIGKILL)
                else:
                    # The recorder may have been frozen during Popen before
                    # the command entered its separate session.
                    signal.pidfd_send_signal(descriptor, signal.SIGKILL)
            # Give the recorder a bounded chance to reap its terminated command
            # and publish failure. If it is stuck elsewhere, every command group
            # has already been signalled while its leader was safely pinned.
            process.send_signal(signal.SIGCONT)
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
        finally:
            # If safe ownership acquisition failed, let the recorder handle
            # the already queued TERM instead of leaving it stopped or issuing
            # an unowned numeric group kill.
            if process.poll() is None:
                process.send_signal(signal.SIGCONT)
            for descriptor in descriptors:
                os.close(descriptor)

    def run_update(self):
        process = self.start()
        out, err = process.communicate(timeout=8)
        self.assertNotIn(b'must-not-escape', out + err)
        self.assertNotIn(str(self.root).encode(), out + err)
        return process.returncode

    def journal(self):
        return json.loads((self.directory / 'application.json').read_text())

    def wait_running(self):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if (self.root / 'ready').exists():
                return int((self.root / 'ready').read_text())
            time.sleep(0.02)
        self.fail('Actual owned update did not begin')

    def test_signal_during_actual_success_fsync_and_after_commit(self):
        wrapper = '''import importlib.util, os, pathlib, signal, sys, time
spec = importlib.util.spec_from_file_location('owned_recorder', sys.argv[1])
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)
sys.argv = [sys.argv[1], sys.argv[2]]
root = pathlib.Path(sys.argv[1]).parent
original_publish, original_fsync = recorder.Journal.publish, os.fsync
inject = False
def fsync(fd):
    global inject
    original_fsync(fd)
    if inject:
        inject = False
        (root / 'publication-ready').write_text('ready')
        deadline = time.monotonic() + 5
        while not (root / 'publication-release').exists():
            if time.monotonic() >= deadline:
                raise RuntimeError('Publication control timed out')
            time.sleep(0.01)
def publish(self, value):
    global inject
    inject = value['outcome'] == 'success'
    return original_publish(self, value)
recorder.Journal.publish, os.fsync = publish, fsync
raise SystemExit(recorder.main())
'''
        for signum in (signal.SIGTERM, signal.SIGINT):
            self.configure()
            ready = self.root / 'publication-ready'
            release = self.root / 'publication-release'
            ready.unlink(missing_ok=True)
            release.unlink(missing_ok=True)
            process = self.start(wrapper)
            deadline = time.monotonic() + 5
            while not ready.exists() and time.monotonic() < deadline:
                time.sleep(0.01)
            self.assertTrue(ready.exists(), 'Actual success publication did not reach fsync')
            process.send_signal(signum)
            release.write_text('continue')
            process.communicate(timeout=8)
            self.assertEqual(process.returncode, 128 + signum)
            self.assertEqual(self.journal()['outcome'], 'failure')
            self.assertTrue(self.journal()['failure_pending'])
        # Once the final pending-signal check commits success, a later signal
        # stays blocked through record() teardown and main()'s status output.
        self.configure()
        after_commit = '''import importlib.util, os, signal, sys
spec = importlib.util.spec_from_file_location('owned_recorder', sys.argv[1])
recorder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(recorder)
sys.argv = [sys.argv[1], sys.argv[2]]
original = recorder.finalize
def finalize(*args):
    result = original(*args)
    os.kill(os.getpid(), signal.SIGTERM)
    return result
recorder.finalize = finalize
raise SystemExit(recorder.main())
'''
        process = self.start(after_commit)
        process.communicate(timeout=8)
        self.assertEqual(process.returncode, 0)
        self.assertEqual(self.journal()['outcome'], 'success')

    def test_cleanup_stops_command_before_forced_recorder_death(self):
        import select
        for force in (False, True):
            self.configure('wait', 30)
            (self.root / 'ready').unlink(missing_ok=True)
            process = self.start()
            child = self.wait_running()
            descriptor = os.pidfd_open(child)
            try:
                if force:
                    process.send_signal(signal.SIGSTOP)
                    os.waitid(os.P_PID, process.pid, os.WSTOPPED | os.WNOWAIT)
                self.stop(process)
                self.assertTrue(select.select([descriptor], [], [], 5)[0], 'Owned command survived cleanup')
                self.assertIsNotNone(process.returncode)
            finally:
                self.stop_pidfd(descriptor)
                os.close(descriptor)

    def test_real_update_missing_input_failure_and_recovery(self):
        self.configure()
        self.assertEqual(self.run_update(), 0)
        self.assertEqual(self.marker.read_bytes(), self.input.read_bytes())
        success = self.journal()['last_success_ms']
        self.input.unlink()
        self.assertNotEqual(self.run_update(), 0)
        self.assertEqual(self.journal()['last_success_ms'], success)
        self.assertTrue(self.journal()['failure_pending'])
        self.input.write_text('owned-version-3')
        self.assertEqual(self.run_update(), 0)
        self.assertFalse(self.journal()['failure_pending'])
        self.assertEqual(self.marker.read_bytes(), self.input.read_bytes())
        self.assertEqual((self.directory / 'application.json').stat().st_mode & 0o777, 0o644)

    def test_nonzero_timeout_spawn_failure_and_sigterm_record_failure(self):
        for mode in ('fail', 'wait'):
            self.configure(mode, 1)
            self.assertNotEqual(self.run_update(), 0)
            self.assertEqual(self.journal()['outcome'], 'failure')
        self.configure(command=[str(self.payload)])  # No shebang: actual exec failure.
        self.assertNotEqual(self.run_update(), 0)
        self.assertTrue(self.journal()['failure_pending'])
        for signum in (signal.SIGTERM, signal.SIGINT):
            (self.root / 'ready').unlink(missing_ok=True)
            self.configure('wait', 30)
            process = self.start()
            child_pid = self.wait_running()
            process.send_signal(signum)
            process.communicate(timeout=8)
            self.assertNotEqual(process.returncode, 0)
            self.assertEqual(self.journal()['outcome'], 'failure')
            self.assertFalse(Path('/proc/' + str(child_pid)).exists())

    def test_exclusive_lock_and_restart_after_sigkill_latches_abandonment(self):
        self.configure('wait', 30)
        first = self.start()
        child_pid = self.wait_running()
        descriptor = os.pidfd_open(child_pid)
        self.addCleanup(os.close, descriptor)
        self.addCleanup(self.stop_pidfd, descriptor)
        self.assertNotEqual(self.run_update(), 0)
        self.assertEqual(self.journal()['outcome'], 'running')
        first.kill()
        first.wait(timeout=5)
        # This handle was captured while the recorder still owned the child;
        # signalling it cannot address an unrelated reused process identifier.
        signal.pidfd_send_signal(descriptor, signal.SIGTERM)
        (self.root / 'ready').unlink()
        second = self.start()
        self.wait_running()
        self.assertTrue(self.journal()['failure_pending'])
        second.send_signal(signal.SIGTERM)
        second.communicate(timeout=8)
        self.assertEqual(self.journal()['outcome'], 'failure')
        self.configure()
        self.assertEqual(self.run_update(), 0)
        self.assertFalse(self.journal()['failure_pending'])

    @staticmethod
    def stop_pidfd(descriptor):
        try:
            signal.pidfd_send_signal(descriptor, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def test_inherited_sigchld_ignore_does_not_discard_command_ownership(self):
        self.configure()
        previous = signal.signal(signal.SIGCHLD, signal.SIG_IGN)
        try:
            process = self.start()
        finally:
            signal.signal(signal.SIGCHLD, previous)
        process.communicate(timeout=8)
        self.assertEqual(process.returncode, 0)
        self.assertEqual(self.marker.read_bytes(), self.input.read_bytes())

    def test_ancestor_symlink_fifo_and_fixed_lock_symlink_are_rejected(self):
        self.configure()
        parent_link = self.root / 'linked-state'
        parent_link.symlink_to(self.directory, target_is_directory=True)
        value = json.loads(self.config.read_text())
        value['state_directory'] = str(parent_link)
        self.config.write_text(json.dumps(value))
        self.assertNotEqual(self.run_update(), 0)
        self.configure()
        lock = self.directory / 'application.lock'
        lock.symlink_to(self.input)
        self.assertNotEqual(self.run_update(), 0)
        self.assertEqual(self.input.read_text(), 'owned-version-2')
        lock.unlink()
        self.config.unlink()
        os.mkfifo(self.config, 0o600)
        self.assertNotEqual(self.run_update(), 0)

    def test_symlinks_writable_sources_and_malformed_prior_state_are_rejected(self):
        self.configure()
        self.assertEqual(self.run_update(), 0)
        journal = self.directory / 'application.json'
        journal.write_text('{malformed')
        self.assertNotEqual(self.run_update(), 0)
        self.assertEqual(journal.read_text(), '{malformed')
        journal.unlink()
        self.config.chmod(0o666)
        self.assertNotEqual(self.run_update(), 0)
        self.config.chmod(0o600)
        original = self.root / 'original.json'
        self.config.rename(original)
        self.config.symlink_to(original)
        self.assertNotEqual(self.run_update(), 0)
        self.config.unlink()
        original.rename(self.config)
        self.directory.chmod(0o777)
        self.assertNotEqual(self.run_update(), 0)
        self.directory.chmod(0o755)
        journal.symlink_to(self.input)
        self.assertNotEqual(self.run_update(), 0)
        self.assertEqual(self.input.read_text(), 'owned-version-2')


if __name__ == '__main__':
    unittest.main()
