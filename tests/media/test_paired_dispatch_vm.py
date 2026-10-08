#!/usr/bin/env python3
"""Owned broker + real Firecracker qualification and native coordinator.

Uses the existing isolated identities, PKI, root broker and Rust TLS gateway.
The direct Unix client runs as the authorized gateway user. Decoder output is
never synthesized. Coordinator qualification requires its explicit test binary.
"""
import os
import pathlib
import re
import signal
import socket
import subprocess
import sys
import time
import pwd
import stat

if sys.flags.optimize:
    raise RuntimeError('optimized Python cannot run qualification')

import paired_vm_fixtures as f
from test_dispatch import Exercise, SAFE


class PairedExercise(Exercise):
    def __init__(self):
        f.qualification_guard()
        super().__init__()

    def exchange(self, payload, *, eof=True, rejected=False, **expected):
        f.qualification_guard()
        source = self.keys / 'request.bin'
        destination = self.keys / 'candidate.bin'
        self.keys.chmod(0o711)
        # Gateway cannot write its configuration directory. Only these owned
        # transfer files are granted; existing keys/configuration stay protected.
        self.write(source, payload, self.gateway)
        self.write(destination, b'', self.gateway)
        command = [sys.executable, __file__, '--unix-client', str(self.broker_dir / 'broker.sock'),
                   str(source), str(destination), 'eof' if eof else 'no-eof']
        started = time.monotonic()
        client_env = {**SAFE, 'MEDIA_PAIRED_VM_QUALIFY': '1', 'MEDIA_PAIRED_VM_UNIX_CLIENT': '1'}
        self.finish(self.launch(command, self.gateway, client_env))
        data = destination.read_bytes()
        if rejected:
            assert data == b'', 'rejected transport returned candidate bytes'
        else:
            assert data[:16] == b'IBOUT002' + f.RESULT_BYTES.to_bytes(8, 'big')
            f.check_result(data[16:], **expected)
        self.clean_vm()
        source.unlink()
        destination.unlink()
        return time.monotonic() - started

    def qualify_coordinator(self):
        f.qualification_guard()
        source = pathlib.Path(os.environ['MEDIA_PAIRED_COORDINATOR_TEST']).resolve()
        assert source.is_file(), 'compiled paired_vm integration test missing'
        destination = self.bin / 'paired-vm-test'
        import shutil
        shutil.copyfile(source, destination)
        destination.chmod(0o755)
        environment = {**self.writer,
                       'INTAKE_DATABASE_URL': os.environ['INTAKE_DATABASE_URL'],
                       'MIGRATION_DATABASE_URL': os.environ['MIGRATION_DATABASE_URL'],
                       'MEDIA_PAIRED_VM_QUALIFY': '1',
                       'MEDIA_PAIRED_CLIENT_CONFIG': str(self.private / 'client.json')}
        process = self.launch([destination, '--ignored', '--exact',
                               'real_guest_candidates_remain_nonpublishable', '--nocapture'],
                              self.coordinator, environment)
        # Five sequential real guest requests, each retaining the existing
        # per-operation bounds. This only extends harness supervision.
        try:
            out, err = process.communicate(timeout=180)
        finally:
            if process.poll() is None:
                self.stop(process)
            captured, _ = process.communicate(timeout=2)
            for match in re.finditer(rb'^PAIRED_VM_OWNED_JOB=([0-9a-f]{32})$', captured, re.MULTILINE):
                self.ids.append(match.group(1).decode('ascii'))
        check_coordinator_completion(process.returncode, out)
        self.clean_vm()
        print('PASS actual paired SQL coordinator -> Rust mTLS -> gateway -> root broker -> guest; no publication', flush=True)

    def exercise_paired(self):
        f.qualification_guard()
        for name in (None, 'empty', 'commands'):
            binding = os.urandom(32)
            replay = None if name is None else f.replay(name)
            self.exchange(f.request(f.frame(f.png(3, 2), replay), binding),
                          binding=binding, width=3, height=2, replay_name=name)
        self.exchange(f.request(f.frame(f.png(size=f.COMPONENT_BYTES), f.maximum_replay())),
                      replay_name='empty')
        request = f.request()
        for data in (request[:15], request[:47], request[:-1], request + b'\0',
                     b'IBJOB003' + request[8:],
                     b'IBJOB002' + (16_777_273).to_bytes(8, 'big') + request[16:]):
            self.exchange(data, rejected=True)
        elapsed = self.exchange(request, eof=False, rejected=True)
        assert 2 <= elapsed < 8, 'broker did not enforce the existing intake deadline'
        self.exchange(f.request())  # failure never poisons the next owned job
        print('PASS paired root broker -> real guest, exact cap, EOF and timeout controls', flush=True)


