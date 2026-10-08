#!/usr/bin/env python3
"""Versioned transport fixtures only: never launch a VM or mount storage."""
import contextlib
import importlib.util
import json
import os
import pathlib
import socket
import sys
import tempfile
import threading
import time
import types
import unittest
from unittest import mock

SCRIPTS = pathlib.Path(__file__).resolve().parents[2] / 'scripts/media'
sys.path.insert(0, str(SCRIPTS))
import dispatch_protocol as protocol
spec = importlib.util.spec_from_file_location('paired_runner', SCRIPTS / 'run-job.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


def pair(payload=b'x'):
    return (b'IBPAIR02\0\2\0\x30' + bytes(4) + len(payload).to_bytes(8, 'big')
            + bytes(8) + bytes(16) + payload + b'IBDONE02')


def request(payload=None):
    body = pair() if payload is None else payload
    return b'IBJOB002' + len(body).to_bytes(8, 'big') + bytes(range(32)) + body


class PairedTransportTest(unittest.TestCase):
    def receive(self, data, target, *, eof=True):
        left, right = socket.socketpair()
        with left, right:
            right.sendall(data)
            if eof:
                right.shutdown(socket.SHUT_WR)
            return protocol.receive_request(left, target, deadline=time.monotonic() + .1)

    def test_explicit_versions_and_caps(self):
        for magic, low, high, kind in ((b'IBJOB001', 1, 8_388_608, protocol.IMAGE_V1),
                                       (b'IBJOB002', 57, 16_777_272, protocol.PAIRED_V2)):
            for size in (low, high):
                self.assertEqual(protocol.request_kind(magic + size.to_bytes(8, 'big')), (kind, size))
            for size in (0, low - 1, high + 1, 2**64 - 1):
                with self.assertRaises(ValueError):
                    protocol.request_kind(magic + size.to_bytes(8, 'big'))
        for magic in (b'IBJOB003', b'IBPAIR02', b'IBOUT002'):
            with self.assertRaises(ValueError):
                protocol.request_kind(magic + (57).to_bytes(8, 'big'))
        with self.assertRaises(ValueError):
            protocol.output_bytes('auto')

    def test_v2_preserves_request_and_v1_never_sniffs_payload(self):
        with tempfile.TemporaryDirectory() as directory:
            for index, (data, kind, payload) in enumerate((
                    (request(), protocol.PAIRED_V2, request()),
                    (b'IBJOB001' + len(request()).to_bytes(8, 'big') + request(),
                     protocol.IMAGE_V1, request()))):
                target = pathlib.Path(directory) / str(index)
                self.assertEqual(self.receive(data, target), kind)
                self.assertEqual(target.read_bytes(), payload)

    def test_every_short_boundary_trailing_and_missing_eof_reject(self):
        data = request()
        with tempfile.TemporaryDirectory() as directory:
            for index, truncated in enumerate([data[:i] for i in range(len(data))] + [data + b'x']):
                with self.subTest(index=index), self.assertRaises(ValueError):
                    self.receive(truncated, pathlib.Path(directory) / str(index))
            with self.assertRaises(TimeoutError):
                self.receive(data, pathlib.Path(directory) / 'no-eof', eof=False)

    def test_versioned_disk_bytes_and_zero_padding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for index, size in enumerate((1, 407, 408, 409, 1024)):
                source, disk = root / f'source{index}', root / f'disk{index}'
                data = request(pair(bytes(size)))
                source.write_bytes(data)
                runner.input_disk(source, disk, input_kind=protocol.PAIRED_V2)
                self.assertEqual(disk.read_bytes(), data + bytes(-len(data) % 512))
            source, disk = root / 'v1', root / 'v1disk'
            source.write_bytes(request())
            runner.input_disk(source, disk)
            expected = len(request()).to_bytes(8, 'big') + request()
            self.assertEqual(disk.read_bytes(), expected + bytes(-len(expected) % 512))

    def test_paired_disk_rejects_wrong_outer_version_size_and_file_type(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            cases = [request()[:-1], request() + b'\0', b'IBJOB001' + request()[8:],
                     b'IBJOB003' + request()[8:], pair(), b'']
            for index, data in enumerate(cases):
                source = root / f'source{index}'
                source.write_bytes(data)
                with self.assertRaises(ValueError):
                    runner.input_disk(source, root / f'disk{index}', input_kind=protocol.PAIRED_V2)
            source = root / 'link'
            source.symlink_to(root / 'source0')
            with self.assertRaises(OSError):
                runner.input_disk(source, root / 'linkdisk', input_kind=protocol.PAIRED_V2)

    def test_output_version_and_size_are_selected_not_detected(self):
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / 'out'
            source.write_bytes(bytes(protocol.PAIRED_OUTPUT_BYTES))
            left, right = socket.socketpair()
            errors = []
            with left, right:
                def send():
                    try:
                        protocol.send_response(left, source, input_kind=protocol.PAIRED_V2)
                    except Exception as error:
                        errors.append(error)
                        left.shutdown(socket.SHUT_WR)
                worker = threading.Thread(target=send)
                worker.start()
                data = bytearray()
                while chunk := right.recv(65536):
                    data.extend(chunk)
                worker.join(2)
                self.assertFalse(worker.is_alive())
                self.assertEqual(errors, [])
                self.assertEqual(data[:16], b'IBOUT002' + (4_456_960).to_bytes(8, 'big'))
                self.assertEqual(len(data), 16 + 4_456_960)
            for kind, size in ((protocol.IMAGE_V1, 4_456_960),
                               (protocol.PAIRED_V2, 4_194_816),
                               (protocol.PAIRED_V2, 4_456_959),
                               (protocol.PAIRED_V2, 4_456_961)):
                source.write_bytes(bytes(size))
                left, right = socket.socketpair()
                with left, right, self.assertRaises(ValueError):
                    protocol.send_response(left, source, input_kind=kind)

    def test_broker_propagates_selected_kind_without_fallback(self):
        spec = importlib.util.spec_from_file_location('paired_broker', SCRIPTS / 'dispatch-broker.py')
        broker = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(broker)
        with tempfile.TemporaryDirectory() as directory:
            left, right = socket.socketpair()
            with left, right:
                right.sendall(request())
                right.shutdown(socket.SHUT_WR)
                def execute(source, destination, *, input_kind):
                    self.assertEqual(input_kind, protocol.PAIRED_V2)
                    self.assertEqual(source.read_bytes(), request())
                with mock.patch.object(broker, 'send_response') as send, \
                     mock.patch.object(broker, 'remove_request') as remove:
                    broker.handle_connection(left, os.getuid(), pathlib.Path(directory), execute)
                    self.assertEqual(send.call_args.kwargs, {'input_kind': protocol.PAIRED_V2})
                    remove.assert_called_once()

    def test_maximum_v2_request_stream_and_disk(self):
        body = bytes(16_777_272)
        data = request(body)
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            left, right = socket.socketpair()
            with left, right:
                errors = []
                def send():
                    try:
                        right.sendall(data)
                        right.shutdown(socket.SHUT_WR)
                    except Exception as error:
                        errors.append(error)
                worker = threading.Thread(target=send)
                worker.start()
                self.assertEqual(protocol.receive_request(left, root / 'source'), protocol.PAIRED_V2)
                worker.join(3)
                self.assertFalse(worker.is_alive())
                self.assertEqual(errors, [])
            runner.input_disk(root / 'source', root / 'disk', input_kind=protocol.PAIRED_V2)
            self.assertEqual((root / 'disk').read_bytes(), data + bytes(-len(data) % 512))

    def test_mocked_runner_preserves_limits_and_retains_output_inode(self):
        cases = ((kind, size, mutation)
                 for kind, size in ((protocol.IMAGE_V1, 4_194_816), (protocol.PAIRED_V2, 4_456_960))
                 for mutation in ('rename', 'truncate', 'grow'))
        for kind, size, mutation in cases:
            with self.subTest(kind=kind, mutation=mutation), tempfile.TemporaryDirectory() as directory:
                root = pathlib.Path(directory)
                source = root / 'source'
                source.write_bytes(request() if kind == protocol.PAIRED_V2 else b'x')
                config = {name: root / name for name in ('kernel', 'initramfs', 'firecracker', 'jailer')}
                for value in config.values():
                    value.write_bytes(b'fixture')
                def run_service(args, lock):
                    self.assertIn('fsize=' + str(size), args)
                    for prop in ('MemoryMax=256M', 'MemorySwapMax=0', 'TasksMax=32',
                                 'CPUQuota=100%', 'RuntimeMaxSec=15s', 'KillMode=control-group'):
                        self.assertIn('--property=' + prop, args)
                    vm = next(root.glob('*/firecracker/*/root/vm.json'))
                    machine = json.loads(vm.read_text())
                    self.assertEqual(machine['machine-config']['mem_size_mib'], 128)
                    self.assertIn('board_media_input_kind=' + kind, machine['boot-source']['boot_args'])
                    output = vm.with_name('output.disk')
                    self.assertEqual(output.stat().st_size, size)
                    if mutation == 'rename':
                        # Renaming cannot redirect the already retained descriptor.
                        output.rename(output.with_name('retained.disk'))
                        output.write_bytes(b'changed name')
                    else:
                        with output.open('r+b') as writer:
                            writer.truncate(size - 1 if mutation == 'truncate' else size + 1)
                user = types.SimpleNamespace(pw_uid=123, pw_gid=123, pw_shell='/usr/sbin/nologin')
                with mock.patch.object(runner, 'JOBS', root), \
                     mock.patch.object(runner, 'locked_jobs', return_value=contextlib.nullcontext(None)), \
                     mock.patch.object(runner, 'reconcile_jobs'), \
                     mock.patch.object(runner, 'command') as command, \
                     mock.patch.object(runner.pwd, 'getpwnam', return_value=user), \
                     mock.patch.object(runner.os, 'chown'), \
                     mock.patch.object(runner, 'run_service', side_effect=run_service), \
                     mock.patch.object(runner, 'ignore_cancellation'), \
                     mock.patch.object(runner, 'stop_job') as stop, \
                     mock.patch.object(runner, 'remove_workspace') as remove:
                    if mutation == 'rename':
                        runner.run(config, source, root / 'result', input_kind=kind)
                        self.assertEqual((root / 'result').read_bytes(), bytes(size))
                    else:
                        with self.assertRaises(ValueError):
                            runner.run(config, source, root / 'result', input_kind=kind)
                        self.assertFalse((root / 'result').exists())
                    self.assertIn('size=96M,nosuid,mode=0700', command.call_args.args[0])
                    stop.assert_called_once()
                    remove.assert_called_once()


if __name__ == '__main__':
    unittest.main()
