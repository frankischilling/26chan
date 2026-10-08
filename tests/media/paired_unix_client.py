#!/usr/bin/env python3
"""Standalone, standard-library-only gateway client for owned qualification.

Staged by the root harness; no checkout imports or credentials are needed.
"""
import os
import pathlib
import pwd
import re
import socket
import stat
import sys

RESULT_BYTES = 4_456_960
STAGES = ('arguments', 'guard', 'identity', 'paths', 'socket', 'input', 'output',
          'connect', 'send', 'receive')
ERROR_CLASSES = ('RuntimeError', 'ValueError', 'KeyError', 'OSError',
                 'PermissionError', 'FileNotFoundError', 'ConnectionRefusedError',
                 'TimeoutError', 'BrokenPipeError', 'ConnectionResetError')


def diagnostic(stage, error):
    """Only fixed vocabulary and a bounded errno cross the child boundary."""
    stage = stage if stage in STAGES else 'arguments'
    kind = type(error).__name__
    kind = kind if kind in ERROR_CLASSES else 'Error'
    number = getattr(error, 'errno', None)
    number = number if type(number) is int and 1 <= number <= 4095 else 0
    return f'PAIRED_UNIX_CLIENT stage={stage} error={kind} errno={number}\n'


def unix_client(endpoint, source, destination, mode, *, report_stage=lambda stage: None):
    report_stage('guard')
    if sys.flags.optimize:
        raise RuntimeError('optimized Python cannot run qualification')
    if os.environ.get('APP_ENV') != 'development':
        raise RuntimeError('explicit APP_ENV=development is required')
    if os.environ.get('MEDIA_PAIRED_VM_QUALIFY') != '1':
        raise RuntimeError('explicit MEDIA_PAIRED_VM_QUALIFY=1 is required')
    if os.getuid() != os.geteuid() or os.geteuid() == 0:
        raise RuntimeError('qualification identity rejected')
    if os.environ.get('MEDIA_PAIRED_VM_UNIX_CLIENT') != '1' or mode not in ('eof', 'no-eof'):
        raise RuntimeError('explicit Unix-client mode required')
    report_stage('identity')
    user = pwd.getpwnam('board-media-gateway')
    if (os.getuid(), os.geteuid(), os.getgid(), os.getegid()) != (user.pw_uid, user.pw_uid, user.pw_gid, user.pw_gid):
        raise RuntimeError('authorized gateway identity required')
    report_stage('paths')
    endpoint, source, destination = map(pathlib.Path, (endpoint, source, destination))
    root = endpoint.parent.parent
    if (root.parent != pathlib.Path('/run') or not re.fullmatch(r'26chan-dispatch-[a-z0-9_]{8}', root.name)
            or endpoint != root / 'broker/broker.sock' or source != root / 'gateway/request.bin'
            or destination != root / 'gateway/candidate.bin'):
        raise RuntimeError('owned Unix-client paths required')
    report_stage('socket')
    socket_info = endpoint.lstat()
    if not stat.S_ISSOCK(socket_info.st_mode) or socket_info.st_uid != 0:
        raise RuntimeError('root broker socket required')
    report_stage('input')
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(source_fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != user.pw_uid or not 1 <= info.st_size <= 48 + 16_777_272:
            raise RuntimeError('bounded owned Unix-client input required')
        payload = stream.read(info.st_size + 1)
        if len(payload) != info.st_size:
            raise RuntimeError('Unix-client input changed')
    report_stage('output')
    output_fd = os.open(destination, os.O_WRONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(output_fd, 'wb') as output:
        info = os.fstat(output.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != user.pw_uid or info.st_size != 0:
            raise RuntimeError('empty owned Unix-client output required')
        report_stage('connect')
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(30)
            connection.connect(str(endpoint))
            report_stage('send')
            try:
                connection.sendall(payload)
                if mode == 'eof':
                    connection.shutdown(socket.SHUT_WR)
            except (BrokenPipeError, ConnectionResetError):
                pass
            report_stage('receive')
            size = 0
            try:
                while chunk := connection.recv(65536):
                    size += len(chunk)
                    if size > 16 + RESULT_BYTES:
                        raise ValueError('oversized response')
                    output.write(chunk)
            except ConnectionResetError:
                pass


def main(arguments):
    stage = 'arguments'
    def report_stage(value):
        nonlocal stage
        stage = value
    try:
        if len(arguments) != 4:
            raise RuntimeError('exact client arguments required')
        unix_client(*arguments, report_stage=report_stage)
    except Exception as error:
        sys.stderr.write(diagnostic(stage, error))
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
elif sys.flags.optimize:
    raise RuntimeError('optimized Python cannot run qualification')
