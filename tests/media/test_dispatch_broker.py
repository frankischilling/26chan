#!/usr/bin/env python3
"""Fixed framing tests; root integration is explicitly requested by environment."""
import contextlib
import importlib.util
import os
import pathlib
import pwd
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

import test_vm

SCRIPTS = test_vm.REPO / 'scripts/media'
sys.path.insert(0, str(SCRIPTS))
import dispatch_protocol as protocol

spec = importlib.util.spec_from_file_location('dispatch_broker', SCRIPTS / 'dispatch-broker.py')
broker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(broker)


def frame(payload=b'x'):
    return b'IBJOB001' + len(payload).to_bytes(8, 'big') + payload


class FramingTest(unittest.TestCase):
    def test_bad_frames_never_finish_intake(self):
        cases = [b'BADMAGIC' + (1).to_bytes(8, 'big') + b'x', b'', b'IBJOB001',
                 b'IBJOB001' + (2).to_bytes(8, 'big') + b'x', frame() + b'x']
        cases += [b'IBJOB001' + n.to_bytes(8, 'big') for n in (0, 8_388_609, 2**64 - 1)]
        for data in cases:
            with self.subTest(data=data), tempfile.TemporaryDirectory() as name:
                left, right = socket.socketpair()
                with left, right:
                    right.sendall(data)
                    right.shutdown(socket.SHUT_WR)
                    with self.assertRaises((ValueError, OSError)):
                        protocol.receive_request(left, pathlib.Path(name) / 'input')

    def test_fragmented_valid_frame_requires_eof_and_preserves_bytes(self):
        with tempfile.TemporaryDirectory() as name:
            left, right = socket.socketpair()
            with left, right:
                def send():
                    for byte in frame(b'fragmented payload'):
                        right.sendall(bytes([byte]))
                    right.shutdown(socket.SHUT_WR)
                thread = threading.Thread(target=send)
                thread.start()
                target = pathlib.Path(name) / 'input'
                protocol.receive_request(left, target)
                thread.join(2)
                self.assertEqual(target.read_bytes(), b'fragmented payload')

    def test_missing_eof_and_slow_fragments_share_one_absolute_deadline(self):
        for data in (frame(), b'IBJOB'):
            with self.subTest(data=data), tempfile.TemporaryDirectory() as name:
                left, right = socket.socketpair()
                with left, right:
                    started = time.monotonic()
                    deadline = started + .2
                    right.sendall(data)
                    def trickle():
                        for _ in range(4):
                            time.sleep(.06)
                            try:
                                right.sendall(b'0')
                            except OSError:
                                return
                    thread = threading.Thread(target=trickle)
                    if data == b'IBJOB':
                        thread.start()
                    with self.assertRaises((ValueError, OSError)):
                        protocol.receive_request(left, pathlib.Path(name) / 'input', deadline=deadline)
                    self.assertLess(time.monotonic() - started, .4)
                    if thread.ident:
                        thread.join(1)

    def test_output_is_exact_regular_file_and_stream_ends(self):
        with tempfile.TemporaryDirectory() as name:
            target = pathlib.Path(name) / 'output'
            target.write_bytes(bytes(4_194_816))
            left, right = socket.socketpair()
            errors = []
            with left, right:
                def send():
                    try:
                        protocol.send_response(left, target)
                    except Exception as error:
                        errors.append(error)
                thread = threading.Thread(target=send)
                thread.start()
                result = bytearray()
                while data := right.recv(65536):
                    result.extend(data)
                thread.join(2)
                self.assertEqual(errors, [])
                self.assertEqual(result[:16], b'IBOUT001' + (4_194_816).to_bytes(8, 'big'))
                self.assertEqual(result[16:], bytes(4_194_816))

    def test_wrong_output_size_and_symlink_emit_nothing(self):
        with tempfile.TemporaryDirectory() as name:
            target = pathlib.Path(name) / 'output'
            for size in (0, 4_194_815, 4_194_817):
                target.write_bytes(bytes(size))
                left, right = socket.socketpair()
                with left, right:
                    with self.assertRaises((OSError, ValueError)):
                        protocol.send_response(left, target)
                    left.shutdown(socket.SHUT_WR)
                    self.assertEqual(right.recv(1), b'')
            target.unlink()
            target.symlink_to('/dev/zero')
            left, right = socket.socketpair()
            with left, right, self.assertRaises((OSError, ValueError)):
                protocol.send_response(left, target)

    def test_nonreading_client_hits_absolute_send_deadline(self):
        with tempfile.TemporaryDirectory() as name:
            target = pathlib.Path(name) / 'output'
            target.write_bytes(bytes(4_194_816))
            left, right = socket.socketpair()
            with left, right:
                started = time.monotonic()
                with self.assertRaises(TimeoutError):
                    protocol.send_response(left, target, deadline=started + .15)
                self.assertLess(time.monotonic() - started, .4)


