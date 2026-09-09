#!/usr/bin/env python3
"""Live synthetic witnesses on an owned host; addresses exist only in a new netns."""
import contextlib
import json
import os
import pathlib
import re
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
import unittest

import test_vm
from owned_process import cancel_test, run_owned

DNS_QUERY = (b'\x01\x02\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00'
             b'\x07witness\x07invalid\x00\x00\x01\x00\x01')
DNS_RESPONSE = DNS_QUERY[:2] + b'\x81\x83' + DNS_QUERY[4:]
ADDRESSES = ('169.254.169.254', '192.0.2.10', '192.0.2.80', '203.0.113.10',
             '192.0.2.53', '2001:db8::10', '2001:db8::53')


@contextlib.contextmanager
def service(family, kind, address, response, expected=None):
    expected = response if expected is None else expected
    with socket.socket(family, kind) as listener:
        listener.bind(address)
        if kind == socket.SOCK_STREAM:
            listener.listen(8)
        listener.settimeout(0.1)
        stopped = threading.Event()
        errors = []

        def serve():
            try:
                while not stopped.is_set():
                    try:
                        if kind == socket.SOCK_DGRAM:
                            request, peer = listener.recvfrom(512)
                            if request == DNS_QUERY:
                                listener.sendto(response, peer)
                        else:
                            client, _ = listener.accept()
                            with client:
                                client.settimeout(0.5)
                                client.sendall(response)
                    except TimeoutError:
                        continue
            except OSError as error:
                errors.append(error)

        thread = threading.Thread(target=serve, daemon=True)
        # Let only the main thread receive cancellation. This keeps the main
        # thread's child-allocation mask effective while witnesses are serving.
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT})
        try:
            thread.start()
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)

        def healthy():
            if errors or not thread.is_alive():
                raise RuntimeError('witness service stopped')
            with socket.socket(family, kind) as client:
                client.settimeout(1)
                client.connect(address)
                if kind == socket.SOCK_DGRAM:
                    client.send(DNS_QUERY)
                    actual = client.recv(512)
                else:
                    actual = b''
                    while chunk := client.recv(512):
                        actual += chunk
                        if len(actual) > 1024:
                            raise RuntimeError('witness response exceeded bound')
                if actual != expected:
                    raise RuntimeError('witness response differs')

        try:
            yield healthy
        finally:
            stopped.set()
            thread.join(timeout=2)
            if thread.is_alive():
                raise RuntimeError('witness service did not stop')