def check_coordinator_completion(returncode, output):
    # Explicit checks remain effective even independently of the entry guards.
    if returncode != 0:
        raise RuntimeError('actual paired coordinator qualification failed')
    summaries = re.findall(rb'^test result: [^\r\n]+', output, re.MULTILINE)
    if len(summaries) != 1 or not re.fullmatch(
            rb'test result: ok\. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in [0-9.]+s',
            summaries[0]):
        raise RuntimeError('exact paired coordinator test did not execute successfully')


def unix_client(endpoint, source, destination, mode):
    f.qualification_guard(root=False)
    if os.environ.get('MEDIA_PAIRED_VM_UNIX_CLIENT') != '1' or mode not in ('eof', 'no-eof'):
        raise RuntimeError('explicit Unix-client mode required')
    user = pwd.getpwnam('board-media-gateway')
    if (os.getuid(), os.geteuid(), os.getgid(), os.getegid()) != (user.pw_uid, user.pw_uid, user.pw_gid, user.pw_gid):
        raise RuntimeError('authorized gateway identity required')
    endpoint, source, destination = map(pathlib.Path, (endpoint, source, destination))
    root = endpoint.parent.parent
    if (root.parent != pathlib.Path('/run') or not re.fullmatch(r'26chan-dispatch-[a-z0-9_]{8}', root.name)
            or endpoint != root / 'broker/broker.sock' or source != root / 'gateway/request.bin'
            or destination != root / 'gateway/candidate.bin'):
        raise RuntimeError('owned Unix-client paths required')
    socket_info = endpoint.lstat()
    if not stat.S_ISSOCK(socket_info.st_mode) or socket_info.st_uid != 0:
        raise RuntimeError('root broker socket required')
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(source_fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != user.pw_uid or not 1 <= info.st_size <= 48 + 16_777_272:
            raise RuntimeError('bounded owned Unix-client input required')
        payload = stream.read(info.st_size + 1)
        if len(payload) != info.st_size:
            raise RuntimeError('Unix-client input changed')
    output_fd = os.open(destination, os.O_WRONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(output_fd, 'wb') as output:
        info = os.fstat(output.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != user.pw_uid or info.st_size != 0:
            raise RuntimeError('empty owned Unix-client output required')
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(30)
            connection.connect(str(endpoint))
            try:
                connection.sendall(payload)
                if mode == 'eof':
                    connection.shutdown(socket.SHUT_WR)
            except (BrokenPipeError, ConnectionResetError):
                pass
            size = 0
            try:
                while chunk := connection.recv(65536):
                    size += len(chunk)
                    if size > 16 + f.RESULT_BYTES:
                        raise ValueError('oversized response')
                    output.write(chunk)
            except ConnectionResetError:
                pass


if __name__ == '__main__':
    if len(sys.argv) == 6 and sys.argv[1] == '--unix-client':
        unix_client(*sys.argv[2:])
    else:
        f.qualification_guard()
        if len(sys.argv) != 1:
            raise RuntimeError('unknown qualification arguments')
        os.umask(0o077)
        def cancelled(_signal, _frame):
            raise KeyboardInterrupt('owned paired dispatch qualification cancelled')
        signal.signal(signal.SIGTERM, cancelled)
        exercise = PairedExercise()
        try:
            exercise.setup()
            exercise.exercise_paired()
            exercise.qualify_coordinator()
        finally:
            exercise.cleanup()
        print('PASS paired owned services and fixture files cleaned', flush=True)