CLIENT = '''import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(35)
s.connect(sys.argv[1])
try:
    s.sendall(sys.stdin.buffer.read())
    s.shutdown(socket.SHUT_WR)
    while True:
        data = s.recv(65536)
        if not data: break
        sys.stdout.buffer.write(data)
except (BrokenPipeError, ConnectionResetError):
    pass
finally:
    s.close()
'''


@unittest.skipUnless(os.environ.get('MEDIA_DISPATCH_ROOT_TESTS') == '1', 'explicit root integration only')
class RootBrokerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if os.geteuid() != 0:
            raise RuntimeError('explicit broker integration requires root')
        cls.config = os.environ['MEDIA_VM_TEST_CONFIG']
        cls.probe = os.environ['MEDIA_VM_PROBE_CONFIG']
        cls.gateway = pwd.getpwnam('board-media-gateway')
        cls.denied = pwd.getpwnam('nobody')
        cls.vmm = pwd.getpwnam('board-media-vmm')
        if (cls.gateway.pw_uid == 0 or cls.gateway.pw_gid == 0
                or cls.gateway.pw_shell != '/usr/sbin/nologin'
                or cls.gateway.pw_uid in (cls.denied.pw_uid, cls.vmm.pw_uid)):
            raise RuntimeError('owned gateway identity prerequisites failed')

    def setUp(self):
        self.vm = test_vm.VmTest()
        self.vm.assert_clean()
        self.temp = tempfile.TemporaryDirectory(prefix='dispatch-test-', dir='/run')
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.root.chmod(0o755)
        self.endpoint = self.root / 'broker'

    def client(self, payload, identity=None, wait=True):
        identity = identity or self.gateway
        process = subprocess.Popen([sys.executable, '-c', CLIENT, str(self.endpoint / 'broker.sock')],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   user=identity.pw_uid, group=self.gateway.pw_gid, extra_groups=[],
                                   env={'PATH': '/usr/bin:/bin'})
        if not wait:
            process.stdin.write(payload)
            process.stdin.close()
            process.stdin = None
            return process
        result, errors = process.communicate(payload, timeout=40)
        self.assertEqual(process.returncode, 0, errors.decode())
        return result

    @contextlib.contextmanager
    def running(self, config=None):
        socket_path = self.endpoint / 'broker.sock'
        old_inode = socket_path.stat().st_ino if socket_path.exists() else None
        process = subprocess.Popen([sys.executable, str(SCRIPTS / 'dispatch-broker.py'),
                                    config or self.config, str(self.endpoint), str(self.gateway.pw_uid)],
                                   env={'PATH': '/usr/sbin:/usr/bin:/sbin:/bin', 'APP_ENV': 'development'},
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            deadline = time.monotonic() + 8
            while (not socket_path.exists() or socket_path.stat().st_ino == old_inode
                   or socket_path.stat().st_mode & 0o777 != 0o660):
                self.assertIsNone(process.poll(), 'broker exited before binding')
                self.assertLess(time.monotonic(), deadline, 'broker did not bind')
                time.sleep(.02)
            yield process
        finally:
            if process.poll() is None:
                process.terminate()
            process.communicate(timeout=15)
            self.vm.assert_clean()

    def test_kernel_peer_uid_gate_precedes_allocation_and_executor(self):
        self.endpoint.mkdir(mode=0o750)
        os.chown(self.endpoint, 0, self.gateway.pw_gid)
        requests = self.endpoint / 'requests'
        requests.mkdir(mode=0o700)
        executed = []
        failures = []
        def execute(source, output):
            executed.append(source.read_bytes())
            output.write_bytes(bytes(4_194_816))
            output.chmod(0o600)
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(self.endpoint / 'broker.sock'))
            os.chown(self.endpoint / 'broker.sock', 0, self.gateway.pw_gid)
            os.chmod(self.endpoint / 'broker.sock', 0o660)
            listener.listen()
            def serve():
                for _ in range(3):
                    connection, _ = listener.accept()
                    with connection:
                        try:
                            broker.handle_connection(connection, self.gateway.pw_uid, requests, execute)
                        except (ValueError, OSError) as error:
                            failures.append(type(error))
            thread = threading.Thread(target=serve)
            thread.start()
            self.assertEqual(self.client(frame(), self.denied), b'')
            self.assertEqual(list(requests.iterdir()), [])
            self.assertEqual(executed, [])
            self.assertEqual(self.client(frame() + b'trailing'), b'')
            self.assertEqual(executed, [])
            self.assertEqual(self.client(frame(b'accepted'))[:16], b'IBOUT001' + (4_194_816).to_bytes(8, 'big'))
            thread.join(3)
            self.assertFalse(thread.is_alive())
            self.assertEqual(executed, [b'accepted'])
            self.assertEqual(len(failures), 2)
            self.assertEqual(list(requests.iterdir()), [])

    def test_real_decode_denied_uid_and_malformed_request(self):
        with self.running():
            self.assertEqual(self.client(frame(test_vm.red_png()), self.denied), b'')
            self.assertEqual(list((self.endpoint / 'requests').iterdir()), [])
            self.assertEqual(self.client(frame(b'sleep') + b'extra'), b'')
            self.vm.assert_clean()
            data = self.client(frame(test_vm.red_png()))
            self.vm.assert_clean()
            self.assertEqual(data[:36], b'IBOUT001' + (4_194_816).to_bytes(8, 'big')
                             + b'IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff')
            self.assertEqual(data[36:], bytes(4_194_796))
            self.assertEqual(list((self.endpoint / 'requests').iterdir()), [])

    def test_failed_cleanup_keeps_request_when_cancellation_is_reenabled(self):
        for signum in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signum=signum):
                self.endpoint = self.root / ('broker-' + str(signum))
                failed_cleanup = False
                original_reset = broker.reset_cancellation
                original_reconcile = broker.reconcile_jobs
                handlers = {number: signal.getsignal(number) for number in broker.SIGNALS}
                client_errors = []

                def controlled_run(config, source, destination):
                    self.assertEqual(source.read_bytes(), b'controlled input')
                    destination.write_bytes(b'controlled stopped-state uncertainty')
                    destination.chmod(0o600)
                    broker.runner.ignore_cancellation()
                    raise RuntimeError('controlled execution failure')

                def controlled_reconcile():
                    nonlocal failed_cleanup
                    if next((self.endpoint / 'requests').iterdir(), None) is None:
                        return original_reconcile()
                    failed_cleanup = True
                    raise RuntimeError('controlled cleanup failure')

                def reset_with_controlled_cancellation():
                    original_reset()
                    if failed_cleanup:
                        # Model cancellation at a handler re-enable boundary.
                        # No process receives a real signal and no VM starts.
                        broker.runner.cancel(signum, None)

                def client():
                    try:
                        deadline = time.monotonic() + 5
                        while not (self.endpoint / 'broker.sock').exists():
                            self.assertLess(time.monotonic(), deadline)
                            time.sleep(.01)
                        self.assertEqual(self.client(frame(b'controlled input')), b'')
                    except BaseException as error:
                        client_errors.append(error)

                thread = threading.Thread(target=client)
                thread.start()
                try:
                    with mock.patch.object(broker.runner, 'run', controlled_run), \
                            mock.patch.object(broker, 'reconcile_jobs', controlled_reconcile), \
                            mock.patch.object(broker, 'reset_cancellation', reset_with_controlled_cancellation):
                        with self.assertRaises((broker.RequestRetained, broker.runner.Cancelled)) as outcome:
                            broker.serve({}, self.endpoint, self.gateway)
                    self.assertTrue(failed_cleanup)
                    requests = list((self.endpoint / 'requests').iterdir())
                    self.assertEqual(len(requests), 1)
                    self.assertIsInstance(outcome.exception, broker.RequestRetained)
                    self.assertEqual((requests[0] / 'input').read_bytes(), b'controlled input')
                    self.assertEqual((requests[0] / 'output').read_bytes(),
                                     b'controlled stopped-state uncertainty')
                finally:
                    for number, handler in handlers.items():
                        signal.signal(number, handler)
                    thread.join(6)
                    for request in (self.endpoint / 'requests').iterdir():
                        broker.remove_request(request)
                self.assertFalse(thread.is_alive())
                self.assertEqual(client_errors, [])
                self.vm.assert_clean()

    def test_second_broker_cannot_change_live_socket_or_requests(self):
        with self.running():
            socket_path = self.endpoint / 'broker.sock'
            inode = socket_path.stat().st_ino
            result = subprocess.run([sys.executable, str(SCRIPTS / 'dispatch-broker.py'), self.config,
                                     str(self.endpoint), str(self.gateway.pw_uid)], capture_output=True,
                                    env={'PATH': '/usr/bin:/bin', 'APP_ENV': 'development'}, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(socket_path.stat().st_ino, inode)
            self.assertEqual(self.client(frame() + b'extra'), b'')

    def test_cancellation_during_live_vmm_cleans_before_request_removal(self):
        for signum in (signal.SIGTERM, signal.SIGINT):
            with self.subTest(signum=signum), self.running(self.probe) as process:
                client = self.client(frame(b'sleep'), wait=False)
                try:
                    deadline = time.monotonic() + 8
                    found = False
                    while time.monotonic() < deadline and not found:
                        for entry in pathlib.Path('/proc').glob('[0-9]*'):
                            try:
                                found = ((entry / 'comm').read_text().strip() == 'firecracker'
                                         and '26chan-media-' in (entry / 'cgroup').read_text())
                            except FileNotFoundError:
                                continue
                            if found:
                                break
                        time.sleep(.03)
                    self.assertTrue(found, 'test must cancel a live owned VMM')
                    self.assertEqual(len(list((self.endpoint / 'requests').iterdir())), 1)
                    process.send_signal(signum)
                    process.wait(timeout=12)
                    self.vm.assert_clean()
                    self.assertEqual(list((self.endpoint / 'requests').iterdir()), [])
                    self.assertEqual(client.communicate(timeout=5)[0], b'')
                finally:
                    if client.poll() is None:
                        client.terminate()
                    client.communicate(timeout=5)

    def test_sigkill_recovery_preserves_request_until_launch_lock_released(self):
        with self.running(self.probe) as process:
            client = self.client(frame(b'sleep'), wait=False)
            try:
                deadline = time.monotonic() + 8
                while True:
                    jobs = list(pathlib.Path('/run/26chan-media-jobs').glob('*-????????'))
                    live = []
                    for entry in pathlib.Path('/proc').glob('[0-9]*'):
                        try:
                            if ((entry / 'comm').read_text().strip() == 'firecracker'
                                    and '26chan-media-' in (entry / 'cgroup').read_text()):
                                live.append(entry)
                        except FileNotFoundError:
                            continue
                    if jobs and live:
                        break
                    self.assertLess(time.monotonic(), deadline, 'owned VMM did not start')
                    time.sleep(.02)
                retained = list((self.endpoint / 'requests').iterdir())
                self.assertEqual(len(retained), 1)
                process.kill()
                process.wait(timeout=3)
                retry = subprocess.run([sys.executable, str(SCRIPTS / 'dispatch-broker.py'), self.config,
                                        str(self.endpoint), str(self.gateway.pw_uid)], capture_output=True,
                                       env={'PATH': '/usr/bin:/bin', 'APP_ENV': 'development'}, timeout=5)
                self.assertNotEqual(retry.returncode, 0)
                self.assertTrue(retained[0].exists())
                self.assertTrue(jobs[0].exists())
                # Only the inherited runner lock establishes that its owned
                # launch client can no longer start a delayed VM.
                deadline = time.monotonic() + 35
                while True:
                    try:
                        with broker.locked_jobs():
                            break
                    except BlockingIOError:
                        self.assertLess(time.monotonic(), deadline)
                        time.sleep(.05)
                with self.running():
                    self.vm.assert_clean()
                    self.assertFalse(retained[0].exists())
                    self.assertEqual(list((self.endpoint / 'requests').iterdir()), [])
            finally:
                if client.poll() is None:
                    client.terminate()
                client.communicate(timeout=5)

    def test_recovery_accepts_only_verified_requests_and_stale_socket(self):
        with self.running():
            pass
        stale = self.endpoint / 'requests' / ('request-' + 'a' * 32)
        stale.mkdir(mode=0o700)
        (stale / 'input').touch(mode=0o600)
        (stale / 'output').touch(mode=0o600)
        # A SIGKILL-style stale pathname is accepted only under the held lock.
        with socket.socket(socket.AF_UNIX) as old:
            old.bind(str(self.endpoint / 'broker.sock'))
        with self.running():
            self.assertFalse(stale.exists())
        self.assertTrue((self.endpoint / 'broker.lock').is_file())
        for kind in ('unknown', 'symlink', 'owner', 'permissions', 'hardlink', 'mount'):
            with self.subTest(kind=kind):
                stale.mkdir(mode=0o700)
                entry = stale / ('unknown' if kind == 'unknown' else 'input')
                if kind == 'symlink':
                    entry.symlink_to('/dev/null')
                elif kind == 'mount':
                    subprocess.run(['mount', '-t', 'tmpfs', '-o', 'size=16K,mode=0700', 'tmpfs', str(stale)], check=True)
                else:
                    entry.touch(mode=0o600)
                    if kind == 'owner': os.chown(entry, self.gateway.pw_uid, self.gateway.pw_gid)
                    if kind == 'permissions': entry.chmod(0o644)
                    if kind == 'hardlink': os.link(entry, self.root / 'witness')
                try:
                    result = subprocess.run([sys.executable, str(SCRIPTS / 'dispatch-broker.py'), self.config,
                                             str(self.endpoint), str(self.gateway.pw_uid)], capture_output=True,
                                            env={'PATH': '/usr/bin:/bin', 'APP_ENV': 'development'}, timeout=10)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertTrue(stale.exists())
                    self.assertTrue(os.path.lexists(entry) or kind == 'mount')
                    self.assertFalse((self.endpoint / 'broker.sock').exists())
                finally:
                    if kind == 'mount': subprocess.run(['umount', str(stale)], check=True)
                    if os.path.lexists(entry): entry.unlink()
                    if (self.root / 'witness').exists(): (self.root / 'witness').unlink()
                    stale.rmdir()

    def test_startup_refuses_vmm_root_and_non_development_or_credentials(self):
        for uid, environment in ((0, {'APP_ENV': 'development'}),
                                 (self.vmm.pw_uid, {'APP_ENV': 'development'}),
                                 (self.gateway.pw_uid, {}),
                                 (self.gateway.pw_uid, {'APP_ENV': 'production'}),
                                 (self.gateway.pw_uid, {'APP_ENV': 'development', 'DATABASE_URL': 'test'}),
                                 (self.gateway.pw_uid, {'APP_ENV': 'development', 'PGPASSFILE': 'test'})):
            with self.subTest(uid=uid, environment=environment):
                result = subprocess.run([sys.executable, str(SCRIPTS / 'dispatch-broker.py'), self.config,
                                         str(self.endpoint), str(uid)], capture_output=True,
                                        env={'PATH': '/usr/bin:/bin', **environment}, timeout=10)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.endpoint.exists())


if __name__ == '__main__':
    unittest.main()