class BoundaryTest(unittest.TestCase):
    def test_outer_deadline_cleans_a_live_vm_and_private_fixture(self):
        with tempfile.TemporaryDirectory(prefix='26chan-boundary-deadline-') as name:
            root = pathlib.Path(name)
            fixture = root / 'fixture.py'
            fixture.write_text('''import pathlib, signal, sys, tempfile, time
sys.path.insert(0, sys.argv[1])
import test_vm
from owned_process import cancel_test
signal.signal(signal.SIGTERM, cancel_test)
signal.signal(signal.SIGINT, cancel_test)
root = pathlib.Path(__file__).parent
try:
    with tempfile.TemporaryDirectory(dir=root) as temporary, test_vm.VmTest().sleeping_vm() as state:
        (pathlib.Path(temporary) / 'witness').touch()
        (root / 'live-vmm').write_text(state[4].name)
        (root / 'temporary').write_text(temporary)
        time.sleep(30)
finally:
    (root / 'unwound').touch()
''')
            vm = test_vm.VmTest()
            started = time.monotonic()
            with self.assertRaises(subprocess.TimeoutExpired):
                run_owned([sys.executable, str(fixture), str(pathlib.Path(__file__).parent)], timeout=8,
                          post_check=vm.assert_clean, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.assertLess(time.monotonic() - started, 20)
            self.assertTrue((root / 'unwound').exists())
            self.assertFalse(pathlib.Path((root / 'temporary').read_text()).exists())
            self.assertFalse(pathlib.Path('/proc', (root / 'live-vmm').read_text()).exists())

    def test_guest_denies_live_network_and_storage_witnesses(self):
        with tempfile.TemporaryDirectory(prefix='26chan-boundary-witness-') as name, contextlib.ExitStack() as stack:
            root = pathlib.Path(name)
            token = os.urandom(16).hex().encode()
            response = b'HTTP/1.0 200 OK\r\nContent-Length: 32\r\n\r\n' + token
            healthy, requests = [], []
            for address, port in (('169.254.169.254', 80), ('192.0.2.10', 8080),
                                  ('192.0.2.80', 3128), ('203.0.113.10', 443),
                                  ('2001:db8::10', 443)):
                family = socket.AF_INET6 if ':' in address else socket.AF_INET
                healthy.append(stack.enter_context(service(family, socket.SOCK_STREAM,
                                                           (address, port), response)))
                endpoint = f'[{address}]:{port}' if family == socket.AF_INET6 else f'{address}:{port}'
                requests.append('tcp\t' + endpoint)
            for address in ('192.0.2.53', '2001:db8::53'):
                family = socket.AF_INET6 if ':' in address else socket.AF_INET
                healthy.append(stack.enter_context(service(family, socket.SOCK_DGRAM,
                                                           (address, 53), DNS_RESPONSE)))
                endpoint = f'[{address}]:53' if family == socket.AF_INET6 else f'{address}:53'
                requests.append('udp\t' + endpoint)
            control_socket = str(root / 'management.sock')
            healthy.append(stack.enter_context(service(socket.AF_UNIX, socket.SOCK_STREAM,
                                                       control_socket, response)))
            requests.append('unix\t' + control_socket)
            files = [root / item for item in ('other-job-input', 'private-credentials', 'unapproved-output')]
            for path in files:
                path.write_bytes(token)
                path.chmod(0o600)
                requests.extend(('read\t' + str(path), 'write\t' + str(path)))

            def allowed_context():
                for check in healthy:
                    check()
                for path in files:
                    self.assertEqual(path.read_bytes(), token)
                    # Exercise existing-file write permission without changing bytes.
                    with path.open('r+b') as stream:
                        self.assertEqual(stream.read(), token)

            allowed_context()
            payload = ('boundaries\n' + '\n'.join(requests) + '\n').encode()
            self.assertLessEqual(len(payload), 4096)
            test_vm.VmTest().run_probe(payload, expected_checks=len(requests))
            allowed_context()
            print(f'{len(requests)} guest denials with live allowed-context controls before and after.', flush=True)

    def test_unhealthy_dns_cannot_pass_the_allowed_context_check(self):
        # Wrong protocol response is a failed witness even though the UDP socket is reachable.
        with service(socket.AF_INET, socket.SOCK_DGRAM, ('192.0.2.53', 5300), b'wrong',
                     expected=DNS_RESPONSE) as healthy:
            with self.assertRaisesRegex(RuntimeError, 'witness response differs'):
                healthy()
        with self.assertRaisesRegex(RuntimeError, 'witness service stopped'):
            healthy()

    def test_invalid_or_excess_witness_requests_produce_no_report(self):
        for payload in (b'boundaries\n', b'boundaries\nread\trelative\n',
                        b'boundaries\ntcp\tnot-an-address\n',
                        b'boundaries\n' + b'read\t/missing\n' * 33):
            with self.subTest(request=payload[:40]):
                test_vm.VmTest().run_probe(payload, valid=False)


def main():
    if os.geteuid() != 0:
        raise SystemExit('Run as root only on an owned disposable Linux host.')
    signal.signal(signal.SIGTERM, cancel_test)
    signal.signal(signal.SIGINT, cancel_test)
    current = os.readlink('/proc/self/ns/net')
    def addresses():
        devices = json.loads(subprocess.check_output(['ip', '-j', 'address', 'show'], text=True))
        # DHCP/IPv6 lifetime counters may advance without a configuration change.
        return [(device['ifname'], [(item['family'], item['local'], item['prefixlen'])
                                   for item in device.get('addr_info', [])]) for device in devices]

    if len(sys.argv) == 1:
        # Only this child configures addresses. No host interfaces/routes are modified.
        before = addresses()
        root = None
        post_checked = False
        def post_check():
            nonlocal post_checked
            post_checked = True
            if before != addresses() or os.readlink('/proc/self/ns/net') != current:
                raise RuntimeError('Host network state changed; inspect retained fixture at ' + str(root))
        try:
            previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM, signal.SIGINT})
            try:
                root = pathlib.Path(tempfile.mkdtemp(prefix='26chan-boundary-suite-', dir='/tmp'))
            finally:
                signal.pthread_sigmask(signal.SIG_SETMASK, previous)
            result = run_owned(['unshare', '--net', sys.executable, __file__, '--isolated', current, str(root)],
                               timeout=90, post_check=post_check)
        finally:
            # This also runs on failed startup, timeout and handled cancellation.
            if not post_checked:
                post_check()
            if root is not None:
                try:
                    root.rmdir()  # Only empty roots: retain uncertain contents for owned recovery.
                except OSError as error:
                    raise RuntimeError('test fixture retained for inspection at ' + str(root)) from error
        raise SystemExit(result.returncode)
    parent = os.readlink(f'/proc/{os.getppid()}/ns/net')
    if (len(sys.argv) != 4 or sys.argv[1] != '--isolated'
            or sys.argv[2] != parent or parent == current):
        raise SystemExit('Expected a fresh private network namespace.')
    root = pathlib.Path(sys.argv[3])
    metadata = root.lstat()
    if (root.parent != pathlib.Path('/tmp') or not re.fullmatch(r'26chan-boundary-suite-[a-z0-9_]+', root.name)
            or not stat.S_ISDIR(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_mode & 0o077):
        raise SystemExit('Expected a private owned suite directory.')
    tempfile.tempdir = str(root)
    devices = json.loads(subprocess.check_output(['ip', '-j', 'link', 'show'], text=True))
    if [device['ifname'] for device in devices] != ['lo']:
        raise SystemExit('Fixture namespace contains an unexpected network device.')
    subprocess.run(['ip', 'link', 'set', 'lo', 'up'], check=True)
    for address in ADDRESSES:
        command = ['ip', 'addr', 'add', address + ('/128' if ':' in address else '/32'), 'dev', 'lo']
        if ':' in address:
            command.append('nodad')
        subprocess.run(command, check=True)
    unittest.main(argv=[sys.argv[0], '-v'])


if __name__ == '__main__':
    main()
